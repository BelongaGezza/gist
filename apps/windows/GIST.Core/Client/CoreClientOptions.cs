using Gist.Core.Keys;

namespace Gist.Core.Client;

/// <summary>
/// Everything <see cref="CoreClient"/> needs to open a store: where the database and storage
/// directory live, and who holds the content-encryption key.
/// </summary>
/// <remarks>
/// <para>
/// This is the seam onto <c>GistStoragePaths</c> (key-custody workstream, <c>Gist.Core.Storage</c>):
/// the shell builds the paths once and hands the three values here, so this project never has to
/// know whether it is running packaged (LocalState) or unpackaged (<c>%LOCALAPPDATA%\GIST</c>).
/// Constructing it from a <c>GistStoragePaths</c> is a one-liner at the shell's composition root:
/// <c>new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir))</c>.
/// </para>
/// <para>
/// Review item Q9 requires the key directory to derive from the same root as the store.
/// <c>GistStoragePaths</c> owns that invariant (its <c>KeyDir</c> and <c>DbPath</c> both hang off
/// <c>Root</c>); this record just carries the results.
/// </para>
/// </remarks>
/// <param name="DbPath">Full path to <c>gist.sqlite3</c>.</param>
/// <param name="StorageDir">Directory holding IR blobs and the ADR-006 <c>originals/</c> copies.</param>
/// <param name="KeyProvider">
/// Key custody (ADR-016). Consulted <b>eagerly, in managed code</b> during
/// <see cref="CoreClient.InitializeAsync"/> — see that method for why.
/// </param>
public sealed record CoreClientOptions(string DbPath, string StorageDir, IKeyProvider KeyProvider)
{
    /// <summary>Throws if any member is missing or blank.</summary>
    public void Validate()
    {
        if (string.IsNullOrWhiteSpace(DbPath))
        {
            throw new ArgumentException("DbPath is required.", nameof(DbPath));
        }

        if (string.IsNullOrWhiteSpace(StorageDir))
        {
            throw new ArgumentException("StorageDir is required.", nameof(StorageDir));
        }

        ArgumentNullException.ThrowIfNull(KeyProvider);
    }
}
