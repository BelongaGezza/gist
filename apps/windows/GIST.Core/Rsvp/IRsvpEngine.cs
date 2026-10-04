namespace Gist.Core.Rsvp;

/// <summary>
/// The pacing engine as the reader sees it. The production implementation is a thin wrapper over
/// the Rust engine (<c>FfiRsvpSession</c>); tests substitute a scripted one. Calls are short,
/// non-blocking and safe on the UI thread.
/// </summary>
/// <remarks>
/// The elapsed-time contract is the core's: <c>elapsedMs</c> is measured from the last
/// <see cref="Resume"/>, and after <see cref="Seek"/>, <see cref="BackWords"/> or
/// <see cref="SetWpm"/> the caller restarts its own counter. <see cref="RsvpPlaybackController"/>
/// is the only caller and always mutates while paused so this stays simple.
/// </remarks>
public interface IRsvpEngine : IDisposable
{
    ulong TokenCount { get; }

    /// <summary>Index the current play window starts from (where the last pause/seek left the reader).</summary>
    ulong Cursor { get; }

    /// <summary>Effective words-per-minute, already clamped by the core.</summary>
    uint Wpm { get; }

    /// <summary>What to show at <paramref name="elapsedMs"/>; null only for an empty stream.</summary>
    RsvpFrame? FrameAtElapsed(ulong elapsedMs);

    /// <summary>Text of token <paramref name="index"/>, or null when out of range.</summary>
    string? TokenText(ulong index);

    /// <summary>True when token <paramref name="index"/> exists and is a word (not a break).</summary>
    bool IsWord(ulong index);

    void Seek(ulong index);

    void Pause(ulong elapsedMs);

    void Resume();

    void BackWords(ulong count, ulong elapsedMs);

    void SetWpm(uint wpm, ulong elapsedMs);
}

/// <summary>Monotonic millisecond clock the playback loop re-anchors to (never wall-clock time).</summary>
public interface IRsvpClock
{
    long NowMs { get; }
}

/// <summary><see cref="System.Diagnostics.Stopwatch"/>-backed production clock.</summary>
public sealed class StopwatchRsvpClock : IRsvpClock
{
    private readonly long _start = System.Diagnostics.Stopwatch.GetTimestamp();

    public long NowMs =>
        (long)System.Diagnostics.Stopwatch.GetElapsedTime(_start).TotalMilliseconds;
}

/// <summary>
/// One-shot timer. The shell implements it with a <c>DispatcherQueueTimer</c>; the callback runs on
/// the UI thread. <see cref="Schedule"/> replaces any pending wake-up.
/// </summary>
public interface IRsvpTimer
{
    void Schedule(TimeSpan delay, Action callback);

    void Cancel();
}
