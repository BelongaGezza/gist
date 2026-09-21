using Gist.Core.Client;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Client;

/// <summary>
/// Real (not mocked) integration tests for <see cref="CoreClient"/>, against a real
/// <c>GistCore</c> — real SQLite, real filesystem — on a fresh temp directory per test. Ported
/// case-for-case from <c>apps/apple/Tests/GISTTests.swift</c> so the two shells are held to the
/// same behaviour (docs/windows-development-plan.md §4.4).
/// </summary>
/// <remarks>
/// Every client here is opened with <c>GistCore.NewWithReadKey</c> (ADR-014), which is exactly how
/// production opens it: plaintext imports by default, but read-decrypt capability so an item the
/// user encrypts stays readable. There is no keyless variant to test, because there is no keyless
/// variant to ship.
/// </remarks>
public sealed class CoreClientTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keyProvider = new();

    public void Dispose() => _workspace.Dispose();

    private async Task<CoreClient> NewReadyClientAsync()
    {
        var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        Assert.True(await client.InitializeAsync(), "the client should open a fresh store");
        Assert.Equal(CoreClientState.Ready, client.State);
        Assert.True(client.IsReady);
        return client;
    }

    // ── Import + refresh ───────────────────────────────────────────────────

    [Fact]
    public async Task ImportFile_appears_in_items_after_refresh()
    {
        using var client = await NewReadyClientAsync();
        Assert.Empty(client.Items);

        var fixture = _workspace.CopyFixture("basic_ascii.txt");
        var id = await client.ImportFileAsync(fixture);

        Assert.NotNull(id);
        Assert.Null(client.LastError);
        var item = Assert.Single(client.Items);
        Assert.Equal(id, item.Id);
        Assert.Equal(fixture, item.SourcePath);
        Assert.False(item.ContentEncrypted);
        Assert.False(string.IsNullOrWhiteSpace(item.Title));
    }

    [Fact]
    public async Task ImportFile_with_a_missing_path_reports_a_typed_error_without_calling_the_core()
    {
        using var client = await NewReadyClientAsync();

        var id = await client.ImportFileAsync(Path.Combine(_workspace.Root, "not-here.txt"));

        Assert.Null(id);
        Assert.NotNull(client.LastError);
        Assert.Equal(CoreErrorKind.SourceFileMissing, client.LastError!.Kind);
        Assert.Empty(client.Items);
    }

    /// <summary>
    /// DRM must be recognised by exception <b>type</b> (<c>GistException.DrmProtected</c>), never by
    /// matching message text — ui-spec §10 item 4 / ADR-004.
    /// </summary>
    [Fact]
    public async Task ImportFile_of_a_DRM_protected_epub_reports_DrmProtected_by_type()
    {
        using var client = await NewReadyClientAsync();
        var fixture = _workspace.CopyFixture("drm_protected.epub");

        var id = await client.ImportFileAsync(fixture);

        Assert.Null(id);
        Assert.NotNull(client.LastError);
        Assert.Equal(CoreErrorKind.DrmProtected, client.LastError!.Kind);
        Assert.Equal(fixture, client.DrmProtectedSource);
        Assert.Empty(client.Items);

        // The path is carried on its own property, never folded into the presentable message.
        Assert.DoesNotContain(fixture, client.LastError.Message, StringComparison.Ordinal);
    }

    /// <summary>
    /// A non-HTTPS URL is rejected by <c>gist-web</c> before any network access (ADR-005/F14), so
    /// this exercises the URL-import error path without a live request.
    /// </summary>
    [Fact]
    public async Task ImportUrl_rejects_a_non_https_url_as_a_typed_core_error()
    {
        using var client = await NewReadyClientAsync();

        var id = await client.ImportUrlAsync("http://example.com/article");

        Assert.Null(id);
        Assert.NotNull(client.LastError);
        Assert.Equal(CoreErrorKind.Core, client.LastError!.Kind);
        Assert.Empty(client.Items);
    }

    // ── Search ─────────────────────────────────────────────────────────────

    [Fact]
    public async Task Search_finds_an_imported_item_by_its_content()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));

        // "adipiscing" appears in basic_ascii.txt's first paragraph.
        await client.SearchAsync("adipiscing");

        Assert.Null(client.LastError);
        var hit = Assert.Single(client.SearchResults);
        Assert.Equal(client.Items[0].Id, hit.Id);
    }

    /// <summary>
    /// Regression guard for F19's follow-up bug: escaping the query as an FTS5 phrase broke
    /// search-as-you-type until a literal prefix <c>*</c> was appended, so a partial word found
    /// nothing. Apple found this by hand; here it is a test.
    /// </summary>
    [Fact]
    public async Task Search_matches_a_partial_word_as_a_prefix()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));

        await client.SearchAsync("adipis");

        Assert.Null(client.LastError);
        Assert.Single(client.SearchResults);
    }

    [Fact]
    public async Task Search_with_hostile_FTS5_syntax_does_not_fail()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));

        await client.SearchAsync("\"foo* OR (");

        Assert.Null(client.LastError);
    }

    [Fact]
    public async Task Search_with_a_whitespace_query_clears_results()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        await client.SearchAsync("adipiscing");
        Assert.NotEmpty(client.SearchResults);

        await client.SearchAsync("   ");

        Assert.Empty(client.SearchResults);
        Assert.Null(client.LastError);
    }

    /// <summary>
    /// Proves the blank-query path returns <em>before</em> touching the core, rather than merely
    /// producing an empty result: the client here has no core at all, so any FFI attempt would
    /// publish a <see cref="CoreErrorKind.NotInitialized"/> error. Nothing is published.
    /// </summary>
    [Fact]
    public async Task Search_with_a_blank_query_never_reaches_the_core()
    {
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        Assert.Equal(CoreClientState.Uninitialized, client.State);

        await client.SearchAsync("");
        await client.SearchAsync("   ");
        await client.SearchAsync(null);

        Assert.Empty(client.SearchResults);
        Assert.Null(client.LastError);
        Assert.Equal(0, _keyProvider.CallCount);
    }

    // ── Removal (ADR-006: the user's own file is never touched) ─────────────

    [Fact]
    public async Task RemoveItems_with_deleteSourceFiles_deletes_the_sandboxed_copy_but_never_the_original()
    {
        using var client = await NewReadyClientAsync();
        var fixture = _workspace.CopyFixture("basic_ascii.txt");
        await client.ImportFileAsync(fixture);
        var item = Assert.Single(client.Items);

        var copyPath = _workspace.PredictSandboxedCopyPath(fixture);
        Assert.True(File.Exists(copyPath), "the ADR-006 sandboxed copy should exist after import");

        await client.RemoveItemsAsync(new[] { item.Id }, deleteSourceFiles: true);

        Assert.Null(client.LastError);
        Assert.Empty(client.Items);
        Assert.False(File.Exists(copyPath), "deleteSourceFiles: true should delete GIST's own copy");

        // The security-relevant assertion: GIST must NEVER delete the user's real file, whatever
        // deleteSourceFiles says. A failure here is a regression against ADR-006 / finding A5.
        Assert.True(File.Exists(fixture), "the user's original file must never be touched");
    }

    [Fact]
    public async Task RemoveItems_without_deleteSourceFiles_keeps_both_the_sandboxed_copy_and_the_original()
    {
        using var client = await NewReadyClientAsync();
        var fixture = _workspace.CopyFixture("basic_ascii.txt");
        await client.ImportFileAsync(fixture);
        var item = Assert.Single(client.Items);
        var copyPath = _workspace.PredictSandboxedCopyPath(fixture);

        await client.RemoveItemsAsync(new[] { item.Id }, deleteSourceFiles: false);

        Assert.Null(client.LastError);
        Assert.Empty(client.Items);
        Assert.True(File.Exists(copyPath), "deleteSourceFiles: false should leave GIST's copy in place");
        Assert.True(File.Exists(fixture), "the user's original file must never be touched");
    }

    [Fact]
    public async Task RemoveItems_with_no_ids_is_a_no_op()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));

        await client.RemoveItemsAsync(Array.Empty<string>(), deleteSourceFiles: true);

        Assert.Single(client.Items);
        Assert.Null(client.LastError);
    }

    // ── Collections ────────────────────────────────────────────────────────

    [Fact]
    public async Task Collections_round_trip()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        var item = Assert.Single(client.Items);

        Assert.Empty(client.Collections);
        var collectionId = await client.CreateCollectionAsync("Test Collection");

        Assert.NotNull(collectionId);
        Assert.Null(client.LastError);
        var collection = Assert.Single(client.Collections);
        Assert.Equal(collectionId, collection.Id);
        Assert.Equal("Test Collection", collection.Name);

        await client.AddItemToCollectionAsync(item.Id, collection.Id);
        Assert.Null(client.LastError);
        var contents = await client.ListItemsInCollectionAsync(collection.Id);
        Assert.Equal(new[] { item.Id }, contents.Select(i => i.Id));

        await client.RemoveItemFromCollectionAsync(item.Id, collection.Id);
        Assert.Null(client.LastError);
        Assert.Empty(await client.ListItemsInCollectionAsync(collection.Id));
    }

    // ── Tags ───────────────────────────────────────────────────────────────

    [Fact]
    public async Task Tags_round_trip()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        var item = Assert.Single(client.Items);

        Assert.Empty(await client.ListTagsForItemAsync(item.Id));

        await client.AddTagAsync(item.Id, "favorites");
        Assert.Equal(new[] { "favorites" }, await client.ListTagsForItemAsync(item.Id));

        await client.RemoveTagAsync(item.Id, "favorites");
        Assert.Empty(await client.ListTagsForItemAsync(item.Id));
        Assert.Null(client.LastError);
    }

    [Fact]
    public async Task ListAllTags_and_ListItemsByTag_round_trip()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        var item = Assert.Single(client.Items);

        await client.ListAllTagsAsync();
        Assert.Empty(client.AllTags);

        await client.AddTagAsync(item.Id, "favorites");
        await client.ListAllTagsAsync();
        Assert.Equal(new[] { "favorites" }, client.AllTags);

        var tagged = await client.ListItemsByTagAsync("favorites");
        Assert.Equal(new[] { item.Id }, tagged.Select(i => i.Id));
        Assert.Empty(await client.ListItemsByTagAsync("nonexistent"));
    }

    // ── Per-item encryption (ADR-014) ──────────────────────────────────────

    [Fact]
    public async Task EncryptItems_encrypts_then_a_second_call_is_an_idempotent_no_op()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        var item = Assert.Single(client.Items);
        Assert.False(item.ContentEncrypted, "a fresh import must land as plaintext");

        var first = await client.EncryptItemsAsync(new[] { item.Id });

        Assert.Null(client.LastError);
        Assert.Equal(new EncryptItemsSummaryShape(1, 0, 0), Shape(first));
        Assert.True(client.Items[0].ContentEncrypted);

        var second = await client.EncryptItemsAsync(new[] { item.Id });

        Assert.Null(client.LastError);
        Assert.Equal(new EncryptItemsSummaryShape(0, 1, 0), Shape(second));
    }

    [Fact]
    public async Task EncryptItems_failure_reports_scrubbed_first_errors_without_ids_or_paths()
    {
        using var client = await NewReadyClientAsync();

        var summary = await client.EncryptItemsAsync(new[] { "no-such-id-123" });

        Assert.Equal(new EncryptItemsSummaryShape(0, 0, 1), Shape(summary));
        var line = Assert.Single(summary.FirstErrors);
        Assert.Equal("An item could no longer be found in the library.", line);
        Assert.DoesNotContain("no-such-id-123", line);
    }

    /// <summary>
    /// The read-after-encrypt guarantee ADR-014 exists for: an item encrypted through this client
    /// must still be readable through the <em>same</em> client, because production opens the store
    /// with <c>NewWithReadKey</c>. Before that fix on Apple, clicking Encrypt permanently locked a
    /// book out of the running app.
    /// </summary>
    [Fact]
    public async Task EncryptItems_then_reading_the_content_back_succeeds_on_the_same_client()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        var item = Assert.Single(client.Items);

        var summary = await client.EncryptItemsAsync(new[] { item.Id });
        Assert.Equal(1, summary.EncryptedCount);
        Assert.True(client.Items[0].ContentEncrypted);

        var document = await client.GetDocumentJsonAsync(item.Id);
        Assert.Null(client.LastError);
        Assert.False(string.IsNullOrWhiteSpace(document));

        var rsvp = await client.StartRsvpAsync(item.Id, 300);
        Assert.Null(client.LastError);
        Assert.False(string.IsNullOrWhiteSpace(rsvp));

        await client.SaveProgressAsync(item.Id, 5);
        Assert.Null(client.LastError);
    }

    /// <summary>
    /// The ADR-006 sandboxed copy is explicitly out of scope for encryption (ADR-014): it must be
    /// byte-for-byte untouched, and so must the user's original.
    /// </summary>
    [Fact]
    public async Task EncryptItems_never_touches_the_sandboxed_original_copy()
    {
        using var client = await NewReadyClientAsync();
        var fixture = _workspace.CopyFixture("basic_ascii.txt");
        await client.ImportFileAsync(fixture);
        var item = Assert.Single(client.Items);

        var copyPath = _workspace.PredictSandboxedCopyPath(fixture);
        var copyBefore = await File.ReadAllBytesAsync(copyPath);
        var originalBefore = await File.ReadAllBytesAsync(fixture);

        await client.EncryptItemsAsync(new[] { item.Id });

        Assert.Equal(copyBefore, await File.ReadAllBytesAsync(copyPath));
        Assert.Equal(originalBefore, await File.ReadAllBytesAsync(fixture));
    }

    [Fact]
    public async Task EncryptItems_with_no_ids_is_a_no_op()
    {
        using var client = await NewReadyClientAsync();

        var summary = await client.EncryptItemsAsync(Array.Empty<string>());

        Assert.True(summary.IsEmpty);
        Assert.Null(client.LastError);
    }

    // ── Threading (plan §2: no FFI on the UI thread) ────────────────────────

    /// <summary>
    /// Proves both halves of the threading contract at once: the FFI ran off the stand-in UI thread,
    /// and every observable mutation arrived through the dispatcher. The dispatcher used here is
    /// genuinely asynchronous (a queue on its own thread), so it also proves an awaited call has
    /// finished publishing by the time it returns.
    /// </summary>
    [Fact]
    public async Task Ffi_runs_off_the_ui_thread_and_results_are_published_through_the_dispatcher()
    {
        using var dispatcher = new RecordingUiDispatcher();
        using var client = new CoreClient(
            _workspace.DbPath, _workspace.StorageDir, _keyProvider, dispatcher);
        Assert.True(await client.InitializeAsync());

        int? ffiThreadId = null;
        client.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(CoreClient.Items))
            {
                ffiThreadId = Environment.CurrentManagedThreadId;
            }
        };

        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));

        Assert.Single(client.Items);
        Assert.True(dispatcher.PostCount > 0, "state must be published through the dispatcher");
        Assert.Equal(dispatcher.ThreadId, ffiThreadId);
        Assert.NotEqual(dispatcher.ThreadId, Environment.CurrentManagedThreadId);

        // The core-bound work itself ran somewhere else entirely (Task.Run), never on the UI thread.
        Assert.NotEqual(dispatcher.ThreadId, _keyProvider.LastCallThreadId);
        Assert.NotEqual(0, _keyProvider.LastCallThreadId);
    }

    [Fact]
    public async Task Concurrent_reads_do_not_corrupt_published_state()
    {
        using var client = await NewReadyClientAsync();
        await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));

        await Task.WhenAll(Enumerable.Range(0, 32).Select(async i =>
        {
            if (i % 2 == 0)
            {
                await client.RefreshAsync();
            }
            else
            {
                await client.SearchAsync("lorem");
            }
        }));

        Assert.Null(client.LastError);
        Assert.Single(client.Items);
    }

    // ── Lifecycle ──────────────────────────────────────────────────────────

    [Fact]
    public async Task Operations_before_initialisation_fail_fast_with_a_typed_error()
    {
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);

        await client.RefreshAsync();

        Assert.False(client.IsReady);
        Assert.NotNull(client.LastError);
        Assert.Equal(CoreErrorKind.NotInitialized, client.LastError!.Kind);
        Assert.False(File.Exists(_workspace.DbPath), "no store should have been created");
    }

    [Fact]
    public async Task Initialize_is_idempotent_and_reuses_the_same_key()
    {
        using var client = await NewReadyClientAsync();

        Assert.True(await client.InitializeAsync());
        Assert.Equal(1, _keyProvider.CallCount);
        Assert.Equal(1, _keyProvider.KeysGenerated);
    }

    private readonly record struct EncryptItemsSummaryShape(int Encrypted, int Already, int Failed);

    private static EncryptItemsSummaryShape Shape(Gist.Core.Models.EncryptItemsSummary summary) =>
        new(summary.EncryptedCount, summary.AlreadyEncryptedCount, summary.FailedCount);
}
