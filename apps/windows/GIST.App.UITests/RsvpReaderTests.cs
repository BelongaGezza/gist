using System.Text.RegularExpressions;
using FlaUI.Core.AutomationElements;
using FlaUI.Core.WindowsAPI;

namespace Gist.App.UITests;

/// <summary>
/// The RSVP reader (W4, docs/windows-ui-spec.md §7.1) driven through UI Automation against the real
/// GIST.exe. Pacing itself is proven in GIST.Core.Tests against the real engine; these tests prove the
/// screen is wired to it: entry from the Library, state, keyboard, WPM controls and progress persistence.
/// </summary>
[Trait("Category", "UI")]
public class RsvpReaderTests
{
    private const string Apple = "apple-orchard";
    private static readonly Regex ProgressRegex = new(@"^(\d+) / (\d+)$");

    private sealed record Ctx(GistAppSession S, LibraryDriver D, SeededLibrary Lib) : IDisposable
    {
        public void Dispose() => S.Dispose();
    }

    private static async Task<Ctx> LaunchAsync()
    {
        SeededLibrary? lib = null;
        var s = await GistAppSession.StartAsync(async root => lib = await LibrarySeed.SeedAsync(root));
        try
        {
            var d = new LibraryDriver(s);
            d.WaitForLibrary();
            d.WaitForTitles(t => t.Length == 5, "five seeded rows");
            return new Ctx(s, d, lib!);
        }
        catch
        {
            s.Dispose();
            throw;
        }
    }

    private static void OpenReader(LibraryDriver d, string title)
    {
        d.ClickRow(title);
        d.Press(VirtualKeyShort.RETURN);
        d.Need("RsvpPage_Root");
        // Loading finished: the play button exists only in the ready state's visible tree.
        LibraryDriver.PollUntil(() => d.ById("RsvpPage_Loading") is null || d.ById("RsvpPage_Loading")!.Properties.IsOffscreen.ValueOrDefault,
            "loading state gone");
        d.Need("RsvpPage_PlayPauseButton");
    }

    private static string Word(LibraryDriver d) => d.Need("RsvpPage_Word").Name;

    private static string PlayName(LibraryDriver d) => d.Need("RsvpPage_PlayPauseButton").Name;

    private static (int N, int Total) Progress(LibraryDriver d)
    {
        var m = ProgressRegex.Match(d.Need("RsvpPage_Progress").Name);
        Assert.True(m.Success, "progress text '" + d.Need("RsvpPage_Progress").Name + "'");
        return (int.Parse(m.Groups[1].Value), int.Parse(m.Groups[2].Value));
    }

    [UiFact]
    public async Task Enter_on_a_row_opens_the_reader_paused_on_the_first_word()
    {
        using var c = await LaunchAsync();
        var d = c.D;

        OpenReader(d, Apple);

        Assert.Equal("The", Word(d));
        Assert.Equal("Play", PlayName(d));
        Assert.Equal(1, Progress(d).N);
        Assert.Equal("300 WPM", d.Need("RsvpPage_WpmLabel").Name);
    }

    [UiFact]
    public async Task Back_button_and_Alt_Left_return_to_the_library()
    {
        using var c = await LaunchAsync();
        var d = c.D;

        OpenReader(d, Apple);
        d.InvokeCommand("RsvpPage_BackButton");
        d.WaitForLibrary();

        OpenReader(d, Apple);
        d.Chord(VirtualKeyShort.ALT, VirtualKeyShort.LEFT);
        d.WaitForLibrary();
    }

    [UiFact]
    public async Task Play_advances_words_and_Pause_holds_position_without_blanking()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenReader(d, Apple);

        d.InvokeCommand("RsvpPage_PlayPauseButton");
        LibraryDriver.PollUntil(() => PlayName(d) == "Pause", "button announces Pause while playing");
        LibraryDriver.PollUntil(() => Progress(d).N >= 3, "playback advanced");

        d.InvokeCommand("RsvpPage_PlayPauseButton");
        LibraryDriver.PollUntil(() => PlayName(d) == "Play", "button announces Play when paused");
        var held = Progress(d);
        Assert.False(string.IsNullOrEmpty(Word(d)));
        LibraryDriver.StaysTrue(() => Progress(d) == held, "paused position holds", TimeSpan.FromSeconds(1.5));
    }

    [UiFact]
    public async Task Space_toggles_playback_even_when_a_slider_has_focus()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenReader(d, Apple);

        d.Need("RsvpPage_Scrubber").Focus();
        d.Press(VirtualKeyShort.SPACE);
        LibraryDriver.PollUntil(() => PlayName(d) == "Pause", "Space started playback");

        d.Need("RsvpPage_Scrubber").Focus();
        d.Press(VirtualKeyShort.SPACE);
        LibraryDriver.PollUntil(() => PlayName(d) == "Play", "Space paused playback");
    }

    [UiFact]
    public async Task Space_on_the_focused_play_button_toggles_exactly_once()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenReader(d, Apple);
        d.Need("RsvpPage_PlayPauseButton").Focus();

        d.Press(VirtualKeyShort.SPACE);
        LibraryDriver.PollUntil(() => PlayName(d) == "Pause", "Space on the button played");
        // A double toggle (button click + page handler) would flip straight back to Play.
        LibraryDriver.StaysTrue(() => PlayName(d) == "Pause", "still playing a moment later", TimeSpan.FromSeconds(1));
        Shot(c.S, "rsvp-playing");
    }

    /// <summary>Best-effort PNG for the human visual check; only when GIST_UI_SCREENSHOT_DIR is set.</summary>
    private static void Shot(GistAppSession s, string name)
    {
        var dir = Environment.GetEnvironmentVariable("GIST_UI_SCREENSHOT_DIR");
        if (!string.IsNullOrEmpty(dir)) s.TrySaveScreenshot(Path.Combine(dir, name + ".png"));
    }

    [UiFact]
    public async Task Arrow_keys_step_one_word_and_Ctrl_arrows_jump_five()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenReader(d, Apple);
        d.Need("RsvpPage_PlayPauseButton").Focus();

        d.Press(VirtualKeyShort.RIGHT);
        LibraryDriver.PollUntil(() => Progress(d).N == 2, "Right steps one word");

        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.RIGHT);
        LibraryDriver.PollUntil(() => Progress(d).N == 7, "Ctrl+Right jumps five words");

        d.Press(VirtualKeyShort.LEFT);
        LibraryDriver.PollUntil(() => Progress(d).N == 6, "Left steps back one word");

        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.LEFT);
        LibraryDriver.PollUntil(() => Progress(d).N == 1, "Ctrl+Left clamps at the start");
    }

    [UiFact]
    public async Task WPM_slider_and_number_box_stay_in_step_and_the_core_range_holds()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenReader(d, Apple);

        d.Need("RsvpPage_WpmSlider").Patterns.RangeValue.Pattern.SetValue(600);
        LibraryDriver.PollUntil(() => d.Need("RsvpPage_WpmLabel").Name == "600 WPM", "slider updates the label");
        LibraryDriver.PollUntil(
            () => Math.Abs(d.Need("RsvpPage_WpmBox").Patterns.RangeValue.Pattern.Value.Value - 600) < 0.5,
            "number box follows the slider");

        d.Need("RsvpPage_WpmBox").Patterns.RangeValue.Pattern.SetValue(250);
        LibraryDriver.PollUntil(() => d.Need("RsvpPage_WpmLabel").Name == "250 WPM", "number box updates the label");
        LibraryDriver.PollUntil(
            () => Math.Abs(d.Need("RsvpPage_WpmSlider").Patterns.RangeValue.Pattern.Value.Value - 250) < 0.5,
            "slider follows the number box");
    }

    [UiFact]
    public async Task Leaving_persists_progress_and_reopening_restores_it()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenReader(d, Apple);
        d.Need("RsvpPage_PlayPauseButton").Focus();
        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.RIGHT); // five words -> token index 5
        LibraryDriver.PollUntil(() => Progress(d).N == 6, "jumped to the sixth word");

        d.InvokeCommand("RsvpPage_BackButton");
        d.WaitForLibrary();
        OpenReader(d, Apple);

        LibraryDriver.PollUntil(() => Progress(d).N == 6, "reopened at the saved position");
        Assert.Equal("Play", PlayName(d)); // restored paused, not auto-playing

        var root = c.S.Root;
        var id = c.Lib.ByFile("apple-orchard.txt").Id;
        Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));

        // Independent of the UI: the core itself has the saved index.
        using var core = await LibrarySeed.OpenAsync(root);
        using var engine = await core.OpenRsvpEngineAsync(id, 300);
        Assert.Equal(5UL, engine!.Cursor);
    }

    [UiFact]
    public async Task Read_current_word_action_exists_and_is_named()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenReader(d, Apple);

        var button = d.Need("RsvpPage_ReadWordButton");

        Assert.Equal("Read current word", button.Name);
        Assert.True(button.Properties.IsEnabled.Value);
    }
}
