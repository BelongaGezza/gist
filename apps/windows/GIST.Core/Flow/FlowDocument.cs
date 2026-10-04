using System.Text;

namespace Gist.Core.Flow;

/// <summary>
/// Client-side mirror of <c>gist_model::Document</c> as the flow reader needs it: the block-structured
/// IR (sections of headings/paragraphs/images/lists/tables), not RSVP's flat token stream. Decoded by
/// <see cref="FlowDocumentDecoder"/> from <c>GistCore.get_document_json</c>.
/// </summary>
public sealed class FlowDocument
{
    public FlowDocument(string id, string title, string? author, IReadOnlyList<FlowSection> sections, int skippedBlockCount = 0)
    {
        Id = id;
        Title = title;
        Author = author;
        Sections = sections;
        SkippedBlockCount = skippedBlockCount;
        Entries = Flatten(sections);
        TableOfContents = BuildToc(sections, Entries);
    }

    public string Id { get; }

    public string Title { get; }

    public string? Author { get; }

    public IReadOnlyList<FlowSection> Sections { get; }

    /// <summary>Blocks of an unknown (future) kind that were dropped while decoding.</summary>
    public int SkippedBlockCount { get; }

    /// <summary>One entry per section that has a heading, in document order (headless sections excluded).</summary>
    public IReadOnlyList<TocEntry> TableOfContents { get; }

    /// <summary>Every block of every section flattened in reading order; what a virtualised list binds to.</summary>
    public IReadOnlyList<FlowBlockEntry> Entries { get; }

    /// <summary>Index of the section with <paramref name="sectionId"/>, or -1.</summary>
    public int FindSectionIndex(string? sectionId)
    {
        if (sectionId is null) return -1;
        for (var i = 0; i < Sections.Count; i++)
        {
            if (string.Equals(Sections[i].Id, sectionId, StringComparison.Ordinal)) return i;
        }
        return -1;
    }

    /// <summary>
    /// Index into <see cref="Entries"/> of the first block of section <paramref name="sectionIndex"/>,
    /// or -1 when the section is unknown or has no blocks.
    /// </summary>
    public int FirstEntryIndexOfSection(int sectionIndex)
    {
        for (var i = 0; i < Entries.Count; i++)
        {
            if (Entries[i].SectionIndex == sectionIndex) return i;
        }
        return -1;
    }

    private static IReadOnlyList<TocEntry> BuildToc(IReadOnlyList<FlowSection> sections, IReadOnlyList<FlowBlockEntry> entries)
    {
        var toc = new List<TocEntry>();
        var firstEntryOfSection = new Dictionary<int, int>();
        for (var e = 0; e < entries.Count; e++)
        {
            firstEntryOfSection.TryAdd(entries[e].SectionIndex, e);
        }

        for (var i = 0; i < sections.Count; i++)
        {
            if (sections[i].Heading is { } h)
            {
                toc.Add(new TocEntry(i, sections[i].Id, h.Level, h.Text, firstEntryOfSection.GetValueOrDefault(i, -1)));
            }
        }

        if (toc.Count > 0) return toc;

        // No current parser populates Section.heading (headings arrive as Heading blocks inside a section),
        // so a document-wide TOC built only from section headings would always be empty. Fall back to the
        // Heading blocks; each entry jumps to its own block.
        for (var e = 0; e < entries.Count; e++)
        {
            if (entries[e].Block is HeadingBlock hb && hb.Text.Length > 0)
            {
                toc.Add(new TocEntry(entries[e].SectionIndex, entries[e].SectionId, hb.Level, hb.Text, e));
            }
        }
        return toc;
    }

    private static IReadOnlyList<FlowBlockEntry> Flatten(IReadOnlyList<FlowSection> sections)
    {
        var entries = new List<FlowBlockEntry>();
        for (var s = 0; s < sections.Count; s++)
        {
            for (var b = 0; b < sections[s].Blocks.Count; b++)
            {
                var chunks = FlowBlockChunker.Split(sections[s].Blocks[b]);
                for (var c = 0; c < chunks.Count; c++)
                {
                    entries.Add(new FlowBlockEntry(s, sections[s].Id, b, chunks[c].Block, c, chunks[c].Utf16Start));
                }
            }
        }
        return entries;
    }
}

public sealed record FlowHeading(int Level, string Text);

/// <summary>
/// A table-of-contents row. <see cref="IndentLevel"/> is <c>max(level - 1, 0)</c>: h1 is flush.
/// <see cref="EntryIndex"/> is the index into <see cref="FlowDocument.Entries"/> to scroll to (-1 if the section
/// has no blocks).
/// </summary>
public sealed record TocEntry(int SectionIndex, string SectionId, int Level, string Title, int EntryIndex = -1)
{
    public int IndentLevel => Math.Max(Level - 1, 0);
}

/// <summary>
/// A block with its position, the flattened shape a virtualised list renders and scrolls by. A very large block
/// is split by <see cref="FlowBlockChunker"/> into several entries sharing <see cref="BlockIndexInSection"/>;
/// <see cref="ChunkUtf16Start"/> is where this chunk starts in the original block's plain text.
/// </summary>
public sealed record FlowBlockEntry(int SectionIndex, string SectionId, int BlockIndexInSection, FlowBlock Block, int ChunkIndex = 0, int ChunkUtf16Start = 0);

public sealed class FlowSection
{
    /// <summary>Separator <c>gist_core::anchoring::section_text</c> joins blocks with. Load-bearing (ADR-003).</summary>
    public const string BlockSeparator = "\n\n";

    public FlowSection(string id, FlowHeading? heading, IReadOnlyList<FlowBlock> blocks)
    {
        Id = id;
        Heading = heading;
        Blocks = blocks;
    }

    public string Id { get; }

    public FlowHeading? Heading { get; }

    public IReadOnlyList<FlowBlock> Blocks { get; }

    /// <summary>
    /// Every block's <see cref="FlowBlock.PlainText"/> joined by <c>"\n\n"</c>: the same text
    /// <c>section_text</c> produces in Rust, so offsets into it agree with annotation anchoring.
    /// </summary>
    public string PlainText => string.Join(BlockSeparator, Blocks.Select(b => b.PlainText));

    /// <summary>
    /// UTF-8 byte offset inside <see cref="PlainText"/> where block <paramref name="index"/> starts
    /// (Rust anchors by byte offset, not UTF-16 unit).
    /// </summary>
    public int BlockUtf8Offset(int index)
    {
        var offset = 0;
        for (var i = 0; i < index && i < Blocks.Count; i++)
        {
            offset += Encoding.UTF8.GetByteCount(Blocks[i].PlainText) + BlockSeparator.Length;
        }
        return offset;
    }
}

/// <summary>One block of a section. Mirrors <c>gist_model::Block</c>; unknown kinds never reach this type.</summary>
public abstract record FlowBlock
{
    /// <summary>Mirrors <c>gist_model::Block::plain_text</c>; used for find and as the anchoring text.</summary>
    public abstract string PlainText { get; }
}

public sealed record HeadingBlock(int Level, string Text) : FlowBlock
{
    public override string PlainText => Text;
}

public sealed record ParagraphBlock(IReadOnlyList<FlowTextRun> Runs) : FlowBlock
{
    public override string PlainText => string.Concat(Runs.Select(r => r.Text));
}

/// <summary>An image contributes its <c>alt</c> text only, never its caption (Rust parity, ADR-003).</summary>
public sealed record ImageBlock(string Src, string? Alt, string? Caption) : FlowBlock
{
    public override string PlainText => Alt ?? string.Empty;
}

public sealed record ListBlock(bool Ordered, IReadOnlyList<string> Items) : FlowBlock
{
    /// <summary>Number shown for the first item of an ordered list; a chunk of a split list continues the count (F60).</summary>
    public int StartNumber { get; init; } = 1;

    public override string PlainText => string.Join(' ', Items);
}

/// <summary>
/// Cells are plain text; rows may be ragged and an empty cell is <c>""</c>. Linearised with tab between
/// cells and newline between rows, matching <c>gist_model::TABLE_CELL_SEPARATOR</c>/<c>TABLE_ROW_SEPARATOR</c>.
/// </summary>
public sealed record TableBlock(IReadOnlyList<IReadOnlyList<string>> Rows, bool HeaderRow) : FlowBlock
{
    public const char CellSeparator = '\t';
    public const char RowSeparator = '\n';

    public override string PlainText =>
        string.Join(RowSeparator, Rows.Select(r => string.Join(CellSeparator, r)));
}

public sealed record FlowTextRun(string Text, bool Bold, bool Italic, bool Code);
