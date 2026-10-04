using Gist.Core.Flow;

namespace Gist.Core.Tests.Flow;

/// <summary>
/// The decoder against the <b>real</b> <c>get_document_json</c> output of corpus fixtures (real
/// <c>GistCore</c>, real SQLite/filesystem), plus hostile/corrupt/future-shaped JSON.
/// </summary>
public sealed class FlowDocumentDecoderTests
{
    // ── Real engine output ────────────────────────────────────────────────────

    [Fact]
    public async Task Plain_text_import_decodes_to_paragraphs_with_metadata()
    {
        using var docs = new RealDocuments();
        var json = await docs.JsonAsync("basic_ascii.txt");
        Assert.Contains("\"token_stream\"", json); // real output carries it; the decoder must ignore it

        var doc = FlowDocumentDecoder.Decode(json).Document!;

        Assert.Equal("basic_ascii", doc.Title);
        Assert.Null(doc.Author);
        Assert.Single(doc.Sections);
        Assert.All(doc.Sections[0].Blocks, b => Assert.IsType<ParagraphBlock>(b));
        Assert.True(doc.Sections[0].Blocks.Count >= 2);
        Assert.StartsWith("Lorem ipsum dolor", doc.Entries[0].Block.PlainText);
        Assert.Equal(0, doc.SkippedBlockCount);
    }

    [Fact]
    public async Task Epub_import_decodes_headings_lists_and_several_sections()
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("multi_chapter.epub");

        Assert.Equal("Multi Chapter", doc.Title);
        Assert.Equal("Public Domain", doc.Author);
        Assert.Equal(5, doc.Sections.Count);
        var blocks = doc.Entries.Select(e => e.Block).ToList();
        Assert.Contains(blocks, b => b is HeadingBlock { Level: 1 });
        Assert.Contains(blocks, b => b is HeadingBlock { Level: 3 });
        var unordered = blocks.OfType<ListBlock>().First(l => !l.Ordered);
        Assert.Equal("First item in an unordered list", unordered.Items[0]);
        Assert.Contains(blocks.OfType<ListBlock>(), l => l.Ordered && l.Items.Count == 2);
    }

    [Fact]
    public async Task Docx_import_decodes_heading_levels()
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("headings_and_paragraphs.docx");

        var levels = doc.Entries.Select(e => e.Block).OfType<HeadingBlock>().Select(h => h.Level).ToList();
        Assert.Equal([1, 1, 2, 3], levels.Take(4));
        Assert.Equal("Document Title", doc.Entries[0].Block.PlainText);
    }

    [Theory]
    [InlineData("with_table.docx")]
    [InlineData("with_table.epub")]
    public async Task Table_documents_decode_the_grid_with_ragged_empty_cells_and_header_flag(string fixture)
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync(fixture);

        var table = Assert.Single(doc.Entries.Select(e => e.Block).OfType<TableBlock>());
        Assert.True(table.HeaderRow);
        Assert.Equal(4, table.Rows.Count);
        Assert.Equal(["Fruit", "Colour", "Count"], table.Rows[0]);
        Assert.Equal(["Banana", "", "12"], table.Rows[2]); // empty cell keeps its column
        Assert.Equal("Dark red almost black", table.Rows[3][1]);
    }

    [Fact]
    public async Task Real_unicode_import_round_trips_multibyte_text_intact()
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("unicode_utf8.txt");

        var all = string.Join('\n', doc.Entries.Select(e => e.Block.PlainText));
        Assert.Contains("你好世界", all);
        Assert.Contains("مرحبا بالعالم", all);
        Assert.Contains("Καλημέρα", all);
    }

    [Fact]
    public async Task Section_plain_text_matches_the_rust_section_text_convention_on_real_output()
    {
        // Rust: section_text = blocks' plain_text joined by "\n\n"; a table linearises with '\t' between
        // cells and '\n' between rows. Pinned as a literal (not recomputed with the same code) against the
        // real with_table.docx output, so a drift on either side fails here.
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("with_table.docx");

        Assert.Equal(
            "Before the table.\n\n" +
            "Fruit\tColour\tCount\nApple\tRed\t3\nBanana\t\t12\nCherry\tDark red almost black\t40" +
            "\n\nAfter the table.",
            doc.Sections[0].PlainText);
    }

    [Fact]
    public void Section_plain_text_matches_the_cross_language_golden()
    {
        // The literal Rust pins in gist-core's `section_text_with_a_table_matches_the_cross_language_golden`
        // and Apple's FlowTableTests: same JSON block, same expected text.
        const string json = """
            {"id":"d","metadata":{"title":"T","author":null},"sections":[{"id":"s0","heading":null,"blocks":[
            {"Paragraph":{"runs":[{"text":"Intro text.","bold":false,"italic":false,"code":false}]}},
            {"Table":{"rows":[["Fruit","Colour"],["Apple",""]],"header_row":true}},
            {"Paragraph":{"runs":[{"text":"Outro text after.","bold":false,"italic":false,"code":false}]}}]}]}
            """;

        var doc = FlowDocumentDecoder.Decode(json).Document!;

        Assert.Equal("Intro text.\n\nFruit\tColour\nApple\t\n\nOutro text after.", doc.Sections[0].PlainText);
    }

    [Fact]
    public void Block_utf8_offsets_count_bytes_and_the_two_byte_separator()
    {
        const string json = """
            {"id":"d","metadata":{"title":"T"},"sections":[{"id":"s0","heading":null,"blocks":[
            {"Paragraph":{"runs":[{"text":"é你","bold":false,"italic":false,"code":false}]}},
            {"Paragraph":{"runs":[{"text":"x","bold":false,"italic":false,"code":false}]}}]}]}
            """;
        var section = FlowDocumentDecoder.Decode(json).Document!.Sections[0];

        Assert.Equal(0, section.BlockUtf8Offset(0));
        Assert.Equal(2 + 3 + 2, section.BlockUtf8Offset(1)); // 'é' 2 bytes + '你' 3 bytes + "\n\n"
    }

    [Fact]
    public void Image_plain_text_is_alt_only_never_the_caption()
    {
        const string json = """
            {"id":"d","metadata":{"title":"T"},"sections":[{"id":"s0","heading":null,"blocks":[
            {"Image":{"src":"a.png","alt":"A cat","caption":"Figure 1"}},
            {"Image":{"src":"b.png","alt":null,"caption":"Only caption"}}]}]}
            """;
        var doc = FlowDocumentDecoder.Decode(json).Document!;

        Assert.Equal("A cat", doc.Entries[0].Block.PlainText);
        Assert.Equal("", doc.Entries[1].Block.PlainText);
        Assert.Equal("Only caption", ((ImageBlock)doc.Entries[1].Block).Caption);
    }

    // ── Unknown / odd content degrades, never throws ──────────────────────────

    [Fact]
    public void Unknown_block_kinds_are_skipped_and_counted_without_throwing()
    {
        const string json = """
            {"id":"d","metadata":{"title":"T"},"sections":[{"id":"s0","heading":null,"blocks":[
            {"Paragraph":{"runs":[{"text":"keep","bold":false,"italic":false,"code":false}]}},
            {"Video":{"src":"x.mp4"}},
            {"Heading":{"level":2,"text":"also keep"}},
            "not an object", 42, null, {}]}]}
            """;

        var result = FlowDocumentDecoder.Decode(json);

        Assert.True(result.Succeeded);
        Assert.Equal(2, result.Document!.Entries.Count);
        Assert.Equal(5, result.Document.SkippedBlockCount);
    }

    [Fact]
    public void Runs_missing_text_are_dropped_and_missing_flags_default_to_false()
    {
        const string json = """
            {"id":"d","metadata":{"title":"T"},"sections":[{"id":"s0","heading":null,"blocks":[
            {"Paragraph":{"runs":[{"bold":true},{"text":"a"},{"text":"b","bold":true,"code":true},7]}}]}]}
            """;

        var para = (ParagraphBlock)FlowDocumentDecoder.Decode(json).Document!.Entries[0].Block;

        Assert.Equal(2, para.Runs.Count);
        Assert.Equal(new FlowTextRun("a", false, false, false), para.Runs[0]);
        Assert.Equal(new FlowTextRun("b", true, false, true), para.Runs[1]);
    }

    [Fact]
    public void Wrong_typed_fields_fall_back_instead_of_throwing()
    {
        const string json = """
            {"id":5,"metadata":{"title":9},"sections":[
              {"id":null,"heading":"nope","blocks":"nope"},
              {"id":"s1","heading":[1],"blocks":[{"Table":{"rows":[["a",3,null],"row",["z"]],"header_row":"yes"}},
                                                  {"List":{"ordered":"x","items":["i",1]}},
                                                  {"Heading":{"level":"two","text":7}}]},
              17]}
            """;

        var doc = FlowDocumentDecoder.Decode(json).Document!;

        Assert.Equal("", doc.Id);
        Assert.Equal("", doc.Title);
        Assert.Equal(2, doc.Sections.Count);
        Assert.Equal("s0", doc.Sections[0].Id);
        Assert.Null(doc.Sections[1].Heading);
        var table = (TableBlock)doc.Sections[1].Blocks[0];
        Assert.False(table.HeaderRow);
        Assert.Equal(["a", "", ""], table.Rows[0]);
        Assert.Equal(["z"], table.Rows[1]);
        var list = (ListBlock)doc.Sections[1].Blocks[1];
        Assert.False(list.Ordered);
        Assert.Equal(["i"], list.Items);
        var heading = (HeadingBlock)doc.Sections[1].Blocks[2];
        Assert.Equal(1, heading.Level);
        Assert.Equal("", heading.Text);
    }

    [Fact]
    public void Heading_level_is_clamped_to_a_byte()
    {
        const string json = """
            {"id":"d","metadata":{"title":"T"},"sections":[{"id":"s0","heading":[300,"Big"],"blocks":[
            {"Heading":{"level":-4,"text":"neg"}}]}]}
            """;
        var doc = FlowDocumentDecoder.Decode(json).Document!;

        Assert.Equal(255, doc.Sections[0].Heading!.Level);
        Assert.Equal(0, ((HeadingBlock)doc.Entries[0].Block).Level);
    }

    // ── Corrupt / hostile input fails typed ───────────────────────────────────

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    public void Empty_input_is_a_typed_error(string? json) =>
        Assert.Equal(FlowDecodeError.Empty, FlowDocumentDecoder.Decode(json).Error);

    [Theory]
    [InlineData("not json at all")]
    [InlineData("{\"sections\":[")]
    [InlineData("{\"sections\":[{\"id\":\"s0\",}]}")]
    [InlineData("\u0000\u0001\u0002")]
    public void Corrupt_json_is_malformed_never_an_exception(string json)
    {
        var result = FlowDocumentDecoder.Decode(json);

        Assert.False(result.Succeeded);
        Assert.Equal(FlowDecodeError.Malformed, result.Error);
    }

    [Theory]
    [InlineData("[]")]
    [InlineData("42")]
    [InlineData("\"str\"")]
    [InlineData("null")]
    [InlineData("{}")]
    [InlineData("{\"sections\":{}}")]
    public void Valid_json_of_the_wrong_shape_is_not_a_document(string json) =>
        Assert.Equal(FlowDecodeError.NotADocument, FlowDocumentDecoder.Decode(json).Error);

    [Fact]
    public void Absurdly_deep_nesting_is_rejected_as_too_deep()
    {
        var json = new string('[', 100_000) + new string(']', 100_000);

        Assert.Equal(FlowDecodeError.TooDeep, FlowDocumentDecoder.Decode(json).Error);
    }

    [Fact]
    public void Deep_nesting_inside_an_unknown_block_is_rejected_rather_than_overflowing_the_stack()
    {
        var nest = new string('[', 5_000) + new string(']', 5_000);
        var json = "{\"sections\":[{\"id\":\"s\",\"blocks\":[{\"Future\":{\"x\":" + nest + "}}]}]}";

        var result = FlowDocumentDecoder.Decode(json);

        Assert.False(result.Succeeded);
        Assert.Equal(FlowDecodeError.TooDeep, result.Error);
    }

    [Fact]
    public void Input_over_the_size_cap_is_rejected_before_parsing()
    {
        var result = FlowDocumentDecoder.Decode("{\"sections\":[]}", maxChars: 5);

        Assert.Equal(FlowDecodeError.TooLarge, result.Error);
    }

    [Fact]
    public void A_document_with_no_blocks_is_valid_and_empty()
    {
        var result = FlowDocumentDecoder.Decode("{\"sections\":[]}");

        Assert.True(result.Succeeded);
        Assert.Empty(result.Document!.Entries);
        Assert.Empty(result.Document.TableOfContents);
    }

    [Fact]
    public void A_very_large_valid_document_decodes_without_trouble()
    {
        // ~100k paragraphs: shape check that decoding is linear and bounded, not a perf benchmark.
        var sb = new System.Text.StringBuilder("{\"id\":\"d\",\"metadata\":{\"title\":\"T\"},\"sections\":[{\"id\":\"s0\",\"heading\":null,\"blocks\":[");
        for (var i = 0; i < 100_000; i++)
        {
            if (i > 0) sb.Append(',');
            sb.Append("{\"Paragraph\":{\"runs\":[{\"text\":\"word word word\",\"bold\":false,\"italic\":false,\"code\":false}]}}");
        }
        sb.Append("]}]}");

        var doc = FlowDocumentDecoder.Decode(sb.ToString()).Document!;

        Assert.Equal(100_000, doc.Entries.Count);
    }

    // ── Table of contents ─────────────────────────────────────────────────────

    [Fact]
    public void Toc_uses_section_headings_excludes_headless_sections_and_nests_by_level()
    {
        const string json = """
            {"id":"d","metadata":{"title":"T"},"sections":[
            {"id":"a","heading":[1,"One"],"blocks":[{"Paragraph":{"runs":[{"text":"x"}]}}]},
            {"id":"b","heading":null,"blocks":[{"Heading":{"level":2,"text":"ignored when section headings exist"}}]},
            {"id":"c","heading":[3,"Three"],"blocks":[]},
            {"id":"d2","heading":[0,"Odd"],"blocks":[{"Paragraph":{"runs":[{"text":"y"}]}}]}]}
            """;

        var toc = FlowDocumentDecoder.Decode(json).Document!.TableOfContents;

        Assert.Equal(["One", "Three", "Odd"], toc.Select(t => t.Title));
        Assert.Equal([0, 2, 0], toc.Select(t => t.IndentLevel)); // h1 flush, h3 -> 2, level 0 clamps to 0
        Assert.Equal([0, 2, 3], toc.Select(t => t.SectionIndex));
        Assert.Equal([0, -1, 2], toc.Select(t => t.EntryIndex)); // section "c" has no blocks
    }

    [Fact]
    public async Task Toc_falls_back_to_heading_blocks_because_real_parsers_never_set_section_headings()
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("headings_and_paragraphs.docx");

        Assert.All(doc.Sections, s => Assert.Null(s.Heading)); // the reason for the fallback
        var toc = doc.TableOfContents;
        Assert.Equal("Document Title", toc[0].Title);
        Assert.Equal([0, 0, 1, 2], toc.Take(4).Select(t => t.IndentLevel));
        Assert.All(toc, t => Assert.IsType<HeadingBlock>(doc.Entries[t.EntryIndex].Block));
    }

    [Fact]
    public async Task Toc_is_empty_for_a_document_without_any_headings()
    {
        using var docs = new RealDocuments();
        var doc = await docs.DocumentAsync("basic_ascii.txt");

        Assert.Empty(doc.TableOfContents);
    }
}
