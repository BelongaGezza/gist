namespace Gist.Core.Rsvp;

/// <summary>Kind of one RSVP token. Mirrors the core's token kinds; breaks display as a pause.</summary>
public enum RsvpTokenKind
{
    Word,
    ParagraphBreak,
    SectionBreak,
}

/// <summary>
/// One frame of the RSVP reader, exactly as the Rust pacing engine answered for a given elapsed
/// time. Nothing here is computed in C#: durations and boundaries come from the core
/// (<c>docs/windows-development-plan.md</c> §4.3).
/// </summary>
/// <param name="Index">Index of the token to display.</param>
/// <param name="Text">Token text; empty for a break token.</param>
/// <param name="Kind">Word, or a paragraph/section break.</param>
/// <param name="DurationMs">How long this token stays on screen.</param>
/// <param name="NextBoundaryMs">
/// Elapsed time (same clock as the query) at which this token stops being the one to show. A timer
/// sleeps until this, then asks again.
/// </param>
/// <param name="IsLast">True when there is nothing after this token.</param>
/// <param name="TokenCount">Total tokens in the stream.</param>
public sealed record RsvpFrame(
    ulong Index,
    string Text,
    RsvpTokenKind Kind,
    ulong DurationMs,
    ulong NextBoundaryMs,
    bool IsLast,
    ulong TokenCount);

/// <summary>The WPM control's range, for the slider and number box only; the core clamps for real.</summary>
public static class RsvpWpm
{
    public const uint Min = 100;
    public const uint Max = 1000;
    public const uint Step = 10;
    public const uint Default = 300;

    /// <summary>Clamps to the supported range (mirrors the core, which remains the authority).</summary>
    public static uint Clamp(double wpm) =>
        double.IsNaN(wpm) ? Default : (uint)Math.Clamp(Math.Round(wpm), Min, Max);
}
