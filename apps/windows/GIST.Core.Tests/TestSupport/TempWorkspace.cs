using System.Security.Cryptography;

namespace Gist.Core.Tests.TestSupport;

/// <summary>
/// A throwaway store location for one test: its own temp directory holding <c>gist.sqlite3</c>, a
/// <c>storage/</c> directory and copies of any fixtures the test imports.
/// </summary>
/// <remarks>
/// There is no mocking framework here on purpose. Every FFI-backed test runs against a <b>real</b>
/// <c>GistCore</c> — real SQLite, real filesystem — exactly as <c>gist-core</c>'s own Rust tests and
/// Apple's <c>GISTTests</c> do. A mock of the core would only prove the mock matches itself.
/// </remarks>
public sealed class TempWorkspace : IDisposable
{
    public TempWorkspace()
    {
        Root = Path.Combine(Path.GetTempPath(), "GistCoreTests-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Root);
        DbPath = Path.Combine(Root, "gist.sqlite3");
        StorageDir = Path.Combine(Root, "storage");
        KeyDir = Path.Combine(Root, "keys");
    }

    public string Root { get; }

    public string DbPath { get; }

    public string StorageDir { get; }

    /// <summary>
    /// A key directory hanging off the same <see cref="Root"/> as the store, mirroring the
    /// invariant review item Q9 requires of <c>GistStoragePaths</c>.
    /// </summary>
    public string KeyDir { get; }

    /// <summary>
    /// Copies a fixture out of the test bundle into this workspace, so importing it is exactly like
    /// importing a file the user picked — and so an import test that (incorrectly) deleted the
    /// user's original would destroy a copy rather than the checked-in corpus file.
    /// </summary>
    /// <remarks>
    /// Fixtures are resolved from <see cref="AppContext.BaseDirectory"/>, never from a relative path
    /// out of the test runner's working directory, which is not guaranteed to be the project folder.
    /// </remarks>
    public string CopyFixture(string fileName, string? asName = null)
    {
        var source = Path.Combine(AppContext.BaseDirectory, "Fixtures", fileName);
        if (!File.Exists(source))
        {
            throw new FileNotFoundException(
                $"Test fixture '{fileName}' was not copied to the output directory.", source);
        }

        var destination = Path.Combine(Root, asName ?? fileName);
        File.Copy(source, destination, overwrite: true);
        return destination;
    }

    /// <summary>
    /// Independently predicts where <c>gist_store::Store::store_original_copy</c> writes the ADR-006
    /// sandboxed copy of <paramref name="sourcePath"/>'s content:
    /// <c>&lt;StorageDir&gt;/originals/&lt;sha256-hex&gt;.&lt;ext&gt;</c>.
    /// </summary>
    /// <remarks>
    /// Recomputed here rather than read back over FFI because <c>FfiLibraryItem</c> exposes
    /// <c>source_path</c> but not <c>source_copy_path</c>. Deriving the name independently is also
    /// the stronger test: it asserts against what is genuinely on disk instead of trusting the core
    /// to report its own behaviour. Same approach as Apple's <c>predictedSandboxedCopyPath</c>.
    /// </remarks>
    public string PredictSandboxedCopyPath(string sourcePath)
    {
        var hash = Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(sourcePath)));
        var ext = Path.GetExtension(sourcePath).TrimStart('.').ToLowerInvariant();
        var fileName = ext.Length == 0 ? hash : $"{hash}.{ext}";
        return Path.Combine(StorageDir, "originals", fileName);
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(Root, recursive: true);
        }
        catch (IOException)
        {
            // A still-open SQLite handle on a torn-down test is not worth failing the run over;
            // the OS reclaims the temp directory.
        }
        catch (UnauthorizedAccessException)
        {
        }
    }
}
