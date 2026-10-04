using System.Text.Json;

namespace Gist.Core.Flow;

/// <summary>Why a document could not be decoded. Never carries exception text.</summary>
public enum FlowDecodeError
{
    None,
    Empty,
    TooLarge,
    TooDeep,
    TooManyItems,
    Malformed,
    NotADocument,
}

public readonly record struct FlowDecodeResult(FlowDocument? Document, FlowDecodeError Error)
{
    public bool Succeeded => Document is not null;
}

/// <summary>
/// Decodes the JSON <c>GistCore.get_document_json</c> returns (serde's <c>gist_model::Document</c>) into a
/// <see cref="FlowDocument"/>. serde's externally tagged enums (<c>{"Heading":{...}}</c>) and the
/// <c>Option&lt;(u8, String)&gt;</c> heading tuple (a 2-element array) don't match System.Text.Json's
/// defaults, so the tree is walked by hand.
/// </summary>
/// <remarks>
/// Robustness contract: content problems degrade, they never throw. An unknown block kind is skipped
/// (counted in <see cref="FlowDocument.SkippedBlockCount"/>); a run/field of the wrong shape falls back to a
/// default or drops that one block/run. Structural problems (not JSON, wrong root, too deep, too big) return a
/// typed <see cref="FlowDecodeError"/>. The <c>token_stream</c> array (RSVP's, potentially huge) is never walked.
/// </remarks>
public static class FlowDocumentDecoder
{
    /// <summary>Largest JSON text accepted, in UTF-16 chars (the token stream makes real documents large).</summary>
    public const int MaxJsonChars = 128 * 1024 * 1024;

    /// <summary>Deepest nesting accepted. Real documents nest about 10 levels.</summary>
    public const int MaxDepth = 32;

    public const int MaxSections = 200_000;

    public const int MaxTotalBlocks = 2_000_000;

    /// <summary>Cap on runs/items/cells in a single block.</summary>
    public const int MaxItemsPerBlock = 1_000_000;

    public static FlowDecodeResult Decode(string? json, int maxChars = MaxJsonChars)
    {
        if (string.IsNullOrWhiteSpace(json)) return Fail(FlowDecodeError.Empty);
        if (json.Length > maxChars) return Fail(FlowDecodeError.TooLarge);

        try
        {
            using var doc = JsonDocument.Parse(json, new JsonDocumentOptions { MaxDepth = MaxDepth });
            return DecodeRoot(doc.RootElement);
        }
        catch (JsonException)
        {
            // System.Text.Json reports an exceeded MaxDepth as a plain JsonException (message text is
            // localised, so it is never inspected); classify by measuring nesting ourselves.
            return Fail(ExceedsDepth(json) ? FlowDecodeError.TooDeep : FlowDecodeError.Malformed);
        }
        catch (ArgumentException)
        {
            return Fail(FlowDecodeError.Malformed);
        }
        catch (InvalidOperationException)
        {
            return Fail(FlowDecodeError.Malformed);
        }
    }

    private static FlowDecodeResult Fail(FlowDecodeError error) => new(null, error);

    /// <summary>String-aware bracket-depth scan; stops as soon as the limit is passed.</summary>
    private static bool ExceedsDepth(string json)
    {
        var depth = 0;
        var inString = false;
        for (var i = 0; i < json.Length; i++)
        {
            var c = json[i];
            if (inString)
            {
                if (c == '\\') i++;
                else if (c == '"') inString = false;
            }
            else if (c == '"') inString = true;
            else if (c is '{' or '[')
            {
                if (++depth > MaxDepth) return true;
            }
            else if (c is '}' or ']') depth--;
        }
        return false;
    }

    private static FlowDecodeResult DecodeRoot(JsonElement root)
    {
        if (root.ValueKind != JsonValueKind.Object) return Fail(FlowDecodeError.NotADocument);
        if (!root.TryGetProperty("sections", out var sectionsEl) || sectionsEl.ValueKind != JsonValueKind.Array)
        {
            return Fail(FlowDecodeError.NotADocument);
        }

        var sectionCount = sectionsEl.GetArrayLength();
        if (sectionCount > MaxSections) return Fail(FlowDecodeError.TooManyItems);

        var sections = new List<FlowSection>(sectionCount);
        var totalBlocks = 0;
        var skipped = 0;
        foreach (var sectionEl in sectionsEl.EnumerateArray())
        {
            if (sectionEl.ValueKind != JsonValueKind.Object)
            {
                skipped++;
                continue;
            }

            var blocks = new List<FlowBlock>();
            if (sectionEl.TryGetProperty("blocks", out var blocksEl) && blocksEl.ValueKind == JsonValueKind.Array)
            {
                totalBlocks += blocksEl.GetArrayLength();
                if (totalBlocks > MaxTotalBlocks) return Fail(FlowDecodeError.TooManyItems);
                foreach (var blockEl in blocksEl.EnumerateArray())
                {
                    var block = DecodeBlock(blockEl);
                    if (block is null) skipped++;
                    else blocks.Add(block);
                }
            }

            sections.Add(new FlowSection(
                GetString(sectionEl, "id") ?? $"s{sections.Count}",
                DecodeHeading(sectionEl),
                blocks));
        }

        string? title = null;
        string? author = null;
        if (root.TryGetProperty("metadata", out var meta) && meta.ValueKind == JsonValueKind.Object)
        {
            title = GetString(meta, "title");
            author = GetString(meta, "author");
        }

        return new FlowDecodeResult(
            new FlowDocument(GetString(root, "id") ?? string.Empty, title ?? string.Empty, author, sections, skipped),
            FlowDecodeError.None);
    }

    /// <summary>Rust's <c>Option&lt;(u8, String)&gt;</c> is null or a 2-element array; anything else is no heading.</summary>
    private static FlowHeading? DecodeHeading(JsonElement section)
    {
        if (!section.TryGetProperty("heading", out var h) || h.ValueKind != JsonValueKind.Array) return null;
        if (h.GetArrayLength() != 2) return null;
        var level = h[0];
        var text = h[1];
        if (level.ValueKind != JsonValueKind.Number || text.ValueKind != JsonValueKind.String) return null;
        return new FlowHeading(ClampLevel(level), text.GetString() ?? string.Empty);
    }

    private static int ClampLevel(JsonElement number) =>
        number.TryGetInt32(out var v) ? Math.Clamp(v, 0, byte.MaxValue) : 1;

    private static FlowBlock? DecodeBlock(JsonElement el)
    {
        // Externally tagged: an object with exactly one property whose name is the variant.
        if (el.ValueKind != JsonValueKind.Object) return null;
        foreach (var prop in el.EnumerateObject())
        {
            var inner = prop.Value;
            if (inner.ValueKind != JsonValueKind.Object) return null;
            switch (prop.Name)
            {
                case "Heading":
                    return new HeadingBlock(
                        inner.TryGetProperty("level", out var lv) && lv.ValueKind == JsonValueKind.Number ? ClampLevel(lv) : 1,
                        GetString(inner, "text") ?? string.Empty);
                case "Paragraph":
                    return new ParagraphBlock(DecodeRuns(inner));
                case "Image":
                    return new ImageBlock(GetString(inner, "src") ?? string.Empty, GetString(inner, "alt"), GetString(inner, "caption"));
                case "List":
                    return new ListBlock(GetBool(inner, "ordered"), DecodeStrings(inner, "items"));
                case "Table":
                    return DecodeTable(inner);
                default:
                    return null;
            }
        }
        return null;
    }

    private static List<FlowTextRun> DecodeRuns(JsonElement paragraph)
    {
        var runs = new List<FlowTextRun>();
        if (!paragraph.TryGetProperty("runs", out var runsEl) || runsEl.ValueKind != JsonValueKind.Array) return runs;
        var n = 0;
        foreach (var r in runsEl.EnumerateArray())
        {
            if (++n > MaxItemsPerBlock) break;
            if (r.ValueKind != JsonValueKind.Object) continue;
            var text = GetString(r, "text");
            if (text is null) continue;
            runs.Add(new FlowTextRun(text, GetBool(r, "bold"), GetBool(r, "italic"), GetBool(r, "code")));
        }
        return runs;
    }

    private static List<string> DecodeStrings(JsonElement parent, string name)
    {
        var list = new List<string>();
        if (!parent.TryGetProperty(name, out var arr) || arr.ValueKind != JsonValueKind.Array) return list;
        var n = 0;
        foreach (var item in arr.EnumerateArray())
        {
            if (++n > MaxItemsPerBlock) break;
            if (item.ValueKind == JsonValueKind.String) list.Add(item.GetString() ?? string.Empty);
        }
        return list;
    }

    private static TableBlock DecodeTable(JsonElement inner)
    {
        var rows = new List<IReadOnlyList<string>>();
        if (inner.TryGetProperty("rows", out var rowsEl) && rowsEl.ValueKind == JsonValueKind.Array)
        {
            var cells = 0;
            foreach (var row in rowsEl.EnumerateArray())
            {
                if (row.ValueKind != JsonValueKind.Array) continue;
                var cellList = new List<string>();
                foreach (var cell in row.EnumerateArray())
                {
                    if (++cells > MaxItemsPerBlock) break;
                    // A non-string cell keeps its column slot as an empty cell.
                    cellList.Add(cell.ValueKind == JsonValueKind.String ? cell.GetString() ?? string.Empty : string.Empty);
                }
                rows.Add(cellList);
                if (cells > MaxItemsPerBlock) break;
            }
        }
        return new TableBlock(rows, GetBool(inner, "header_row"));
    }

    private static string? GetString(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    private static bool GetBool(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;
}
