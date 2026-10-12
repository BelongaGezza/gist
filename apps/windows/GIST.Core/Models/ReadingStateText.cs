using System.Globalization;

namespace Gist.Core.Models;

/// <summary>
/// Display text for an item's reading state (ADR-021), kept UI-free so it is unit-testable.
/// Progress counts RSVP reading only, so a flow-view-only item shows a last-read date but no bar.
/// </summary>
public static class ReadingStateText
{
    /// <summary>"Not read yet", "Last read today", "Last read yesterday" or "Last read 12 Mar 2026".</summary>
    public static string LastRead(long? unixMs, DateTimeOffset now)
    {
        if (unixMs is not { } ms) return "Not read yet";
        DateTimeOffset when;
        try { when = DateTimeOffset.FromUnixTimeMilliseconds(ms).ToLocalTime(); }
        catch (ArgumentOutOfRangeException) { return "Not read yet"; }

        var days = (now.ToLocalTime().Date - when.Date).Days;
        if (days <= 0) return "Last read today";
        if (days == 1) return "Last read yesterday";
        return "Last read " + when.ToString("d MMM yyyy", CultureInfo.CurrentCulture);
    }

    /// <summary>Whole percent 0..100; non-finite or out-of-range input is clamped.</summary>
    public static int ProgressPercent(double fraction) =>
        double.IsFinite(fraction) ? (int)Math.Round(Math.Clamp(fraction, 0, 1) * 100) : 0;
}
