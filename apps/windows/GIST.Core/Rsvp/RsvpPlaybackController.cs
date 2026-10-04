namespace Gist.Core.Rsvp;

/// <summary>
/// The RSVP playback loop, UI-free. Owns no pacing knowledge: every "what is on screen now" and
/// "when do I next need to look" answer comes from the engine (plan §4.3).
/// </summary>
/// <remarks>
/// <para>
/// <b>Drift model.</b> Playback is anchored to a monotonic clock at <see cref="Play"/>. Each tick
/// measures <c>elapsed = clock - anchor</c>, asks the engine what to show at that time, then sleeps
/// only until the engine's <c>NextBoundaryMs</c>. A late or coalesced tick therefore shows the
/// token that is correct <em>now</em> and re-aims at the next boundary; lateness never
/// accumulates. Pause banks the elapsed time in the engine (so stats exclude paused time) and
/// resume re-anchors, so paused time is never counted as reading time.
/// </para>
/// <para>
/// <b>Mutations while playing</b> (seek, step, WPM change) are done as pause -> mutate at elapsed 0
/// -> resume -> re-anchor. That way the engine banks the time played so far, the position is pinned
/// under the <em>old</em> speed, and the caller's counter restarts exactly when the engine's does.
/// </para>
/// <para>Not thread-safe: call on one thread (the UI thread).</para>
/// </remarks>
public sealed class RsvpPlaybackController : IDisposable
{
    private readonly IRsvpEngine _engine;
    private readonly IRsvpClock _clock;
    private readonly IRsvpTimer _timer;

    private long _anchorMs;
    private bool _disposed;

    public RsvpPlaybackController(IRsvpEngine engine, IRsvpClock clock, IRsvpTimer timer)
    {
        ArgumentNullException.ThrowIfNull(engine);
        ArgumentNullException.ThrowIfNull(clock);
        ArgumentNullException.ThrowIfNull(timer);
        _engine = engine;
        _clock = clock;
        _timer = timer;
        Wpm = engine.Wpm;
        Refresh();
    }

    /// <summary>Raised after any change to the frame, play state or WPM.</summary>
    public event Action? Changed;

    /// <summary>The frame currently shown; null only for an empty document.</summary>
    public RsvpFrame? Frame { get; private set; }

    public bool IsPlaying { get; private set; }

    /// <summary>True once playback ran off the end of the document and stopped by itself.</summary>
    public bool IsFinished { get; private set; }

    public uint Wpm { get; private set; }

    public ulong TokenCount => _engine.TokenCount;

    /// <summary>
    /// What the word area shows. Break tokens blank it while playing (the pause is part of the
    /// rhythm), but a paused reader never blanks: it shows the nearest preceding word.
    /// </summary>
    public string DisplayWord
    {
        get
        {
            if (Frame is null) return string.Empty;
            if (Frame.Kind == RsvpTokenKind.Word) return Frame.Text;
            if (IsPlaying) return string.Empty;
            var i = Frame.Index;
            for (var back = 0; back < 8 && i > 0; back++)
            {
                i--;
                if (_engine.IsWord(i)) return _engine.TokenText(i) ?? string.Empty;
            }

            return string.Empty;
        }
    }

    private ulong ElapsedMs => (ulong)Math.Max(0, _clock.NowMs - _anchorMs);

    // ── Transport ──────────────────────────────────────────────────────────

    public void Play()
    {
        ThrowIfDisposed();
        if (IsPlaying || Frame is null) return;

        if (IsFinished || Frame.IsLast)
        {
            _engine.Seek(0);
        }

        IsFinished = false;
        _engine.Resume();
        _anchorMs = _clock.NowMs;
        IsPlaying = true;
        Tick();
    }

    public void Pause()
    {
        ThrowIfDisposed();
        if (!IsPlaying) return;
        _timer.Cancel();
        _engine.Pause(ElapsedMs);
        IsPlaying = false;
        Refresh();
        Changed?.Invoke();
    }

    public void Toggle()
    {
        if (IsPlaying) Pause();
        else Play();
    }

    /// <summary>Jumps to token <paramref name="index"/> (clamped by the engine).</summary>
    public void Seek(ulong index) => Mutate(() => _engine.Seek(index));

    /// <summary>
    /// Moves by <paramref name="words"/> word tokens (negative = back); breaks are skipped over and
    /// not counted. Back-stepping is the engine's own <c>BackWords</c>.
    /// </summary>
    public void StepWords(int words)
    {
        if (words == 0) return;
        Mutate(() =>
        {
            if (words < 0)
            {
                _engine.BackWords((ulong)(-(long)words), 0);
                return;
            }

            var last = _engine.TokenCount == 0 ? 0 : _engine.TokenCount - 1;
            var i = _engine.Cursor;
            var remaining = words;
            while (remaining > 0 && i < last)
            {
                i++;
                if (_engine.IsWord(i)) remaining--;
            }

            _engine.Seek(i);
        });
    }

    /// <summary>Changes speed. The core clamps and the position never jumps.</summary>
    public void SetWpm(uint wpm) => Mutate(() => _engine.SetWpm(wpm, 0));

    // ── Internals ──────────────────────────────────────────────────────────

    private void Mutate(Action op)
    {
        ThrowIfDisposed();
        if (Frame is null) return;

        var wasPlaying = IsPlaying;
        if (wasPlaying)
        {
            _timer.Cancel();
            _engine.Pause(ElapsedMs);
            IsPlaying = false;
        }

        op();
        IsFinished = false;
        Wpm = _engine.Wpm;

        if (wasPlaying)
        {
            _engine.Resume();
            _anchorMs = _clock.NowMs;
            IsPlaying = true;
            Tick();
        }
        else
        {
            Refresh();
            Changed?.Invoke();
        }
    }

    /// <summary>One playback step: show what is correct now, then sleep until the next boundary.</summary>
    private void Tick()
    {
        if (_disposed || !IsPlaying) return;

        var elapsed = ElapsedMs;
        var frame = _engine.FrameAtElapsed(elapsed);
        if (frame is null)
        {
            _engine.Pause(elapsed);
            IsPlaying = false;
            Refresh();
            Changed?.Invoke();
            return;
        }

        Frame = frame;

        if (frame.IsLast && elapsed >= frame.NextBoundaryMs)
        {
            // The final token's display time has run out: stop on it (never blank) and bank the time.
            _engine.Pause(elapsed);
            IsPlaying = false;
            IsFinished = true;
            Changed?.Invoke();
            return;
        }

        Changed?.Invoke();
        var wait = frame.NextBoundaryMs > elapsed ? frame.NextBoundaryMs - elapsed : 1;
        _timer.Schedule(TimeSpan.FromMilliseconds(Math.Max(1UL, wait)), Tick);
    }

    /// <summary>Re-reads the frame at the engine's pinned cursor (paused, or elapsed 0 after a mutation).</summary>
    private void Refresh()
    {
        Frame = _engine.FrameAtElapsed(0);
        Wpm = _engine.Wpm;
    }

    private void ThrowIfDisposed() => ObjectDisposedException.ThrowIf(_disposed, this);

    /// <summary>Stops the timer and releases the engine. Does not bank elapsed time; call <see cref="Pause"/> first to.</summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _timer.Cancel();
        _engine.Dispose();
    }
}
