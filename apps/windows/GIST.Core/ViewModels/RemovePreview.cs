using Gist.Core.Models;

namespace Gist.Core.ViewModels;

/// <summary>
/// Everything the Remove confirmation dialog needs, computed from the current selection
/// (<c>docs/windows-ui-spec.md</c> §4.5 and §5).
/// </summary>
/// <remarks>
/// <para>
/// Apple's dialog shows a count and nothing else. The Windows spec marks the title list a
/// <b>[+]</b> improvement (product spec §4): the user is told <em>which</em> books are about to go,
/// truncated to the first five, and told plainly that it cannot be undone.
/// </para>
/// <para>
/// Computed as data rather than formatted inside a XAML page so the truncation arithmetic —
/// the part with off-by-one risk — is unit-tested directly.
/// </para>
/// </remarks>
/// <param name="Count">How many items the confirmation covers.</param>
/// <param name="Title">Dialog title, e.g. <c>"Remove 3 items?"</c>.</param>
/// <param name="Titles">
/// The first <see cref="MaxListedTitles"/> item titles, in display order. Items with no parsed
/// title read <c>"Untitled"</c> (§4.1), which the client already substitutes at the FFI boundary.
/// </param>
/// <param name="MoreCount">How many selected items are <em>not</em> in <see cref="Titles"/>.</param>
/// <param name="MoreText">
/// <c>"and 2 more"</c>, or <see langword="null"/> when nothing was truncated, so the shell can bind
/// a line's visibility straight to it.
/// </param>
/// <param name="Message">The irreversibility line, always present.</param>
/// <param name="AllowsDeletingStoredCopy">
/// Whether the dialog offers the second, destructive button. True on the Library screen
/// ("Also Delete Stored Copy"), false on a Collection screen, where removal only detaches the item
/// from the collection.
/// </param>
public sealed record RemovePreview(
    int Count,
    string Title,
    IReadOnlyList<string> Titles,
    int MoreCount,
    string? MoreText,
    string Message,
    bool AllowsDeletingStoredCopy)
{
    /// <summary>How many titles are listed before the dialog switches to "and N more".</summary>
    public const int MaxListedTitles = 5;

    /// <summary>
    /// The irreversibility line for removing items from the library. States what is and is not
    /// deleted, because the two buttons differ only in whether GIST's own stored copy goes too —
    /// the user's original file is never touched by either (ADR-006, spec §10 item 1).
    /// </summary>
    public const string LibraryIrreversibleLine =
        "This can't be undone. GIST forgets these items and deletes its own stored copy of the text. "
        + "Your original files are never touched, wherever they live — "
        + "\"Also Delete Stored Copy\" only removes the copy GIST made when importing.";

    /// <summary>The irreversibility line for detaching items from a collection.</summary>
    public const string CollectionIrreversibleLine =
        "The items stay in your library; only their membership of this collection is removed.";

    /// <summary>Builds the Library-screen preview for <paramref name="selected"/>.</summary>
    public static RemovePreview ForLibrary(IReadOnlyList<LibraryItemVM> selected) =>
        Build(
            selected,
            count => count == 1 ? "Remove 1 item?" : $"Remove {count} items?",
            LibraryIrreversibleLine,
            allowsDeletingStoredCopy: true);

    /// <summary>
    /// Builds the Collection-screen preview. The title names the collection so the two removals can
    /// never be mistaken for each other (§5).
    /// </summary>
    public static RemovePreview ForCollection(IReadOnlyList<LibraryItemVM> selected, string collectionName) =>
        Build(
            selected,
            count => count == 1
                ? $"Remove 1 item from “{collectionName}”?"
                : $"Remove {count} items from “{collectionName}”?",
            CollectionIrreversibleLine,
            allowsDeletingStoredCopy: false);

    private static RemovePreview Build(
        IReadOnlyList<LibraryItemVM> selected,
        Func<int, string> title,
        string message,
        bool allowsDeletingStoredCopy)
    {
        ArgumentNullException.ThrowIfNull(selected);

        var count = selected.Count;
        var listed = selected.Take(MaxListedTitles).Select(i => i.Title).ToList();
        var more = count - listed.Count;

        return new RemovePreview(
            count,
            title(count),
            listed,
            more,
            more > 0 ? $"and {more} more" : null,
            message,
            allowsDeletingStoredCopy);
    }
}
