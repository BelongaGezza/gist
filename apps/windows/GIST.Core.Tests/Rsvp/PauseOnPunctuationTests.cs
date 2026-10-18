using Gist.Core.Client;
using Gist.Core.Rsvp;
using Gist.Core.Tests.TestSupport;

namespace Gist.Core.Tests.Rsvp;

/// <summary>"Pause longer at punctuation" (Rust <c>Config::pause_on_punctuation</c>): view model, store, real engine.</summary>
public sealed class PauseOnPunctuationTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly FakeKeyProvider _keys = new();
    private readonly FakeRsvpClock _clock = new();
    private readonly FakeRsvpTimer _timer = new();

    public void Dispose() => _workspace.Dispose();

    private RsvpReaderViewModel Create(ScriptedRsvpEngine engine, bool initial = true, Action<bool>? persist = null) =>
        new("item", "T", _ => Task.FromResult<IRsvpEngine?>(engine), _ => Task.CompletedTask, _clock, _timer,
            initialPauseOnPunctuation: initial, persistPauseOnPunctuation: persist);

    [Fact]
    public async Task A_remembered_off_preference_is_applied_once_before_any_playback()
    {
        var engine = new ScriptedRsvpEngine(new[] { "a", "b", "c" });
        using var vm = Create(engine, initial: false);

        await vm.LoadAsync();

        Assert.False(vm.PauseOnPunctuation);
        Assert.False(engine.PauseOnPunctuation);
        Assert.Equal((false, 0UL), Assert.Single(engine.PunctuationCalls));
    }

    [Fact]
    public async Task The_default_on_preference_makes_no_engine_call()
    {
        var engine = new ScriptedRsvpEngine(new[] { "a", "b", "c" });
        using var vm = Create(engine);

        await vm.LoadAsync();

        Assert.True(vm.PauseOnPunctuation);
        Assert.Empty(engine.PunctuationCalls);
    }

    [Fact]
    public async Task Toggling_while_playing_pauses_first_resumes_and_persists_the_choice()
    {
        var engine = new ScriptedRsvpEngine(new[] { "a", "b", "c", "d" });
        var saved = new List<bool>();
        using var vm = Create(engine, persist: saved.Add);
        await vm.LoadAsync();
        vm.TogglePlayPause();
        var pausesBefore = engine.PauseCalls.Count;

        vm.SetPauseOnPunctuation(false);

        Assert.False(vm.PauseOnPunctuation);
        Assert.True(vm.IsPlaying);
        Assert.Equal(pausesBefore + 1, engine.PauseCalls.Count);
        Assert.Equal(new[] { false }, saved);
    }

    [Fact]
    public async Task Setting_the_current_value_is_a_no_op()
    {
        var engine = new ScriptedRsvpEngine(new[] { "a", "b" });
        var saved = new List<bool>();
        using var vm = Create(engine, persist: saved.Add);
        await vm.LoadAsync();

        vm.SetPauseOnPunctuation(true);

        Assert.Empty(engine.PunctuationCalls);
        Assert.Empty(saved);
    }

    [Fact]
    public async Task A_failing_persist_callback_never_interrupts_reading()
    {
        var engine = new ScriptedRsvpEngine(new[] { "a", "b" });
        using var vm = Create(engine, persist: _ => throw new IOException("disk full"));
        await vm.LoadAsync();

        vm.SetPauseOnPunctuation(false);

        Assert.Equal(RsvpLoadState.Ready, vm.LoadState);
        Assert.False(vm.PauseOnPunctuation);
    }

    [Fact]
    public void Store_round_trips_and_ignores_garbage()
    {
        var path = Path.Combine(_workspace.Root, "rsvp-settings.txt");
        var store = new FileRsvpSettingsStore(path);
        Assert.Null(store.LoadPauseOnPunctuation());

        store.SavePauseOnPunctuation(false);
        Assert.False(new FileRsvpSettingsStore(path).LoadPauseOnPunctuation());
        store.SavePauseOnPunctuation(true);
        Assert.True(new FileRsvpSettingsStore(path).LoadPauseOnPunctuation());

        File.WriteAllText(path + ".punctuation", "banana");
        Assert.Null(new FileRsvpSettingsStore(path).LoadPauseOnPunctuation());
        Assert.Null(new FileRsvpSettingsStore(path).LoadWpm()); // independent of the WPM file
    }

    [Fact]
    public async Task The_real_engine_pauses_longer_at_sentence_ends_only_when_enabled()
    {
        using var client = new CoreClient(_workspace.DbPath, _workspace.StorageDir, _keys);
        Assert.True(await client.InitializeAsync());
        var path = Path.Combine(_workspace.Root, "sentences.txt");
        await File.WriteAllTextAsync(path, "Alpha beta gamma. Delta epsilon zeta. Eta theta iota.");
        var id = await client.ImportFileAsync(path);
        Assert.NotNull(id);

        using var engine = await client.OpenRsvpEngineAsync(id!, 300);
        Assert.NotNull(engine);
        Assert.True(engine!.PauseOnPunctuation); // the pacing default

        ulong DurationOfSentenceEnd()
        {
            engine.Seek(2); // "gamma."
            return engine.FrameAtElapsed(0)!.DurationMs;
        }

        var withPause = DurationOfSentenceEnd();
        engine.SetPauseOnPunctuation(false, 0);
        Assert.False(engine.PauseOnPunctuation);
        var without = DurationOfSentenceEnd();

        Assert.True(withPause > without, $"expected a longer pause with punctuation on ({withPause} vs {without} ms)");
        Assert.Equal(60_000UL / 300, without);
    }
}
