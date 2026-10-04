using Gist.Core.Rsvp;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Rsvp;

public sealed class RsvpReaderViewModelTests
{
    private readonly FakeRsvpClock _clock = new();
    private readonly FakeRsvpTimer _timer = new();
    private readonly List<ulong> _saved = new();

    private RsvpReaderViewModel Create(Func<uint, Task<IRsvpEngine?>> open) =>
        new("item-1", "A Book", open, idx =>
        {
            _saved.Add(idx);
            return Task.CompletedTask;
        }, _clock, _timer);

    private static Func<uint, Task<IRsvpEngine?>> Engine(params string[] tokens) =>
        wpm => Task.FromResult<IRsvpEngine?>(new ScriptedRsvpEngine(tokens, wpm));

    private async Task<RsvpReaderViewModel> ReadyAsync(params string[] tokens)
    {
        var vm = Create(Engine(tokens.Length == 0 ? new[] { "a", "b", "c", "d", "e", "f" } : tokens));
        await vm.LoadAsync();
        return vm;
    }

    [Fact]
    public void Before_load_it_is_loading_with_the_initial_wpm_and_no_word()
    {
        using var vm = Create(Engine("a"));

        Assert.Equal(RsvpLoadState.Loading, vm.LoadState);
        Assert.Equal(300U, vm.Wpm);
        Assert.Equal("300 WPM", vm.WpmLabel);
        Assert.Equal(string.Empty, vm.Word);
    }

    [Fact]
    public async Task Load_success_is_ready_and_shows_progress_one_based()
    {
        using var vm = await ReadyAsync("a", "b", "c");

        Assert.Equal(RsvpLoadState.Ready, vm.LoadState);
        Assert.Equal("a", vm.Word);
        Assert.Equal("1 / 3", vm.ProgressText);
        Assert.Equal(2UL, vm.MaxPosition);
        Assert.Equal("Play", vm.PlayPauseLabel);
    }

    [Fact]
    public async Task Load_with_no_tokens_is_empty_and_releases_the_engine()
    {
        var engine = new ScriptedRsvpEngine(Array.Empty<string>());
        using var vm = Create(_ => Task.FromResult<IRsvpEngine?>(engine));

        await vm.LoadAsync();

        Assert.Equal(RsvpLoadState.Empty, vm.LoadState);
        Assert.True(engine.Disposed);
    }

    [Fact]
    public async Task Load_failure_is_Failed_whether_the_source_returns_null_or_throws()
    {
        using var nullVm = Create(_ => Task.FromResult<IRsvpEngine?>(null));
        await nullVm.LoadAsync();
        Assert.Equal(RsvpLoadState.Failed, nullVm.LoadState);

        using var throwVm = Create(_ => throw new InvalidOperationException("boom"));
        await throwVm.LoadAsync();
        Assert.Equal(RsvpLoadState.Failed, throwVm.LoadState);
    }

    [Fact]
    public async Task Toggle_flips_the_label_and_notifies()
    {
        using var vm = await ReadyAsync();
        var changed = new List<string?>();
        vm.PropertyChanged += (_, e) => changed.Add(e.PropertyName);

        vm.TogglePlayPause();

        Assert.True(vm.IsPlaying);
        Assert.Equal("Pause", vm.PlayPauseLabel);
        Assert.Contains(nameof(RsvpReaderViewModel.PlayPauseLabel), changed);
        Assert.Contains(nameof(RsvpReaderViewModel.IsPlaying), changed);

        vm.TogglePlayPause();
        Assert.Equal("Play", vm.PlayPauseLabel);
    }

    [Fact]
    public async Task Pausing_saves_progress_at_the_word_on_screen()
    {
        using var vm = await ReadyAsync("a", "b", "c", "d", "e", "f");
        vm.TogglePlayPause();
        _clock.Advance(450); // 200 ms per word at the default 300 WPM
        _timer.Fire();

        vm.TogglePlayPause();

        Assert.Equal(new ulong[] { 2 }, _saved.TakeLast(1).ToArray());
    }

    [Fact]
    public async Task Saving_while_playing_is_throttled_and_never_repeats_a_position()
    {
        var tokens = Enumerable.Range(0, 400).Select(i => "w" + i).ToArray();
        using var vm = await ReadyAsync(tokens);
        vm.TogglePlayPause();

        // First tick saves; the next 4.9 s of ticks do not.
        var saves = _saved.Count;
        for (var i = 0; i < 49; i++)
        {
            _clock.Advance(100);
            _timer.Fire();
        }

        Assert.Equal(1, saves);
        Assert.Single(_saved);

        _clock.Advance(100);
        _timer.Fire();
        Assert.Equal(2, _saved.Count);
        Assert.Equal(_saved.Distinct().Count(), _saved.Count);
    }

    [Fact]
    public async Task Leaving_pauses_saves_the_exact_position_once_and_disposes()
    {
        var engine = new ScriptedRsvpEngine(new[] { "a", "b", "c", "d", "e" });
        using var vm = Create(_ => Task.FromResult<IRsvpEngine?>(engine));
        await vm.LoadAsync();
        vm.TogglePlayPause();
        _clock.Advance(310);
        _timer.Fire();
        _saved.Clear();

        await vm.LeaveAsync();

        Assert.Equal(new ulong[] { 3 }, _saved.ToArray());
        Assert.True(engine.Disposed);
        Assert.False(_timer.IsPending);
    }

    [Fact]
    public async Task A_failing_save_never_interrupts_reading()
    {
        var vm = new RsvpReaderViewModel(
            "i",
            "t",
            Engine("a", "b", "c"),
            _ => throw new IOException("disk full"),
            _clock,
            _timer);
        await vm.LoadAsync();

        vm.TogglePlayPause();
        vm.TogglePlayPause();

        Assert.Equal(RsvpLoadState.Ready, vm.LoadState);
        vm.Dispose();
    }

    [Fact]
    public async Task SetWpm_clamps_and_ignores_a_no_op()
    {
        using var vm = await ReadyAsync();

        vm.SetWpm(5000);
        Assert.Equal(1000U, vm.Wpm);

        vm.SetWpm(double.NaN);
        Assert.Equal(RsvpWpm.Default, vm.Wpm);

        vm.SetWpm(20);
        Assert.Equal(100U, vm.Wpm);
    }

    [Fact]
    public async Task Step_and_jump_use_one_and_five_words_in_either_direction()
    {
        var tokens = Enumerable.Range(0, 30).Select(i => "w" + i).ToArray();
        using var vm = await ReadyAsync(tokens);

        vm.JumpWords(1);
        Assert.Equal(5UL, vm.Position);
        vm.StepWord(1);
        Assert.Equal(6UL, vm.Position);
        vm.StepWord(-1);
        vm.JumpWords(-1);
        Assert.Equal(0UL, vm.Position);
    }

    [Fact]
    public async Task SeekTo_moves_the_position()
    {
        using var vm = await ReadyAsync("a", "b", "c", "d");

        vm.SeekTo(3);

        Assert.Equal(3UL, vm.Position);
        Assert.Equal("4 / 4", vm.ProgressText);
    }

    [Fact]
    public async Task An_engine_that_throws_mid_read_stops_playback_and_reports_Failed_without_crashing()
    {
        var engine = new ThrowingOnPauseEngine();
        using var vm = Create(_ => Task.FromResult<IRsvpEngine?>(engine));
        await vm.LoadAsync();
        vm.TogglePlayPause();

        vm.TogglePlayPause(); // pause throws inside the engine

        Assert.Equal(RsvpLoadState.Failed, vm.LoadState);
        Assert.False(_timer.IsPending);
    }

    private sealed class ThrowingOnPauseEngine : IRsvpEngine
    {
        private readonly ScriptedRsvpEngine _inner = new(new[] { "a", "b", "c" });

        public ulong TokenCount => _inner.TokenCount;

        public ulong Cursor => _inner.Cursor;

        public uint Wpm => _inner.Wpm;

        public RsvpFrame? FrameAtElapsed(ulong elapsedMs) => _inner.FrameAtElapsed(elapsedMs);

        public string? TokenText(ulong index) => _inner.TokenText(index);

        public bool IsWord(ulong index) => _inner.IsWord(index);

        public void Seek(ulong index) => _inner.Seek(index);

        public void Pause(ulong elapsedMs) => throw new InvalidOperationException("engine failure");

        public void Resume() => _inner.Resume();

        public void BackWords(ulong count, ulong elapsedMs) => _inner.BackWords(count, elapsedMs);

        public void SetWpm(uint wpm, ulong elapsedMs) => _inner.SetWpm(wpm, elapsedMs);

        public void Dispose() => _inner.Dispose();
    }
}
