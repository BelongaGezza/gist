using Gist.Core.Models;

namespace Gist.Core.Tests.Models;

/// <summary>
/// Equality and identity behaviour the sidebar and list views depend on. Ported from Apple's
/// <c>LibrarySelection</c>/<c>CollectionVM</c> tests.
/// </summary>
public sealed class LibraryModelsTests
{
    // ── CollectionVM ───────────────────────────────────────────────────────

    /// <summary>
    /// Component-wise, not id-only. If this ever degraded to comparing ids, a sidebar selection
    /// captured before a rename would still compare equal afterwards and the detail pane could show
    /// the wrong collection's contents.
    /// </summary>
    [Fact]
    public void CollectionVM_equality_is_component_wise()
    {
        var a = new CollectionVM("same-id", "A", 1);
        var renamed = new CollectionVM("same-id", "B", 1);
        var retimed = new CollectionVM("same-id", "A", 2);

        Assert.Equal(a, new CollectionVM("same-id", "A", 1));
        Assert.NotEqual(a, renamed);
        Assert.NotEqual(a, retimed);
    }

    [Fact]
    public void CollectionVM_hashes_component_wise()
    {
        var set = new HashSet<CollectionVM>
        {
            new("same-id", "A", 1),
            new("same-id", "B", 1),
            new("same-id", "A", 1),
        };

        Assert.Equal(2, set.Count);
    }

    // ── LibrarySelection ───────────────────────────────────────────────────

    [Fact]
    public void LibrarySelection_titles()
    {
        Assert.Equal("Library", LibrarySelection.AllItems.Title);
        Assert.Equal("Sci-Fi", LibrarySelection.ForCollection(new CollectionVM("c1", "Sci-Fi", 0)).Title);
    }

    [Fact]
    public void LibrarySelection_equality_is_component_wise_through_the_collection()
    {
        var a = LibrarySelection.ForCollection(new CollectionVM("same-id", "A", 1));
        var b = LibrarySelection.ForCollection(new CollectionVM("same-id", "B", 1));

        Assert.NotEqual(a, b);
        Assert.Equal(a, LibrarySelection.ForCollection(new CollectionVM("same-id", "A", 1)));
        Assert.NotEqual(LibrarySelection.AllItems, a);
    }

    [Fact]
    public void LibrarySelection_is_usable_as_a_set_element()
    {
        var collection = new CollectionVM("c1", "Sci-Fi", 0);
        var selections = new HashSet<LibrarySelection>
        {
            LibrarySelection.AllItems,
            LibrarySelection.ForCollection(collection),
            LibrarySelection.AllItems,
            LibrarySelection.ForCollection(collection),
        };

        Assert.Equal(2, selections.Count);
    }

    [Fact]
    public void LibrarySelection_All_is_a_singleton_and_compares_equal_to_itself()
    {
        Assert.Same(LibrarySelection.AllItems, LibrarySelection.AllItems);
        Assert.Equal(LibrarySelection.AllItems, LibrarySelection.AllItems);
    }

    // ── LibraryItemVM ──────────────────────────────────────────────────────

    /// <summary>
    /// Authors compare element-by-element. The compiler-generated record equality would compare the
    /// collection by reference, so two identical items mapped from two separate FFI calls would
    /// never compare equal — a trap for any caller diffing list snapshots.
    /// </summary>
    [Fact]
    public void LibraryItemVM_equality_compares_authors_element_wise()
    {
        var a = new LibraryItemVM("1", "Title", new[] { "Ann", "Bo" }, "C:/x.txt");
        var same = new LibraryItemVM("1", "Title", new List<string> { "Ann", "Bo" }, "C:/x.txt");
        var reordered = new LibraryItemVM("1", "Title", new[] { "Bo", "Ann" }, "C:/x.txt");

        Assert.Equal(a, same);
        Assert.Equal(a.GetHashCode(), same.GetHashCode());
        Assert.NotEqual(a, reordered);
    }

    [Fact]
    public void LibraryItemVM_defaults_are_unencrypted_with_no_source_path()
    {
        var item = new LibraryItemVM("1", "Title", Array.Empty<string>());

        Assert.Null(item.SourcePath);
        Assert.False(item.ContentEncrypted);
        Assert.Equal(string.Empty, item.PrimaryAuthor);
    }

    [Fact]
    public void LibraryItemVM_primary_author_is_the_first_author()
    {
        Assert.Equal("Ann", new LibraryItemVM("1", "T", new[] { "Ann", "Bo" }).PrimaryAuthor);
    }

    // ── EncryptItemsSummary ────────────────────────────────────────────────

    [Theory]
    [InlineData(0, 0, 0, "No items were selected.")]
    [InlineData(1, 0, 0, "1 item encrypted.")]
    [InlineData(2, 1, 0, "2 items encrypted, 1 was already encrypted.")]
    [InlineData(0, 2, 1, "2 were already encrypted, 1 failed.")]
    public void EncryptItemsSummary_message_mentions_only_what_happened(
        int encrypted, int already, int failed, string expected)
    {
        Assert.Equal(expected, new EncryptItemsSummary(encrypted, already, failed).Message);
    }

    [Fact]
    public void EncryptItemsSummary_empty_is_empty()
    {
        Assert.True(EncryptItemsSummary.Empty.IsEmpty);
        Assert.False(new EncryptItemsSummary(1, 0, 0).IsEmpty);
    }
}
