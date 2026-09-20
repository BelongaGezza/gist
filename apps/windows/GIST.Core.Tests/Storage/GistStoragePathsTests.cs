using Gist.Core.Keys;
using Gist.Core.Storage;

namespace Gist.Core.Tests.Storage;

// Hermetic: every path-composition test uses ForRoot with a temp directory. The two tests that look
// at the real machine (ForUnpackaged, Resolve) only compare strings — they create nothing.
public sealed class GistStoragePathsTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "gist-paths-test-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        try { Directory.Delete(_dir, true); } catch (DirectoryNotFoundException) { }
    }

    [Fact]
    public void ForRoot_ComposesTheThreeWellKnownPaths()
    {
        var paths = GistStoragePaths.ForRoot(_dir);

        Assert.Equal(_dir, paths.Root);
        Assert.Equal(Path.Combine(_dir, "gist.sqlite3"), paths.DbPath);
        Assert.Equal(Path.Combine(_dir, "storage"), paths.StorageDir);
        Assert.Equal(Path.Combine(_dir, "keys"), paths.KeyDir);
    }

    [Theory]
    [InlineData("plain")]
    [InlineData("with space")]
    [InlineData("ünïcödé-📚")]
    [InlineData("trailing-sep\\")]
    [InlineData("nested\\deeper\\still")]
    public void KeyDir_IsAlwaysUnderTheSameRootAsDbAndStorage(string leaf)
    {
        // The invariant this type exists for: one root chooses all three, so the key and the
        // encrypted store can never drift apart (review finding Q9).
        var paths = GistStoragePaths.ForRoot(Path.Combine(_dir, leaf));

        Assert.Equal(paths.Root, Path.GetDirectoryName(paths.DbPath));
        Assert.Equal(paths.Root, Path.GetDirectoryName(paths.StorageDir));
        Assert.Equal(paths.Root, Path.GetDirectoryName(paths.KeyDir));
        foreach (var p in new[] { paths.DbPath, paths.StorageDir, paths.KeyDir })
            Assert.StartsWith(paths.Root + Path.DirectorySeparatorChar, p, StringComparison.Ordinal);
    }

    [Fact]
    public void ForRoot_NormalisesRelativeAndTrailingSeparatorRoots()
    {
        var withSep = GistStoragePaths.ForRoot(_dir + Path.DirectorySeparatorChar);
        Assert.Equal(_dir, withSep.Root);
        Assert.Equal(GistStoragePaths.ForRoot(_dir).KeyDir, withSep.KeyDir);

        var relative = GistStoragePaths.ForRoot(".");
        Assert.True(Path.IsPathFullyQualified(relative.Root));
        Assert.True(Path.IsPathFullyQualified(relative.DbPath));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    public void ForRoot_RejectsAMissingRoot(string? root)
    {
        Assert.Throws<ArgumentException>(() => GistStoragePaths.ForRoot(root!));
    }

    [Fact]
    public void KeyDirAndStorageDir_AreSeparateDirectories()
    {
        // The key must not sit loose among the IR blobs and originals/ copies.
        var paths = GistStoragePaths.ForRoot(_dir);
        Assert.NotEqual(paths.StorageDir, paths.KeyDir);
        Assert.False(paths.KeyDir.StartsWith(paths.StorageDir + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase));
    }

    [Fact]
    public void EnsureCreated_CreatesRootStorageAndKeyDirectories_AndIsIdempotent()
    {
        var paths = GistStoragePaths.ForRoot(Path.Combine(_dir, "root"));
        paths.EnsureCreated();
        paths.EnsureCreated();

        Assert.True(Directory.Exists(paths.Root));
        Assert.True(Directory.Exists(paths.StorageDir));
        Assert.True(Directory.Exists(paths.KeyDir));
        Assert.False(File.Exists(paths.DbPath)); // the core creates the database, not this type
    }

    [Fact]
    public void ForUnpackaged_IsLocalAppDataGist_NotRoaming()
    {
        var expected = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "GIST");
        var paths = GistStoragePaths.ForUnpackaged();

        Assert.Equal(expected, paths.Root);
        Assert.DoesNotContain("Roaming", paths.Root, StringComparison.OrdinalIgnoreCase);
        Assert.Equal(Path.Combine(expected, "keys"), paths.KeyDir);
    }

    [Fact]
    public void UnpackagedTestHost_HasNoPackageIdentity_SoResolvePicksTheUnpackagedLayout()
    {
        // `dotnet test` never runs with package identity; the packaged branch is unverifiable here
        // (see the ADR-016 addendum — it needs a real MSIX).
        Assert.False(GistStoragePaths.IsPackaged);
        Assert.Null(GistStoragePaths.CurrentPackageFamilyName);
        Assert.Equal(GistStoragePaths.ForUnpackaged().Root, GistStoragePaths.Resolve().Root);
    }

    [Fact]
    public void ForPackaged_FailsLoudlyWhenThereIsNoPackageIdentity()
    {
        // Never silently falls back to the unpackaged root: that would be the "debug build reads the
        // production store with the wrong key" bug in a different disguise.
        var ex = Assert.Throws<InvalidOperationException>(GistStoragePaths.ForPackaged);
        Assert.Contains("package identity", ex.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Paths_AreStableAcrossInstances()
    {
        var a = GistStoragePaths.ForRoot(_dir);
        var b = GistStoragePaths.ForRoot(_dir);
        Assert.Equal(a.Root, b.Root);
        Assert.Equal(a.DbPath, b.DbPath);
        Assert.Equal(a.StorageDir, b.StorageDir);
        Assert.Equal(a.KeyDir, b.KeyDir);
    }

    [Fact]
    public void TheKeyProviderDefaultFileName_LivesInKeyDir()
    {
        var paths = GistStoragePaths.ForRoot(_dir);
        var provider = new DpapiKeyProvider(paths.KeyDir);
        Assert.Equal(Path.Combine(paths.KeyDir, DpapiKeyProvider.FileName), provider.KeyFilePath);
    }
}
