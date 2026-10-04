using System.Globalization;
using Gist.Core.Flow;

namespace Gist.Core.Tests.Flow;

public sealed class FlowBlockChunkerTests
{
    private static ParagraphBlock Para(params string[] runs) =>
        new(runs.Select((t, i) => new FlowTextRun(t, i % 2 == 1, false, false)).ToList());

    private static void AssertTilesPlainText(FlowBlock original, IReadOnlyList<FlowBlockChunker.Chunk> chunks, string separator)
    {
        var text = original.PlainText;
        Assert.Equal(text, string.Join(separator, chunks.Select(c => c.Block.PlainText)));
        foreach (var c in chunks)
        {
            Assert.Equal(c.Block.PlainText, text.Substring(c.Utf16Start, c.Block.PlainText.Length));
        }
        Assert.Equal(0, chunks[0].Utf16Start);
    }

    [Fact]
    public void Small_blocks_are_a_single_unchanged_chunk()
    {
        var p = Para("hello world");
        var chunks = FlowBlockChunker.Split(p, 100);
        Assert.Single(chunks);
        Assert.Same(p, chunks[0].Block);
        Assert.Single(FlowBlockChunker.Split(new HeadingBlock(1, new string('x', 50_000))));
        Assert.Single(FlowBlockChunker.Split(new ImageBlock("s", "alt", null)));
    }

    [Fact]
    public void Large_paragraph_chunks_rejoin_to_the_original_and_respect_the_cap()
    {
        var words = string.Join(' ', Enumerable.Range(0, 5000).Select(i => "word" + i.ToString(CultureInfo.InvariantCulture)));
        var p = Para(words[..10_000], words[10_000..20_000], words[20_000..]);
        var chunks = FlowBlockChunker.Split(p, 1000);
        Assert.True(chunks.Count > 20);
        AssertTilesPlainText(p, chunks, "");
        Assert.All(chunks, c => Assert.True(c.Block.PlainText.Length <= 1000));
        Assert.All(chunks.Skip(1), c => Assert.True(p.PlainText[c.Utf16Start - 1] == ' '));
    }

    [Fact]
    public void Run_formatting_survives_the_split()
    {
        var p = new ParagraphBlock([
            new FlowTextRun(new string('a', 30), true, false, false),
            new FlowTextRun(new string('b', 30), false, true, false),
            new FlowTextRun(new string('c', 30), false, false, true)]);
        var chunks = FlowBlockChunker.Split(p, 20);
        AssertTilesPlainText(p, chunks, "");
        var pairs = chunks.SelectMany(c => ((ParagraphBlock)c.Block).Runs).SelectMany(r => r.Text.Select(ch => (ch, r.Bold, r.Italic, r.Code)));
        Assert.All(pairs, t =>
        {
            if (t.ch == 'a') Assert.True(t.Bold);
            if (t.ch == 'b') Assert.True(t.Italic);
            if (t.ch == 'c') Assert.True(t.Code);
        });
    }

    [Fact]
    public void Newlines_then_sentence_ends_are_preferred_cut_points()
    {
        var text = new string('a', 60) + "\n" + new string('b', 30) + ". " + new string('c', 100);
        var cut = FlowBlockChunker.Split(Para(text), 100)[1].Utf16Start;
        Assert.Equal(61, cut);
        var text2 = new string('a', 60) + ". " + new string('b', 10) + " " + new string('c', 100);
        Assert.Equal(62, FlowBlockChunker.Split(Para(text2), 80)[1].Utf16Start);
    }

    [Fact]
    public void Text_without_whitespace_hard_cuts_but_never_inside_an_emoji_or_combining_sequence()
    {
        const string family = "\U0001F468‍\U0001F469‍\U0001F467";
        var unit = "é" + family + "\U0001F44D\U0001F3FD" + "x";
        var text = string.Concat(Enumerable.Repeat(unit, 300));
        var p = Para(text);
        foreach (var max in new[] { 16, 17, 19, 23, 31, 64 })
        {
            var chunks = FlowBlockChunker.Split(p, max);
            AssertTilesPlainText(p, chunks, "");
            foreach (var c in chunks)
            {
                var t = c.Block.PlainText;
                Assert.False(char.IsLowSurrogate(t[0]));
                Assert.False(char.IsHighSurrogate(t[^1]));
            }
            var elements = chunks.Sum(c => new StringInfo(c.Block.PlainText).LengthInTextElements);
            Assert.Equal(new StringInfo(text).LengthInTextElements, elements);
        }
    }

    [Fact]
    public void Large_list_keeps_numbering_and_rejoins_with_spaces()
    {
        var items = Enumerable.Range(1, 500).Select(i => "item " + i.ToString(CultureInfo.InvariantCulture)).ToList();
        var list = new ListBlock(true, items);
        var chunks = FlowBlockChunker.Split(list, 200);
        Assert.True(chunks.Count > 5);
        AssertTilesPlainText(list, chunks, " ");
        var expected = 1;
        foreach (var c in chunks)
        {
            var l = (ListBlock)c.Block;
            Assert.Equal(expected, l.StartNumber);
            Assert.True(l.PlainText.Length <= 200);
            expected += l.Items.Count;
        }
        Assert.Equal(501, expected);
    }

    [Fact]
    public void Large_table_splits_on_rows_and_only_the_first_chunk_keeps_the_header()
    {
        var rows = Enumerable.Range(0, 400).Select(r => (IReadOnlyList<string>)new[] { "r" + r.ToString(CultureInfo.InvariantCulture), "b", "" }).ToList();
        var table = new TableBlock(rows, true);
        var chunks = FlowBlockChunker.Split(table, 300);
        Assert.True(chunks.Count > 5);
        AssertTilesPlainText(table, chunks, "\n");
        Assert.True(((TableBlock)chunks[0].Block).HeaderRow);
        Assert.All(chunks.Skip(1), c => Assert.False(((TableBlock)c.Block).HeaderRow));
        Assert.Equal(400, chunks.Sum(c => ((TableBlock)c.Block).Rows.Count));
    }

    [Fact]
    public void An_oversized_single_item_stays_whole_and_nothing_is_lost()
    {
        var list = new ListBlock(false, ["a", new string('z', 5000), "b"]);
        var chunks = FlowBlockChunker.Split(list, 100);
        AssertTilesPlainText(list, chunks, " ");
        Assert.Equal(3, chunks.Sum(c => ((ListBlock)c.Block).Items.Count));
    }

    [Fact]
    public void Document_entries_chunk_a_huge_block_and_keep_toc_section_indexing_and_find_working()
    {
        var huge = string.Join(' ', Enumerable.Repeat("lorem ipsum dolor sit amet.", 3000)) + " NEEDLE tail";
        var doc = new FlowDocument("d", "T", null,
        [
            new FlowSection("s0", null, [new HeadingBlock(1, "Intro"), Para(huge), new HeadingBlock(2, "Second")]),
            new FlowSection("s1", null, [Para("end")]),
        ]);
        Assert.True(doc.Entries.Count > 5);
        var chunked = doc.Entries.Where(e => e.BlockIndexInSection == 1 && e.SectionIndex == 0).ToList();
        Assert.True(chunked.Count > 3);
        Assert.Equal(Enumerable.Range(0, chunked.Count), chunked.Select(e => e.ChunkIndex));
        Assert.Equal(huge, string.Concat(chunked.Select(e => e.Block.PlainText)));
        Assert.Equal(["Intro", "Second"], doc.TableOfContents.Select(t => t.Title));
        Assert.IsType<HeadingBlock>(doc.Entries[doc.TableOfContents[1].EntryIndex].Block);
        Assert.Equal(0, doc.TableOfContents[0].EntryIndex);
        Assert.Equal(doc.Entries.Count - 1, doc.FirstEntryIndexOfSection(1));
        Assert.Equal("Intro\n\n" + huge + "\n\nSecond", doc.Sections[0].PlainText);
        var matches = FlowSearch.FindAll(doc, "needle", out _);
        var m = Assert.Single(matches);
        Assert.Equal("NEEDLE", doc.Entries[m.EntryIndex].Block.PlainText.Substring(m.Utf16Start, m.Utf16Length));
        Assert.Equal(huge.IndexOf("NEEDLE", StringComparison.Ordinal), doc.Entries[m.EntryIndex].ChunkUtf16Start + m.Utf16Start);
    }
}
