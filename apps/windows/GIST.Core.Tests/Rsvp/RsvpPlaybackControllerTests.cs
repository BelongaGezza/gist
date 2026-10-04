using Gist.Core.Rsvp;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Rsvp;

/// <summary>
/// The playback loop against a scripted engine and hand-driven clock/timer: deterministic, no real
/// time. (The real Rust engine is exercised in <see cref="RsvpRealEngineTests"/>.)
/// </summary>
public sealed class RsvpPlaybackControllerTests
{
    private static readonly string[] Words = { "one", "two", "three", "four", "five", "six", "seven", "eight" };

    private readonly FakeRsvpClock _clock = new();
    private readonly FakeRsvpTimer _timer = new();

    private (RsvpPlaybackController Controller, ScriptedRsvpEngine Engine) Create(params string[] tokens)
    {
        var engine = new ScriptedRsvpEngine(tokens.Length == 0 ? Words : tokens);
        return (new RsvpPlaybackController(engine, _clock, _timer), engine);
    }

    [Fact]
    public void Starts_paused_on_the_first_word_without_a_pending_timer()
    {
        var (c, _) = Create();

        Assert.False(c.IsPlaying);
        Assert.Equal(0UL, c.Frame!.Index);
        Assert.Equal("one", c.DisplayWord);
        Assert.False(_timer.IsPending);
    }

    [Fact]
    public void Play_shows_the_current_word_and_sleeps_until_the_engines_next_boundary()
    {
        var (c, _) = Create();

        c.Play();

        Assert.True(c.IsPlaying);
        Assert.Equal(0UL, c.Frame!.Index);
        Assert.Equal(TimeSpan.FromMilliseconds(100), _timer.PendingDelay);
    }

    [Fact]
    public void A_late_tick_shows_the_token_correct_now_and_aims_at_the_next_boundary_not_a_full_interval()
    {
        var (c, _) = Create();
        c.Play();

        // The timer fires 250 ms in instead of 100: tokens 0 and 1 have already gone by.
        _clock.Advance(250);
        _timer.Fire();

        Assert.Equal(2UL, c.Frame!.Index);
        // Token 2 ends at 300 ms on the window clock; 250 ms have elapsed -> sleep 50, not 100.
        Assert.Equal(TimeSpan.FromMilliseconds(50), _timer.PendingDelay);
    }

    [Fact]
    public void Lateness_does_not_accumulate_over_many_ticks()
    {
        var tokens = Enumerable.Range(0, 200).Select(i => "w" + i).ToArray();
        var (c, _) = Create(tokens);
        c.Play();

        // Every wake-up is 30 ms late (a heavily loaded UI thread).
        for (var i = 0; i < 150 && _timer.IsPending; i++)
        {
            _clock.Advance((long)_timer.PendingDelay!.Value.TotalMilliseconds + 30);
            _timer.Fire();
        }

        // Position tracks wall time exactly (100 ms per word), regardless of the 150 late ticks.
        Assert.Equal((ulong)(_clock.NowMs / 100), c.Frame!.Index);
    }

    [Fact]
    public void Pause_banks_elapsed_time_with_the_engine_and_stops_the_timer()
    {
        var (c, e) = Create();
        c.Play();
        _clock.Advance(250);

        c.Pause();

        Assert.False(c.IsPlaying);
        Assert.False(_timer.IsPending);
        Assert.Equal(new ulong[] { 250 }, e.PauseCalls);
        Assert.Equal(2UL, c.Frame!.Index);
    }

    [Fact]
    public void Paused_time_is_not_reading_time()
    {
        var (c, e) = Create();
        c.Play();
        _clock.Advance(150);
        c.Pause();

        _clock.Advance(60_000); // a minute away from the desk
        c.Play();

        Assert.Equal(1UL, c.Frame!.Index); // still on the word we paused on, not a minute ahead
        _clock.Advance(100);
        _timer.Fire();
        Assert.Equal(2UL, c.Frame!.Index);
        Assert.Equal(2, e.ResumeCalls);
    }

    [Fact]
    public void Reaching_the_last_token_stops_playback_on_it_after_its_display_time()
    {
        var (c, e) = Create("a", "b", "c");
        c.Play();

        _clock.Advance(250);
        _timer.Fire();
        Assert.True(c.IsPlaying);
        Assert.True(c.Frame!.IsLast);
        Assert.Equal(TimeSpan.FromMilliseconds(50), _timer.PendingDelay);

        _clock.Advance(50);
        _timer.Fire();

        Assert.False(c.IsPlaying);
        Assert.True(c.IsFinished);
        Assert.Equal("c", c.DisplayWord);
        Assert.False(_timer.IsPending);
        Assert.Equal(new ulong[] { 300 }, e.PauseCalls);
    }

    [Fact]
    public void Playing_again_after_finishing_restarts_from_the_beginning()
    {
        var (c, _) = Create("a", "b");
        c.Play();
        _clock.Advance(500);
        _timer.Fire();
        Assert.True(c.IsFinished);

        c.Play();

        Assert.True(c.IsPlaying);
        Assert.False(c.IsFinished);
        Assert.Equal(0UL, c.Frame!.Index);
    }

    [Fact]
    public void Changing_wpm_while_playing_does_not_jump_the_reader_and_re_anchors()
    {
        var (c, e) = Create();
        c.Play();
        _clock.Advance(250); // on token 2 at 600 WPM
        _timer.Fire();

        c.SetWpm(300); // 200 ms per word now

        Assert.True(c.IsPlaying);
        Assert.Equal(300U, c.Wpm);
        Assert.Equal(2UL, c.Frame!.Index);
        Assert.Equal(TimeSpan.FromMilliseconds(200), _timer.PendingDelay);
        Assert.Equal(new ulong[] { 250 }, e.PauseCalls);
    }

    [Fact]
    public void Wpm_is_whatever_the_core_clamped_it_to()
    {
        var (c, _) = Create();

        c.SetWpm(5000);

        Assert.Equal(1000U, c.Wpm);
    }

    [Fact]
    public void Step_forward_and_back_move_by_word_tokens_skipping_breaks()
    {
        var (c, _) = Create("a", "b", "", "c", "d");

        c.StepWords(2);
        Assert.Equal("c", c.DisplayWord); // skipped the break
        Assert.Equal(3UL, c.Frame!.Index);

        c.StepWords(-2);
        Assert.Equal(0UL, c.Frame!.Index);
    }

    [Fact]
    public void Step_clamps_at_both_ends()
    {
        var (c, _) = Create("a", "b", "c");

        c.StepWords(-5);
        Assert.Equal(0UL, c.Frame!.Index);
        c.StepWords(50);
        Assert.Equal(2UL, c.Frame!.Index);
    }

    [Fact]
    public void Seek_while_playing_continues_from_the_new_position()
    {
        var (c, _) = Create();
        c.Play();
        _clock.Advance(120);

        c.Seek(5);

        Assert.True(c.IsPlaying);
        Assert.Equal(5UL, c.Frame!.Index);
        Assert.Equal(TimeSpan.FromMilliseconds(100), _timer.PendingDelay);
    }

    [Fact]
    public void A_paused_reader_on_a_break_shows_the_previous_word_but_a_playing_one_blanks()
    {
        var (c, _) = Create("a", "b", "", "c");

        c.Seek(2);
        Assert.Equal("b", c.DisplayWord); // paused: never blank

        c.Play();
        Assert.Equal(string.Empty, c.DisplayWord); // playing: the break is a visible pause
    }

    [Fact]
    public void Changed_fires_for_each_new_frame_and_for_transport_changes()
    {
        var (c, _) = Create();
        var count = 0;
        c.Changed += () => count++;

        c.Play();
        _clock.Advance(100);
        _timer.Fire();
        c.Pause();

        Assert.Equal(3, count);
    }

    [Fact]
    public void Dispose_cancels_the_timer_and_releases_the_engine()
    {
        var (c, e) = Create();
        c.Play();

        c.Dispose();

        Assert.False(_timer.IsPending);
        Assert.True(e.Disposed);
        Assert.Throws<ObjectDisposedException>(() => c.Play());
    }

    [Fact]
    public void An_empty_document_has_no_frame_and_play_is_a_no_op()
    {
        var engine = new ScriptedRsvpEngine(Array.Empty<string>());
        using var c = new RsvpPlaybackController(engine, _clock, _timer);

        c.Play();

        Assert.Null(c.Frame);
        Assert.False(c.IsPlaying);
        Assert.Equal(string.Empty, c.DisplayWord);
    }
}
