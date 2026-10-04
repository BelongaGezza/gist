using Gist.Core.Rsvp;

namespace Gist.Core.Tests.TestSupport;

/// <summary>A clock a test advances by hand.</summary>
public sealed class FakeRsvpClock : IRsvpClock
{
    public long NowMs { get; set; }

    public void Advance(long ms) => NowMs += ms;
}

/// <summary>
/// A timer that never fires on its own: it records the pending wake-up so a test can assert how
/// long the controller asked to sleep, then run the callback "late" by advancing the clock first.
/// </summary>
public sealed class FakeRsvpTimer : IRsvpTimer
{
    private Action? _callback;

    public TimeSpan? PendingDelay { get; private set; }

    public bool IsPending => _callback is not null;

    public int ScheduleCount { get; private set; }

    public int CancelCount { get; private set; }

    public void Schedule(TimeSpan delay, Action callback)
    {
        PendingDelay = delay;
        _callback = callback;
        ScheduleCount++;
    }

    public void Cancel()
    {
        PendingDelay = null;
        _callback = null;
        CancelCount++;
    }

    /// <summary>Runs the pending callback (as the UI thread would when the timer fires).</summary>
    public void Fire()
    {
        var cb = _callback ?? throw new InvalidOperationException("No pending wake-up.");
        _callback = null;
        PendingDelay = null;
        cb();
    }
}

/// <summary>
/// A scripted <see cref="IRsvpEngine"/> that honours the core's elapsed-time contract with a trivial
/// pacing rule (every word <c>PerWordMs</c>, every break <c>PerBreakMs</c>). It exists to test the
/// controller's <em>use</em> of the contract (anchoring, banking, re-anchoring on mutation); it is
/// deliberately not a copy of the real pacing table — that stays in Rust, and the integration tests
/// run against the real engine.
/// </summary>
public sealed class ScriptedRsvpEngine : IRsvpEngine
{
    private readonly List<(string Text, RsvpTokenKind Kind)> _tokens;
    private bool _playing;
    private ulong _cursor;
    private uint _wpm;

    public ScriptedRsvpEngine(IEnumerable<string> tokens, uint wpm = 600, int perBreakMs = 100)
    {
        // "" is a paragraph break.
        _tokens = tokens.Select(t => (t, t.Length == 0 ? RsvpTokenKind.ParagraphBreak : RsvpTokenKind.Word)).ToList();
        _wpm = wpm;
        PerBreakMs = perBreakMs;
    }

    public int PerBreakMs { get; }

    public ulong PerWordMs => 60_000UL / _wpm;

    public List<ulong> PauseCalls { get; } = new();

    public int ResumeCalls { get; private set; }

    public bool Disposed { get; private set; }

    /// <summary>Test seam: make the named engine call fault like a native error would.</summary>
    public bool ThrowOnTokenCount { get; set; }

    public bool ThrowOnPause { get; set; }

    public ulong TokenCount => ThrowOnTokenCount ? throw new InvalidOperationException("engine fault") : (ulong)_tokens.Count;

    public ulong Cursor => _cursor;

    public uint Wpm => _wpm;

    private ulong DurationOf(int i) => _tokens[i].Kind == RsvpTokenKind.Word ? PerWordMs : (ulong)PerBreakMs;

    public RsvpFrame? FrameAtElapsed(ulong elapsedMs)
    {
        if (_tokens.Count == 0) return null;
        var effective = _playing ? elapsedMs : 0;
        var i = (int)_cursor;
        ulong end = 0;
        // end of token i on this window's clock
        end = DurationOf(i);
        while (effective >= end && i < _tokens.Count - 1)
        {
            i++;
            end += DurationOf(i);
        }

        return new RsvpFrame(
            (ulong)i,
            _tokens[i].Text,
            _tokens[i].Kind,
            DurationOf(i),
            end,
            i == _tokens.Count - 1,
            (ulong)_tokens.Count);
    }

    public string? TokenText(ulong index) => index < (ulong)_tokens.Count ? _tokens[(int)index].Text : null;

    public bool IsWord(ulong index) => index < (ulong)_tokens.Count && _tokens[(int)index].Kind == RsvpTokenKind.Word;

    public void Seek(ulong index) => _cursor = Math.Min(index, (ulong)Math.Max(0, _tokens.Count - 1));

    public void Pause(ulong elapsedMs)
    {
        PauseCalls.Add(elapsedMs);
        if (ThrowOnPause) throw new InvalidOperationException("engine fault");
        if (!_playing) return;
        _cursor = FrameAtElapsed(elapsedMs)!.Index;
        _playing = false;
    }

    public void Resume()
    {
        ResumeCalls++;
        _playing = true;
    }

    public void BackWords(ulong count, ulong elapsedMs)
    {
        var current = (int)FrameAtElapsed(elapsedMs)!.Index;
        var skipped = 0UL;
        while (current > 0 && skipped < count)
        {
            current--;
            if (_tokens[current].Kind == RsvpTokenKind.Word) skipped++;
        }

        _cursor = (ulong)current;
    }

    public void SetWpm(uint wpm, ulong elapsedMs)
    {
        _cursor = FrameAtElapsed(elapsedMs)!.Index;
        _wpm = Math.Clamp(wpm, 100u, 1000u);
    }

    public void Dispose() => Disposed = true;
}
