using Gist.Core.Client;
using Gist.Core.Filtering;
using Gist.Core.Models;

namespace Gist.Core.ViewModels;

/// <summary>
/// The Library screen's logic (<c>docs/windows-ui-spec.md</c> §4), with no UI attached: the list
/// and what it shows, debounced search, sort, tag filter, the command-enablement table, the import
/// and removal and encryption flows, and the data behind every dialog.
/// </summary>
/// <remarks>
/// <para>
/// Windows counterpart of Apple's <c>LibraryView</c>, except that on Apple all of this lives
/// <em>inside</em> the SwiftUI view and is therefore only reachable by a person clicking. Pulling
/// it into a plain class is the same move Apple made for <c>LibraryFiltering</c>, taken further:
/// every rule in §4 — including the two that cost Apple real bugs, the search/tag-filter exclusion
/// and the DRM error surviving the refresh that follows an import — is exercised by
/// <c>dotnet test</c> on a headless runner.
/// </para>
/// <para>
/// <b>Threading.</b> Nothing here touches the FFI directly; every call goes through
/// <see cref="CoreClient"/>, which runs the core off the UI thread and republishes on the injected
/// dispatcher. Awaiting any method here therefore leaves the observable state already updated.
/// </para>
/// </remarks>
public sealed class LibraryViewModel : LibraryViewModelBase
{
    /// <summary>
    /// The search debounce (§4.2). 300 ms, matching Apple's
    /// <c>Task.sleep(nanoseconds: 300_000_000)</c>.
    /// </summary>
    public static readonly TimeSpan SearchDebounce = TimeSpan.FromMilliseconds(300);

    private static readonly IReadOnlyList<LibraryItemVM> NoItems = Array.Empty<LibraryItemVM>();

    private readonly ILibraryScheduler _scheduler;

    private string _searchText = string.Empty;
    private string? _tagFilter;
    private IReadOnlyList<LibraryItemVM> _tagFilteredItems = NoItems;
    private CancellationTokenSource? _searchCts;
    private EncryptItemsSummary? _lastEncryptSummary;

    /// <param name="core">The client this screen drives.</param>
    /// <param name="scheduler">
    /// Supplies the debounce delay. Null means a real timer; tests inject a fake they advance by
    /// hand.
    /// </param>
    public LibraryViewModel(CoreClient core, ILibraryScheduler? scheduler = null)
        : base(core) => _scheduler = scheduler ?? LibraryScheduler.Default;

    // ── List sources ───────────────────────────────────────────────────────

    /// <inheritdoc />
    /// <remarks>
    /// A tag filter wins outright when one is set, because a tag filter and a search are mutually
    /// exclusive (§4.2) and setting either clears the other — so only one of the three lists is
    /// ever in force.
    /// </remarks>
    protected override IReadOnlyList<LibraryItemVM> BaseItems =>
        _tagFilter is not null
            ? _tagFilteredItems
            : LibraryFiltering.DisplayedItems(_searchText, Core.Items, Core.SearchResults);

    /// <summary>Every collection in the library, for the sidebar and the Add to Collection menu.</summary>
    public IReadOnlyList<CollectionVM> Collections => Core.Collections;

    /// <summary>Every tag in the library, for the Filter menu (§4.1 command 4).</summary>
    public IReadOnlyList<string> AllTags => Core.AllTags;

    // ── Search (§4.2) ──────────────────────────────────────────────────────

    /// <summary>
    /// The search box text. Setting it restarts the 300 ms debounce and, when it becomes non-blank,
    /// clears any tag filter.
    /// </summary>
    /// <remarks>
    /// A blank or whitespace-only value still goes through the debounce (so it cancels a pending
    /// query) but clears the results <b>without an FFI call</b> —
    /// <see cref="CoreClient.SearchAsync"/> short-circuits before it touches the core, matching
    /// Apple. The core escapes the query itself as a prefix-matching FTS5 phrase (F19); nothing
    /// here adds escaping or wildcards (§4.2).
    /// </remarks>
    public string SearchText
    {
        get => _searchText;
        set
        {
            var next = value ?? string.Empty;
            if (string.Equals(_searchText, next, StringComparison.Ordinal))
            {
                return;
            }

            _searchText = next;
            OnPropertyChanged(nameof(SearchText));
            OnPropertyChanged(nameof(IsSearchActive));

            if (LibraryFiltering.IsSearchActive(next) && _tagFilter is not null)
            {
                // Mutual exclusion, search → tag filter. Nothing to reload: the tag-filtered list
                // is simply no longer the one BaseItems reads.
                _tagFilter = null;
                _tagFilteredItems = NoItems;
                OnPropertyChanged(nameof(TagFilter));
            }

            NotifyItemsChanged();
            RestartSearchDebounce(next);
        }
    }

    /// <summary>Whether the list is showing search results rather than the whole library.</summary>
    public bool IsSearchActive => LibraryFiltering.IsSearchActive(_searchText);

    /// <summary>
    /// The debounced search currently in flight, or a completed task.
    /// </summary>
    /// <remarks>
    /// Exposed so a test can await the exact operation a <see cref="SearchText"/> assignment
    /// started, instead of sleeping and hoping. The shell has no reason to touch it.
    /// </remarks>
    public Task SearchInFlight { get; private set; } = Task.CompletedTask;

    private void RestartSearchDebounce(string query)
    {
        // Cancel-and-restart, exactly as Apple: the previous wait is cancelled, not left to fire.
        var previous = _searchCts;
        _searchCts = new CancellationTokenSource();
        previous?.Cancel();
        previous?.Dispose();

        SearchInFlight = RunSearchAsync(query, _searchCts.Token);
        OnPropertyChanged(nameof(SearchInFlight));
    }

    private async Task RunSearchAsync(string query, CancellationToken cancellationToken)
    {
        try
        {
            await _scheduler.Delay(SearchDebounce, cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            return;
        }

        if (cancellationToken.IsCancellationRequested)
        {
            return;
        }

        await Core.SearchAsync(query).ConfigureAwait(false);

        if (cancellationToken.IsCancellationRequested)
        {
            return;
        }

        CaptureOutcome();
        NotifyItemsChanged();
    }

    // ── Tag filter (§4.1 command 4) ────────────────────────────────────────

    /// <summary>
    /// The active tag filter, or null for "All Tags". Setting a tag clears any active search
    /// (§4.2's mutual exclusion) and loads that tag's items.
    /// </summary>
    public string? TagFilter
    {
        get => _tagFilter;
        set
        {
            var next = string.IsNullOrWhiteSpace(value) ? null : value;
            if (string.Equals(_tagFilter, next, StringComparison.Ordinal))
            {
                return;
            }

            _tagFilter = next;
            OnPropertyChanged(nameof(TagFilter));

            var hadSearch = LibraryFiltering.IsSearchActive(_searchText);
            if (next is not null && hadSearch)
            {
                // Mutual exclusion, tag filter → search. The pending debounce is cancelled so a
                // stale query cannot land after the filter has been applied.
                _searchCts?.Cancel();
                _searchText = string.Empty;
                OnPropertyChanged(nameof(SearchText));
                OnPropertyChanged(nameof(IsSearchActive));
            }

            NotifyItemsChanged();
            TagFilterInFlight = ApplyTagFilterAsync(next, clearSearchResults: hadSearch);
            OnPropertyChanged(nameof(TagFilterInFlight));
        }
    }

    /// <summary>
    /// The tag-filter reload currently in flight, or a completed task. Test seam, like
    /// <see cref="SearchInFlight"/>.
    /// </summary>
    public Task TagFilterInFlight { get; private set; } = Task.CompletedTask;

    private async Task ApplyTagFilterAsync(string? tag, bool clearSearchResults)
    {
        if (clearSearchResults)
        {
            await Core.ClearSearchAsync().ConfigureAwait(false);
        }

        _tagFilteredItems = tag is null
            ? NoItems
            : await Core.ListItemsByTagAsync(tag).ConfigureAwait(false);

        if (tag is not null)
        {
            CaptureOutcome();
        }

        NotifyItemsChanged();
    }

    // ── Empty states (§4.4) ────────────────────────────────────────────────

    /// <inheritdoc />
    public override LibraryEmptyState EmptyState
    {
        get
        {
            if (DisplayedItems.Count > 0 || IsLoading)
            {
                return LibraryEmptyState.None;
            }

            if (_tagFilter is not null)
            {
                return LibraryEmptyState.NoTagHits;
            }

            return IsSearchActive ? LibraryEmptyState.NoSearchHits : LibraryEmptyState.NoItems;
        }
    }

    /// <inheritdoc />
    public override string EmptyTitle => EmptyState switch
    {
        LibraryEmptyState.NoItems => "No books yet",
        LibraryEmptyState.NoSearchHits => "No matches",
        LibraryEmptyState.NoTagHits => "No items",
        _ => string.Empty,
    };

    /// <inheritdoc />
    public override string EmptyMessage => EmptyState switch
    {
        LibraryEmptyState.NoItems => "Import a .txt, .epub, or .docx file to get started",
        LibraryEmptyState.NoSearchHits => $"No items match \"{_searchText}\"",
        LibraryEmptyState.NoTagHits => $"No items are tagged \"{_tagFilter}\"",
        _ => string.Empty,
    };

    // ── Loading ────────────────────────────────────────────────────────────

    /// <summary>
    /// Loads (or reloads) everything the screen binds to: items, collections, the tag vocabulary,
    /// and the tag-filtered list when a filter is active.
    /// </summary>
    /// <remarks>
    /// Deliberately does <b>not</b> clear a latched failure: this runs on navigation and after every
    /// mutation, and wiping an unacknowledged DRM rejection here is precisely the Apple bug this
    /// layer exists to avoid. See <see cref="LibraryViewModelBase.CaptureOutcome"/>.
    /// </remarks>
    public async Task LoadAsync()
    {
        IsLoading = true;
        try
        {
            await Core.RefreshAsync().ConfigureAwait(false);
            await Core.ListCollectionsAsync().ConfigureAwait(false);
            await Core.ListAllTagsAsync().ConfigureAwait(false);

            if (_tagFilter is not null)
            {
                _tagFilteredItems = await Core.ListItemsByTagAsync(_tagFilter).ConfigureAwait(false);
            }

            CaptureOutcome();
        }
        finally
        {
            IsLoading = false;
            NotifyItemsChanged();
        }
    }

    // ── Import (§4.1 commands 1–2, §4.5) ───────────────────────────────────

    /// <summary>
    /// Imports a file the user picked. A DRM-protected file surfaces as
    /// <see cref="LibraryDialog.DrmProtected"/> with the path on
    /// <see cref="LibraryViewModelBase.LastDrmFile"/>.
    /// </summary>
    /// <returns>The new item id, or null on failure (including a blank path).</returns>
    public async Task<string?> ImportFileAsync(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return null;
        }

        BeginUserOperation();
        IsBusy = true;
        try
        {
            var id = await Core.ImportFileAsync(path).ConfigureAwait(false);
            CaptureOutcome();
            await AfterLibraryMutationAsync().ConfigureAwait(false);
            return id;
        }
        finally
        {
            IsBusy = false;
        }
    }

    /// <summary>
    /// Imports a web page. A blank URL is rejected here, before any FFI call — the dialog's Import
    /// button is disabled for the same input (§4.5), so this is belt and braces rather than the
    /// only guard.
    /// </summary>
    /// <returns>The new item id, or null on failure (including a blank URL).</returns>
    public async Task<string?> ImportUrlAsync(string url)
    {
        if (string.IsNullOrWhiteSpace(url))
        {
            return null;
        }

        BeginUserOperation();
        IsBusy = true;
        try
        {
            var id = await Core.ImportUrlAsync(url.Trim()).ConfigureAwait(false);
            CaptureOutcome();
            await AfterLibraryMutationAsync().ConfigureAwait(false);
            return id;
        }
        finally
        {
            IsBusy = false;
        }
    }

    // ── Remove (§4.1 command 7, §4.5) ──────────────────────────────────────

    /// <summary>
    /// Builds the Remove confirmation's content for the current selection: title, the first five
    /// item titles, an "and N more" line, and the irreversibility line.
    /// </summary>
    public RemovePreview RemovePreview() => ViewModels.RemovePreview.ForLibrary(SelectedItems);

    /// <summary>
    /// Removes the selected items.
    /// </summary>
    /// <param name="deleteStoredCopy">
    /// Whether to also delete GIST's own ADR-006 stored copy under <c>storage/originals/</c>. The
    /// user's original file is never touched either way — which is why the Windows button says
    /// "Also Delete Stored Copy" rather than Apple's inaccurate "Also Delete Original File"
    /// (§4.5 wording note). There is a test asserting the original survives both paths.
    /// </param>
    public async Task RemoveAsync(bool deleteStoredCopy)
    {
        var ids = SelectedIds;
        if (ids.Count == 0)
        {
            return;
        }

        BeginUserOperation();
        PendingDialog = LibraryDialog.None;
        await Core.RemoveItemsAsync(ids, deleteStoredCopy).ConfigureAwait(false);
        CaptureOutcome();
        ClearSelection();
        await AfterLibraryMutationAsync().ConfigureAwait(false);
    }

    // ── Encrypt (§4.1 command 6, §4.5, ADR-014) ────────────────────────────

    /// <summary>
    /// Builds the Encrypt confirmation's content, including the maintainer-mandated no-recovery
    /// warning (see <see cref="EncryptPreview"/>).
    /// </summary>
    public EncryptPreview EncryptPreview() => ViewModels.EncryptPreview.For(SelectionCount);

    /// <summary>
    /// The tally from the last <see cref="EncryptAsync"/>, for the result dialog (§4.5). Null until
    /// one has run.
    /// </summary>
    public EncryptItemsSummary? LastEncryptSummary
    {
        get => _lastEncryptSummary;
        private set => SetProperty(ref _lastEncryptSummary, value);
    }

    /// <summary>
    /// Encrypts the selected items at rest, then raises the result dialog.
    /// </summary>
    /// <remarks>
    /// Idempotent per item: an already-encrypted item counts as
    /// <see cref="EncryptItemsSummary.AlreadyEncryptedCount"/>, not a failure. The selection is
    /// deliberately kept afterwards — unlike removal, the rows are still there, and clearing it
    /// would be a surprise.
    /// </remarks>
    public async Task<EncryptItemsSummary> EncryptAsync()
    {
        var ids = SelectedIds;
        if (ids.Count == 0)
        {
            LastEncryptSummary = EncryptItemsSummary.Empty;
            PendingDialog = LibraryDialog.EncryptResult;
            return EncryptItemsSummary.Empty;
        }

        BeginUserOperation();
        var summary = await Core.EncryptItemsAsync(ids).ConfigureAwait(false);
        CaptureOutcome();
        LastEncryptSummary = summary;

        // A whole-call failure gets the error dialog; otherwise the summary is the outcome the
        // user asked for, even when some items failed individually.
        if (PendingDialog is not (LibraryDialog.Error or LibraryDialog.DrmProtected))
        {
            PendingDialog = LibraryDialog.EncryptResult;
        }

        await AfterLibraryMutationAsync().ConfigureAwait(false);
        return summary;
    }

    // ── Collections (§4.1 command 5) ───────────────────────────────────────

    /// <summary>
    /// Creates a collection and adds the current selection to it (the "New Collection…" entry in the
    /// Add to Collection menu). A blank name is rejected before any FFI call.
    /// </summary>
    /// <returns>The new collection, or null when the name was blank or creation failed.</returns>
    public async Task<CollectionVM?> CreateCollectionAndAddAsync(string name)
    {
        var trimmed = name?.Trim();
        if (string.IsNullOrEmpty(trimmed))
        {
            return null;
        }

        BeginUserOperation();
        var id = await Core.CreateCollectionAsync(trimmed).ConfigureAwait(false);
        if (id is null)
        {
            CaptureOutcome();
            return null;
        }

        await AddIdsToCollectionAsync(id).ConfigureAwait(false);
        CaptureOutcome();
        OnPropertyChanged(nameof(Collections));

        return Core.Collections.FirstOrDefault(c => string.Equals(c.Id, id, StringComparison.Ordinal));
    }

    /// <summary>Adds the current selection to an existing collection.</summary>
    public async Task AddToCollectionAsync(CollectionVM collection)
    {
        ArgumentNullException.ThrowIfNull(collection);
        if (SelectedIds.Count == 0)
        {
            return;
        }

        BeginUserOperation();
        await AddIdsToCollectionAsync(collection.Id).ConfigureAwait(false);
        CaptureOutcome();
    }

    private async Task AddIdsToCollectionAsync(string collectionId)
    {
        foreach (var itemId in SelectedIds)
        {
            await Core.AddItemToCollectionAsync(itemId, collectionId).ConfigureAwait(false);
        }
    }

    // ── Plumbing ───────────────────────────────────────────────────────────

    /// <summary>
    /// Re-reads whatever a mutation could have changed besides <see cref="CoreClient.Items"/>,
    /// which the client already reloads for us: the tag-filtered list (an import or removal changes
    /// what is in it) and the tag vocabulary (removing the last item carrying a tag removes the tag
    /// from the Filter menu).
    /// </summary>
    private async Task AfterLibraryMutationAsync()
    {
        await Core.ListAllTagsAsync().ConfigureAwait(false);

        if (_tagFilter is not null)
        {
            _tagFilteredItems = await Core.ListItemsByTagAsync(_tagFilter).ConfigureAwait(false);
        }

        NotifyItemsChanged();
    }

    /// <inheritdoc />
    protected override void OnCoreChanged(string? propertyName)
    {
        base.OnCoreChanged(propertyName);

        switch (propertyName)
        {
            case nameof(CoreClient.Collections):
                OnPropertyChanged(nameof(Collections));
                break;
            case nameof(CoreClient.AllTags):
                OnPropertyChanged(nameof(AllTags));
                break;
            default:
                break;
        }
    }

    /// <inheritdoc />
    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _searchCts?.Cancel();
            _searchCts?.Dispose();
            _searchCts = null;
        }

        base.Dispose(disposing);
    }
}
