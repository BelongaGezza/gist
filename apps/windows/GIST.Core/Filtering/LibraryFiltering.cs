using Gist.Core.Models;

namespace Gist.Core.Filtering;

/// <summary>
/// Pure, UI-free list logic for the library and collection views: whether a search is active,
/// which list to display, and client-side sorting. Factored out (exactly as Apple factored
/// <c>LibraryFiltering</c> out of <c>LibraryView</c>) so it is unit-testable without standing up
/// any XAML or view state.
/// </summary>
public static class LibraryFiltering
{
    /// <summary>
    /// Whether <paramref name="searchText"/> is non-blank, i.e. the view should render search
    /// results instead of the full item list. Null, empty and whitespace-only all count as
    /// inactive, matching Apple's <c>trimmingCharacters(in: .whitespacesAndNewlines)</c> check
    /// and <see cref="Gist.Core.Client.CoreClient.SearchAsync"/>, which clears results without an
    /// FFI round trip for exactly these inputs.
    /// </summary>
    public static bool IsSearchActive(string? searchText) => !string.IsNullOrWhiteSpace(searchText);

    /// <summary>
    /// Picks the list the view should render: <paramref name="searchResults"/> while a search is
    /// active, otherwise the full <paramref name="items"/> list.
    /// </summary>
    public static IReadOnlyList<LibraryItemVM> DisplayedItems(
        string? searchText,
        IReadOnlyList<LibraryItemVM> items,
        IReadOnlyList<LibraryItemVM> searchResults)
    {
        ArgumentNullException.ThrowIfNull(items);
        ArgumentNullException.ThrowIfNull(searchResults);
        return IsSearchActive(searchText) ? searchResults : items;
    }

    /// <summary>
    /// Applies <paramref name="order"/> to <paramref name="items"/>, client-side.
    /// </summary>
    /// <remarks>
    /// <para><b>Q8 decisions</b> (docs/windows-development-plan.md, W1 review item Q8), settled here
    /// once so every view and both platforms agree:</para>
    /// <list type="bullet">
    /// <item>
    /// <description>
    /// <b>Stable ordering.</b> Sorting uses LINQ <c>OrderBy</c>/<c>OrderByDescending</c>, which are
    /// documented stable: items with equal keys keep their incoming relative order — which is the
    /// core's newest-first order — in <em>both</em> directions. So "Title Z–A" is not a literal
    /// reversal of "Title A–Z": ties stay newest-first either way rather than flipping. That is the
    /// deliberate choice; a plain reverse would make tie order depend on the previous sort.
    /// </description>
    /// </item>
    /// <item>
    /// <description>
    /// <b>Comparer.</b> Title and author use <see cref="StringComparer.CurrentCultureIgnoreCase"/>,
    /// so "apple" sorts next to "Banana" by letter rather than after it by ordinal case, and
    /// accented/locale-specific letters collate the way the user's Windows locale expects. This is
    /// the closest match to Apple's <c>localizedCaseInsensitiveCompare</c>. It is culture-sensitive
    /// by design: the order shown follows the user's locale, so tests must not assume an
    /// ordinal ordering.
    /// </description>
    /// </item>
    /// <item>
    /// <description>
    /// <b><see cref="LibrarySortOrder.DateAddedNewest"/> is a true no-op</b>, returning the input
    /// untouched. Every FFI list call (<c>list_items</c>/<c>search_items</c>/
    /// <c>list_items_by_tag</c>/<c>list_items_in_collection</c>) already returns newest-first
    /// straight from SQL's <c>ORDER BY created_at DESC</c>, so re-sorting would be wasted work,
    /// and <see cref="LibrarySortOrder.DateAddedOldest"/> is therefore a reverse rather than a
    /// second query.
    /// </description>
    /// </item>
    /// <item>
    /// <description>
    /// <b>No author sorts as the empty string</b> (<see cref="LibraryItemVM.PrimaryAuthor"/>), so
    /// author-less items group first rather than being dropped or sorted last.
    /// </description>
    /// </item>
    /// </list>
    /// </remarks>
    public static IReadOnlyList<LibraryItemVM> Sorted(IReadOnlyList<LibraryItemVM> items, LibrarySortOrder order)
    {
        ArgumentNullException.ThrowIfNull(items);

        return order switch
        {
            // Already newest-first from SQL; deliberately returns the input as-is.
            LibrarySortOrder.DateAddedNewest => items,
            LibrarySortOrder.DateAddedOldest => items.Reverse().ToList(),
            LibrarySortOrder.TitleAZ =>
                items.OrderBy(i => i.Title, StringComparer.CurrentCultureIgnoreCase).ToList(),
            LibrarySortOrder.TitleZA =>
                items.OrderByDescending(i => i.Title, StringComparer.CurrentCultureIgnoreCase).ToList(),
            LibrarySortOrder.AuthorAZ =>
                items.OrderBy(i => i.PrimaryAuthor, StringComparer.CurrentCultureIgnoreCase).ToList(),
            _ => throw new ArgumentOutOfRangeException(nameof(order), order, null),
        };
    }
}
