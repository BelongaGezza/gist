using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using Gist.Core.Client;
using Gist.Core.Filtering;
using Gist.Core.Models;

namespace Gist.Core.ViewModels;

/// <summary>
/// What the Library screen (<c>docs/windows-ui-spec.md</c> §4) and the Collection screen (§5) have
/// in common: a sorted list, a multi-selection, the command-enablement rules derived from it, the
/// tag-editor operations, and one error/dialog surface.
/// </summary>
/// <remarks>
/// <para>
/// §5 says the two screens are "two view models sharing one row template and one sort
/// implementation" — the removal semantics genuinely differ (remove from library vs. detach from a
/// collection) and Apple keeps them apart in <c>LibraryView</c>/<c>CollectionDetailView</c>. This
/// base holds only what is provably identical; each subclass supplies its own list source and its
/// own removal.
/// </para>
/// <para>
/// <b>No UI types.</b> This assembly must not reference WinUI, so nothing here knows what a dialog
/// looks like; <see cref="PendingDialog"/> only says which one the shell should raise.
/// </para>
/// <para>
/// <b>Nothing here logs.</b> Titles and paths must never reach a log above <c>debug</c>
/// (CLAUDE.md), and the simplest way to guarantee that in a layer that handles both is to emit no
/// log records at all — the same choice <see cref="CoreClient"/> made.
/// </para>
/// </remarks>
public abstract class LibraryViewModelBase : ObservableObject, IDisposable
{
    private static readonly IReadOnlyList<string> NoIds = Array.Empty<string>();

    private IReadOnlyList<string> _selectedIds = NoIds;
    private LibrarySortOrder _sortOrder = LibrarySortOrder.DateAddedNewest;
    private LibraryDialog _pendingDialog = LibraryDialog.None;
    private CoreError? _lastError;
    private string? _lastDrmFile;
    private bool _isLoading;
    private bool _isBusy;
    private bool _disposed;

    /// <param name="core">The single touch point to the Rust core. Not owned: never disposed here.</param>
    protected LibraryViewModelBase(CoreClient core)
    {
        ArgumentNullException.ThrowIfNull(core);
        Core = core;
        Core.PropertyChanged += OnCorePropertyChanged;
    }

    /// <summary>The client this view model drives. Owned by the shell, not by this instance.</summary>
    protected CoreClient Core { get; }

    // ── List ───────────────────────────────────────────────────────────────

    /// <summary>
    /// The list before sorting: whichever of the library/search/tag-filtered/collection lists is
    /// currently in force. Subclasses decide; <see cref="DisplayedItems"/> adds the sort.
    /// </summary>
    protected abstract IReadOnlyList<LibraryItemVM> BaseItems { get; }

    /// <summary>
    /// The rows to render: <see cref="BaseItems"/> put through
    /// <see cref="LibraryFiltering.Sorted"/>. Client-side, exactly as §4.3 requires — the core
    /// already returns newest-first, so <see cref="LibrarySortOrder.DateAddedNewest"/> is a no-op.
    /// </summary>
    public IReadOnlyList<LibraryItemVM> DisplayedItems => LibraryFiltering.Sorted(BaseItems, SortOrder);

    /// <summary>The active sort order (§4.3). Applied client-side; never re-queries the core.</summary>
    public LibrarySortOrder SortOrder
    {
        get => _sortOrder;
        set
        {
            if (SetProperty(ref _sortOrder, value))
            {
                NotifyItemsChanged();
            }
        }
    }

    // ── Selection and command enablement (§4.1 command table) ──────────────

    /// <summary>The selected item ids, de-duplicated, in the order the shell supplied them.</summary>
    public IReadOnlyList<string> SelectedIds => _selectedIds;

    /// <summary>How many items are selected.</summary>
    public int SelectionCount => _selectedIds.Count;

    /// <summary>
    /// The selected items, resolved against <see cref="DisplayedItems"/>. Ids that no longer
    /// appear — a row removed under a stale selection — are dropped rather than faked.
    /// </summary>
    public IReadOnlyList<LibraryItemVM> SelectedItems
    {
        get
        {
            if (_selectedIds.Count == 0)
            {
                return Array.Empty<LibraryItemVM>();
            }

            var wanted = new HashSet<string>(_selectedIds, StringComparer.Ordinal);
            return DisplayedItems.Where(i => wanted.Contains(i.Id)).ToList();
        }
    }

    /// <summary>Command 5, "Add to Collection": at least one item selected.</summary>
    public virtual bool CanAddToCollection => SelectionCount >= 1;

    /// <summary>Command 6, "Encrypt": at least one item selected.</summary>
    public virtual bool CanEncrypt => SelectionCount >= 1;

    /// <summary>Command 7, "Remove": at least one item selected.</summary>
    public virtual bool CanRemove => SelectionCount >= 1;

    /// <summary>Command 8, "Open": exactly one item selected.</summary>
    public bool CanOpen => SelectionCount == 1;

    /// <summary>Command 9, "Tags": exactly one item selected (the editor is single-item, §6).</summary>
    public bool CanEditTags => SelectionCount == 1;

    /// <summary>
    /// Replaces the selection. Duplicates and blank ids are dropped; an unchanged selection raises
    /// no notifications.
    /// </summary>
    public void SetSelection(IEnumerable<string> ids)
    {
        ArgumentNullException.ThrowIfNull(ids);

        var next = ids
            .Where(id => !string.IsNullOrEmpty(id))
            .Distinct(StringComparer.Ordinal)
            .ToArray();

        if (next.SequenceEqual(_selectedIds, StringComparer.Ordinal))
        {
            return;
        }

        _selectedIds = next;
        NotifySelectionChanged();
    }

    /// <summary>Clears the selection.</summary>
    public void ClearSelection() => SetSelection(Array.Empty<string>());

    /// <summary>
    /// The single selected item, or null when the selection is not exactly one — the shape the
    /// "Open" and "Tags" commands need.
    /// </summary>
    public LibraryItemVM? SingleSelectedItem =>
        SelectionCount == 1 ? SelectedItems.FirstOrDefault() : null;

    // ── Progress ───────────────────────────────────────────────────────────

    /// <summary>A list reload is in flight. Suppresses the empty states (§4.4).</summary>
    public bool IsLoading
    {
        get => _isLoading;
        protected set
        {
            if (SetProperty(ref _isLoading, value))
            {
                NotifyItemsChanged();
            }
        }
    }

    /// <summary>
    /// A long operation (file import, URL fetch) is in flight: the shell shows an indeterminate
    /// <c>ProgressRing</c> and disables the Import commands (§4.5).
    /// </summary>
    public bool IsBusy
    {
        get => _isBusy;
        protected set
        {
            if (SetProperty(ref _isBusy, value))
            {
                OnPropertyChanged(nameof(IsImportEnabled));
            }
        }
    }

    /// <summary>Commands 1 and 2 ("Import File"/"Import URL") are disabled while busy.</summary>
    public bool IsImportEnabled => !IsBusy;

    // ── Errors and dialogs ─────────────────────────────────────────────────

    /// <summary>Which dialog the shell should present, if any.</summary>
    public LibraryDialog PendingDialog
    {
        get => _pendingDialog;
        protected set => SetProperty(ref _pendingDialog, value);
    }

    /// <summary>
    /// The last failure, latched. Typed (<see cref="CoreErrorKind"/>), so the shell branches on a
    /// kind and shows <see cref="CoreError.Message"/> — it never parses message text.
    /// </summary>
    public CoreError? LastError
    {
        get => _lastError;
        private set => SetProperty(ref _lastError, value);
    }

    /// <summary>
    /// The file or URL that was rejected as DRM-protected, latched, or null. Carried separately
    /// from any message so a path is never embedded in presentable prose.
    /// </summary>
    public string? LastDrmFile
    {
        get => _lastDrmFile;
        private set => SetProperty(ref _lastDrmFile, value);
    }

    /// <summary>Asks the shell to present <paramref name="dialog"/>.</summary>
    public void RequestDialog(LibraryDialog dialog) => PendingDialog = dialog;

    /// <summary>
    /// Dismisses the pending dialog. When it was the DRM or error dialog the latched failure is
    /// cleared here <em>and only here</em> — see <see cref="CaptureOutcome"/>.
    /// </summary>
    public async Task DismissDialogAsync()
    {
        var dismissed = PendingDialog;
        PendingDialog = LibraryDialog.None;

        if (dismissed is LibraryDialog.DrmProtected or LibraryDialog.Error)
        {
            await ClearErrorAsync().ConfigureAwait(false);
        }
    }

    /// <summary>Clears the latched failure, here and in the client.</summary>
    public async Task ClearErrorAsync()
    {
        LastError = null;
        LastDrmFile = null;
        await Core.ClearErrorAsync().ConfigureAwait(false);
    }

    /// <summary>
    /// Called at the start of an operation the user explicitly asked for, so a stale failure from a
    /// previous attempt is not still on screen when the new one finishes. Deliberately <b>not</b>
    /// called by list reloads — see <see cref="CaptureOutcome"/>.
    /// </summary>
    protected void BeginUserOperation()
    {
        LastError = null;
        LastDrmFile = null;
        if (PendingDialog is LibraryDialog.DrmProtected or LibraryDialog.Error)
        {
            PendingDialog = LibraryDialog.None;
        }
    }

    /// <summary>
    /// Reads the client's outcome after an operation and latches any failure.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Success never clears a latched failure.</b> That is the whole point. <c>CoreClient</c>
    /// reloads its item list at the end of every import/removal/encrypt, and
    /// <see cref="CoreClient.RefreshAsync"/> clears <see cref="CoreClient.LastError"/> when it
    /// succeeds — so a view model that simply mirrored the client's live error would wipe a DRM
    /// failure the instant the following refresh landed, and the dialog would never appear. That
    /// exact bug was found on Apple during W1; latching here, and clearing only on an explicit
    /// dismissal, is what stops it recurring on Windows. A regression test covers it.
    /// </para>
    /// <para>
    /// DRM is checked first because a DRM rejection also sets a
    /// <see cref="CoreErrorKind.DrmProtected"/> error, and the spec requires its own dialog
    /// (§4.5, §10 item 4).
    /// </para>
    /// </remarks>
    protected void CaptureOutcome()
    {
        if (Core.DrmProtectedSource is { } source)
        {
            LastDrmFile = source;

            // Only overwrite with a *new* error. On the refresh that follows the failed import the
            // client has already cleared its own error (its DRM source survives, which is how we
            // got here), so assigning unconditionally would null out the latched kind and leave the
            // dialog with nothing to say.
            if (Core.LastError is { } drmError)
            {
                LastError = drmError;
            }

            PendingDialog = LibraryDialog.DrmProtected;
            return;
        }

        if (Core.LastError is { } error)
        {
            LastError = error;
            PendingDialog = LibraryDialog.Error;
        }
    }

    // ── Tag editor operations (§6) ─────────────────────────────────────────

    /// <summary>The tags on one item, for the tag editor.</summary>
    public Task<IReadOnlyList<string>> TagsForAsync(string itemId)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(itemId);
        return Core.ListTagsForItemAsync(itemId);
    }

    /// <summary>
    /// Adds a tag. The name is trimmed, and a blank one is rejected here without an FFI call —
    /// same rule as Apple's <c>TagEditorView.add()</c>. Duplicates are the core's business
    /// (§6: "duplicates ignored by the core").
    /// </summary>
    /// <returns>False when the name was blank, so the shell can leave the text box alone.</returns>
    public async Task<bool> AddTagAsync(string itemId, string tagName)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(itemId);

        var trimmed = tagName?.Trim();
        if (string.IsNullOrEmpty(trimmed))
        {
            return false;
        }

        BeginUserOperation();
        await Core.AddTagAsync(itemId, trimmed).ConfigureAwait(false);
        CaptureOutcome();
        await RefreshTagVocabularyAsync().ConfigureAwait(false);
        return LastError is null;
    }

    /// <summary>Removes a tag from one item.</summary>
    public async Task<bool> RemoveTagAsync(string itemId, string tagName)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(itemId);

        var trimmed = tagName?.Trim();
        if (string.IsNullOrEmpty(trimmed))
        {
            return false;
        }

        BeginUserOperation();
        await Core.RemoveTagAsync(itemId, trimmed).ConfigureAwait(false);
        CaptureOutcome();
        await RefreshTagVocabularyAsync().ConfigureAwait(false);
        return LastError is null;
    }

    /// <summary>
    /// Reloads the library-wide tag list after a tag edit, so the Filter menu stays in step
    /// (§6: "Closing the dialog refreshes allTags"). The Collection screen has no Filter menu and
    /// overrides this to nothing.
    /// </summary>
    protected virtual Task RefreshTagVocabularyAsync() => Core.ListAllTagsAsync();

    // ── Change plumbing ────────────────────────────────────────────────────

    /// <summary>
    /// Raises the notifications every list-shape change implies. Subclasses call this after
    /// swapping their own list source.
    /// </summary>
    protected void NotifyItemsChanged()
    {
        OnPropertyChanged(nameof(DisplayedItems));
        OnPropertyChanged(nameof(SelectedItems));
        OnPropertyChanged(nameof(SingleSelectedItem));
        OnPropertyChanged(nameof(EmptyState));
        OnPropertyChanged(nameof(EmptyTitle));
        OnPropertyChanged(nameof(EmptyMessage));
    }

    private void NotifySelectionChanged()
    {
        OnPropertyChanged(nameof(SelectedIds));
        OnPropertyChanged(nameof(SelectionCount));
        OnPropertyChanged(nameof(SelectedItems));
        OnPropertyChanged(nameof(SingleSelectedItem));
        OnPropertyChanged(nameof(CanAddToCollection));
        OnPropertyChanged(nameof(CanEncrypt));
        OnPropertyChanged(nameof(CanRemove));
        OnPropertyChanged(nameof(CanOpen));
        OnPropertyChanged(nameof(CanEditTags));
    }

    private void OnCorePropertyChanged(object? sender, PropertyChangedEventArgs e) =>
        OnCoreChanged(e.PropertyName);

    /// <summary>Reacts to the client republishing its state. Subclasses extend, not replace.</summary>
    protected virtual void OnCoreChanged(string? propertyName)
    {
        if (propertyName is nameof(CoreClient.Items) or nameof(CoreClient.SearchResults))
        {
            NotifyItemsChanged();
        }
    }

    // ── Empty states (§4.4) ────────────────────────────────────────────────

    /// <summary>Which empty state to render, if any.</summary>
    public virtual LibraryEmptyState EmptyState =>
        DisplayedItems.Count == 0 && !IsLoading ? LibraryEmptyState.NoItems : LibraryEmptyState.None;

    /// <summary>Empty-state headline, or the empty string when the list has rows.</summary>
    public virtual string EmptyTitle =>
        EmptyState == LibraryEmptyState.NoItems ? "No items" : string.Empty;

    /// <summary>Empty-state secondary line, or the empty string when the list has rows.</summary>
    public virtual string EmptyMessage =>
        EmptyState == LibraryEmptyState.NoItems ? "This collection is empty." : string.Empty;

    // ── Lifetime ───────────────────────────────────────────────────────────

    /// <summary>
    /// Detaches from the client's change notifications. The client itself belongs to the shell and
    /// is deliberately not disposed here.
    /// </summary>
    public void Dispose()
    {
        Dispose(disposing: true);
        GC.SuppressFinalize(this);
    }

    /// <summary>Subclass disposal hook.</summary>
    protected virtual void Dispose(bool disposing)
    {
        if (_disposed || !disposing)
        {
            return;
        }

        _disposed = true;
        Core.PropertyChanged -= OnCorePropertyChanged;
    }
}
