using CommunityToolkit.Mvvm.ComponentModel;

namespace Gist.Core.Rsvp;

/// <summary>Where the reader screen is in its lifecycle (spec §7.1: loading state, then the reader).</summary>
public enum RsvpLoadState
{
    Loading,
    Ready,

    /// <summary>The item opened but has no readable tokens.</summary>
    Empty,

    /// <summary>The core could not open a session (fixed message only; never raw error text).</summary>
    Failed,
}

/// <summary>
/// View model for the RSVP reader (<c>docs/windows-ui-spec.md</c> §7.1). Wraps
/// <see cref="RsvpPlaybackController"/> with the display strings, the progress persistence policy
/// and the load/leave lifecycle. No UI types; everything is driven through the injected engine
/// source, clock, timer and save callback so it is testable headless.
/// </summary>
/// <remarks>
/// <b>Progress persistence.</b> Progress is saved (via the core's <c>save_progress</c>) when
/// playback stops, on leaving the screen, and at most every <see cref="SaveInterval"/> while
/// playing, so a crash or window close loses seconds, not the session. Identical consecutive
/// positions are not re-saved. Failures never interrupt reading.
/// </remarks>
public sealed class RsvpReaderViewModel : ObservableObject, IDisposable
{
    /// <summary>Minimum gap between saves while playing continuously.</summary>
    public static readonly TimeSpan SaveInterval = TimeSpan.FromSeconds(5);

    private readonly Func<uint, Task<IRsvpEngine?>> _openEngine;
    private readonly Func<ulong, Task> _saveProgress;
    private readonly IRsvpClock _clock;
    private readonly IRsvpTimer _timer;
    private readonly uint _initialWpm;

    private RsvpPlaybackController? _controller;
    private RsvpLoadState _loadState = RsvpLoadState.Loading;
    private long? _lastSaveMs;
    private ulong? _lastSavedIndex;
    private bool _wasPlaying;
    private bool _disposed;

    /// <param name="itemId">The library item being read.</param>
    /// <param name="title">Shown in the header and used as the screen's accessible name.</param>
    /// <param name="openEngine">Opens the pacing session at the given WPM; null on failure.</param>
    /// <param name="saveProgress">Persists a token index (production: <c>CoreClient.SaveProgressAsync</c>).</param>
    public RsvpReaderViewModel(
        string itemId,
        string title,
        Func<uint, Task<IRsvpEngine?>> openEngine,
        Func<ulong, Task> saveProgress,
        IRsvpClock clock,
        IRsvpTimer timer,
        uint initialWpm = RsvpWpm.Default)
    {
        ArgumentException.ThrowIfNullOrEmpty(itemId);
        ArgumentNullException.ThrowIfNull(openEngine);
        ArgumentNullException.ThrowIfNull(saveProgress);
        ArgumentNullException.ThrowIfNull(clock);
        ArgumentNullException.ThrowIfNull(timer);
        ItemId = itemId;
        Title = title;
        _openEngine = openEngine;
        _saveProgress = saveProgress;
        _clock = clock;
        _timer = timer;
        _initialWpm = initialWpm;
    }

    public string ItemId { get; }

    public string Title { get; }

    public RsvpLoadState LoadState
    {
        get => _loadState;
        private set => SetProperty(ref _loadState, value);
    }

    /// <summary>The word to draw. Never blank while paused (see <see cref="RsvpPlaybackController.DisplayWord"/>).</summary>
    public string Word => _controller?.DisplayWord ?? string.Empty;

    public bool IsPlaying => _controller?.IsPlaying ?? false;

    /// <summary>Accessible/visible Play-Pause label: the action the button will perform.</summary>
    public string PlayPauseLabel => IsPlaying ? "Pause" : "Play";

    public uint Wpm => _controller?.Wpm ?? _initialWpm;

    public string WpmLabel => $"{Wpm} WPM";

    public ulong TokenCount => _controller?.TokenCount ?? 0;

    /// <summary>Zero-based index of the token on screen.</summary>
    public ulong Position => _controller?.Frame?.Index ?? 0;

    /// <summary>Scrubber maximum (last token index; 0 for an empty stream).</summary>
    public ulong MaxPosition => TokenCount == 0 ? 0 : TokenCount - 1;

    /// <summary>"n / total", n one-based (spec §7.1 item 2).</summary>
    public string ProgressText => TokenCount == 0 ? "0 / 0" : $"{Position + 1} / {TokenCount}";

    /// <summary>Opens the session. Safe to call once.</summary>
    public async Task LoadAsync()
    {
        LoadState = RsvpLoadState.Loading;
        IRsvpEngine? engine;
        try
        {
            engine = await _openEngine(_initialWpm).ConfigureAwait(true);
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            engine = null;
        }

        if (_disposed)
        {
            engine?.Dispose();
            return;
        }

        if (engine is null)
        {
            LoadState = RsvpLoadState.Failed;
            return;
        }

        if (engine.TokenCount == 0)
        {
            engine.Dispose();
            LoadState = RsvpLoadState.Empty;
            return;
        }

        _controller = new RsvpPlaybackController(engine, _clock, _timer);
        _controller.Changed += OnControllerChanged;
        LoadState = RsvpLoadState.Ready;
        RaiseAllDisplayChanged();
    }

    // ── Commands (all UI-thread) ───────────────────────────────────────────

    public void TogglePlayPause() => Guarded(c => c.Toggle());

    public void Pause() => Guarded(c => c.Pause());

    /// <summary>Spec §7.1 stretch: step one word (positive forward, negative back).</summary>
    public void StepWord(int direction) => Guarded(c => c.StepWords(direction < 0 ? -1 : 1));

    /// <summary>Spec §7.1 stretch: jump five words.</summary>
    public void JumpWords(int direction) => Guarded(c => c.StepWords(direction < 0 ? -5 : 5));

    public void SeekTo(ulong index) => Guarded(c =>
    {
        if (index != Position) c.Seek(index);
    });

    public void SetWpm(double wpm) => Guarded(c =>
    {
        var target = RsvpWpm.Clamp(wpm);
        if (target != c.Wpm) c.SetWpm(target);
    });

    /// <summary>
    /// Stops playback and persists the position. Await it when leaving the screen; the progress
    /// restores on the next open (spec §7.1 item 2).
    /// </summary>
    public async Task LeaveAsync()
    {
        if (_controller is { } c)
        {
            // Unsubscribe first so the pause below does not also queue its own (fire-and-forget) save.
            c.Changed -= OnControllerChanged;
            c.Pause();
            await SaveAsync(force: true).ConfigureAwait(true);
        }

        Dispose();
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        if (_controller is { } c)
        {
            c.Changed -= OnControllerChanged;
            c.Dispose();
        }
    }

    // ── Internals ──────────────────────────────────────────────────────────

    private void Guarded(Action<RsvpPlaybackController> action)
    {
        if (_disposed || _controller is not { } c) return;
        try
        {
            action(c);
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            // An engine failure mid-read stops playback rather than crashing the shell; the reader
            // stays on the last good frame. No exception text is surfaced (it may carry content).
            try
            {
                _timer.Cancel();
            }
            catch (Exception)
            {
                // Nothing further to do.
            }

            LoadState = RsvpLoadState.Failed;
        }
    }

    private void OnControllerChanged()
    {
        RaiseAllDisplayChanged();

        var playing = IsPlaying;
        var stopped = _wasPlaying && !playing;
        _wasPlaying = playing;
        if (stopped)
        {
            _ = SaveAsync(force: true);
        }
        else if (playing)
        {
            _ = SaveAsync(force: false);
        }
    }

    private async Task SaveAsync(bool force)
    {
        if (_controller?.Frame is not { } frame) return;
        var now = _clock.NowMs;
        if (!force && _lastSaveMs is { } last && now - last < (long)SaveInterval.TotalMilliseconds) return;
        if (_lastSavedIndex == frame.Index) return;

        _lastSaveMs = now;
        _lastSavedIndex = frame.Index;
        try
        {
            await _saveProgress(frame.Index).ConfigureAwait(true);
        }
        catch (Exception e) when (e is not OutOfMemoryException)
        {
            // Saving is best-effort; reading continues. Retry on the next save point.
            _lastSavedIndex = null;
        }
    }

    private void RaiseAllDisplayChanged()
    {
        OnPropertyChanged(nameof(Word));
        OnPropertyChanged(nameof(IsPlaying));
        OnPropertyChanged(nameof(PlayPauseLabel));
        OnPropertyChanged(nameof(Wpm));
        OnPropertyChanged(nameof(WpmLabel));
        OnPropertyChanged(nameof(TokenCount));
        OnPropertyChanged(nameof(Position));
        OnPropertyChanged(nameof(MaxPosition));
        OnPropertyChanged(nameof(ProgressText));
    }
}
