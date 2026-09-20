using System.Security.Cryptography;
using System.Text.RegularExpressions;

namespace Gist.Core.Keys;

/// <summary>
/// Windows key custody (ADR-016): a 32-byte random key protected with DPAPI (CurrentUser scope + optional
/// entropy) and stored in a file under a caller-supplied directory — normally
/// <see cref="Gist.Core.Storage.GistStoragePaths.KeyDir"/>, so the key always sits under the same root as
/// the store it unlocks.
/// <para>
/// Creation is race-safe: the key is written to a temp file, then moved into place without overwrite;
/// a loser of the race discards its candidate and re-reads the winner's key (mirrors
/// <c>KeychainKeyProvider</c>'s <c>SecItemAdd</c> duplicate handling).
/// </para>
/// <para>
/// <b>Invariant:</b> on any failure — corrupt, unavailable or I/O — this type never deletes, overwrites
/// or recreates <see cref="KeyFilePath"/>. The only file it ever removes is its own
/// <c>content-key.dpapi.&lt;guid&gt;.tmp</c> scratch file.
/// </para>
/// </summary>
public sealed partial class DpapiKeyProvider : IKeyProvider
{
    public const int KeyLength = 32;
    public const string FileName = "content-key.dpapi";

    /// <summary>
    /// A temp file younger than this may belong to a creator racing us right now, so it is never reaped.
    /// Publishing a temp file takes milliseconds; anything this old is the debris of a crashed create.
    /// </summary>
    private static readonly TimeSpan StaleTempFileAge = TimeSpan.FromMinutes(1);

    // File format: 4-byte magic "GKP1" followed by the DPAPI blob.
    private static readonly byte[] Magic = "GKP1"u8.ToArray();
    private static readonly byte[] DefaultEntropy = "GIST.KeyProvider.v1"u8.ToArray();

    private readonly string _directory;
    private readonly byte[] _entropy;
    private int _maintenanceDone;

    public DpapiKeyProvider(string directory, byte[]? entropy = null)
    {
        if (string.IsNullOrWhiteSpace(directory)) throw new ArgumentException("directory required", nameof(directory));
        _directory = directory;
        _entropy = entropy is { Length: > 0 } ? (byte[])entropy.Clone() : DefaultEntropy;
    }

    public string KeyFilePath => Path.Combine(_directory, FileName);

    public byte[] GetOrCreateKey()
    {
        RunDirectoryMaintenanceOnce();

        if (File.Exists(KeyFilePath)) return ReadKey();

        try { Directory.CreateDirectory(_directory); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        { throw new KeyStoreIoException("Cannot create key directory.", e); }
        KeyStoreAcl.TryRestrictDirectoryToCurrentUser(_directory);

        var candidate = RandomNumberGenerator.GetBytes(KeyLength);
        var tmp = Path.Combine(_directory, FileName + "." + Guid.NewGuid().ToString("N") + ".tmp");
        try
        {
            byte[] blob;
            try { blob = Protect(candidate); }
            catch (CryptographicException e)
            {
                // Cannot wrap a brand-new key: the platform is unavailable, nothing on disk is wrong.
                CryptographicOperations.ZeroMemory(candidate);
                throw new KeyStoreUnavailableException(
                    "Windows data protection is unavailable, so a new key could not be created. Nothing was changed; retry.", e);
            }

            try
            {
                using var fs = new FileStream(tmp, FileMode.CreateNew, FileAccess.Write, FileShare.None);
                fs.Write(Magic);
                fs.Write(blob);
                fs.Flush(true);
            }
            finally { CryptographicOperations.ZeroMemory(blob); }

            try
            {
                // Atomic no-overwrite publish. Throws IOException if the target already exists.
                File.Move(tmp, KeyFilePath, overwrite: false);
                return candidate;
            }
            catch (IOException) when (File.Exists(KeyFilePath))
            {
                // Lost the race: never return our unpersisted candidate; use the winner's key.
                CryptographicOperations.ZeroMemory(candidate);
                return ReadKey();
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            CryptographicOperations.ZeroMemory(candidate);
            throw new KeyStoreIoException("Cannot persist key file.", e);
        }
        finally
        {
            try { if (File.Exists(tmp)) File.Delete(tmp); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
    }

    private byte[] ReadKey()
    {
        byte[] raw;
        try { raw = ReadKeyFileBytes(); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        { throw new KeyStoreIoException("Cannot read key file.", e); }

        if (raw.Length <= Magic.Length || !raw.AsSpan(0, Magic.Length).SequenceEqual(Magic))
            throw new KeyStoreCorruptException("Key file is empty, truncated or has an unrecognised header.");

        byte[] key;
        try
        {
            key = ProtectedData.Unprotect(raw.AsSpan(Magic.Length).ToArray(), _entropy, DataProtectionScope.CurrentUser);
        }
        catch (CryptographicException e)
        {
            // Q9: only a provably bad blob is "corrupt"; anything else is transient and retryable.
            throw KeyStoreFailureClassifier.Classify(e);
        }

        if (key.Length != KeyLength)
        {
            CryptographicOperations.ZeroMemory(key);
            throw new KeyStoreCorruptException("Decrypted key has an unexpected length.");
        }
        return key;
    }

    /// <summary>
    /// Reads the key file, retrying briefly on a sharing violation. On Windows the file can be held
    /// momentarily by something that is not us — an AV scanner, the search indexer, a backup agent,
    /// or Windows propagating an inherited ACL — and reporting "cannot read your key" on the first
    /// transient collision would be a false alarm. A genuinely missing or inaccessible file still
    /// fails fast.
    /// </summary>
    private byte[] ReadKeyFileBytes()
    {
        const int attempts = 5;
        for (int attempt = 1; ; attempt++)
        {
            try { return File.ReadAllBytes(KeyFilePath); }
            catch (IOException e) when (attempt < attempts
                                        && e is not FileNotFoundException and not DirectoryNotFoundException)
            {
                Thread.Sleep(10 * attempt);
            }
        }
    }

    private byte[] Protect(byte[] key) =>
        ProtectedData.Protect(key, _entropy, DataProtectionScope.CurrentUser);

    /// <summary>
    /// Once per instance (so a long-lived provider does not re-scan the directory on every callback
    /// from Rust): harden the directory ACL and reap scratch files left behind by a create that
    /// crashed between writing the temp file and publishing it (review finding Q9).
    /// </summary>
    private void RunDirectoryMaintenanceOnce()
    {
        if (Interlocked.Exchange(ref _maintenanceDone, 1) != 0) return;
        if (!Directory.Exists(_directory)) return;

        KeyStoreAcl.TryRestrictDirectoryToCurrentUser(_directory);
        ReapStaleTempFiles();
    }

    private void ReapStaleTempFiles()
    {
        var cutoff = DateTime.UtcNow - StaleTempFileAge;
        try
        {
            foreach (var path in Directory.EnumerateFiles(_directory, FileName + ".*.tmp"))
            {
                // Only files this provider created: the wildcard alone would also match e.g.
                // "content-key.dpapi.backup.tmp" dropped there by something else. The key file
                // itself can never match (it has no ".<32 hex>.tmp" suffix).
                if (!TempFileName().IsMatch(Path.GetFileName(path))) continue;
                try
                {
                    if (File.GetLastWriteTimeUtc(path) > cutoff) continue; // a live creator may own it
                    File.Delete(path);
                }
                catch (Exception e) when (e is IOException or UnauthorizedAccessException) { /* in use; next launch */ }
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or DirectoryNotFoundException)
        {
            // Best-effort housekeeping: never fail GetOrCreateKey because of it.
        }
    }

    [GeneratedRegex(@"^content-key\.dpapi\.[0-9a-fA-F]{32}\.tmp$", RegexOptions.CultureInvariant)]
    private static partial Regex TempFileName();
}
