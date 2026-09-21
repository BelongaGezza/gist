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
/// <para>
/// <b>Removal is one thing (maintainer decision, 2026-09-21).</b> There used to be two library
/// buttons, "Remove from Library" (keep GIST's stored copy) and "Also Delete Stored Copy". The
/// first was misleading: the startup orphan sweep reclaims any stored copy no row references, so
/// "keep" only meant "until the next launch". Removing an item now always deletes everything GIST
/// holds for it, and the dialog offers one destructive button. See ADR-006's addendum.
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
public sealed record RemovePreview(
    int Count,
    string Title,
    IReadOnlyList<string> Titles,
    int MoreCount,
    string? MoreText,
    string Message)
{
    /// <summary>How many titles are listed before the dialog switches to "and N more".</summary>
    public const int MaxListedTitles = 5;

    /// <summary>
    /// The irreversibility line for removing exactly one item from the library. Says plainly what
    /// goes (the item and GIST's own stored copy, from this PC) and what does not (the file the
    /// user imported, wherever it lives — ADR-006, spec §10 item 1).
    /// </summary>
    public const string LibraryIrreversibleLineOne =
        "This permanently deletes this item, and GIST's stored copy of it, from this PC. "
        + "It can't be undone. The original file you imported is not touched.";

    /// <summary>The same line for a multi-item selection.</summary>
    public const string LibraryIrreversibleLineMany =
        "This permanently deletes these items, and GIST's stored copies of them, from this PC. "
        + "It can't be undone. The original files you imported are not touched.";

    /// <summary>The irreversibility line for detaching items from a collection.</summary>
    public const string CollectionIrreversibleLine =
        "The items stay in your library; only their membership of this collection is removed.";

    /// <summary>The library irreversibility line for a selection of <paramref name="count"/> items.</summary>
    public static string LibraryIrreversibleLine(int count) =>
        count == 1 ? LibraryIrreversibleLineOne : LibraryIrreversibleLineMany;

    /// <summary>Builds the Library-screen preview for <paramref name="selected"/>.</summary>
    public static RemovePreview ForLibrary(IReadOnlyList<LibraryItemVM> selected) =>
        Build(
            selected,
            count => count == 1 ? "Remove 1 item?" : $"Remove {count} items?",
            LibraryIrreversibleLine);

    /// <summary>
    /// Builds the Collection-screen preview. The title names the collection so the two removals can
    /// never be mistaken for each other (§5). Removing from a collection is not destructive and is
    /// deliberately untouched by the 2026-09-21 complete-delete decision.
    /// </summary>
    public static RemovePreview ForCollection(IReadOnlyList<LibraryItemVM> selected, string collectionName) =>
        Build(
            selected,
            count => count == 1
                ? $"Remove 1 item from “{collectionName}”?"
                : $"Remove {count} items from “{collectionName}”?",
            _ => CollectionIrreversibleLine);

    private static RemovePreview Build(
        IReadOnlyList<LibraryItemVM> selected,
        Func<int, string> title,
        Func<int, string> message)
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
            message(count));
    }
}
