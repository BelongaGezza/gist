using Gist.Core.Client;
using Gist.Core.Models;

namespace Gist.Core.ViewModels;

/// <summary>
/// The Collection screen's logic (<c>docs/windows-ui-spec.md</c> §5): one collection's contents,
/// with the same row template, sort menu, selection rules and tag editor as the Library screen —
/// and deliberately different removal semantics.
/// </summary>
/// <remarks>
/// <para>
/// <b>Remove here means remove <i>from the collection</i>.</b> The item stays in the library and
/// nothing on disk is deleted. Apple keeps this separate in <c>CollectionDetailView</c> rather than
/// unifying it with library removal, and §5 says Windows does the same; the shared parts live in
/// <see cref="LibraryViewModelBase"/> and the parts that differ are overridden here, so the two
/// removals cannot be confused for one another by accident.
/// </para>
/// <para>
/// No search, no tag filter, no Encrypt and no Import (§5, Apple parity) — the Sort menu is the
/// only list control. <see cref="CanEncrypt"/> and <see cref="CanAddToCollection"/> are therefore
/// pinned false rather than left inherited, so a shell that binds them uniformly still gets the
/// right answer.
/// </para>
/// </remarks>
public sealed class CollectionViewModel : LibraryViewModelBase
{
    private static readonly IReadOnlyList<LibraryItemVM> NoItems = Array.Empty<LibraryItemVM>();

    private IReadOnlyList<LibraryItemVM> _items = NoItems;

    /// <param name="core">The client this screen drives.</param>
    /// <param name="collection">The collection being browsed.</param>
    public CollectionViewModel(CoreClient core, CollectionVM collection)
        : base(core)
    {
        ArgumentNullException.ThrowIfNull(collection);
        Collection = collection;
    }

    /// <summary>The collection being browsed.</summary>
    public CollectionVM Collection { get; }

    /// <summary>Page header (§2: "page header shows Library/collection name").</summary>
    public string Title => Collection.Name;

    /// <inheritdoc />
    protected override IReadOnlyList<LibraryItemVM> BaseItems => _items;

    /// <inheritdoc />
    /// <remarks>Always false: §5 has no Encrypt command.</remarks>
    public override bool CanEncrypt => false;

    /// <inheritdoc />
    /// <remarks>
    /// Always false: adding to a collection is a Library-screen command. Moving items between
    /// collections from here is not in Apple's app either, so it is not in this one (§5).
    /// </remarks>
    public override bool CanAddToCollection => false;

    /// <summary>Loads this collection's contents (newest first, then sorted client-side).</summary>
    public async Task LoadAsync()
    {
        IsLoading = true;
        try
        {
            _items = await Core.ListItemsInCollectionAsync(Collection.Id).ConfigureAwait(false);
            CaptureOutcome();
        }
        finally
        {
            IsLoading = false;
            NotifyItemsChanged();
        }
    }

    /// <summary>
    /// Builds the "Remove N items from &#x201C;name&#x201D;?" confirmation. There is no second,
    /// destructive button: nothing is deleted.
    /// </summary>
    public RemovePreview RemovePreview() =>
        ViewModels.RemovePreview.ForCollection(SelectedItems, Collection.Name);

    /// <summary>
    /// Detaches the selected items from this collection. The items themselves are untouched.
    /// </summary>
    public async Task RemoveAsync()
    {
        var ids = SelectedIds;
        if (ids.Count == 0)
        {
            return;
        }

        BeginUserOperation();
        PendingDialog = LibraryDialog.None;

        foreach (var itemId in ids)
        {
            await Core.RemoveItemFromCollectionAsync(itemId, Collection.Id).ConfigureAwait(false);
        }

        CaptureOutcome();
        ClearSelection();
        await LoadAsync().ConfigureAwait(false);
    }

    /// <inheritdoc />
    /// <remarks>
    /// The Collection screen has no Filter menu, so there is no tag vocabulary to refresh after a
    /// tag edit. Skipping the call keeps a tag edit here to exactly the FFI work it needs.
    /// </remarks>
    protected override Task RefreshTagVocabularyAsync() => Task.CompletedTask;

    /// <inheritdoc />
    /// <remarks>
    /// This list comes from <c>list_items_in_collection</c>, not from
    /// <see cref="CoreClient.Items"/>, so the client republishing the library list must not be
    /// mistaken for this collection changing.
    /// </remarks>
    protected override void OnCoreChanged(string? propertyName)
    {
        // Deliberately does not call base: see the remark above.
    }
}
