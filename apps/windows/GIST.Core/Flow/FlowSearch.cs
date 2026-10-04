using System.Globalization;

namespace Gist.Core.Flow;

/// <summary>
/// One find hit inside one block's <see cref="FlowBlock.PlainText"/>. The UTF-16 span is what
/// <c>TextHighlighter</c>/string APIs want; the element span counts user-perceived characters
/// (<see cref="StringInfo"/> text elements: a surrogate pair, emoji ZWJ sequence or base + combining mark is one).
/// <see cref="EntryIndex"/> indexes <see cref="FlowDocument.Entries"/>.
/// </summary>
public readonly record struct FlowSearchMatch(
    int EntryIndex,
    int SectionIndex,
    int BlockIndexInSection,
    int Utf16Start,
    int Utf16Length,
    int ElementStart,
    int ElementLength);

/// <summary>Pure find logic; <see cref="FlowSearchState"/> wraps it with the current-match cursor.</summary>
public static class FlowSearch
{
    /// <summary>Stops a one-letter query over a huge document from producing an unbounded list.</summary>
    public const int MaxMatches = 20_000;

    /// <summary>
    /// Case-insensitive (ordinal, culture-independent), non-overlapping matches of <paramref name="query"/> in
    /// <paramref name="text"/>. A match must start and end on a text-element boundary, so searching "e" never
    /// matches the base letter of "e" + U+0301 and a lone surrogate half never matches. Empty query or text: none.
    /// </summary>
    public static IReadOnlyList<(int Utf16Start, int Utf16Length, int ElementStart, int ElementLength)> FindInText(
        string? text, string? query, int limit = MaxMatches)
    {
        var results = new List<(int, int, int, int)>();
        if (string.IsNullOrEmpty(text) || string.IsNullOrEmpty(query) || limit <= 0) return results;

        int[]? starts = null; // text-element start offsets, computed lazily on the first candidate
        var from = 0;
        while (from <= text.Length - query.Length)
        {
            var idx = text.IndexOf(query, from, StringComparison.OrdinalIgnoreCase);
            if (idx < 0) break;

            starts ??= StringInfo.ParseCombiningCharacters(text);
            var end = idx + query.Length;
            var startElement = Array.BinarySearch(starts, idx);
            var onBoundary = startElement >= 0 && (end == text.Length || Array.BinarySearch(starts, end) >= 0);
            if (onBoundary)
            {
                var endElement = end == text.Length ? starts.Length : Array.BinarySearch(starts, end);
                results.Add((idx, query.Length, startElement, endElement - startElement));
                if (results.Count >= limit) break;
                from = end;
            }
            else
            {
                from = idx + 1;
            }
        }
        return results;
    }

    /// <summary>All matches across the document in reading order, capped at <see cref="MaxMatches"/>.</summary>
    public static IReadOnlyList<FlowSearchMatch> FindAll(FlowDocument document, string? query, out bool truncated)
    {
        ArgumentNullException.ThrowIfNull(document);
        truncated = false;
        var matches = new List<FlowSearchMatch>();
        if (string.IsNullOrEmpty(query)) return matches;

        for (var i = 0; i < document.Entries.Count; i++)
        {
            var entry = document.Entries[i];
            foreach (var (start, length, elStart, elLength) in FindInText(entry.Block.PlainText, query, MaxMatches - matches.Count))
            {
                matches.Add(new FlowSearchMatch(i, entry.SectionIndex, entry.BlockIndexInSection, start, length, elStart, elLength));
            }
            if (matches.Count >= MaxMatches)
            {
                truncated = true;
                break;
            }
        }
        return matches;
    }
}

/// <summary>
/// The find box's state: query, matches over a document, and the "3 of 27" cursor. Find-next/previous wrap.
/// Raises <see cref="Changed"/> when the query, matches or current index change.
/// </summary>
public sealed class FlowSearchState
{
    private FlowDocument? _document;
    private string _query = string.Empty;
    private IReadOnlyList<FlowSearchMatch> _matches = [];
    private int _current;

    public event EventHandler? Changed;

    public string Query => _query;

    public IReadOnlyList<FlowSearchMatch> Matches => _matches;

    public int MatchCount => _matches.Count;

    /// <summary>True when the hit list was cut at <see cref="FlowSearch.MaxMatches"/>.</summary>
    public bool Truncated { get; private set; }

    /// <summary>Zero-based index of the current match; 0 when there are none.</summary>
    public int CurrentMatchIndex => _current;

    public FlowSearchMatch? CurrentMatch => _matches.Count == 0 ? null : _matches[_current];

    public void SetDocument(FlowDocument? document)
    {
        _document = document;
        Recompute();
    }

    public void SetQuery(string? query)
    {
        var q = query ?? string.Empty;
        if (string.Equals(q, _query, StringComparison.Ordinal)) return;
        _query = q;
        Recompute();
    }

    public void FindNext()
    {
        if (_matches.Count == 0) return;
        _current = (_current + 1) % _matches.Count;
        Changed?.Invoke(this, EventArgs.Empty);
    }

    public void FindPrevious()
    {
        if (_matches.Count == 0) return;
        _current = (_current - 1 + _matches.Count) % _matches.Count;
        Changed?.Invoke(this, EventArgs.Empty);
    }

    private void Recompute()
    {
        if (_document is null || _query.Length == 0)
        {
            _matches = [];
            Truncated = false;
        }
        else
        {
            _matches = FlowSearch.FindAll(_document, _query, out var truncated);
            Truncated = truncated;
        }
        _current = 0;
        Changed?.Invoke(this, EventArgs.Empty);
    }
}
