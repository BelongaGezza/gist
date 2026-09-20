using System.Globalization;
using Gist.Core.Filtering;
using Gist.Core.Models;

namespace Gist.Core.Tests.Filtering;

/// <summary>
/// <see cref="LibraryFiltering"/> is pure logic precisely so it can be tested without any view
/// state. Ported from Apple's <c>LibraryFilteringTests</c>, plus the Unicode cases review item Q8
/// asks for.
/// </summary>
public sealed class LibraryFilteringTests
{
    private static LibraryItemVM Item(string id, string title, params string[] authors) =>
        new(id, title, authors);

    private static readonly IReadOnlyList<LibraryItemVM> Unsorted = new[]
    {
        Item("1", "Banana", "Zeta"),
        Item("2", "apple", "Alpha"),
        Item("3", "Cherry"),
    };

    // ── IsSearchActive / DisplayedItems ────────────────────────────────────

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("\t\n")]
    public void IsSearchActive_is_false_for_blank_text(string? text) =>
        Assert.False(LibraryFiltering.IsSearchActive(text));

    [Theory]
    [InlineData("abc")]
    [InlineData("  abc  ")]
    public void IsSearchActive_is_true_for_non_blank_text(string text) =>
        Assert.True(LibraryFiltering.IsSearchActive(text));

    [Fact]
    public void DisplayedItems_returns_the_full_list_when_search_is_inactive()
    {
        var items = new[] { Item("1", "A") };
        var results = new[] { Item("2", "B") };

        Assert.Equal(new[] { "1" }, LibraryFiltering.DisplayedItems("", items, results).Select(i => i.Id));
    }

    [Fact]
    public void DisplayedItems_returns_search_results_when_search_is_active()
    {
        var items = new[] { Item("1", "A") };
        var results = new[] { Item("2", "B") };

        Assert.Equal(new[] { "2" }, LibraryFiltering.DisplayedItems("query", items, results).Select(i => i.Id));
    }

    // ── Sorting ────────────────────────────────────────────────────────────

    /// <summary>
    /// A true no-op, not merely "looks the same today": every FFI list call already returns
    /// newest-first, so re-sorting would be redundant at best and wrong if a caller's list were not
    /// already in that order.
    /// </summary>
    [Fact]
    public void Sorted_by_DateAddedNewest_returns_the_input_untouched()
    {
        var result = LibraryFiltering.Sorted(Unsorted, LibrarySortOrder.DateAddedNewest);

        Assert.Same(Unsorted, result);
        Assert.Equal(new[] { "1", "2", "3" }, result.Select(i => i.Id));
    }

    [Fact]
    public void Sorted_by_DateAddedOldest_reverses_the_input()
    {
        var result = LibraryFiltering.Sorted(Unsorted, LibrarySortOrder.DateAddedOldest);

        Assert.Equal(new[] { "3", "2", "1" }, result.Select(i => i.Id));
    }

    [Fact]
    public void Sorted_by_title_is_case_insensitive()
    {
        var result = LibraryFiltering.Sorted(Unsorted, LibrarySortOrder.TitleAZ);

        // "apple" sorts with the others by letter, not after them by ordinal case.
        Assert.Equal(new[] { "apple", "Banana", "Cherry" }, result.Select(i => i.Title));
    }

    [Fact]
    public void Sorted_by_title_descending_is_the_reverse_ordering()
    {
        var result = LibraryFiltering.Sorted(Unsorted, LibrarySortOrder.TitleZA);

        Assert.Equal(new[] { "Cherry", "Banana", "apple" }, result.Select(i => i.Title));
    }

    [Fact]
    public void Sorted_by_author_treats_a_missing_author_as_the_empty_string()
    {
        var result = LibraryFiltering.Sorted(Unsorted, LibrarySortOrder.AuthorAZ);

        // "" (Cherry, no author) before "Alpha" before "Zeta".
        Assert.Equal(new[] { "3", "2", "1" }, result.Select(i => i.Id));
    }

    /// <summary>
    /// Q8: the sort must be <b>stable</b>, so items with equal keys keep the core's newest-first
    /// order rather than being shuffled. A naive <c>List.Sort</c>/<c>Array.Sort</c> would not
    /// guarantee this.
    /// </summary>
    [Fact]
    public void Sorted_is_stable_for_equal_title_keys()
    {
        var items = new[]
        {
            Item("first", "Same"),
            Item("second", "SAME"),
            Item("third", "same"),
        };

        Assert.Equal(
            new[] { "first", "second", "third" },
            LibraryFiltering.Sorted(items, LibrarySortOrder.TitleAZ).Select(i => i.Id));

        // Descending is OrderByDescending, not a reverse, so ties keep their incoming order too.
        Assert.Equal(
            new[] { "first", "second", "third" },
            LibraryFiltering.Sorted(items, LibrarySortOrder.TitleZA).Select(i => i.Id));
    }

    [Fact]
    public void Sorted_is_stable_for_equal_author_keys()
    {
        var items = new[]
        {
            Item("a", "One"),
            Item("b", "Two"),
            Item("c", "Three"),
        };

        Assert.Equal(
            new[] { "a", "b", "c" },
            LibraryFiltering.Sorted(items, LibrarySortOrder.AuthorAZ).Select(i => i.Id));
    }

    /// <summary>
    /// Q8: the comparer is culture-aware, so titles that are not plain ASCII still sort sensibly
    /// rather than by UTF-16 code unit. These assertions deliberately check only properties that
    /// hold in every culture — an accented letter groups with its base letter rather than after
    /// every unaccented word, and case folding still applies.
    /// </summary>
    [Fact]
    public void Sorted_by_title_handles_accented_letters_as_their_base_letter()
    {
        var items = new[]
        {
            Item("z", "Zebra"),
            Item("e", "Éclair"),
            Item("a", "apple"),
        };

        // The comparer is current-culture by design, so the culture is pinned here rather than
        // letting the assertion depend on the machine's locale (a Swedish collation, for one,
        // orders accented letters differently and would legitimately fail this expectation).
        var previous = CultureInfo.CurrentCulture;
        try
        {
            CultureInfo.CurrentCulture = new CultureInfo("en-US");
            var titles = LibraryFiltering.Sorted(items, LibrarySortOrder.TitleAZ).Select(i => i.Title).ToList();
            Assert.Equal(new[] { "apple", "Éclair", "Zebra" }, titles);
        }
        finally
        {
            CultureInfo.CurrentCulture = previous;
        }
    }

    /// <summary>
    /// Emoji, surrogate pairs and combining marks must not throw or lose items. Their relative
    /// order is a locale decision, so only the invariants are asserted: nothing is dropped or
    /// duplicated.
    /// </summary>
    [Fact]
    public void Sorted_by_title_survives_emoji_surrogate_pairs_and_combining_marks()
    {
        var items = new[]
        {
            Item("emoji", "📚 Reading list"),
            Item("combining", "Café"),      // "Café" as e + combining acute
            Item("precomposed", "Café"),      // "Café" precomposed
            Item("cjk", "日本語"),
            Item("empty", string.Empty),
        };

        foreach (var order in new[] { LibrarySortOrder.TitleAZ, LibrarySortOrder.TitleZA, LibrarySortOrder.AuthorAZ })
        {
            var result = LibraryFiltering.Sorted(items, order);

            Assert.Equal(items.Length, result.Count);
            Assert.Equal(
                items.Select(i => i.Id).OrderBy(i => i, StringComparer.Ordinal),
                result.Select(i => i.Id).OrderBy(i => i, StringComparer.Ordinal));
        }
    }

    [Fact]
    public void Sorted_of_an_empty_list_is_empty_for_every_order()
    {
        foreach (var order in Enum.GetValues<LibrarySortOrder>())
        {
            Assert.Empty(LibraryFiltering.Sorted(Array.Empty<LibraryItemVM>(), order));
        }
    }

    [Fact]
    public void Every_sort_order_has_a_distinct_label()
    {
        var labels = Enum.GetValues<LibrarySortOrder>().Select(o => o.Label()).ToList();

        Assert.Equal(labels.Count, labels.Distinct(StringComparer.Ordinal).Count());
        Assert.DoesNotContain(labels, string.IsNullOrWhiteSpace);
    }
}
