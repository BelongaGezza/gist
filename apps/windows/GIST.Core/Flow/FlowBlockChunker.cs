using System.Globalization;

namespace Gist.Core.Flow;

/// <summary>
/// F60: splits a very large paragraph, list or table into bounded pieces so the virtualised flow list can
/// virtualise inside it. UI-free. Chunks tile the original block's <see cref="FlowBlock.PlainText"/> exactly:
/// joining the paragraph chunks' text, the list chunks' text with one space, or the table chunks' text with one
/// newline reproduces the original, and <see cref="Chunk.Utf16Start"/> is where each chunk begins in it.
/// Section-level plain text (<see cref="FlowSection.PlainText"/>, the anchoring convention) is computed from the
/// original blocks and is not affected. Known limits: a single list item or table row larger than the cap stays
/// whole, and a find query that spans a chunk boundary (at most one whitespace position per chunk) is not matched.
/// </summary>
public static class FlowBlockChunker
{
    /// <summary>Default per-chunk cap in UTF-16 units (about 1,500 words, a few screens of text).</summary>
    public const int DefaultMaxChars = 8_000;

    public readonly record struct Chunk(FlowBlock Block, int Utf16Start);

    public static IReadOnlyList<Chunk> Split(FlowBlock block, int maxChars = DefaultMaxChars)
    {
        ArgumentNullException.ThrowIfNull(block);
        if (maxChars < 16) maxChars = 16;
        return block switch
        {
            ParagraphBlock p => SplitParagraph(p, maxChars),
            ListBlock l => SplitList(l, maxChars),
            TableBlock t => SplitTable(t, maxChars),
            _ => [new Chunk(block, 0)],
        };
    }

    private static IReadOnlyList<Chunk> SplitParagraph(ParagraphBlock p, int max)
    {
        var total = 0;
        foreach (var r in p.Runs) total += r.Text.Length;
        if (total <= max) return [new Chunk(p, 0)];

        var text = string.Concat(p.Runs.Select(r => r.Text));
        var cuts = new List<int> { 0 };
        while (text.Length - cuts[^1] > max)
        {
            cuts.Add(FindCut(text, cuts[^1], max));
        }
        cuts.Add(text.Length);

        // Slice the runs at the cut offsets, keeping each run's formatting.
        var chunks = new List<Chunk>(cuts.Count - 1);
        var runIndex = 0;
        var runStart = 0; // absolute offset where Runs[runIndex] starts
        for (var c = 0; c + 1 < cuts.Count; c++)
        {
            var from = cuts[c];
            var to = cuts[c + 1];
            var runs = new List<FlowTextRun>();
            while (runIndex < p.Runs.Count && from < to)
            {
                var run = p.Runs[runIndex];
                var runEnd = runStart + run.Text.Length;
                if (runEnd <= from)
                {
                    runStart = runEnd;
                    runIndex++;
                    continue;
                }
                var sliceFrom = Math.Max(from, runStart) - runStart;
                var sliceTo = Math.Min(to, runEnd) - runStart;
                if (sliceTo > sliceFrom) runs.Add(run with { Text = run.Text.Substring(sliceFrom, sliceTo - sliceFrom) });
                from = Math.Min(to, runEnd);
                if (runEnd <= to)
                {
                    runStart = runEnd;
                    runIndex++;
                }
            }
            chunks.Add(new Chunk(new ParagraphBlock(runs), cuts[c]));
        }
        return chunks;
    }

    /// <summary>
    /// Chooses where the chunk starting at <paramref name="start"/> ends: the latest newline, else sentence end,
    /// else whitespace in the window's second half; else a hard cut that never splits a text element.
    /// </summary>
    internal static int FindCut(string text, int start, int max)
    {
        var limit = start + max; // exclusive upper bound; text.Length - start > max so limit < text.Length
        var floor = start + (max / 2);

        for (var i = limit; i > floor; i--)
        {
            if (text[i - 1] == '\n' && IsSafeCut(text, i)) return i;
        }
        for (var i = limit; i > floor; i--)
        {
            if (char.IsWhiteSpace(text[i - 1]) && i - 2 >= start && (text[i - 2] is '.' or '!' or '?' or '。') && IsSafeCut(text, i)) return i;
        }
        for (var i = limit; i > floor; i--)
        {
            if (char.IsWhiteSpace(text[i - 1]) && IsSafeCut(text, i)) return i;
        }
        for (var i = limit; i > start; i--)
        {
            if (IsSafeCut(text, i)) return i;
        }
        return limit; // unreachable in practice: every run of 'max' chars has a safe cut
    }

    /// <summary>True when cutting before index <paramref name="i"/> splits no surrogate pair, combining sequence or ZWJ sequence.</summary>
    private static bool IsSafeCut(string text, int i)
    {
        if (i <= 0 || i >= text.Length) return true;
        if (char.IsHighSurrogate(text[i - 1]) && char.IsLowSurrogate(text[i])) return false;
        if (text[i - 1] == '‍' || text[i] == '‍') return false;
        var next = char.IsHighSurrogate(text[i]) && i + 1 < text.Length
            ? CharUnicodeInfo.GetUnicodeCategory(text, i)
            : CharUnicodeInfo.GetUnicodeCategory(text[i]);
        if (next is UnicodeCategory.NonSpacingMark or UnicodeCategory.SpacingCombiningMark or UnicodeCategory.EnclosingMark) return false;
        // Variation selectors and emoji skin-tone modifiers attach to the previous character.
        if (text[i] is >= '︀' and <= '️') return false;
        if (char.IsHighSurrogate(text[i]) && i + 1 < text.Length)
        {
            var cp = char.ConvertToUtf32(text[i], text[i + 1]);
            if (cp is (>= 0x1F3FB and <= 0x1F3FF) or (>= 0xE0100 and <= 0xE01EF) or (>= 0xE0020 and <= 0xE007F)) return false;
        }
        return true;
    }

    private static IReadOnlyList<Chunk> SplitList(ListBlock list, int max)
    {
        var total = 0;
        foreach (var item in list.Items) total += item.Length + 1;
        if (total - 1 <= max) return [new Chunk(list, 0)];

        var chunks = new List<Chunk>();
        var items = new List<string>();
        var size = 0;
        var offset = 0;
        var chunkOffset = 0;
        var firstNumber = list.StartNumber;
        for (var i = 0; i < list.Items.Count; i++)
        {
            var item = list.Items[i];
            if (items.Count > 0 && size + 1 + item.Length > max)
            {
                chunks.Add(new Chunk(list with { Items = items, StartNumber = firstNumber }, chunkOffset));
                firstNumber += items.Count;
                chunkOffset = offset;
                items = [];
                size = 0;
            }
            size += (items.Count > 0 ? 1 : 0) + item.Length;
            items.Add(item);
            offset += item.Length + 1;
        }
        if (items.Count > 0) chunks.Add(new Chunk(list with { Items = items, StartNumber = firstNumber }, chunkOffset));
        return chunks;
    }

    private static IReadOnlyList<Chunk> SplitTable(TableBlock table, int max)
    {
        var rowLengths = new int[table.Rows.Count];
        var total = 0;
        for (var r = 0; r < table.Rows.Count; r++)
        {
            var row = table.Rows[r];
            var len = Math.Max(row.Count - 1, 0);
            foreach (var cell in row) len += cell.Length;
            rowLengths[r] = len;
            total += len + 1;
        }
        if (total - 1 <= max) return [new Chunk(table, 0)];

        var chunks = new List<Chunk>();
        var rows = new List<IReadOnlyList<string>>();
        var size = 0;
        var offset = 0;
        var chunkOffset = 0;
        for (var r = 0; r < table.Rows.Count; r++)
        {
            if (rows.Count > 0 && size + 1 + rowLengths[r] > max)
            {
                chunks.Add(new Chunk(new TableBlock(rows, table.HeaderRow && chunks.Count == 0), chunkOffset));
                chunkOffset = offset;
                rows = [];
                size = 0;
            }
            size += (rows.Count > 0 ? 1 : 0) + rowLengths[r];
            rows.Add(table.Rows[r]);
            offset += rowLengths[r] + 1;
        }
        if (rows.Count > 0) chunks.Add(new Chunk(new TableBlock(rows, table.HeaderRow && chunks.Count == 0), chunkOffset));
        return chunks;
    }
}
