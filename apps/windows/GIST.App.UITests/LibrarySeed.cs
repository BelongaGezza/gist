using System.Security.Cryptography;
using Gist.Core.Client;
using Gist.Core.Keys;
using Gist.Core.Storage;

namespace Gist.App.UITests;

/// <summary>One seeded library item: what the test needs to find it in the UI and on disk.</summary>
public sealed record SeededItem(string Id, string Title, string Authors, string SourcePath, string UniqueWord, string[] Tags);

/// <summary>The result of <see cref="LibrarySeed.SeedAsync"/>.</summary>
public sealed record SeededLibrary(IReadOnlyList<SeededItem> Items, string CollectionName, string UserFilesDir)
{
    public SeededItem ByFile(string fileName) => Items.Single(i => Path.GetFileName(i.SourcePath) == fileName);
}

/// <summary>
/// Seeds a scratch GIST root in-process through GIST.Core (real DPAPI key, real SQLite store) so the UI tests start
/// from a known, multi-item library. The user's own files live in <c>&lt;root&gt;/user-files</c>, deliberately outside the
/// storage directory, so "the original is never touched" is a meaningful assertion.
/// </summary>
public static class LibrarySeed
{
    public const string CollectionName = "Favourites";

    // (file name, unique word, tags). Imported oldest to newest in this order.
    private static readonly (string File, string Word, string[] Tags)[] TextFiles =
    {
        ("zebra-notes.txt", "quokka", new[] { "fiction" }),
        ("apple-orchard.txt", "pomegranate", new[] { "fiction", "garden" }),
        ("mango-diary.txt", "persimmon", new[] { "reference" }),
    };

    private static readonly string[] EpubFiles = { "minimal_valid.epub", "multi_chapter.epub" };

    public static async Task<SeededLibrary> SeedAsync(string root)
    {
        var paths = GistStoragePaths.ForRoot(root);
        paths.EnsureCreated();
        var userFiles = Path.Combine(root, "user-files");
        Directory.CreateDirectory(userFiles);

        using var core = new CoreClient(new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir)));
        Assert.True(await core.InitializeAsync(), $"seed init failed: {core.State}");

        var seeded = new List<SeededItem>();
        foreach (var (file, word, tags) in TextFiles)
        {
            var path = Path.Combine(userFiles, file);
            await File.WriteAllTextAsync(path,
                $"The {word} appears once in this document, along with filler prose about {Path.GetFileNameWithoutExtension(file)}.\n"
                + "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n");
            seeded.Add(await ImportAsync(core, path, word, tags));
        }

        foreach (var epub in EpubFiles)
        {
            var path = Path.Combine(userFiles, epub);
            File.Copy(Path.Combine(FixturesDir(), "epub", epub), path);
            seeded.Add(await ImportAsync(core, path, "", Array.Empty<string>()));
        }

        var collectionId = await core.CreateCollectionAsync(CollectionName);
        Assert.NotNull(collectionId);
        await core.AddItemToCollectionAsync(seeded[0].Id, collectionId!);
        return new SeededLibrary(seeded, CollectionName, userFiles);
    }

    private static async Task<SeededItem> ImportAsync(CoreClient core, string path, string word, string[] tags)
    {
        var id = await core.ImportFileAsync(path);
        Assert.NotNull(id);
        foreach (var t in tags) await core.AddTagAsync(id!, t);
        await core.RefreshAsync();
        var item = core.Items.Single(i => i.Id == id);
        // created_at has one-second resolution in the store; a real gap makes "date added" order deterministic.
        await Task.Delay(1100);
        return new SeededItem(id!, item.Title, string.Join(", ", item.Authors), path, word, tags);
    }

    private static string FixturesDir()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            var candidate = Path.Combine(dir.FullName, "fixtures");
            if (Directory.Exists(Path.Combine(candidate, "epub"))) return candidate;
        }
        throw new DirectoryNotFoundException("fixtures/ not found above the test binaries.");
    }

    /// <summary>Opens the scratch store for verification of DB/disk effects (call after the app has exited).</summary>
    public static async Task<CoreClient> OpenAsync(string root)
    {
        var paths = GistStoragePaths.ForRoot(root);
        // Right after the app exits, Windows can briefly still hold the SQLite files (handle teardown / AV scan),
        // which shows up as StoreUnavailable; that is transient, so retry for a few seconds before failing.
        for (var attempt = 0; ; attempt++)
        {
            var core = new CoreClient(new CoreClientOptions(paths.DbPath, paths.StorageDir, new DpapiKeyProvider(paths.KeyDir)));
            if (await core.InitializeAsync())
            {
                await core.RefreshAsync();
                return core;
            }
            var state = core.State;
            core.Dispose();
            Assert.True(attempt < 20 && state == CoreClientState.StoreUnavailable, $"verify init failed: {state}");
            await Task.Delay(500);
        }
    }

    /// <summary>Where GIST's ADR-006 sandboxed copy of <paramref name="sourcePath"/> lives (content-addressed).</summary>
    public static string PredictSandboxedCopyPath(string root, string sourcePath)
    {
        var hash = Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(sourcePath)));
        var ext = Path.GetExtension(sourcePath).TrimStart('.').ToLowerInvariant();
        return Path.Combine(GistStoragePaths.ForRoot(root).StorageDir, "originals", ext.Length == 0 ? hash : $"{hash}.{ext}");
    }
}
