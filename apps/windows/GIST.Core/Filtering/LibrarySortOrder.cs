namespace Gist.Core.Filtering;

/// <summary>
/// The five sort orders offered by the library Sort menu
/// (<c>docs/windows-ui-spec.md</c> §4.3). Mirrors Apple's <c>LibrarySortOrder</c> one-for-one;
/// adding a case here without adding it there is a parity break.
/// </summary>
public enum LibrarySortOrder
{
    /// <summary>Newest first. A no-op — see <see cref="LibraryFiltering.Sorted"/>.</summary>
    DateAddedNewest = 0,

    /// <summary>Oldest first: the reverse of the core's own ordering.</summary>
    DateAddedOldest,

    TitleAZ,
    TitleZA,
    AuthorAZ,
}

/// <summary>Display labels for <see cref="LibrarySortOrder"/>, kept out of the enum itself.</summary>
public static class LibrarySortOrderExtensions
{
    /// <summary>Menu label, matching Apple's wording and the Windows UI spec §4.2 item 3.</summary>
    public static string Label(this LibrarySortOrder order) => order switch
    {
        LibrarySortOrder.DateAddedNewest => "Date Added (Newest)",
        LibrarySortOrder.DateAddedOldest => "Date Added (Oldest)",
        LibrarySortOrder.TitleAZ => "Title (A–Z)",
        LibrarySortOrder.TitleZA => "Title (Z–A)",
        LibrarySortOrder.AuthorAZ => "Author (A–Z)",
        _ => throw new ArgumentOutOfRangeException(nameof(order), order, null),
    };
}
