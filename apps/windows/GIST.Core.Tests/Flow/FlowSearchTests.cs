using Gist.Core.Flow;

namespace Gist.Core.Tests.Flow;

public sealed class FlowSearchTests
{
    private static FlowDocument Doc(params string[] paragraphs)
    {
        var blocks = paragraphs.Select(p => (FlowBlock)new ParagraphBlock([new FlowTextRun(p, false, false, false)])).ToList();
        return new FlowDocument("d", "T", null, [new FlowSection("s0", null, blocks)]);
    }

    [Fact]
    public void Empty_query_or_empty_text_has_no_matches()
    {
        Assert.Empty(FlowSearch.FindInText("hello", ""));
        Assert.Empty(FlowSearch.FindInText("hello", null));
        Assert.Empty(FlowSearch.FindInText("", "a"));
        Assert.Empty(FlowSearch.FindInText(null, "a"));
        Assert.Empty(FlowSearch.FindAll(Doc("hello"), "", out _));
    }

    [Fact]
    public void No_match_returns_empty()
    {
        Assert.Empty(FlowSearch.FindInText("hello world", "xyz"));
    }

    [Fact]
    public void Matching_is_case_insensitive_and_non_overlapping()
    {
        var hits = FlowSearch.FindInText("Aaaa aAa", "aa");

        // "Aaaa" -> 0..2, 2..4 (non-overlapping), "aAa" -> 5..7
        Assert.Equal([0, 2, 5], hits.Select(h => h.Utf16Start));
        Assert.All(hits, h => Assert.Equal(2, h.Utf16Length));
    }

    [Fact]
    public void Greek_and_ascii_fold_case_without_culture_dependence()
    {
        var hits = FlowSearch.FindInText("Καλημέρα κόσμε", "ΚΌΣΜΕ");

        Assert.Single(hits);
        Assert.Equal(9, hits[0].Utf16Start);
    }

    [Fact]
    public void Surrogate_pairs_count_as_one_element_but_two_utf16_units()
    {
        const string text = "a😀b😀c"; // each emoji is a surrogate pair
        var hits = FlowSearch.FindInText(text, "😀");

        Assert.Equal(2, hits.Count);
        Assert.Equal((1, 2, 1, 1), hits[0]); // utf16 start 1 len 2; element start 1 len 1
        Assert.Equal((4, 2, 3, 1), hits[1]);
    }

    [Fact]
    public void A_match_may_not_split_a_surrogate_pair()
    {
        // A lone high surrogate cannot match half of a pair.
        Assert.Empty(FlowSearch.FindInText("x😀y", "\uD83D"));
        Assert.Empty(FlowSearch.FindInText("x😀y", "\uDE00"));
    }

    [Fact]
    public void A_base_letter_does_not_match_inside_a_combining_sequence()
    {
        const string decomposed = "résumé e"; // "résumé e" with combining acute accents

        var hits = FlowSearch.FindInText(decomposed, "e");

        // Only the standalone e (index 9) matches; the e's carrying U+0301 are not on a text-element boundary.
        Assert.Single(hits);
        Assert.Equal(9, hits[0].Utf16Start);
        Assert.Equal(7, hits[0].ElementStart); // r, é, s, u, m, é, space, e
    }

    [Fact]
    public void Searching_the_full_combining_sequence_matches_and_counts_one_element()
    {
        var hits = FlowSearch.FindInText("café!", "é");

        Assert.Single(hits);
        Assert.Equal((3, 2, 3, 1), hits[0]);
    }

    [Fact]
    public void Emoji_zwj_sequence_is_one_element()
    {
        const string family = "👨‍👩‍👧"; // man ZWJ woman ZWJ girl
        var hits = FlowSearch.FindInText("a" + family + "b", family);

        Assert.Single(hits);
        Assert.Equal(1, hits[0].ElementStart);
        Assert.Equal(1, hits[0].ElementLength);
        Assert.Equal(family.Length, hits[0].Utf16Length);
    }

    [Fact]
    public void Match_count_is_capped_for_a_pathological_query()
    {
        var text = new string('a', FlowSearch.MaxMatches * 3);

        var hits = FlowSearch.FindInText(text, "a");

        Assert.Equal(FlowSearch.MaxMatches, hits.Count);
    }

    [Fact]
    public void FindAll_orders_matches_by_entry_and_reports_truncation()
    {
        var doc = Doc("one two", "two three two");

        var hits = FlowSearch.FindAll(doc, "two", out var truncated);

        Assert.False(truncated);
        Assert.Equal([0, 1, 1], hits.Select(h => h.EntryIndex));
        Assert.Equal([4, 0, 10], hits.Select(h => h.Utf16Start));

        var big = Doc(Enumerable.Repeat(new string('a', 10_000), 5).ToArray());
        FlowSearch.FindAll(big, "a", out var cut);
        Assert.True(cut);
    }

    [Fact]
    public void Tables_are_searched_across_their_cells_text()
    {
        var table = new TableBlock([["Fruit", "Colour"], ["Apple", "Red"]], true);
        var doc = new FlowDocument("d", "T", null, [new FlowSection("s0", null, [table])]);

        var hits = FlowSearch.FindAll(doc, "red", out _);

        Assert.Single(hits);
        Assert.Equal(0, hits[0].EntryIndex);
    }

    [Fact]
    public async Task Real_document_search_finds_text_case_insensitively()
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("basic_ascii.txt");

        var hits = FlowSearch.FindAll(doc, "LOREM IPSUM", out _);

        Assert.NotEmpty(hits);
        Assert.Equal(0, hits[0].Utf16Start);
    }

    [Fact]
    public async Task Real_unicode_document_search_matches_cjk_and_greek()
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("unicode_utf8.txt");

        Assert.NotEmpty(FlowSearch.FindAll(doc, "世界", out _));
        Assert.NotEmpty(FlowSearch.FindAll(doc, "καλημέρα", out _));
    }

    // ── State / cursor ────────────────────────────────────────────────────────

    [Fact]
    public void State_next_and_previous_wrap_and_do_nothing_without_matches()
    {
        var state = new FlowSearchState();
        state.SetDocument(Doc("a b a b a"));
        state.FindNext();
        state.FindPrevious();
        Assert.Equal(0, state.CurrentMatchIndex);
        Assert.Null(state.CurrentMatch);

        state.SetQuery("a");
        Assert.Equal(3, state.MatchCount);
        state.FindNext();
        state.FindNext();
        Assert.Equal(2, state.CurrentMatchIndex);
        state.FindNext();
        Assert.Equal(0, state.CurrentMatchIndex); // wrapped
        state.FindPrevious();
        Assert.Equal(2, state.CurrentMatchIndex); // wrapped back
    }

    [Fact]
    public void State_resets_the_cursor_and_raises_changed_when_the_query_changes()
    {
        var state = new FlowSearchState();
        state.SetDocument(Doc("a b a b a"));
        state.SetQuery("a");
        state.FindNext();
        var changes = 0;
        state.Changed += (_, _) => changes++;

        state.SetQuery("b");
        state.SetQuery("b"); // unchanged: no event
        state.SetQuery(null); // cleared

        Assert.Equal(2, changes);
        Assert.Equal(0, state.CurrentMatchIndex);
        Assert.Equal(0, state.MatchCount);
    }

    [Fact]
    public void Setting_a_null_document_clears_matches()
    {
        var state = new FlowSearchState();
        state.SetDocument(Doc("abc"));
        state.SetQuery("a");

        state.SetDocument(null);

        Assert.Equal(0, state.MatchCount);
    }
}
