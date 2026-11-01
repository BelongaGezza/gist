namespace Gist.Core.Rsvp;

/// <summary>
/// Pure fit maths for the RSVP word (issue #131): shrink an over-wide token uniformly instead of
/// letting its layout slot clip it. Mirrors the macOS <c>OrpWordFit</c> rule (never below 8 pt), minus
/// the pivot: Windows has no ORP pivot, so the word is simply centred and needs the full width.
/// Kept free of UI types so it is unit-testable without a window.
/// </summary>
public static class RsvpWordFit
{
    /// <summary>The word is never drawn smaller than this (epx); any remainder is trimmed with an ellipsis.</summary>
    public const double MinimumFontSize = 8;

    /// <summary>
    /// Font size at which a word measuring <paramref name="naturalWidth"/> at <paramref name="baseFontSize"/>
    /// fits <paramref name="availableWidth"/>. Scales down only (never above the base), never below
    /// <see cref="MinimumFontSize"/> (or the base itself, if that is already smaller). Degenerate input
    /// (empty token, no measured width, no space yet) returns the base size unchanged.
    /// </summary>
    public static double FittedFontSize(double baseFontSize, double naturalWidth, double availableWidth)
    {
        if (!(baseFontSize > 0)) return baseFontSize;
        if (!(naturalWidth > 0) || !(availableWidth > 0) || naturalWidth <= availableWidth) return baseFontSize;

        var floor = Math.Min(MinimumFontSize, baseFontSize);
        var fitted = baseFontSize * (availableWidth / naturalWidth);
        return Math.Clamp(fitted, floor, baseFontSize);
    }
}
