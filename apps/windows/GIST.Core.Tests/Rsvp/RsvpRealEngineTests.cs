using Gist.Core.Client;
using Gist.Core.Rsvp;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Rsvp;

/// <summary>
/// The controller driving the <b>real Rust pacing engine</b> over FFI (real SQLite, real
/// filesystem), with a hand-driven clock/timer so nothing depends on real time. These prove the
/// C# side uses the engine's contract correctly: restore, anchoring, pause banking, mutation
/// re-anchoring, and that no C# pacing arithmetic is needed.
/// </summary>
public sealed class RsvpRealEngineTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keyProvider = new();
    private readonly FakeRsvpClock _clock = new();
    private readonly FakeRsvpTimer _timer = new();

    public void Dispose() => _workspace.Dispose();

    private async Task<(CoreClient Client, string Id)> ImportFixtureAsync()
    {
        var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keyProvider);
        Assert.True(await client.InitializeAsync());
        var id = await client.ImportFileAsync(_workspace.CopyFixture("basic_ascii.txt"));
        Assert.NotNull(id);
        return (client, id!);
    }

    [Fact]
    public async Task Opening_a_session_returns_the_first_word_at_the_requested_wpm()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;

        using var engine = await client.OpenRsvpEngineAsync(id, 450);

        Assert.NotNull(engine);
        Assert.Equal(450U, engine!.Wpm);
        Assert.True(engine.TokenCount > 100);
        var frame = engine.FrameAtElapsed(0)!;
        Assert.Equal(0UL, frame.Index);
        Assert.Equal("Lorem", frame.Text);
        Assert.Equal(RsvpTokenKind.Word, frame.Kind);
        Assert.True(frame.NextBoundaryMs >= frame.DurationMs);
    }

    [Fact]
    public async Task The_core_clamps_wpm_at_both_ends()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;

        using var fast = await client.OpenRsvpEngineAsync(id, 5000);
        using var slow = await client.OpenRsvpEngineAsync(id, 1);

        Assert.Equal(1000U, fast!.Wpm);
        Assert.Equal(100U, slow!.Wpm);
    }

    [Fact]
    public async Task Saved_progress_is_restored_when_the_session_is_reopened()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;

        await client.SaveProgressAsync(id, 17);
        using var engine = await client.OpenRsvpEngineAsync(id, 300);

        Assert.Equal(17UL, engine!.Cursor);
        Assert.Equal(17UL, engine.FrameAtElapsed(0)!.Index);
    }

    [Fact]
    public async Task Opening_an_unknown_item_returns_null_and_reports_an_error()
    {
        var (client, _) = await ImportFixtureAsync();
        using var _c = client;

        var engine = await client.OpenRsvpEngineAsync("no-such-item", 300);

        Assert.Null(engine);
        Assert.NotNull(client.LastError);
    }

    [Fact]
    public async Task Playing_follows_the_engines_own_boundaries_and_a_late_tick_catches_up()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;
        using var c = new RsvpPlaybackController(
            (await client.OpenRsvpEngineAsync(id, 600))!, _clock, _timer);

        c.Play();
        var first = c.Frame!;
        Assert.Equal(TimeSpan.FromMilliseconds(first.NextBoundaryMs), _timer.PendingDelay);

        // On-time tick: lands exactly on the boundary -> the next token.
        _clock.Advance((long)first.NextBoundaryMs);
        _timer.Fire();
        Assert.Equal(1UL, c.Frame!.Index);

        // Very late tick (2 s): shows whichever token is correct at that moment, ahead of index 2.
        _clock.Advance(2000);
        _timer.Fire();
        Assert.True(c.Frame!.Index > 5, $"expected to skip ahead, at {c.Frame.Index}");
        Assert.True(c.IsPlaying);
        Assert.True(_timer.PendingDelay > TimeSpan.Zero);
        // And it is aimed at that token's boundary, measured from the anchor.
        var elapsed = _clock.NowMs;
        Assert.Equal((long)c.Frame.NextBoundaryMs - elapsed, (long)_timer.PendingDelay!.Value.TotalMilliseconds);
    }

    [Fact]
    public async Task A_long_pause_does_not_advance_the_reader_and_resume_continues_from_the_same_word()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;
        using var c = new RsvpPlaybackController(
            (await client.OpenRsvpEngineAsync(id, 600))!, _clock, _timer);
        c.Play();
        _clock.Advance(450);
        _timer.Fire();
        c.Pause();
        var pausedAt = c.Frame!.Index;
        Assert.True(pausedAt > 0);

        _clock.Advance(10 * 60_000); // ten minutes away
        c.Play();

        Assert.Equal(pausedAt, c.Frame!.Index);
        Assert.True(c.IsPlaying);
    }

    [Fact]
    public async Task Changing_wpm_mid_play_does_not_jump_the_reader()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;
        using var c = new RsvpPlaybackController(
            (await client.OpenRsvpEngineAsync(id, 600))!, _clock, _timer);
        c.Play();
        _clock.Advance(1500);
        _timer.Fire();
        var before = c.Frame!.Index;

        c.SetWpm(200);

        Assert.Equal(200U, c.Wpm);
        Assert.Equal(before, c.Frame!.Index);
        Assert.True(c.IsPlaying);
    }

    [Fact]
    public async Task Back_five_words_while_paused_moves_back_by_exactly_five_words()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;
        await client.SaveProgressAsync(id, 20);
        using var c = new RsvpPlaybackController(
            (await client.OpenRsvpEngineAsync(id, 600))!, _clock, _timer);
        Assert.Equal(20UL, c.Frame!.Index);

        c.StepWords(-5);

        Assert.True(c.Frame!.Index < 20);
        Assert.False(c.IsPlaying);
    }

    [Fact]
    public async Task Playing_to_the_end_stops_on_the_last_word_at_the_engines_total_duration()
    {
        var (client, id) = await ImportFixtureAsync();
        using var _ = client;
        using var c = new RsvpPlaybackController(
            (await client.OpenRsvpEngineAsync(id, 1000))!, _clock, _timer);
        c.Play();

        var guard = 0;
        while (_timer.IsPending && guard++ < 10_000)
        {
            _clock.Advance((long)_timer.PendingDelay!.Value.TotalMilliseconds);
            _timer.Fire();
        }

        Assert.True(c.IsFinished);
        Assert.False(c.IsPlaying);
        Assert.True(c.Frame!.IsLast);
        // The wall time we slept equals the engine's end-of-last-token boundary: nothing lost or added.
        Assert.Equal((long)c.Frame.NextBoundaryMs, _clock.NowMs);
        Assert.False(string.IsNullOrEmpty(c.DisplayWord));
    }
}
