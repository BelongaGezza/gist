using CommunityToolkit.Mvvm.ComponentModel;
using Gist.Core.Keys;
using Gist.Core.Models;
using uniffi.gist_ffi;

namespace Gist.Core.Client;

/// <summary>
/// The single touch point between the Windows shell and the Rust core. Windows counterpart of
/// Apple's <c>CoreClient</c> (<c>apps/apple/Shared/CoreClient.swift</c>).
/// </summary>
/// <remarks>
/// <para>
/// <b>Threading.</b> Every FFI call runs inside <see cref="Task.Run(Action)"/>, so the UI thread is
/// never blocked on the core (docs/windows-development-plan.md §2). Results are published back
/// through the injected <see cref="IUiDispatcher"/>, and each publish is awaited, so when an
/// <c>…Async</c> method returns its observable properties have already been updated — with an
/// asynchronous dispatcher too. A single semaphore serialises the FFI calls themselves: the
/// binding spike proved 64 concurrent calls are safe, so this is defence in depth and keeps the
/// published state coherent rather than a correctness requirement.
/// </para>
/// <para>
/// <b>UI-free.</b> This assembly must not reference WinUI. The shell supplies
/// <c>new DelegateUiDispatcher(a =&gt; dispatcherQueue.TryEnqueue(() =&gt; a()))</c>; tests use
/// <see cref="ImmediateUiDispatcher"/>.
/// </para>
/// <para>
/// <b>Errors.</b> Nothing here throws at the caller for an expected failure: the typed
/// <see cref="CoreError"/> is published on <see cref="LastError"/> and the call returns an empty
/// result, mirroring Apple's published <c>error</c> string but typed
/// (<see cref="CoreErrorKind"/>) so the UI can branch instead of matching message text. DRM is a
/// distinct exception <em>type</em> across the boundary
/// (<c>GistException.DrmProtected</c>) and is matched as such; a caught Rust panic surfaces as
/// <see cref="CoreErrorKind.InternalPanic"/> with the panic text discarded.
/// </para>
/// <para>
/// <b>Logging.</b> This class logs nothing at all. That is the simplest way to honour the
/// standing rule that source paths and document titles are never logged above <c>debug</c>.
/// </para>
/// </remarks>
public sealed partial class CoreClient : ObservableObject, IDisposable
{
    private static readonly IReadOnlyList<LibraryItemVM> NoItems = Array.Empty<LibraryItemVM>();
    private static readonly IReadOnlyList<string> NoStrings = Array.Empty<string>();

    private readonly CoreClientOptions _options;
    private readonly IUiDispatcher _dispatcher;
    private readonly SemaphoreSlim _ffiGate = new(1, 1);

    private GistCore? _core;

    /// <summary>
    /// The validated 32-byte key, fetched once in managed code. Handed to Rust through
    /// <see cref="PreObtainedKeyProvider"/>, which cannot fail. Never logged, never copied into an
    /// error message.
    /// </summary>
    private byte[]? _key;

    private bool _disposed;

    public CoreClient(CoreClientOptions options, IUiDispatcher? dispatcher = null)
    {
        ArgumentNullException.ThrowIfNull(options);
        options.Validate();
        _options = options;
        _dispatcher = dispatcher ?? ImmediateUiDispatcher.Instance;
    }

    /// <summary>Convenience overload mirroring Apple's <c>init(dbPath:storageDir:keyProvider:)</c>.</summary>
    public CoreClient(string dbPath, string storageDir, IKeyProvider keyProvider, IUiDispatcher? dispatcher = null)
        : this(new CoreClientOptions(dbPath, storageDir, keyProvider), dispatcher)
    {
    }

    // ── Observable state ────────────────────────────────────────────────────

    [ObservableProperty]
    private IReadOnlyList<LibraryItemVM> _items = NoItems;

    [ObservableProperty]
    private IReadOnlyList<LibraryItemVM> _searchResults = NoItems;

    [ObservableProperty]
    private IReadOnlyList<CollectionVM> _collections = Array.Empty<CollectionVM>();

    [ObservableProperty]
    private IReadOnlyList<string> _allTags = NoStrings;

    [ObservableProperty]
    private bool _isLoading;

    [ObservableProperty]
    private bool _isSearching;

    /// <summary>
    /// The most recent failure, or null. Each operation publishes its own outcome here and clears
    /// it on success; the internal list reload that follows an import, removal or encrypt
    /// deliberately does not clear it, so a failure the user has not acknowledged yet (a DRM
    /// dialog, say) survives long enough to be shown.
    /// </summary>
    [ObservableProperty]
    private CoreError? _lastError;

    /// <summary>
    /// Set — instead of <see cref="LastError"/> carrying a generic message — when an import failed
    /// specifically because the source is DRM-protected, so the shell can raise its dedicated DRM
    /// dialog (ui-spec §4.5). Holds the path or URL the user chose, which the shell already knows;
    /// it is deliberately not folded into any error message.
    /// </summary>
    [ObservableProperty]
    private string? _drmProtectedSource;

    /// <summary>
    /// Lifecycle/key state. Anything other than <see cref="CoreClientState.Ready"/> is a blocking
    /// state in which no <c>GistCore</c> exists.
    /// </summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsReady))]
    private CoreClientState _state = CoreClientState.Uninitialized;

    public bool IsReady => State == CoreClientState.Ready && _core is not null;

    // ── Initialisation and key handling (ADR-016 / W1 requirement 1) ────────

    /// <summary>
    /// Obtains the content key and opens the store. Safe to call once; use
    /// <see cref="RetryAsync"/> to re-attempt after a retryable failure.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Why the key is fetched eagerly, here, in managed code.</b> The uniffi
    /// <c>KeyProvider</c> callback has no error channel: an exception thrown inside it becomes a
    /// Rust-side failure and, in the worst case, an opaque <c>InternalPanic</c> with no way for the
    /// shell to tell "your key store is corrupt" from "the core has a bug". So
    /// <see cref="IKeyProvider.GetOrCreateKey"/> is called once here, where
    /// <see cref="KeyStoreCorruptException"/> and friends are ordinary typed exceptions, and the
    /// provider handed to Rust (<see cref="PreObtainedKeyProvider"/>) merely returns the bytes
    /// already in hand and therefore cannot throw.
    /// </para>
    /// <para>
    /// <b>Length is validated here too.</b> The contract is exactly 32 bytes; a wrong length is a
    /// fatal misconfiguration that Rust would turn into a panic
    /// (<c>InternalPanic</c>, as the ADR-015 spike demonstrated). Catching it in managed code turns
    /// that into the blocking <see cref="CoreClientState.KeyStoreCorrupt"/> state with a clear
    /// message instead.
    /// </para>
    /// <para>
    /// <b>On <see cref="KeyStoreCorruptException"/> the store is not opened at all</b> and no
    /// replacement key is ever generated — generating one would silently orphan every item already
    /// encrypted under the old key (ADR-016). Nothing is deleted, moved or rewritten. The client
    /// parks in <see cref="CoreClientState.KeyStoreCorrupt"/> and every operation fails fast.
    /// </para>
    /// <para>
    /// <b>Production uses <c>GistCore.NewWithReadKey</c></b> (ADR-014): read-decrypt capability
    /// without write-auto-encrypt, so new imports still land as plaintext while an item the user
    /// encrypts stays readable in the same running app. Opening with plain <c>new GistCore(...)</c>
    /// is what made a freshly-encrypted item permanently unreadable on Apple before ADR-014.
    /// </para>
    /// </remarks>
    /// <returns><see langword="true"/> when the client reached <see cref="CoreClientState.Ready"/>.</returns>
    public async Task<bool> InitializeAsync()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);

        if (_core is not null)
        {
            return true;
        }

        byte[] key;
        try
        {
            // Off the UI thread: DPAPI + file I/O, and on first run key generation.
            key = await Task.Run(_options.KeyProvider.GetOrCreateKey).ConfigureAwait(false);
        }
        catch (KeyStoreCorruptException)
        {
            await EnterBlockedStateAsync(
                CoreClientState.KeyStoreCorrupt,
                new CoreError(
                    CoreErrorKind.KeyStoreCorrupt,
                    "Your encryption key could not be recovered. Encrypted items cannot be opened. "
                    + "No new key has been created, so nothing has been lost that was not already unreadable."))
                .ConfigureAwait(false);
            return false;
        }
        catch (KeyStoreUnavailableException)
        {
            await EnterBlockedStateAsync(
                CoreClientState.KeyStoreUnavailable,
                new CoreError(
                    CoreErrorKind.KeyStoreUnavailable,
                    "The encryption key store is temporarily unavailable. Try again."))
                .ConfigureAwait(false);
            return false;
        }
        catch (KeyProviderException)
        {
            // KeyStoreIoException and any future sibling: not evidence of corruption, so retryable.
            await EnterBlockedStateAsync(
                CoreClientState.KeyStoreUnavailable,
                new CoreError(
                    CoreErrorKind.KeyStoreUnavailable,
                    "The encryption key could not be read. Try again."))
                .ConfigureAwait(false);
            return false;
        }

        if (key is not { Length: 32 })
        {
            // Never hand this to Rust: the callback has no error channel and a wrong length
            // becomes an opaque InternalPanic.
            await EnterBlockedStateAsync(
                CoreClientState.KeyStoreCorrupt,
                new CoreError(
                    CoreErrorKind.KeyStoreCorrupt,
                    "The encryption key is not valid. Encrypted items cannot be opened. "
                    + "No new key has been created."))
                .ConfigureAwait(false);
            return false;
        }

        GistCore core;
        try
        {
            var provider = new PreObtainedKeyProvider(key);
            core = await Task.Run(
                () => GistCore.NewWithReadKey(_options.DbPath, _options.StorageDir, provider))
                .ConfigureAwait(false);
        }
        catch (GistException e)
        {
            await EnterBlockedStateAsync(
                CoreClientState.StoreUnavailable,
                MapGistException(e, storeOpening: true))
                .ConfigureAwait(false);
            return false;
        }
        catch (Exception e) when (e is not OutOfMemoryException and not StackOverflowException)
        {
            await EnterBlockedStateAsync(
                CoreClientState.StoreUnavailable,
                new CoreError(CoreErrorKind.StoreUnavailable, "The GIST library could not be opened."))
                .ConfigureAwait(false);
            return false;
        }

        _key = key;
        _core = core;
        await OnUiAsync(() =>
        {
            State = CoreClientState.Ready;
            LastError = null;
        }).ConfigureAwait(false);

        // Startup housekeeping: reclaim files a previous run's failed delete left behind.
        // Fire-and-forget by design; see StartBackgroundSweep.
        StartBackgroundSweep();
        return true;
    }

    // ── Orphan sweep ───────────────────────────────────────────────────────

    /// <summary>
    /// The outcome of the most recent orphan sweep (counts only), or null if none has completed.
    /// Diagnostic; nothing in the UI reads it.
    /// </summary>
    [ObservableProperty]
    private SweepResult? _lastSweepResult;

    private Task _sweepTask = Task.CompletedTask;
    private int _sweepPending;

    /// <summary>Completes when any sweep started so far has finished. For tests and diagnostics.</summary>
    public Task WaitForSweepAsync() => Volatile.Read(ref _sweepTask);

    /// <summary>
    /// Runs <c>SweepOrphanedFiles</c> once, in the background. <b>Rule (deliberately simple):</b> it
    /// runs at startup (state Ready), and once more on the first public <see cref="RefreshAsync"/>
    /// after a removal that reported <c>FilesFailed &gt; 0</c> — never after an ordinary removal.
    /// </summary>
    /// <remarks>
    /// The call goes through <see cref="InvokeAsync{T}"/>, i.e. the same <c>_ffiGate</c> as every
    /// removal, import and encrypt, so it can never overlap one: it queues behind whatever is in
    /// flight. It never runs on the caller's or the UI thread, and every failure is swallowed
    /// (this class logs nothing, so there is nothing to log; the outcome is counts-only anyway).
    /// </remarks>
    private void StartBackgroundSweep()
    {
        if (_disposed || _core is null)
        {
            return;
        }

        var previous = Volatile.Read(ref _sweepTask);
        var next = Task.Run(async () =>
        {
            try
            {
                await previous.ConfigureAwait(false);
                if (!TryGetCore(out var core))
                {
                    return;
                }

                var result = await InvokeAsync(() => core.SweepOrphanedFiles()).ConfigureAwait(false);
                if (result.Failed)
                {
                    return;
                }

                var o = result.Value;
                var mapped = new SweepResult(
                    (int)o.FilesScanned, (int)o.FilesDeleted, (int)o.FilesMissing, (int)o.FilesFailed,
                    MapKinds(o.FailureKinds));
                await OnUiAsync(() => LastSweepResult = mapped).ConfigureAwait(false);
            }
            catch (Exception e) when (e is not OutOfMemoryException and not StackOverflowException)
            {
                // Housekeeping only: never surfaced, and this class logs nothing (see remarks).
            }
        });
        Volatile.Write(ref _sweepTask, next);
    }

    private static IReadOnlyList<FileDeleteFailureKind> MapKinds(FfiFileDeleteFailureKind[]? kinds) =>
        (kinds ?? Array.Empty<FfiFileDeleteFailureKind>())
            .Select(k => k switch
            {
                FfiFileDeleteFailureKind.Locked => FileDeleteFailureKind.Locked,
                FfiFileDeleteFailureKind.Permission => FileDeleteFailureKind.Permission,
                _ => FileDeleteFailureKind.Other,
            })
            .ToArray();

    /// <summary>
    /// Re-attempts initialisation after any non-ready state, including
    /// <see cref="CoreClientState.KeyStoreCorrupt"/> (review Q9: Retry is always offered).
    /// </summary>
    /// <remarks>
    /// This only re-reads. It never deletes, moves, rewrites or regenerates the key store — a
    /// corrupt key file that is later repaired (profile restored, correct user logged in) simply
    /// starts working again, and one that is not stays blocked.
    /// </remarks>
    public Task<bool> RetryAsync() => InitializeAsync();

    /// <summary>
    /// Bridges the already-obtained key to uniffi's callback interface. It returns a value it
    /// already holds, so it cannot throw — which is the whole point: a throw inside a uniffi
    /// callback has no error channel and degrades to an opaque panic.
    /// </summary>
    private sealed class PreObtainedKeyProvider : KeyProvider
    {
        private readonly byte[] _key;

        internal PreObtainedKeyProvider(byte[] key) => _key = key;

        public byte[] GetOrCreateKey() => _key;
    }

    // ── Library ────────────────────────────────────────────────────────────

    /// <summary>Reloads <see cref="Items"/> from the core (newest first).</summary>
    public async Task RefreshAsync()
    {
        await ReloadItemsAsync(clearErrorOnSuccess: true).ConfigureAwait(false);

        // The idle moment a failed-file removal was waiting for (see StartBackgroundSweep).
        if (IsReady && Interlocked.Exchange(ref _sweepPending, 0) == 1)
        {
            StartBackgroundSweep();
        }
    }

    /// <summary>
    /// Reloads <see cref="Items"/>.
    /// </summary>
    /// <param name="clearErrorOnSuccess">
    /// False when this reload is the tail end of another operation (import, remove, encrypt). That
    /// operation has already published its own outcome, and a successful reload must not wipe a
    /// failure the user has not seen yet — the DRM dialog would never appear if it did.
    /// </param>
    private async Task ReloadItemsAsync(bool clearErrorOnSuccess)
    {
        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return;
        }

        await OnUiAsync(() => IsLoading = true).ConfigureAwait(false);
        try
        {
            var result = await InvokeAsync(() => core.ListItems(0, 500)).ConfigureAwait(false);
            if (result.Failed)
            {
                await PublishErrorAsync(result.Error).ConfigureAwait(false);
                return;
            }

            var mapped = Map(result.Value);
            await OnUiAsync(() =>
            {
                Items = mapped;
                if (clearErrorOnSuccess)
                {
                    LastError = null;
                }
            }).ConfigureAwait(false);
        }
        finally
        {
            await OnUiAsync(() => IsLoading = false).ConfigureAwait(false);
        }
    }

    /// <summary>
    /// Runs an FTS5 search and publishes <see cref="SearchResults"/>.
    /// </summary>
    /// <remarks>
    /// A null, empty or whitespace-only query clears the results <b>without an FFI round trip</b>,
    /// matching Apple. Debouncing keystrokes is a UI concern and deliberately lives in the view,
    /// not here (ui-spec §4.2).
    /// </remarks>
    public async Task SearchAsync(string? query)
    {
        if (string.IsNullOrWhiteSpace(query))
        {
            await OnUiAsync(() => SearchResults = NoItems).ConfigureAwait(false);
            return;
        }

        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return;
        }

        await OnUiAsync(() => IsSearching = true).ConfigureAwait(false);
        try
        {
            var result = await InvokeAsync(() => core.SearchItems(query, 200)).ConfigureAwait(false);
            if (result.Failed)
            {
                await PublishErrorAsync(result.Error).ConfigureAwait(false);
                return;
            }

            var mapped = Map(result.Value);
            await OnUiAsync(() =>
            {
                SearchResults = mapped;
                LastError = null;
            }).ConfigureAwait(false);
        }
        finally
        {
            await OnUiAsync(() => IsSearching = false).ConfigureAwait(false);
        }
    }

    /// <summary>Clears any published search results without touching the core.</summary>
    public Task ClearSearchAsync() => OnUiAsync(() => SearchResults = NoItems);

    /// <summary>
    /// Imports a file (txt/epub/docx — the core sniffs the format), then refreshes
    /// <see cref="Items"/>.
    /// </summary>
    /// <remarks>
    /// A DRM-protected file is reported by setting <see cref="DrmProtectedSource"/> and a
    /// <see cref="CoreErrorKind.DrmProtected"/> error, matched on the
    /// <c>GistException.DrmProtected</c> <em>type</em>, never on message text (ui-spec §10 item 4).
    /// A path that no longer exists is caught in managed code as
    /// <see cref="CoreErrorKind.SourceFileMissing"/> rather than being sent to Rust to come back as
    /// an untyped core error.
    /// </remarks>
    /// <returns>The new item id, or null on failure.</returns>
    public async Task<string?> ImportFileAsync(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);

        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return null;
        }

        await OnUiAsync(() => DrmProtectedSource = null).ConfigureAwait(false);

        if (!File.Exists(path))
        {
            await PublishErrorAsync(new CoreError(
                CoreErrorKind.SourceFileMissing,
                "The selected file could not be found.")).ConfigureAwait(false);
            return null;
        }

        await OnUiAsync(() => IsLoading = true).ConfigureAwait(false);
        try
        {
            var result = await InvokeAsync(() => core.ImportFile(path)).ConfigureAwait(false);
            if (result.Failed)
            {
                if (result.Error.Kind == CoreErrorKind.DrmProtected)
                {
                    await OnUiAsync(() => DrmProtectedSource = path).ConfigureAwait(false);
                }

                await PublishErrorAsync(result.Error).ConfigureAwait(false);
                return null;
            }

            await OnUiAsync(() => LastError = null).ConfigureAwait(false);
            return result.Value;
        }
        finally
        {
            await OnUiAsync(() => IsLoading = false).ConfigureAwait(false);
            await ReloadItemsAsync(clearErrorOnSuccess: false).ConfigureAwait(false);
        }
    }

    /// <summary>
    /// Imports a web page by URL. Mirrors <see cref="ImportFileAsync"/>'s error shape, including
    /// the DRM branch — unreachable in practice for a web fetch, but the shared error type carries
    /// the case, so it is handled rather than silently lumped in with generic failures.
    /// </summary>
    public async Task<string?> ImportUrlAsync(string url)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(url);

        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return null;
        }

        await OnUiAsync(() =>
        {
            DrmProtectedSource = null;
            IsLoading = true;
        }).ConfigureAwait(false);

        try
        {
            var result = await InvokeAsync(() => core.ImportUrl(url)).ConfigureAwait(false);
            if (result.Failed)
            {
                if (result.Error.Kind == CoreErrorKind.DrmProtected)
                {
                    await OnUiAsync(() => DrmProtectedSource = url).ConfigureAwait(false);
                }

                await PublishErrorAsync(result.Error).ConfigureAwait(false);
                return null;
            }

            await OnUiAsync(() => LastError = null).ConfigureAwait(false);
            return result.Value;
        }
        finally
        {
            await OnUiAsync(() => IsLoading = false).ConfigureAwait(false);
            await ReloadItemsAsync(clearErrorOnSuccess: false).ConfigureAwait(false);
        }
    }

    /// <summary>
    /// Removes items from the library.
    /// </summary>
    /// <param name="ids">Item ids; ids with no matching row are silently skipped by the core.</param>
    /// <param name="deleteSourceFiles">
    /// Whether to also delete GIST's own sandboxed ADR-006 copy under
    /// <c>&lt;storageDir&gt;/originals/</c>. <b>The user's real file is never touched either way</b>
    /// — that is the whole point of copy-on-import, and there is a test for it. The Windows UI
    /// always passes <see langword="true"/> (removal is a complete delete, maintainer decision
    /// 2026-09-21); the parameter survives because Apple still offers the older two-button dialog.
    /// A copy shared with a surviving item is kept by the core regardless — see
    /// <see cref="RemoveResult.SharedCopiesKept"/>.
    /// </param>
    /// <returns>
    /// What the removal did. A file that could not be deleted is reported in the result
    /// (<see cref="RemoveResult.HasFileFailures"/>) and is <b>not</b> an error: the database row is
    /// gone, which is the success. <see cref="LastError"/> is only set when the removal itself failed.
    /// </returns>
    public async Task<RemoveResult> RemoveItemsAsync(IReadOnlyList<string> ids, bool deleteSourceFiles)
    {
        ArgumentNullException.ThrowIfNull(ids);
        if (ids.Count == 0)
        {
            return RemoveResult.Empty;
        }

        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return RemoveResult.Empty;
        }

        var idArray = ids.ToArray();
        var result = await InvokeAsync(() => core.RemoveItemsDetailed(idArray, deleteSourceFiles))
            .ConfigureAwait(false);

        var mapped = RemoveResult.Empty;
        if (result.Failed)
        {
            await PublishErrorAsync(result.Error).ConfigureAwait(false);
        }
        else
        {
            var o = result.Value;
            mapped = new RemoveResult(
                o.RemovedIds ?? Array.Empty<string>(),
                (int)o.FilesDeleted, (int)o.FilesMissing, (int)o.FilesFailed,
                MapKinds(o.FailureKinds), (int)o.SharedCopiesKept);
            if (mapped.HasFileFailures)
            {
                // Retry on the next idle refresh (see StartBackgroundSweep).
                Interlocked.Exchange(ref _sweepPending, 1);
            }

            await OnUiAsync(() => LastError = null).ConfigureAwait(false);
        }

        await ReloadItemsAsync(clearErrorOnSuccess: false).ConfigureAwait(false);
        return mapped;
    }

    // ── Collections ────────────────────────────────────────────────────────

    /// <summary>Creates a collection and refreshes <see cref="Collections"/>.</summary>
    /// <returns>The new collection id, or null on failure.</returns>
    public async Task<string?> CreateCollectionAsync(string name)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        var id = await RunAsync(core => core.CreateCollection(name), (string?)null).ConfigureAwait(false);
        await ListCollectionsAsync().ConfigureAwait(false);
        return id;
    }

    /// <summary>Reloads <see cref="Collections"/>.</summary>
    public async Task ListCollectionsAsync()
    {
        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return;
        }

        var result = await InvokeAsync(core.ListCollections).ConfigureAwait(false);
        if (result.Failed)
        {
            await PublishErrorAsync(result.Error).ConfigureAwait(false);
            return;
        }

        var mapped = result.Value.Select(c => new CollectionVM(c.Id, c.Name, c.CreatedAt)).ToList();
        await OnUiAsync(() =>
        {
            Collections = mapped;
            LastError = null;
        }).ConfigureAwait(false);
    }

    public Task AddItemToCollectionAsync(string itemId, string collectionId) =>
        RunVoidAsync(core => core.AddItemToCollection(itemId, collectionId));

    public Task RemoveItemFromCollectionAsync(string itemId, string collectionId) =>
        RunVoidAsync(core => core.RemoveItemFromCollection(itemId, collectionId));

    /// <summary>
    /// Items in a collection (newest first). Returned rather than published: only one collection is
    /// browsed at a time, so the view holds it as local state (same choice Apple made).
    /// </summary>
    public Task<IReadOnlyList<LibraryItemVM>> ListItemsInCollectionAsync(string collectionId) =>
        RunAsync(core => Map(core.ListItemsInCollection(collectionId)), NoItems);

    // ── Tags ───────────────────────────────────────────────────────────────

    public Task AddTagAsync(string itemId, string tagName) =>
        RunVoidAsync(core => core.AddTag(itemId, tagName));

    public Task RemoveTagAsync(string itemId, string tagName) =>
        RunVoidAsync(core => core.RemoveTag(itemId, tagName));

    public Task<IReadOnlyList<string>> ListTagsForItemAsync(string itemId) =>
        RunAsync(core => (IReadOnlyList<string>)core.ListTagsForItem(itemId), NoStrings);

    /// <summary>Reloads <see cref="AllTags"/> — every tag in the library, for the Filter menu.</summary>
    public async Task ListAllTagsAsync()
    {
        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return;
        }

        var result = await InvokeAsync(core.ListAllTags).ConfigureAwait(false);
        if (result.Failed)
        {
            await PublishErrorAsync(result.Error).ConfigureAwait(false);
            return;
        }

        var tags = (IReadOnlyList<string>)result.Value;
        await OnUiAsync(() =>
        {
            AllTags = tags;
            LastError = null;
        }).ConfigureAwait(false);
    }

    /// <summary>Items carrying a tag (newest first). Local view state, like collection contents.</summary>
    public Task<IReadOnlyList<LibraryItemVM>> ListItemsByTagAsync(string tagName) =>
        RunAsync(core => Map(core.ListItemsByTag(tagName)), NoItems);

    // ── Per-item encryption (ADR-014) ──────────────────────────────────────

    /// <summary>
    /// Retroactively encrypts the given items at rest, then refreshes so
    /// <see cref="LibraryItemVM.ContentEncrypted"/> reflects the outcome.
    /// </summary>
    /// <remarks>
    /// Uses the same key this client was opened with, so an item encrypted here is still readable
    /// through this same instance (<c>NewWithReadKey</c>, ADR-014) — the bug Apple shipped and
    /// fixed. Calling it twice is an idempotent no-op for already-encrypted items. It never touches
    /// the ADR-006 sandboxed copy under <c>originals/</c>, nor the user's real file.
    /// </remarks>
    public async Task<EncryptItemsSummary> EncryptItemsAsync(IReadOnlyList<string> ids)
    {
        ArgumentNullException.ThrowIfNull(ids);
        if (ids.Count == 0)
        {
            return EncryptItemsSummary.Empty;
        }

        if (!TryGetCore(out var core) || _key is null)
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return new EncryptItemsSummary(0, 0, ids.Count);
        }

        var idArray = ids.ToArray();
        var provider = new PreObtainedKeyProvider(_key);
        var result = await InvokeAsync(() => core.EncryptItems(idArray, provider)).ConfigureAwait(false);

        EncryptItemsSummary summary;
        if (result.Failed)
        {
            await PublishErrorAsync(result.Error).ConfigureAwait(false);
            summary = new EncryptItemsSummary(0, 0, idArray.Length);
        }
        else
        {
            var encrypted = 0;
            var already = 0;
            var failed = 0;
            var firstErrors = new List<string>(EncryptItemsSummary.MaxFirstErrors);
            foreach (var item in result.Value)
            {
                switch (item.Outcome)
                {
                    case FfiEncryptOutcome.Encrypted:
                        encrypted++;
                        break;
                    case FfiEncryptOutcome.AlreadyEncrypted:
                        already++;
                        break;
                    default:
                        failed++;
                        if (firstErrors.Count < EncryptItemsSummary.MaxFirstErrors)
                        {
                            // The raw string can embed ids/paths; keep only scrubbed fixed text.
                            var line = EncryptItemsSummary.ScrubFailure(item.Error);
                            if (!firstErrors.Contains(line))
                            {
                                firstErrors.Add(line);
                            }
                        }

                        break;
                }
            }

            summary = new EncryptItemsSummary(encrypted, already, failed) { FirstErrors = firstErrors };
            await OnUiAsync(() => LastError = null).ConfigureAwait(false);
        }

        await ReloadItemsAsync(clearErrorOnSuccess: false).ConfigureAwait(false);
        return summary;
    }

    // ── Reading surfaces (JSON passthroughs) ───────────────────────────────

    /// <summary>
    /// Raw <c>start_rsvp</c> JSON (token stream + pacing config). Passed through undecoded: the
    /// RSVP view models land in W4, and decoding here would fix a shape before that design exists.
    /// </summary>
    public Task<string?> StartRsvpAsync(string itemId, uint wpm) =>
        RunAsync(core => (string?)core.StartRsvp(itemId, wpm), null);

    /// <summary>
    /// Raw <c>get_document_json</c> (section/block structure) for the flow reader. Passed through
    /// undecoded for the same reason as <see cref="StartRsvpAsync"/> — the flow models are W5.
    /// </summary>
    public Task<string?> GetDocumentJsonAsync(string itemId) =>
        RunAsync(core => (string?)core.GetDocumentJson(itemId), null);

    /// <summary>Persists the RSVP token index so playback can resume later.</summary>
    public Task SaveProgressAsync(string itemId, ulong tokenIndex) =>
        RunVoidAsync(core => core.SaveProgress(itemId, tokenIndex));

    /// <summary>Cheap liveness check on the core (used by the shell's startup diagnostics).</summary>
    public Task HealthAsync() => RunVoidAsync(core => core.Health());

    /// <summary>Clears <see cref="LastError"/> and <see cref="DrmProtectedSource"/>.</summary>
    public Task ClearErrorAsync() => OnUiAsync(() =>
    {
        LastError = null;
        DrmProtectedSource = null;
    });

    // ── Plumbing ───────────────────────────────────────────────────────────

    private bool TryGetCore(out GistCore core)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        var current = _core;
        core = current!;
        return current is not null;
    }

    /// <summary>Outcome of one FFI call: a value, or a typed error. Never both.</summary>
    private readonly struct FfiResult<T>
    {
        private FfiResult(T value, CoreError? error)
        {
            Value = value;
            Error = error!;
        }

        public T Value { get; }

        public CoreError Error { get; }

        public bool Failed => Error is not null;

        public static FfiResult<T> Ok(T value) => new(value, null);

        public static FfiResult<T> Fail(CoreError error) => new(default!, error);
    }

    /// <summary>
    /// Runs one FFI call off the UI thread and turns every failure into a typed
    /// <see cref="CoreError"/>. The semaphore serialises calls into the core.
    /// </summary>
    private async Task<FfiResult<T>> InvokeAsync<T>(Func<T> call)
    {
        await _ffiGate.WaitAsync().ConfigureAwait(false);
        try
        {
            var value = await Task.Run(call).ConfigureAwait(false);
            return FfiResult<T>.Ok(value);
        }
        catch (GistException e)
        {
            return FfiResult<T>.Fail(MapGistException(e, storeOpening: false));
        }
        catch (Exception e) when (e is not OutOfMemoryException and not StackOverflowException)
        {
            return FfiResult<T>.Fail(new CoreError(
                CoreErrorKind.Unexpected,
                "Something went wrong talking to the GIST core."));
        }
        finally
        {
            _ffiGate.Release();
        }
    }

    /// <summary>Shared shape for the small value-returning operations.</summary>
    private async Task<T> RunAsync<T>(Func<GistCore, T> call, T fallback)
    {
        if (!TryGetCore(out var core))
        {
            await PublishNotInitializedAsync().ConfigureAwait(false);
            return fallback;
        }

        var result = await InvokeAsync(() => call(core)).ConfigureAwait(false);
        if (result.Failed)
        {
            await PublishErrorAsync(result.Error).ConfigureAwait(false);
            return fallback;
        }

        await OnUiAsync(() => LastError = null).ConfigureAwait(false);
        return result.Value;
    }

    private async Task RunVoidAsync(Action<GistCore> call)
    {
        await RunAsync(
            core =>
            {
                call(core);
                return true;
            },
            false).ConfigureAwait(false);
    }

    /// <summary>
    /// Maps a <c>GistException</c> to a typed, presentable error.
    /// </summary>
    /// <remarks>
    /// DRM is matched on the exception <b>type</b>, per ui-spec §10 item 4 — never by inspecting
    /// the message. <c>InternalPanic</c>'s message is deliberately dropped: panic payloads are
    /// never shown to the user (F22 keeps them out of logs; this keeps them out of the UI).
    /// </remarks>
    private static CoreError MapGistException(GistException e, bool storeOpening) => e switch
    {
        GistException.DrmProtected => new CoreError(
            CoreErrorKind.DrmProtected,
            "This book is DRM-protected and can't be imported."),
        GistException.InternalPanic => new CoreError(
            CoreErrorKind.InternalPanic,
            "An internal error occurred in the GIST core. The operation was cancelled."),
        _ when storeOpening => new CoreError(
            CoreErrorKind.StoreUnavailable,
            "The GIST library could not be opened."),
        _ => new CoreError(CoreErrorKind.Core, e.Message),
    };

    private static IReadOnlyList<LibraryItemVM> Map(FfiLibraryItem[] items) =>
        Array.ConvertAll(
            items,
            i => new LibraryItemVM(i.Id, i.Title ?? "Untitled", i.Authors, i.SourcePath, i.ContentEncrypted));

    private Task PublishNotInitializedAsync()
    {
        // In a blocking key state the published error already says what is wrong and why; do not
        // overwrite it with a vaguer one.
        if (LastError is { IsBlocking: true })
        {
            return Task.CompletedTask;
        }

        return PublishErrorAsync(new CoreError(
            CoreErrorKind.NotInitialized,
            "The GIST library is not open."));
    }

    private Task PublishErrorAsync(CoreError error) => OnUiAsync(() => LastError = error);

    private async Task EnterBlockedStateAsync(CoreClientState state, CoreError error)
    {
        _core = null;
        _key = null;
        await OnUiAsync(() =>
        {
            State = state;
            LastError = error;
        }).ConfigureAwait(false);
    }

    /// <summary>
    /// Runs <paramref name="action"/> on the UI thread and awaits it, so callers observe published
    /// state as soon as the awaited call returns even when the dispatcher posts asynchronously.
    /// </summary>
    private Task OnUiAsync(Action action)
    {
        var completion = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        _dispatcher.Post(() =>
        {
            try
            {
                action();
                completion.TrySetResult();
            }
            catch (Exception e)
            {
                completion.TrySetException(e);
            }
        });
        return completion.Task;
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;

        // Don't free the native core under a running background sweep (bounded, best effort).
        try
        {
            Volatile.Read(ref _sweepTask).Wait(TimeSpan.FromSeconds(2));
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            // Sweep failures are swallowed by the sweep itself; nothing to do here.
        }

        _core?.Dispose();
        _core = null;
        _key = null;
        _ffiGate.Dispose();
    }
}
