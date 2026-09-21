using Gist.Core.Models;
using Gist.Core.ViewModels;

namespace Gist.Core.Tests.ViewModels;

/// <summary>
/// Pure tests for the Remove and Encrypt dialog content (<c>docs/windows-ui-spec.md</c> §4.5).
/// No core, no filesystem: this is string and arithmetic logic, and the arithmetic — "first five,
/// then and N more" — is exactly the part with off-by-one risk.
/// </summary>
public sealed class RemovePreviewTests
{
    private static IReadOnlyList<LibraryItemVM> Items(int count) =>
        Enumerable.Range(1, count)
            .Select(i => new LibraryItemVM($"id-{i}", $"Book {i}", Array.Empty<string>()))
            .ToList();

    [Theory]
    [InlineData(0, 0, 0)]
    [InlineData(1, 1, 0)]
    [InlineData(4, 4, 0)]
    [InlineData(5, 5, 0)]
    [InlineData(6, 5, 1)]
    [InlineData(7, 5, 2)]
    public void Library_preview_lists_at_most_five_titles_and_counts_the_rest(
        int selected,
        int expectedListed,
        int expectedMore)
    {
        var preview = RemovePreview.ForLibrary(Items(selected));

        Assert.Equal(selected, preview.Count);
        Assert.Equal(expectedListed, preview.Titles.Count);
        Assert.Equal(expectedMore, preview.MoreCount);

        if (expectedMore == 0)
        {
            Assert.Null(preview.MoreText);
        }
        else
        {
            Assert.Equal($"and {expectedMore} more", preview.MoreText);
        }

        // Listed titles are the first N in display order, not an arbitrary subset.
        Assert.Equal(
            Items(selected).Take(expectedListed).Select(i => i.Title),
            preview.Titles);
    }

    [Theory]
    [InlineData(1, "Remove 1 item?")]
    [InlineData(2, "Remove 2 items?")]
    [InlineData(7, "Remove 7 items?")]
    public void Library_preview_title_is_singular_only_for_one_item(int count, string expected) =>
        Assert.Equal(expected, RemovePreview.ForLibrary(Items(count)).Title);

    /// <summary>
    /// The irreversibility line is a spec requirement (§4.5 <b>[+]</b>), and since 2026-09-21 it
    /// has to describe a complete delete: the item <em>and</em> GIST's stored copy go, from this
    /// PC, permanently — while the file the user imported is not touched (ADR-006, §10 item 1).
    /// Both halves are asserted so neither can be quietly dropped.
    /// </summary>
    [Theory]
    [InlineData(1)]
    [InlineData(3)]
    public void Library_preview_says_the_delete_is_complete_permanent_and_spares_the_users_own_file(int count)
    {
        var preview = RemovePreview.ForLibrary(Items(count));

        Assert.Contains("permanently deletes", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("stored cop", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("from this PC", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("can't be undone", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("original file", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("not touched", preview.Message, StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>The line agrees with itself about how many items are going.</summary>
    [Fact]
    public void Library_irreversibility_line_is_singular_only_for_one_item()
    {
        Assert.Equal(RemovePreview.LibraryIrreversibleLineOne, RemovePreview.ForLibrary(Items(1)).Message);
        Assert.Equal(RemovePreview.LibraryIrreversibleLineMany, RemovePreview.ForLibrary(Items(2)).Message);
        Assert.Contains("this item", RemovePreview.LibraryIrreversibleLineOne, StringComparison.Ordinal);
        Assert.Contains("these items", RemovePreview.LibraryIrreversibleLineMany, StringComparison.Ordinal);
    }

    /// <summary>
    /// A collection removal is a different operation and must read like one: it names the
    /// collection and says the items stay in the library (§5). It is <b>not</b> destructive, so the
    /// complete-delete wording must never leak into it.
    /// </summary>
    [Fact]
    public void Collection_preview_names_the_collection_and_never_reads_as_a_delete()
    {
        var preview = RemovePreview.ForCollection(Items(2), "Reading List");

        Assert.Equal("Remove 2 items from “Reading List”?", preview.Title);
        Assert.Contains("stay in your library", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("permanently", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("can't be undone", preview.Message, StringComparison.OrdinalIgnoreCase);
        Assert.Equal(1, RemovePreview.ForCollection(Items(1), "X").Count);
        Assert.Equal(
            "Remove 1 item from “X”?",
            RemovePreview.ForCollection(Items(1), "X").Title);
    }

    /// <summary>
    /// Maintainer decision, 2026-09-21: no key recovery in v1, so the confirmation has to say so
    /// before the user commits. These assertions exist so the warning cannot be quietly softened or
    /// dropped without a test failing.
    /// </summary>
    [Fact]
    public void Encrypt_preview_warns_plainly_that_there_is_no_recovery()
    {
        var preview = EncryptPreview.For(2);

        Assert.Equal("Encrypt 2 items?", preview.Title);
        Assert.Equal("Encrypt 1 item?", EncryptPreview.For(1).Title);

        Assert.False(string.IsNullOrWhiteSpace(preview.RecoveryWarning));
        Assert.Contains("no way to recover", preview.RecoveryWarning, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("reset", preview.RecoveryWarning, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("no recovery code", preview.RecoveryWarning, StringComparison.OrdinalIgnoreCase);

        // Encryption is opt-in per item …
        Assert.Contains("optional", preview.OptionalNote, StringComparison.OrdinalIgnoreCase);
        // … and never reaches the user's file or GIST's stored copy (ADR-014 scope).
        Assert.Contains("original files are never touched", preview.OriginalsNote, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("stored", preview.OriginalsNote, StringComparison.OrdinalIgnoreCase);

        // And it still explains that the items remain readable, or the warning reads as "you lose
        // access now" rather than "you lose access if the profile goes".
        Assert.Contains("stay readable", preview.Message, StringComparison.OrdinalIgnoreCase);
    }
}
