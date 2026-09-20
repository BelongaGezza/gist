using System.Security.Cryptography;

namespace Gist.KeyProvider;

/// <summary>
/// Plain-C# mirror of uniffi's generated <c>KeyProvider.GetOrCreateKey()</c> callback interface
/// (gist-ffi, ADR-011). Contract: exactly 32 bytes, the same key for the life of the data.
/// </summary>
public interface IKeyProvider
{
    byte[] GetOrCreateKey();
}

/// <summary>Base type for all key-custody failures. Messages never contain key material.</summary>
public class KeyProviderException : Exception
{
    public KeyProviderException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>
/// The stored key file exists but cannot be trusted (bad header, truncated, tampered, wrong user/entropy,
/// DPAPI master key lost, wrong decrypted length). A new key is NEVER generated in this state: doing so
/// would orphan every item encrypted under the old key. The app must surface an "encrypted items are
/// unrecoverable / restore profile" state instead.
/// </summary>
public sealed class KeyStoreCorruptException : KeyProviderException
{
    public KeyStoreCorruptException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>I/O problem (permissions, disk) that is not evidence of corruption.</summary>
public sealed class KeyStoreIoException : KeyProviderException
{
    public KeyStoreIoException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>
/// Windows key custody (ADR-016): a 32-byte random key protected with DPAPI (CurrentUser scope + optional
/// entropy) and stored in a file under a caller-supplied directory (packaged app: LocalState).
/// Creation is race-safe: the key is written to a temp file, then moved into place without overwrite;
/// a loser of the race discards its candidate and re-reads the winner's key
/// (mirrors KeychainKeyProvider's SecItemAdd duplicate handling).
/// </summary>
public sealed class DpapiKeyProvider : IKeyProvider
{
    public const int KeyLength = 32;
    public const string FileName = "content-key.dpapi";

    // File format: 4-byte magic "GKP1" followed by the DPAPI blob.
    private static readonly byte[] Magic = "GKP1"u8.ToArray();
    private static readonly byte[] DefaultEntropy = "GIST.KeyProvider.v1"u8.ToArray();

    private readonly string _directory;
    private readonly byte[] _entropy;

    public DpapiKeyProvider(string directory, byte[]? entropy = null)
    {
        if (string.IsNullOrWhiteSpace(directory)) throw new ArgumentException("directory required", nameof(directory));
        _directory = directory;
        _entropy = entropy is { Length: > 0 } ? (byte[])entropy.Clone() : DefaultEntropy;
    }

    public string KeyFilePath => Path.Combine(_directory, FileName);

    public byte[] GetOrCreateKey()
    {
        if (File.Exists(KeyFilePath)) return ReadKey();

        try { Directory.CreateDirectory(_directory); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        { throw new KeyStoreIoException("Cannot create key directory.", e); }

        var candidate = RandomNumberGenerator.GetBytes(KeyLength);
        var tmp = Path.Combine(_directory, FileName + "." + Guid.NewGuid().ToString("N") + ".tmp");
        try
        {
            var blob = Protect(candidate);
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
        try { raw = File.ReadAllBytes(KeyFilePath); }
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
            throw new KeyStoreCorruptException(
                "Key file cannot be decrypted (tampered, truncated, different user/entropy, or DPAPI master key lost).", e);
        }

        if (key.Length != KeyLength)
        {
            CryptographicOperations.ZeroMemory(key);
            throw new KeyStoreCorruptException("Decrypted key has an unexpected length.");
        }
        return key;
    }

    private byte[] Protect(byte[] key) =>
        ProtectedData.Protect(key, _entropy, DataProtectionScope.CurrentUser);
}
