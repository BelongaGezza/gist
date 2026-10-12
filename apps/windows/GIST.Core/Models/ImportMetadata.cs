using System.Globalization;

namespace Gist.Core.Models;

/// <summary>One selectable text encoding: <see cref="Label"/> is the WHATWG label handed to the core.</summary>
public sealed record TextEncodingOption(string Label, string Name);

/// <summary>
/// SC-01 encoding choices. Mirrors Apple's <c>TextEncodingChoices</c> so both shells offer the same list.
/// </summary>
public static class TextEncodingChoices
{
    public static IReadOnlyList<TextEncodingOption> Curated { get; } = new[]
    {
        new TextEncodingOption("utf-8", "UTF-8"),
        new TextEncodingOption("utf-16le", "UTF-16 (little-endian)"),
        new TextEncodingOption("utf-16be", "UTF-16 (big-endian)"),
        new TextEncodingOption("windows-1252", "Windows-1252 (Western European)"),
        new TextEncodingOption("windows-1251", "Windows-1251 (Cyrillic)"),
        new TextEncodingOption("iso-8859-2", "ISO-8859-2 (Central European)"),
        new TextEncodingOption("iso-8859-15", "ISO-8859-15 (Western European, Euro)"),
        new TextEncodingOption("koi8-r", "KOI8-R (Cyrillic)"),
        new TextEncodingOption("shift_jis", "Shift_JIS (Japanese)"),
        new TextEncodingOption("gbk", "GBK (Simplified Chinese)"),
        new TextEncodingOption("big5", "Big5 (Traditional Chinese)"),
        new TextEncodingOption("euc-kr", "EUC-KR (Korean)"),
    };

    /// <summary>The curated list, with <paramref name="detected"/> prepended when it is not already in it.</summary>
    public static IReadOnlyList<TextEncodingOption> Options(string detected)
    {
        if (Curated.Any(o => string.Equals(o.Label, detected, StringComparison.OrdinalIgnoreCase)))
        {
            return Curated;
        }

        return new[] { new TextEncodingOption(detected, detected) }.Concat(Curated).ToList();
    }

    /// <summary>The option label matching <paramref name="detected"/> (case-insensitive), else <paramref name="detected"/>.</summary>
    public static string Selection(string detected) =>
        Curated.FirstOrDefault(o => string.Equals(o.Label, detected, StringComparison.OrdinalIgnoreCase))?.Label
        ?? detected;

    /// <summary>File extensions the encoding prompt applies to (plain-text imports).</summary>
    public static bool IsPlainTextFile(string path) =>
        Path.GetExtension(path).ToLowerInvariant() is ".txt" or ".text" or ".md" or ".markdown";
}

/// <summary>What the core guessed for a plain-text file (<c>detect_text_encoding</c>).</summary>
public sealed record TextEncodingProbe(string Name, bool Uncertain, bool Lossy)
{
    /// <summary>Whether the import should stop and ask the user (SC-01).</summary>
    public bool NeedsPrompt => Uncertain || Lossy;
}

/// <summary>A plain-text import paused while the user picks an encoding.</summary>
public sealed record PendingEncodingImport(string Path, string DetectedName, bool Lossy)
{
    public string FileName => System.IO.Path.GetFileName(Path);
}

/// <summary>The import-time facts the UI needs from <c>get_item_metadata</c> (SC-03).</summary>
public sealed record ItemMetadata(string SourceType, bool TrackedChanges);

/// <summary>SC-15 word-count / reading-time text, matching Apple's <c>ReadingTimeFormat</c>.</summary>
public static class ReadingTimeFormat
{
    /// <summary>"1,234 words · ≈ 5 min", or null for an empty document.</summary>
    public static string? Caption(uint wordCount, uint seconds) =>
        wordCount == 0
            ? null
            : string.Create(CultureInfo.CurrentCulture, $"{wordCount:N0} words · ≈ {Minutes(seconds)} min");

    public static string? AccessibilityLabel(uint wordCount, uint seconds) =>
        wordCount == 0
            ? null
            : string.Create(CultureInfo.CurrentCulture, $"{wordCount:N0} words, about {Minutes(seconds)} minutes to read");

    /// <summary>Whole minutes, rounded up, never below 1 for a non-empty document.</summary>
    public static uint Minutes(uint seconds) => Math.Max(1u, (uint)(((ulong)seconds + 59) / 60));
}
