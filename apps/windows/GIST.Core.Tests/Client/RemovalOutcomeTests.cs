using Gist.Core.Client;
using Gist.Core.Models;
using Gist.Core.Tests.TestSupport;
using Gist.Core.Tests.ViewModels;
using Gist.Core.ViewModels;

namespace Gist.Core.Tests.Client;

/// <summary>
/// Removal outcome, the "locked stored file" warning and the orphan sweep — real <c>GistCore</c>,
/// real SQLite and filesystem on temp dirs, no mocking framework. Windows refuses to delete a file
/// another handle holds open without <c>FileShare.Delete</c>, which is what the locked-file tests
/// use to provoke a genuine failed delete.
/// </summary>
public sealed class RemovalOutcomeTests : IDisposable
{
    private const string SecretTitle = "SecretTitleQuxzy";

    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keyProvider = new();
    private readonly List<IDisposable> _disposables = new();

    public void Dispose()
    {
        for (var i = _disposables.Count - 1; i >= 0; i--)
        {
            _disposables[i].Dispose();
        }

        _workspace.Dispose();
    }

    private async Task<CoreClient> NewReadyClientAsync()
    {
        var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        _disposables.Add(client);
        Assert.True(await client.InitializeAsync());
        await client.WaitForSweepAsync();
        return client;
    }

    private async Task<(string Id, string Original)> ImportAsync(CoreClient client, string stem, string body)
    {
        var path = Path.Combine(_workspace.Root, stem + ".txt");
        File.WriteAllText(path, body);
        var id = await client.ImportFileAsync(path);
        Assert.NotNull(id);
        return (id!, path);
    }

    private string[] FilesOf(string id) =>
        Directory.GetFiles(_workspace.StorageDir)
            .Where(f => Path.GetFileName(f).StartsWith(id, StringComparison.Ordinal))
            .ToArray();

    private string[] AllStorageFiles() =>
        Directory.GetFiles(_workspace.StorageDir, "*", SearchOption.AllDirectories)
            .Select(f => f.ToUpperInvariant()).OrderBy(f => f, StringComparer.Ordinal).ToArray();

    [Fact]
    public async Task Normal_removal_reports_deleted_files_and_no_failures()
    {
        var client = await NewReadyClientAsync();
        var (id, _) = await ImportAsync(client, "plain", "Some readable text.");
        Assert.NotEmpty(FilesOf(id));

        var result = await client.RemoveItemsAsync(new[] { id }, deleteSourceFiles: false);

        Assert.Equal(new[] { id }, result.RemovedIds);
        Assert.True(result.FilesDeleted > 0);
        Assert.Equal(0, result.FilesFailed);
        Assert.Empty(result.FailureKinds);
        Assert.False(result.HasFileFailures);
        Assert.Empty(FilesOf(id));
        Assert.Empty(client.Items);
        Assert.Null(client.LastError);
    }

    /// <summary>
    /// Removal is a complete delete (maintainer decision, 2026-09-21): GIST's own stored copy goes
    /// with the item, every time. The user's file is verified byte-for-byte, not merely present.
    /// </summary>
    [Fact]
    public async Task Removal_deletes_the_stored_copy_and_leaves_the_users_own_file_byte_identical()
    {
        var client = await NewReadyClientAsync();
        var (id, original) = await ImportAsync(client, "complete", "The user's own file.");
        var storedCopy = _workspace.PredictSandboxedCopyPath(original);
        var originalBytes = File.ReadAllBytes(original);
        Assert.True(File.Exists(storedCopy));

        var result = await client.RemoveItemsAsync(new[] { id }, deleteSourceFiles: true);

        Assert.Equal(new[] { id }, result.RemovedIds);
        Assert.Equal(0, result.FilesFailed);
        Assert.Equal(0, result.SharedCopiesKept);
        Assert.Empty(FilesOf(id));
        Assert.False(File.Exists(storedCopy), "the stored copy must go with the item");
        Assert.Equal(originalBytes, File.ReadAllBytes(original));
    }

    /// <summary>
    /// ADR-006's content-addressed dedup, at the client boundary: two items imported from
    /// byte-identical files share one stored copy, so removing the first must keep it — reported as
    /// <see cref="RemoveResult.SharedCopiesKept"/>, which is deliberately <b>not</b> a failure and
    /// <b>not</b> a missing file, so no warning is raised. Removing the second deletes it.
    /// </summary>
    [Fact]
    public async Task A_stored_copy_shared_with_another_item_is_kept_reported_and_never_warns()
    {
        var client = await NewReadyClientAsync();
        const string Shared = "One set of bytes, two library items.";
        var (first, firstFile) = await ImportAsync(client, "sharer-one", Shared);
        var (second, secondFile) = await ImportAsync(client, "sharer-two", Shared);

        var storedCopy = _workspace.PredictSandboxedCopyPath(firstFile);
        Assert.Equal(storedCopy, _workspace.PredictSandboxedCopyPath(secondFile));
        Assert.True(File.Exists(storedCopy));

        var kept = await client.RemoveItemsAsync(new[] { first }, deleteSourceFiles: true);
        Assert.True(File.Exists(storedCopy), "the surviving item still references this copy");
        Assert.Equal(1, kept.SharedCopiesKept);
        Assert.Equal(0, kept.FilesFailed);
        Assert.Equal(0, kept.FilesMissing);
        Assert.False(kept.HasFileFailures, "a deliberately-kept shared copy must raise no warning");
        Assert.Null(RemoveWarning.For(kept));

        var last = await client.RemoveItemsAsync(new[] { second }, deleteSourceFiles: true);
        Assert.Equal(0, last.SharedCopiesKept);
        Assert.Equal(0, last.FilesFailed);
        Assert.False(File.Exists(storedCopy), "the last item sharing it is gone, so the copy goes too");
        Assert.True(File.Exists(firstFile) && File.Exists(secondFile));
    }

    /// <summary>
    /// Both sharers removed in one call: nothing survives to reference the copy, so it is deleted —
    /// once, not once per item, and with no "already gone" noise in the tally.
    /// </summary>
    [Fact]
    public async Task Removing_both_sharers_in_one_call_deletes_the_shared_copy_exactly_once()
    {
        var client = await NewReadyClientAsync();
        const string Shared = "Batch-removed shared bytes.";
        var (first, firstFile) = await ImportAsync(client, "batch-one", Shared);
        var (second, secondFile) = await ImportAsync(client, "batch-two", Shared);
        var storedCopy = _workspace.PredictSandboxedCopyPath(firstFile);

        var result = await client.RemoveItemsAsync(new[] { first, second }, deleteSourceFiles: true);

        Assert.Equal(2, result.RemovedIds.Count);
        Assert.Equal(0, result.SharedCopiesKept);
        Assert.Equal(0, result.FilesFailed);
        Assert.Equal(0, result.FilesMissing);
        Assert.False(File.Exists(storedCopy));
        Assert.Empty(FilesOf(first));
        Assert.Empty(FilesOf(second));
        Assert.True(File.Exists(firstFile) && File.Exists(secondFile));
    }

    [Fact]
    public async Task Pre_ADR013_shaped_item_without_checksum_sidecars_removes_with_no_warning()
    {
        var client = await NewReadyClientAsync();
        var vm = new LibraryViewModel(client, new FakeLibraryScheduler());
        _disposables.Add(vm);
        await vm.LoadAsync();
        var (id, _) = await ImportAsync(client, "legacy", "Imported before sidecars existed.");
        await client.RefreshAsync();

        foreach (var sidecar in FilesOf(id).Where(f => f.EndsWith(".blake3", StringComparison.Ordinal)))
        {
            File.Delete(sidecar);
        }

        vm.SetSelection(new[] { id });
        await vm.RemoveAsync();

        Assert.Null(vm.LastRemoveWarning);
        Assert.Null(vm.LastError);
        Assert.Empty(vm.DisplayedItems);
        Assert.Empty(FilesOf(id));
    }

    [Fact]
    public async Task Locked_stored_file_still_removes_the_row_warns_and_cleans_the_other_files()
    {
        var client = await NewReadyClientAsync();
        var vm = new LibraryViewModel(client, new FakeLibraryScheduler());
        _disposables.Add(vm);
        await vm.LoadAsync();
        var (id, original) = await ImportAsync(client, SecretTitle, "Body that will be locked.");
        await client.RefreshAsync();

        var locked = FilesOf(id).Single(f => f.EndsWith(".tokens.json", StringComparison.Ordinal));
        using var hold = new FileStream(locked, FileMode.Open, FileAccess.Read, FileShare.None);

        vm.SetSelection(new[] { id });
        await vm.RemoveAsync();

        // The row is gone: that is the success. The failed file is only a warning.
        Assert.Empty(vm.DisplayedItems);
        Assert.Empty(client.Items);
        Assert.Null(vm.LastError);
        Assert.Null(client.LastError);
        Assert.Equal(RemoveWarning.Locked, vm.LastRemoveWarning);

        // Everything that could be deleted was; only the locked file remains.
        Assert.Equal(new[] { locked }, FilesOf(id));
        Assert.True(File.Exists(original), "the user's own file must never be touched");

        // Fixed text: never a path or a title.
        Assert.DoesNotContain(_workspace.Root, vm.LastRemoveWarning, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(SecretTitle, vm.LastRemoveWarning, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(id, vm.LastRemoveWarning, StringComparison.OrdinalIgnoreCase);

        // Dismissal clears it.
        vm.DismissRemoveWarning();
        Assert.Null(vm.LastRemoveWarning);
    }

    [Fact]
    public async Task Failed_removal_result_carries_counts_and_kinds_but_no_paths()
    {
        var client = await NewReadyClientAsync();
        var (id, _) = await ImportAsync(client, SecretTitle, "Body.");
        var locked = FilesOf(id).Single(f => f.EndsWith(".json", StringComparison.Ordinal)
                                             && !f.EndsWith(".tokens.json", StringComparison.Ordinal));
        using var hold = new FileStream(locked, FileMode.Open, FileAccess.Read, FileShare.None);

        var result = await client.RemoveItemsAsync(new[] { id }, deleteSourceFiles: false);

        Assert.Equal(new[] { id }, result.RemovedIds);
        Assert.Equal(1, result.FilesFailed);
        Assert.Equal(new[] { FileDeleteFailureKind.Locked }, result.FailureKinds);
        Assert.True(result.FilesDeleted > 0);
        Assert.Null(client.LastError);
    }

    [Fact]
    public async Task Sweep_at_startup_reclaims_the_file_once_the_lock_is_released()
    {
        string lockedPath;
        string id;
        var first = await NewReadyClientAsync();
        (id, _) = await ImportAsync(first, "orphan", "Will be orphaned.");
        lockedPath = FilesOf(id).Single(f => f.EndsWith(".tokens.json", StringComparison.Ordinal));
        var hold = new FileStream(lockedPath, FileMode.Open, FileAccess.Read, FileShare.None);
        try
        {
            await first.RemoveItemsAsync(new[] { id }, deleteSourceFiles: false);
            Assert.True(File.Exists(lockedPath));
        }
        finally
        {
            hold.Dispose();
        }

        first.Dispose();

        // "GIST restarts": a fresh client on the same directory sweeps in the background.
        var second = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        _disposables.Add(second);
        Assert.True(await second.InitializeAsync());
        await second.WaitForSweepAsync();

        Assert.False(File.Exists(lockedPath), "the sweep should reclaim the orphaned file");
        Assert.NotNull(second.LastSweepResult);
        Assert.True(second.LastSweepResult!.FilesDeleted >= 1);
        Assert.Equal(0, second.LastSweepResult.FilesFailed);
    }

    [Fact]
    public async Task Sweep_never_deletes_anything_that_is_referenced()
    {
        var first = await NewReadyClientAsync();
        await ImportAsync(first, "keep-one", "First document text.");
        await ImportAsync(first, "keep-two", "Second document text.");
        var before = AllStorageFiles();
        first.Dispose();

        var second = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        _disposables.Add(second);
        Assert.True(await second.InitializeAsync());
        await second.WaitForSweepAsync();
        await second.RefreshAsync();

        Assert.Equal(before, AllStorageFiles());
        Assert.Equal(2, second.Items.Count);
        Assert.Equal(0, second.LastSweepResult!.FilesDeleted);
    }

    [Fact]
    public async Task Startup_sweep_never_races_with_imports_that_are_in_flight()
    {
        // The sweep and imports share the client's FFI gate, so the sweep cannot observe an import's
        // blobs on disk before its database row exists and mistake them for orphans.
        var seed = await NewReadyClientAsync();
        seed.Dispose();

        for (var round = 0; round < 3; round++)
        {
            var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
            _disposables.Add(client);
            var init = client.InitializeAsync();
            Assert.True(await init);

            var paths = Enumerable.Range(0, 5).Select(i =>
            {
                var p = Path.Combine(_workspace.Root, $"race-{round}-{i}.txt");
                File.WriteAllText(p, $"Document {round}/{i} for the race.");
                return p;
            }).ToArray();

            var ids = await Task.WhenAll(paths.Select(p => client.ImportFileAsync(p)));
            await client.WaitForSweepAsync();

            Assert.All(ids, i => Assert.NotNull(i));
            foreach (var i in ids)
            {
                Assert.NotEmpty(FilesOf(i!));
            }

            client.Dispose();
        }
    }

    [Fact]
    public async Task Failed_removal_schedules_one_sweep_on_the_next_refresh_and_ordinary_removal_does_not()
    {
        var client = await NewReadyClientAsync();
        var startupSweep = client.LastSweepResult;
        Assert.NotNull(startupSweep);

        // Ordinary removal: no extra sweep.
        var (plain, _) = await ImportAsync(client, "ordinary", "Nothing locked here.");
        await client.RemoveItemsAsync(new[] { plain }, deleteSourceFiles: false);
        await client.RefreshAsync();
        await client.WaitForSweepAsync();
        Assert.Same(startupSweep, client.LastSweepResult);

        // Failed removal: the next idle refresh sweeps, and reclaims the file once unlocked.
        var (id, _) = await ImportAsync(client, "stubborn", "Locked for a while.");
        var lockedPath = FilesOf(id).Single(f => f.EndsWith(".tokens.json", StringComparison.Ordinal));
        var hold = new FileStream(lockedPath, FileMode.Open, FileAccess.Read, FileShare.None);
        try
        {
            var result = await client.RemoveItemsAsync(new[] { id }, deleteSourceFiles: false);
            Assert.True(result.HasFileFailures);
        }
        finally
        {
            hold.Dispose();
        }

        await client.RefreshAsync();
        await client.WaitForSweepAsync();

        Assert.NotSame(startupSweep, client.LastSweepResult);
        Assert.False(File.Exists(lockedPath));
    }

    [Fact]
    public async Task Next_user_operation_clears_the_warning()
    {
        var client = await NewReadyClientAsync();
        var vm = new LibraryViewModel(client, new FakeLibraryScheduler());
        _disposables.Add(vm);
        await vm.LoadAsync();
        var (id, _) = await ImportAsync(client, "warned", "Body.");
        await client.RefreshAsync();
        var lockedPath = FilesOf(id).Single(f => f.EndsWith(".tokens.json", StringComparison.Ordinal));
        using (new FileStream(lockedPath, FileMode.Open, FileAccess.Read, FileShare.None))
        {
            vm.SetSelection(new[] { id });
            await vm.RemoveAsync();
        }

        Assert.NotNull(vm.LastRemoveWarning);

        var path = Path.Combine(_workspace.Root, "another.txt");
        File.WriteAllText(path, "Another.");
        await vm.ImportFileAsync(path);

        Assert.Null(vm.LastRemoveWarning);
    }

    [Theory]
    [InlineData(new[] { FileDeleteFailureKind.Locked }, RemoveWarning.Locked)]
    [InlineData(new[] { FileDeleteFailureKind.Permission }, RemoveWarning.Permission)]
    [InlineData(new[] { FileDeleteFailureKind.Other }, RemoveWarning.Other)]
    [InlineData(new[] { FileDeleteFailureKind.Other, FileDeleteFailureKind.Permission }, RemoveWarning.Permission)]
    [InlineData(new[] { FileDeleteFailureKind.Permission, FileDeleteFailureKind.Locked }, RemoveWarning.Locked)]
    public void Warning_wording_follows_the_failure_kind(FileDeleteFailureKind[] kinds, string expected)
    {
        var result = new RemoveResult(new[] { "x" }, 1, 0, kinds.Length, kinds);
        Assert.Equal(expected, RemoveWarning.For(result));
    }

    [Fact]
    public void Missing_files_alone_never_produce_a_warning()
    {
        var result = new RemoveResult(new[] { "x" }, 1, 3, 0, Array.Empty<FileDeleteFailureKind>());
        Assert.Null(RemoveWarning.For(result));
    }
}
