using System.Globalization;

namespace Gist.Core.Flow;

/// <summary>
/// How far through the document the reader is, as a <c>0..1</c> fraction rather than anything layout-specific
/// (a block index, a pixel offset), so it stays meaningful for a future paginated layout. The layout restores to
/// <see cref="InitialFraction"/> once on first render and then keeps <see cref="Fraction"/> live.
/// </summary>
public sealed class ReadingProgress
{
    public ReadingProgress(double initialFraction = 0)
    {
        InitialFraction = Sanitize(initialFraction, 0);
        Fraction = InitialFraction;
    }

    public event EventHandler? Changed;

    public double InitialFraction { get; }

    public double Fraction { get; private set; }

    /// <summary>Clamps into 0..1; a non-finite value is ignored (the previous fraction stays).</summary>
    public void Update(double fraction)
    {
        if (!double.IsFinite(fraction)) return;
        var clamped = Math.Clamp(fraction, 0, 1);
        if (clamped == Fraction) return;
        Fraction = clamped;
        Changed?.Invoke(this, EventArgs.Empty);
    }

    /// <summary>Whole-percent text for the bottom bar, e.g. <c>"42%"</c>.</summary>
    public string PercentText => string.Create(CultureInfo.InvariantCulture, $"{(int)Math.Round(Fraction * 100)}%");

    /// <summary>Fraction of the block at <paramref name="index"/> in a list of <paramref name="count"/> (0 for 0 or 1 blocks).</summary>
    public static double FractionForBlock(int index, int count) =>
        count <= 1 ? 0 : Math.Clamp(index, 0, count - 1) / (double)(count - 1);

    /// <summary>Block index to restore to for <paramref name="fraction"/>; -1 when there are no blocks.</summary>
    public static int BlockForFraction(double fraction, int count)
    {
        if (count <= 0) return -1;
        if (!double.IsFinite(fraction)) return 0;
        return (int)Math.Round(Math.Clamp(fraction, 0, 1) * (count - 1));
    }

    internal static double Sanitize(double value, double fallback) =>
        double.IsFinite(value) ? Math.Clamp(value, 0, 1) : fallback;
}

/// <summary>
/// Persists each item's flow scroll position (a <c>0..1</c> fraction) as <c>FlowScrollPosition.&lt;itemId&gt;</c>
/// in one directory. Deliberately separate from <c>gist-store</c>'s <c>reading_progress</c> (an RSVP token index);
/// mixing the two would make one number mean two things. Best-effort like <c>FileRsvpSettingsStore</c>.
/// </summary>
/// <remarks>
/// Item ids become file names, so they are validated: only <c>[A-Za-z0-9_-]{1,64}</c> is accepted (UUIDs fit).
/// Anything else (separators, dots, drive/stream colons, empty, over-long) is refused: <see cref="Load"/> returns 0
/// and <see cref="Save"/> does nothing, so an id can never address a file outside the store directory.
/// </remarks>
public sealed class FileFlowScrollPositionStore
{
    public const string FilePrefix = "FlowScrollPosition.";
    private const int MaxIdLength = 64;
    private const int MaxFileBytes = 64;
    private readonly string _directory;

    /// <param name="directory">Absolute directory path; created on first save when missing.</param>
    public FileFlowScrollPositionStore(string directory)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(directory);
        _directory = directory;
    }

    public static bool IsValidItemId(string? itemId)
    {
        if (string.IsNullOrEmpty(itemId) || itemId.Length > MaxIdLength) return false;
        foreach (var c in itemId)
        {
            var ok = c is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or >= '0' and <= '9' or '-' or '_';
            if (!ok) return false;
        }
        return true;
    }

    /// <summary>The saved fraction clamped to 0..1, or 0 when absent, invalid id, unreadable or non-finite.</summary>
    public double Load(string itemId)
    {
        if (!IsValidItemId(itemId)) return 0;
        try
        {
            var path = PathFor(itemId);
            if (!File.Exists(path)) return 0;
            using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
            var buffer = new byte[MaxFileBytes];
            var read = stream.Read(buffer, 0, buffer.Length);
            var text = System.Text.Encoding.UTF8.GetString(buffer, 0, read).Trim();
            return double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var v)
                ? ReadingProgress.Sanitize(v, 0)
                : 0;
        }
        catch (IOException) { return 0; }
        catch (UnauthorizedAccessException) { return 0; }
    }

    /// <summary>Best-effort: a refused id, non-finite value or write failure must not interrupt reading.</summary>
    public void Save(string itemId, double fraction)
    {
        if (!IsValidItemId(itemId) || !double.IsFinite(fraction)) return;
        try
        {
            Directory.CreateDirectory(_directory);
            File.WriteAllText(PathFor(itemId), Math.Clamp(fraction, 0, 1).ToString("R", CultureInfo.InvariantCulture));
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }

    private string PathFor(string itemId) => Path.Combine(_directory, FilePrefix + itemId);
}
