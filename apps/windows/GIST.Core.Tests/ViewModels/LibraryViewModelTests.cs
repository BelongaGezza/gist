using Gist.Core.Client;
using Gist.Core.Filtering;
using Gist.Core.Models;
using Gist.Core.Tests.TestSupport;
using Gist.Core.ViewModels;

namespace Gist.Core.Tests.ViewModels;

/// <summary>
/// Tests for <see cref="LibraryViewModel"/> against a <b>real</b> <c>GistCore</c> — real SQLite,
/// real filesystem, a fresh temp directory per test — exactly as <see cref="Client.CoreClientTests"/>
/// and Apple's <c>GISTTests</c> do. No mocking framework: the only test double is the scheduler,
/// because the alternative is a 300 ms sleep in every search test.
/// </summary>
/// <remarks>
/// Each client is opened the way production opens it (<c>NewWithReadKey</c>, ADR-014), so
/// encrypt-then-read behaves here as it does in the shipped app.
/// </remarks>
public sealed class LibraryViewModelTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keyProvider = new();
    private readonly FakeLibraryScheduler _scheduler = new();
    private readonly List<IDisposable> _disposables = new();

    /// <summary>
    /// The client the view model under test is driving, kept so a few assertions can read the core
    /// directly — the view model has no reason to expose, say, "list another collection's contents".
    /// </summary>
    private CoreClient? _client;

    public void Dispose()
    {
        for (var i = _disposables.Count - 1; i >= 0; i--)
        {
            _disposables[i].Dispose();
        }

        _workspace.Dispose();
    }

    // ── Fixtures ───────────────────────────────────────────────────────────

    private async Task<CoreClient> NewReadyClientAsync()
    {
        var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        _disposables.Add(client);
        Assert.True(await client.InitializeAsync(), "the client should open a fresh store");
        _client = client;
        return client;
    }

    private LibraryViewModel NewViewModel(CoreClient client)
    {
        var vm = new LibraryViewModel(client, _scheduler);
        _disposables.Add(vm);
        return vm;
    }

    private async Task<LibraryViewModel> NewLoadedViewModelAsync()
    {
        var vm = NewViewModel(await NewReadyClientAsync());
        await vm.LoadAsync();
        return vm;
    }

    /// <summary>
    /// Writes a real .txt file in the workspace. The import pipeline takes the file stem as the
    /// title (<c>gist-core</c>'s <c>import_file</c>), so the name chosen here is the title the
    /// sort tests assert on, and the body is what FTS indexes for the search tests.
    /// </summary>
    private string WriteTextFile(string name, string body)
    {
        var path = Path.Combine(_workspace.Root, name);
        File.WriteAllText(path, body);
        return path;
    }

    /// <summary>Runs the debounce out and waits for the search it started.</summary>
    private async Task SettleSearchAsync(LibraryViewModel vm)
    {
        _scheduler.Advance();
        await vm.SearchInFlight;
    }

    // ── Search: debounce (§4.2) ────────────────────────────────────────────

    /// <summary>
    /// Cancel-and-restart, not "two timers": the first wait must be <em>cancelled</em>, and only
    /// the last query may reach the core.
    /// </summary>
    [Fact]
    public async Task Typing_restarts_the_debounce_and_cancels_the_previous_wait()
    {
        var vm = await NewLoadedViewModelAsync();
        await vm.ImportFileAsync(WriteTextFile("alpha.txt", "Zebrafish swim quietly."));
        await vm.ImportFileAsync(WriteTextFile("beta.txt", "Kangaroos hop loudly."));

        vm.SearchText = "zebrafish";
        vm.SearchText = "kangaroos";

        Assert.Equal(2, _scheduler.RequestedCount);
        Assert.Equal(1, _scheduler.CanceledCount);
        Assert.Equal(LibraryViewModel.SearchDebounce, _scheduler.RequestedDelays[0]);

        await SettleSearchAsync(vm);

        Assert.Equal(1, _scheduler.CompletedCount);
        var hit = Assert.Single(vm.DisplayedItems);
        Assert.Equal("beta", hit.Title);
    }

    /// <summary>
    /// A blank or whitespace-only query clears the results <b>without touching the core</b>
    /// (§4.2, Apple parity). Proved against a client that was never initialised: a query that
    /// reached the FFI path would publish <see cref="CoreErrorKind.NotInitialized"/>, and a blank
    /// one must not.
    /// </summary>
    [Fact]
    public async Task A_blank_query_clears_results_without_an_FFI_call()
    {
        var unopened = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        _disposables.Add(unopened);
        var vm = NewViewModel(unopened);

        vm.SearchText = "   ";
        await SettleSearchAsync(vm);

        Assert.Null(unopened.LastError);
        Assert.Null(vm.LastError);
        Assert.Equal(LibraryDialog.None, vm.PendingDialog);
        Assert.Empty(unopened.SearchResults);
        Assert.False(vm.IsSearchActive);

        // Contrast: a non-blank query does go to the client, which reports it has no core.
        vm.SearchText = "anything";
        await SettleSearchAsync(vm);

        Assert.NotNull(vm.LastError);
        Assert.Equal(CoreErrorKind.NotInitialized, vm.LastError!.Kind);
        Assert.Equal(LibraryDialog.Error, vm.PendingDialog);
    }

    // ── Search / tag filter mutual exclusion (§4.2) ────────────────────────

    [Fact]
    public async Task Starting_a_search_clears_an_active_tag_filter()
    {
        var vm = await NewLoadedViewModelAsync();
        var id = await vm.ImportFileAsync(WriteTextFile("tagged.txt", "Some content."));
        Assert.True(await vm.AddTagAsync(id!, "history"));

        vm.TagFilter = "history";
        await vm.TagFilterInFlight;
        Assert.Single(vm.DisplayedItems);

        vm.SearchText = "content";

        Assert.Null(vm.TagFilter);
        Assert.True(vm.IsSearchActive);

        await SettleSearchAsync(vm);
        Assert.Single(vm.DisplayedItems);
    }

    [Fact]
    public async Task Choosing_a_tag_filter_clears_an_active_search_and_cancels_its_debounce()
    {
        var vm = await NewLoadedViewModelAsync();
        var id = await vm.ImportFileAsync(WriteTextFile("tagged.txt", "Some content."));
        Assert.True(await vm.AddTagAsync(id!, "history"));

        vm.SearchText = "content";
        Assert.True(vm.IsSearchActive);

        vm.TagFilter = "history";
        await vm.TagFilterInFlight;

        Assert.Equal(string.Empty, vm.SearchText);
        Assert.False(vm.IsSearchActive);
        Assert.Equal("history", vm.TagFilter);
        Assert.Equal(1, _scheduler.CanceledCount);

        // The pending debounce was cancelled, so advancing it now must not resurrect the search.
        await SettleSearchAsync(vm);
        Assert.Single(vm.DisplayedItems);
        Assert.Equal("history", vm.TagFilter);
    }

    [Fact]
    public async Task Clearing_the_tag_filter_returns_the_whole_library()
    {
        var vm = await NewLoadedViewModelAsync();
        var tagged = await vm.ImportFileAsync(WriteTextFile("tagged.txt", "One."));
        await vm.ImportFileAsync(WriteTextFile("untagged.txt", "Two."));
        Assert.True(await vm.AddTagAsync(tagged!, "history"));

        vm.TagFilter = "history";
        await vm.TagFilterInFlight;
        Assert.Single(vm.DisplayedItems);

        vm.TagFilter = null;
        await vm.TagFilterInFlight;

        Assert.Equal(2, vm.DisplayedItems.Count);
        Assert.Equal(LibraryEmptyState.None, vm.EmptyState);
    }

    // ── Sort (§4.3) ────────────────────────────────────────────────────────

    /// <summary>
    /// All five orders, driven through the view model rather than
    /// <see cref="LibraryFiltering.Sorted"/> directly, so the wiring is covered too. "alpha" is
    /// imported first, so the core's newest-first order is ["zulu", "alpha"].
    /// </summary>
    [Fact]
    public async Task Sort_order_reorders_the_displayed_items()
    {
        var vm = await NewLoadedViewModelAsync();
        await vm.ImportFileAsync(WriteTextFile("alpha.txt", "First."));
        await vm.ImportFileAsync(WriteTextFile("zulu.txt", "Second."));

        static string[] Titles(LibraryViewModel vm) => vm.DisplayedItems.Select(i => i.Title).ToArray();

        vm.SortOrder = LibrarySortOrder.DateAddedNewest;
        Assert.Equal(new[] { "zulu", "alpha" }, Titles(vm));

        vm.SortOrder = LibrarySortOrder.DateAddedOldest;
        Assert.Equal(new[] { "alpha", "zulu" }, Titles(vm));

        vm.SortOrder = LibrarySortOrder.TitleAZ;
        Assert.Equal(new[] { "alpha", "zulu" }, Titles(vm));

        vm.SortOrder = LibrarySortOrder.TitleZA;
        Assert.Equal(new[] { "zulu", "alpha" }, Titles(vm));

        // Neither has an author, so the stable sort leaves the core's order alone.
        vm.SortOrder = LibrarySortOrder.AuthorAZ;
        Assert.Equal(new[] { "zulu", "alpha" }, Titles(vm));
    }

    [Fact]
    public async Task Sort_order_change_notifies_the_displayed_items()
    {
        var vm = await NewLoadedViewModelAsync();
        await vm.ImportFileAsync(WriteTextFile("alpha.txt", "First."));

        var seen = new List<string?>();
        vm.PropertyChanged += (_, e) => seen.Add(e.PropertyName);

        vm.SortOrder = LibrarySortOrder.TitleAZ;

        Assert.Contains(nameof(LibraryViewModel.SortOrder), seen);
        Assert.Contains(nameof(LibraryViewModel.DisplayedItems), seen);
    }

    // ── Command enablement (§4.1 command table) ────────────────────────────

    [Fact]
    public async Task Command_enablement_follows_the_selection_count()
    {
        var vm = await NewLoadedViewModelAsync();
        var a = await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));
        var b = await vm.ImportFileAsync(WriteTextFile("b.txt", "Two."));
        var c = await vm.ImportFileAsync(WriteTextFile("c.txt", "Three."));

        // Nothing selected: every selection command is off.
        Assert.Equal(0, vm.SelectionCount);
        Assert.False(vm.CanAddToCollection);
        Assert.False(vm.CanEncrypt);
        Assert.False(vm.CanRemove);
        Assert.False(vm.CanOpen);
        Assert.False(vm.CanEditTags);
        Assert.Null(vm.SingleSelectedItem);

        // Exactly one: everything is available.
        vm.SetSelection(new[] { a! });
        Assert.True(vm.CanAddToCollection);
        Assert.True(vm.CanEncrypt);
        Assert.True(vm.CanRemove);
        Assert.True(vm.CanOpen);
        Assert.True(vm.CanEditTags);
        Assert.Equal(a, vm.SingleSelectedItem!.Id);

        // Many: the bulk commands stay on, the single-item ones go off.
        vm.SetSelection(new[] { a!, b!, c! });
        Assert.Equal(3, vm.SelectionCount);
        Assert.True(vm.CanAddToCollection);
        Assert.True(vm.CanEncrypt);
        Assert.True(vm.CanRemove);
        Assert.False(vm.CanOpen);
        Assert.False(vm.CanEditTags);
        Assert.Null(vm.SingleSelectedItem);
    }

    [Fact]
    public async Task SetSelection_drops_duplicates_and_resolves_only_ids_that_are_displayed()
    {
        var vm = await NewLoadedViewModelAsync();
        var a = await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));

        vm.SetSelection(new[] { a!, a!, "ghost-id", string.Empty });

        Assert.Equal(new[] { a!, "ghost-id" }, vm.SelectedIds);
        var resolved = Assert.Single(vm.SelectedItems);
        Assert.Equal(a, resolved.Id);
    }

    [Fact]
    public async Task Import_commands_are_disabled_while_a_long_operation_runs()
    {
        var vm = await NewLoadedViewModelAsync();
        Assert.True(vm.IsImportEnabled);

        var busyStates = new List<bool>();
        vm.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(LibraryViewModel.IsBusy))
            {
                busyStates.Add(vm.IsBusy);
            }
        };

        await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));

        Assert.Equal(new[] { true, false }, busyStates);
        Assert.True(vm.IsImportEnabled);
    }

    // ── Empty states (§4.4) ────────────────────────────────────────────────

    [Fact]
    public async Task An_empty_library_shows_the_no_items_state()
    {
        var vm = await NewLoadedViewModelAsync();

        Assert.Equal(LibraryEmptyState.NoItems, vm.EmptyState);
        Assert.Equal("No books yet", vm.EmptyTitle);
        Assert.Equal("Import a .txt, .epub, or .docx file to get started", vm.EmptyMessage);
    }

    [Fact]
    public async Task A_search_with_no_hits_shows_the_no_matches_state_quoting_the_query()
    {
        var vm = await NewLoadedViewModelAsync();
        await vm.ImportFileAsync(WriteTextFile("a.txt", "Ordinary prose."));

        vm.SearchText = "zzzznotpresent";
        await SettleSearchAsync(vm);

        Assert.Empty(vm.DisplayedItems);
        Assert.Equal(LibraryEmptyState.NoSearchHits, vm.EmptyState);
        Assert.Equal("No matches", vm.EmptyTitle);
        Assert.Equal("No items match \"zzzznotpresent\"", vm.EmptyMessage);
    }

    [Fact]
    public async Task A_tag_filter_with_no_hits_shows_the_no_items_tagged_state()
    {
        var vm = await NewLoadedViewModelAsync();
        await vm.ImportFileAsync(WriteTextFile("a.txt", "Ordinary prose."));

        vm.TagFilter = "ghost-tag";
        await vm.TagFilterInFlight;

        Assert.Empty(vm.DisplayedItems);
        Assert.Equal(LibraryEmptyState.NoTagHits, vm.EmptyState);
        Assert.Equal("No items", vm.EmptyTitle);
        Assert.Equal("No items are tagged \"ghost-tag\"", vm.EmptyMessage);
    }

    [Fact]
    public async Task No_empty_state_is_shown_while_the_list_is_still_loading()
    {
        var vm = await NewLoadedViewModelAsync();
        Assert.Equal(LibraryEmptyState.NoItems, vm.EmptyState);

        var seen = new List<LibraryEmptyState>();
        vm.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(LibraryViewModel.IsLoading) && vm.IsLoading)
            {
                seen.Add(vm.EmptyState);
            }
        };

        await vm.LoadAsync();

        Assert.Equal(new[] { LibraryEmptyState.None }, seen);
    }

    // ── Remove (§4.5, §10 item 1) ──────────────────────────────────────────

    [Fact]
    public async Task Remove_preview_describes_the_current_selection()
    {
        var vm = await NewLoadedViewModelAsync();
        var a = await vm.ImportFileAsync(WriteTextFile("alpha.txt", "One."));
        var b = await vm.ImportFileAsync(WriteTextFile("beta.txt", "Two."));
        await vm.ImportFileAsync(WriteTextFile("gamma.txt", "Three."));

        vm.SetSelection(new[] { a!, b! });
        var preview = vm.RemovePreview();

        Assert.Equal(2, preview.Count);
        Assert.Equal("Remove 2 items?", preview.Title);
        Assert.True(preview.AllowsDeletingStoredCopy);
        Assert.Equal(2, preview.Titles.Count);
        Assert.Contains("alpha", preview.Titles);
        Assert.Contains("beta", preview.Titles);
        Assert.Null(preview.MoreText);
    }

    /// <summary>
    /// The security-relevant one (spec §10 item 1, ADR-006): "Remove from Library" leaves GIST's
    /// stored copy alone, "Also Delete Stored Copy" deletes it — and <b>neither</b> touches the
    /// file the user imported from. The stored copy's path is recomputed independently rather than
    /// asked of the core, so this asserts against what is genuinely on disk.
    /// </summary>
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task Remove_deletes_the_stored_copy_only_when_asked_and_never_the_original(
        bool deleteStoredCopy)
    {
        var vm = await NewLoadedViewModelAsync();
        var original = WriteTextFile("keepme.txt", "The user's own file.");
        var id = await vm.ImportFileAsync(original);
        Assert.NotNull(id);

        var storedCopy = _workspace.PredictSandboxedCopyPath(original);
        Assert.True(File.Exists(storedCopy), "copy-on-import should have written a stored copy");

        vm.SetSelection(new[] { id! });
        await vm.RemoveAsync(deleteStoredCopy);

        Assert.Empty(vm.DisplayedItems);
        Assert.Empty(vm.SelectedIds);
        Assert.Null(vm.LastError);
        Assert.Equal(deleteStoredCopy, !File.Exists(storedCopy));

        // The invariant that matters either way.
        Assert.True(File.Exists(original), "GIST must never delete the user's own file");
        Assert.Equal("The user's own file.", File.ReadAllText(original));
    }

    [Fact]
    public async Task Remove_with_nothing_selected_does_nothing()
    {
        var vm = await NewLoadedViewModelAsync();
        await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));

        await vm.RemoveAsync(deleteStoredCopy: true);

        Assert.Single(vm.DisplayedItems);
        Assert.Null(vm.LastError);
    }

    // ── Import errors (§4.5, §10 item 4) ───────────────────────────────────

    /// <summary>
    /// DRM must surface as its own dialog, matched by error <i>kind</i>, and — the regression this
    /// test exists for — it must still be there after the list reload that follows an import.
    /// Apple's client clears its error on a successful refresh, so a view model that mirrored the
    /// live error instead of latching it would dismiss the dialog before anyone saw it. That bug
    /// was found during W1; this is the guard against it recurring.
    /// </summary>
    [Fact]
    public async Task A_DRM_protected_import_raises_the_DRM_dialog_and_survives_a_refresh()
    {
        var vm = await NewLoadedViewModelAsync();
        var fixture = _workspace.CopyFixture("drm_protected.epub");

        var id = await vm.ImportFileAsync(fixture);

        Assert.Null(id);
        Assert.Equal(LibraryDialog.DrmProtected, vm.PendingDialog);
        Assert.Equal(fixture, vm.LastDrmFile);
        Assert.Equal(CoreErrorKind.DrmProtected, vm.LastError!.Kind);
        Assert.DoesNotContain(fixture, vm.LastError.Message, StringComparison.Ordinal);

        await vm.LoadAsync();

        Assert.Equal(LibraryDialog.DrmProtected, vm.PendingDialog);
        Assert.Equal(fixture, vm.LastDrmFile);
        Assert.Equal(CoreErrorKind.DrmProtected, vm.LastError!.Kind);

        // Only an explicit dismissal clears it — here and in the client.
        await vm.DismissDialogAsync();

        Assert.Equal(LibraryDialog.None, vm.PendingDialog);
        Assert.Null(vm.LastDrmFile);
        Assert.Null(vm.LastError);
    }

    [Fact]
    public async Task A_generic_import_failure_raises_the_error_dialog_with_a_typed_error()
    {
        var vm = await NewLoadedViewModelAsync();

        var id = await vm.ImportUrlAsync("http://example.com/article");

        Assert.Null(id);
        Assert.Equal(LibraryDialog.Error, vm.PendingDialog);
        Assert.Equal(CoreErrorKind.Core, vm.LastError!.Kind);
        Assert.Null(vm.LastDrmFile);
    }

    [Fact]
    public async Task A_blank_url_is_rejected_without_an_FFI_call()
    {
        var vm = await NewLoadedViewModelAsync();

        Assert.Null(await vm.ImportUrlAsync("   "));

        Assert.Null(vm.LastError);
        Assert.Equal(LibraryDialog.None, vm.PendingDialog);
        Assert.Empty(vm.DisplayedItems);
    }

    [Fact]
    public async Task A_blank_path_is_rejected_without_an_FFI_call()
    {
        var vm = await NewLoadedViewModelAsync();

        Assert.Null(await vm.ImportFileAsync(" "));

        Assert.Null(vm.LastError);
        Assert.Equal(LibraryDialog.None, vm.PendingDialog);
    }

    // ── Encrypt (§4.5, ADR-014) ────────────────────────────────────────────

    [Fact]
    public async Task Encrypt_reports_one_encrypted_then_one_already_encrypted()
    {
        var vm = await NewLoadedViewModelAsync();
        var id = await vm.ImportFileAsync(WriteTextFile("secret.txt", "Private reading."));
        vm.SetSelection(new[] { id! });

        var first = await vm.EncryptAsync();

        Assert.Equal(1, first.EncryptedCount);
        Assert.Equal(0, first.AlreadyEncryptedCount);
        Assert.Equal(0, first.FailedCount);
        Assert.Equal(LibraryDialog.EncryptResult, vm.PendingDialog);
        Assert.Equal(first, vm.LastEncryptSummary);
        Assert.True(vm.DisplayedItems.Single().ContentEncrypted);

        var second = await vm.EncryptAsync();

        Assert.Equal(0, second.EncryptedCount);
        Assert.Equal(1, second.AlreadyEncryptedCount);
        Assert.Equal(0, second.FailedCount);
        Assert.Contains("already encrypted", second.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task Encrypt_never_touches_the_stored_copy_or_the_original()
    {
        var vm = await NewLoadedViewModelAsync();
        var original = WriteTextFile("secret.txt", "Private reading.");
        var id = await vm.ImportFileAsync(original);
        var storedCopy = _workspace.PredictSandboxedCopyPath(original);
        var storedBytes = await File.ReadAllBytesAsync(storedCopy);

        vm.SetSelection(new[] { id! });
        await vm.EncryptAsync();

        Assert.True(File.Exists(original));
        Assert.Equal("Private reading.", File.ReadAllText(original));
        Assert.True(File.Exists(storedCopy));
        Assert.Equal(storedBytes, await File.ReadAllBytesAsync(storedCopy));
    }

    // ── Collections (§4.1 command 5) ───────────────────────────────────────

    [Fact]
    public async Task Creating_a_collection_adds_the_current_selection_to_it()
    {
        var vm = await NewLoadedViewModelAsync();
        var a = await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));
        var b = await vm.ImportFileAsync(WriteTextFile("b.txt", "Two."));
        await vm.ImportFileAsync(WriteTextFile("c.txt", "Three."));
        vm.SetSelection(new[] { a!, b! });

        var created = await vm.CreateCollectionAndAddAsync("  Reading List  ");

        Assert.NotNull(created);
        Assert.Equal("Reading List", created!.Name);
        Assert.Contains(vm.Collections, c => c.Id == created.Id);
        Assert.Null(vm.LastError);

        var contents = await _client!.ListItemsInCollectionAsync(created.Id);
        Assert.Equal(2, contents.Count);
    }

    [Fact]
    public async Task Adding_to_an_existing_collection_uses_the_current_selection()
    {
        var vm = await NewLoadedViewModelAsync();
        var a = await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));
        var b = await vm.ImportFileAsync(WriteTextFile("b.txt", "Two."));

        vm.SetSelection(new[] { a! });
        var collection = await vm.CreateCollectionAndAddAsync("Later");
        Assert.NotNull(collection);

        vm.SetSelection(new[] { b! });
        await vm.AddToCollectionAsync(collection!);

        var contents = await _client!.ListItemsInCollectionAsync(collection!.Id);
        Assert.Equal(2, contents.Count);
    }

    [Fact]
    public async Task A_blank_collection_name_is_rejected_without_an_FFI_call()
    {
        var vm = await NewLoadedViewModelAsync();
        await vm.LoadAsync();

        Assert.Null(await vm.CreateCollectionAndAddAsync("   "));

        Assert.Empty(vm.Collections);
        Assert.Null(vm.LastError);
    }

    // ── Tags (§6) ──────────────────────────────────────────────────────────

    [Fact]
    public async Task Tags_are_trimmed_added_listed_and_removed()
    {
        var vm = await NewLoadedViewModelAsync();
        var id = await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));

        Assert.True(await vm.AddTagAsync(id!, "  science fiction  "));

        var tags = await vm.TagsForAsync(id!);
        Assert.Equal(new[] { "science fiction" }, tags);

        // The Filter menu's vocabulary is refreshed by the same call (§6).
        Assert.Contains("science fiction", vm.AllTags);

        Assert.True(await vm.RemoveTagAsync(id!, "science fiction"));
        Assert.Empty(await vm.TagsForAsync(id!));

        // Documented current core behaviour, not an assertion that it is ideal: `remove_tag`
        // deletes the item↔tag row but leaves the `tags` row, and `list_all_tags` reads that table
        // directly, so a tag nobody uses any more stays in the Filter menu (and selecting it shows
        // the "No items are tagged" empty state). Same on Apple; flagged for the lead as a
        // shared Rust-side question rather than papered over here.
        Assert.Contains("science fiction", vm.AllTags);
    }

    [Fact]
    public async Task A_blank_tag_is_rejected_without_an_FFI_call()
    {
        var vm = await NewLoadedViewModelAsync();
        var id = await vm.ImportFileAsync(WriteTextFile("a.txt", "One."));

        Assert.False(await vm.AddTagAsync(id!, "   "));
        Assert.False(await vm.RemoveTagAsync(id!, "\t"));

        Assert.Empty(await vm.TagsForAsync(id!));
        Assert.Null(vm.LastError);
    }

    // ── Dialog requests ────────────────────────────────────────────────────

    [Fact]
    public async Task Requesting_and_dismissing_a_plain_dialog_leaves_the_error_surface_alone()
    {
        var vm = await NewLoadedViewModelAsync();

        vm.RequestDialog(LibraryDialog.ImportUrl);
        Assert.Equal(LibraryDialog.ImportUrl, vm.PendingDialog);

        await vm.DismissDialogAsync();
        Assert.Equal(LibraryDialog.None, vm.PendingDialog);
        Assert.Null(vm.LastError);
    }
}
