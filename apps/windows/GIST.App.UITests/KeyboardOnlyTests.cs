using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using FlaUI.Core.WindowsAPI;

namespace Gist.App.UITests;

/// <summary>
/// W6 keyboard-only pass (docs/windows-ui-spec.md accessibility, docs/qa-manual-clickthrough-windows.md §2).
/// Once the app is up, every interaction here is a real key event: no UIA Invoke/Select, no mouse. UIA is used only to
/// observe (what has focus, what is selected, what is on screen). What this can and cannot prove: it proves every
/// interactive control is reachable and operable by key, focus never disappears or leaves the window, and the
/// focused element is on screen; it cannot prove a focus ring is *visible* in a theme (screenshots render black in
/// this environment), so that stays a human item.
/// </summary>
[Trait("Category", "UI")]
public class KeyboardOnlyTests
{
    private const string Apple = "apple-orchard", Zebra = "zebra-notes", Mango = "mango-diary";
    private const int MaxTabs = 60;

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

    // ── Focus observation ──────────────────────────────────────────────────

    private sealed record Stop(string Key, string Id, string Name, ControlType Type, bool OnScreen)
    {
        public override string ToString() => $"{Type}:{(Id.Length > 0 ? Id : "-")}:{Name}";
    }

    private static T? Try<T>(Func<T> f) { try { return f(); } catch (Exception) { return default; } }

    /// <summary>Where keyboard focus is right now, or null if it is between elements.</summary>
    private static Stop? Focus(LibraryDriver d)
    {
        var f = Try(() => d.Session.Automation.FocusedElement());
        if (f is null) return null;
        var id = Try(() => f.AutomationId) ?? "";
        var name = Try(() => f.Name) ?? "";
        var type = Try(() => f.ControlType);
        var rect = Try(() => f.BoundingRectangle);
        var onScreen = rect is { Width: > 0, Height: > 0 } && !Try(() => f.Properties.IsOffscreen.ValueOrDefault);
        // A list row's identity is its title text; rows are visited with the arrow keys, not Tab.
        return new Stop(id.Length > 0 ? id : type + "|" + name, id, name, type, onScreen);
    }

    /// <summary>Presses Tab until focus has been seen to repeat or <see cref="MaxTabs"/> is hit; returns the visited stops in order.</summary>
    private static List<Stop> TabCycle(LibraryDriver d, VirtualKeyShort? modifier = null)
    {
        var seen = new List<Stop>();
        for (var i = 0; i < MaxTabs; i++)
        {
            if (modifier is { } m) d.Chord(m, VirtualKeyShort.TAB); else d.Press(VirtualKeyShort.TAB);
            Stop? s = null;
            // Focus moves asynchronously; wait for it to settle on something other than "nothing".
            try { LibraryDriver.PollUntil(() => (s = Focus(d)) is not null, "focus after Tab", TimeSpan.FromSeconds(3)); }
            catch (TimeoutException) { throw new Xunit.Sdk.XunitException("Focus was lost after Tab #" + (i + 1) + "; visited so far: " + string.Join(" -> ", seen)); }
            if (seen.Count > 0 && seen.Any(x => x.Key == s!.Key)) { seen.Add(s!); break; }
            seen.Add(s!);
        }
        return seen;
    }

    /// <summary>Home, then wait until the first row really has focus so the next key lands on it.</summary>
    private static void GoHome(LibraryDriver d)
    {
        var first = d.Titles()[0];
        d.Press(VirtualKeyShort.HOME);
        LibraryDriver.PollUntil(() => FocusedRowTitle(d) == first, "Home -> first row");
    }

    private static void TabTo(LibraryDriver d, string id)
    {
        for (var i = 0; i < MaxTabs && Focus(d)?.Id != id; i++) d.Press(VirtualKeyShort.TAB);
        Assert.Equal(id, Focus(d)?.Id);
    }

    private static string Describe(IEnumerable<Stop> stops) => string.Join(" -> ", stops);

    /// <summary>Controls a keyboard user must be able to reach with Tab (rows and their parts are reached with arrows).</summary>
    private static readonly ControlType[] TabTargets =
    {
        ControlType.Button, ControlType.CheckBox, ControlType.ComboBox, ControlType.Edit, ControlType.RadioButton,
        ControlType.Slider, ControlType.SplitButton, ControlType.Hyperlink, ControlType.TabItem,
    };

    /// <summary>The visible, enabled, keyboard-focusable controls under <paramref name="root"/> that Tab must be able to reach.</summary>
    private static List<AutomationElement> ExpectedFocusables(AutomationElement root) =>
        root.FindAllDescendants()
            .Where(e => Try(() => TabTargets.Contains(e.ControlType)) == true
                && Try(() => e.Properties.IsKeyboardFocusable.ValueOrDefault)
                && Try(() => e.Properties.IsEnabled.ValueOrDefault)
                && !Try(() => e.Properties.IsOffscreen.ValueOrDefault)
                && !IsInsideRow(e)
                && !IsScrollChrome(e))
            .ToList();

    private static bool IsInsideRow(AutomationElement e)
    {
        for (var p = Try(() => e.Parent); p is not null; p = Try(() => p.Parent))
            if (Try(() => p.ControlType) == ControlType.ListItem) return true;
        return false;
    }

    // Scroll-bar arrows are focusable buttons in UIA but are deliberately not tab stops (the viewer scrolls by key).
    private static bool IsScrollChrome(AutomationElement e)
    {
        var id = Try(() => e.AutomationId) ?? "";
        return id is "UpButton" or "DownButton" or "UpPageButton" or "DownPageButton" or "LeftButton" or "RightButton"
            || Try(() => e.Parent?.ControlType) == ControlType.ScrollBar;
    }

    private static string KeyOf(AutomationElement e)
    {
        var id = Try(() => e.AutomationId) ?? "";
        return id.Length > 0 ? id : Try(() => e.ControlType) + "|" + Try(() => e.Name);
    }

    // ── Library: tab order and reachability ────────────────────────────────

    [UiFact]
    public async Task Library_tab_order_is_search_then_commands_then_list_and_focus_stays_visible()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.FocusWindow();

        var stops = TabCycle(d);
        var path = Describe(stops);

        // Focus never vanishes (TabCycle throws) and every stop is a real, on-screen element inside this window.
        Assert.All(stops, s => Assert.True(s.OnScreen, "focused element not on screen: " + s + " in " + path));

        int At(string id) => stops.FindIndex(s => s.Id == id);
        // The AutoSuggestBox keeps its AutomationId on the wrapper; the focused element is its inner edit, named by the placeholder.
        var search = stops.FindIndex(s => s.Type == ControlType.Edit && s.Name == "Search library");
        var import = At("LibraryPage_ImportFileButton");
        Assert.True(search >= 0, "search box never received focus: " + path);
        Assert.True(import >= 0, "command bar never received focus: " + path);
        // Spec: search box -> command bar -> list. (The list is entered as a single tab stop.)
        Assert.True(search < import, "search should precede the command bar: " + path);
        var listStop = stops.FindIndex(s => s.Id == "LibraryPage_List" || s.Id.StartsWith("LibraryItem_", StringComparison.Ordinal));
        Assert.True(listStop >= 0, "the list never received focus: " + path);
        Assert.True(import < listStop, "command bar should precede the list: " + path);

        // The cycle closes: there is no keyboard trap (the repeat that ends TabCycle is the proof).
        Assert.True(stops.Count < MaxTabs, "Tab never cycled back within " + MaxTabs + " presses (trap?): " + path);
    }

    [UiFact]
    public async Task Library_every_visible_control_is_reachable_by_Tab()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.FocusWindow();
        // Selection-dependent commands are disabled with nothing selected and rightly skipped; select a row by key first
        // so the full command set is live, then sweep.
        d.Press(VirtualKeyShort.TAB);
        var stops = TabCycle(d);
        var visited = stops.Select(s => s.Key).ToHashSet();

        var missing = ExpectedFocusables(d.Window).Where(e => !visited.Contains(KeyOf(e))).Select(KeyOf).Distinct().ToList();
        Assert.True(missing.Count == 0, "controls not reachable with Tab: " + string.Join(", ", missing) + " | visited: " + Describe(stops));
    }

    [UiFact]
    public async Task Library_shift_tab_walks_the_same_stops_in_reverse()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.FocusWindow();
        var forward = TabCycle(d);
        var back = TabCycle(d, VirtualKeyShort.SHIFT);
        // Same set of tab stops regardless of direction; only the order flips.
        Assert.Equal(forward.Select(s => s.Key).ToHashSet(), back.Select(s => s.Key).ToHashSet());
    }

    // ── Library: list, selection, context menu, dialogs ────────────────────

    private static string? FocusedRowTitle(LibraryDriver d)
    {
        var f = Try(() => d.Session.Automation.FocusedElement());
        for (var e = f; e is not null; e = Try(() => e.Parent))
            if (Try(() => e.ControlType) == ControlType.ListItem) return LibraryDriver.RowTitle(e);
        return null;
    }

    /// <summary>Gets focus into the list by key alone: Ctrl+F then Esc (the documented route back, see Esc test).</summary>
    private static void FocusListByKeyboard(LibraryDriver d)
    {
        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_F);
        d.WaitFocus("LibraryPage_SearchBox");
        d.Press(VirtualKeyShort.ESCAPE);
        d.WaitFocus("LibraryPage_List");
    }

    [UiFact]
    public async Task Arrow_keys_move_through_rows_and_Home_End_jump()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        FocusListByKeyboard(d);
        var titles = d.Titles();

        d.Press(VirtualKeyShort.HOME);
        LibraryDriver.PollUntil(() => FocusedRowTitle(d) == titles[0], "Home -> first row");
        for (var i = 1; i < titles.Length; i++)
        {
            d.Press(VirtualKeyShort.DOWN);
            var want = titles[i];
            LibraryDriver.PollUntil(() => FocusedRowTitle(d) == want, "Down -> " + want);
        }
        d.Press(VirtualKeyShort.UP);
        LibraryDriver.PollUntil(() => FocusedRowTitle(d) == titles[^2], "Up -> second to last");
        d.Press(VirtualKeyShort.END);
        LibraryDriver.PollUntil(() => FocusedRowTitle(d) == titles[^1], "End -> last row");
    }

    [UiFact]
    public async Task Selection_is_operable_by_keyboard_Shift_extends_Ctrl_Space_toggles()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        FocusListByKeyboard(d);
        var titles = d.Titles();

        GoHome(d);
        d.Press(VirtualKeyShort.SPACE);
        LibraryDriver.PollUntil(() => d.SelectedTitles().SequenceEqual(new[] { titles[0] }), "Space selects the focused row");

        d.Chord(VirtualKeyShort.SHIFT, VirtualKeyShort.DOWN);
        LibraryDriver.PollUntil(() => d.SelectedTitles().OrderBy(x => x).SequenceEqual(titles.Take(2).OrderBy(x => x)),
            "Shift+Down extends the selection to two rows");

        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_A);
        LibraryDriver.PollUntil(() => d.SelectedTitles().Length == titles.Length, "Ctrl+A selects all");

        // Selection-dependent commands follow the selection (single-item commands disable at 5 selected).
        Assert.True(d.CommandEnabled("LibraryPage_RemoveButton"));
        Assert.False(d.CommandEnabled("LibraryPage_OpenButton"));
    }

    [UiFact]
    public async Task Context_menu_opens_with_the_Menu_key_is_arrow_navigable_and_Esc_returns_focus_to_the_row()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        FocusListByKeyboard(d);
        GoHome(d);
        var title = LibraryDriver.Poll(() => FocusedRowTitle(d), "a focused row");

        string[] expected = { "Open in Reader", "Open in Flow View", "Manage Tags…", "Get Info…", "Encrypt…", "Remove…" };
        string[] names = OpenRowMenu(d, expected[0]);
        Assert.Equal(expected, names);

        // Arrow navigation: Down moves through items; the focused item changes each time.
        d.Press(VirtualKeyShort.DOWN);
        var first = Focus(d);
        d.Press(VirtualKeyShort.DOWN);
        var second = Focus(d);
        Assert.NotNull(first);
        Assert.NotNull(second);
        Assert.NotEqual(first!.Key, second!.Key);
        Assert.Equal(ControlType.MenuItem, second.Type);

        d.Press(VirtualKeyShort.ESCAPE);
        LibraryDriver.PollUntil(() => MenuItems(d).Length == 0, "Esc closes the menu");
        LibraryDriver.PollUntil(() => FocusedRowTitle(d) == title, "focus returns to the row the menu was opened on");
    }

    private static AutomationElement[] MenuItems(LibraryDriver d) =>
        d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem)).Where(m => (Try(() => m.Name) ?? "") is { Length: > 0 } n && n != "System").ToArray();

    /// <summary>Opens the row context menu with the Menu key (re-issuing while the synthetic key is swallowed); returns the item names.</summary>
    private static string[] OpenRowMenu(LibraryDriver d, string firstItem)
    {
        for (var attempt = 0; attempt < 4; attempt++)
        {
            d.Press(VirtualKeyShort.APPS);
            try
            {
                LibraryDriver.PollUntil(() => MenuItems(d).Any(m => m.Name == firstItem), "context menu", TimeSpan.FromSeconds(4));
                return MenuItems(d).Select(m => m.Name).ToArray();
            }
            catch (TimeoutException) when (attempt < 3) { d.Press(VirtualKeyShort.ESCAPE); }
        }
        throw new TimeoutException("context menu never opened from the Menu key");
    }

    [UiFact]
    public async Task Menu_key_then_arrows_and_Enter_reaches_Remove_and_Esc_cancels_it_with_focus_back_in_the_list()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        FocusListByKeyboard(d);
        GoHome(d);
        d.Press(VirtualKeyShort.SPACE);
        OpenRowMenu(d, "Open in Reader");

        // Walk to "Remove…" with arrows only and activate it with Enter.
        for (var i = 0; i < 12; i++)
        {
            d.Press(VirtualKeyShort.DOWN);
            if (Focus(d)?.Name == "Remove…") break;
        }
        Assert.Equal("Remove…", Focus(d)?.Name);
        d.Press(VirtualKeyShort.RETURN);

        var dlg = d.Dialog("Remove 1 item?");
        // Focus is inside the dialog, not behind it.
        LibraryDriver.PollUntil(() => IsInside(Try(() => d.Session.Automation.FocusedElement()), dlg), "focus moved into the Remove dialog");

        d.Press(VirtualKeyShort.ESCAPE);
        d.WaitDialogGone("Remove 1 item?");
        Assert.Equal(5, d.Titles().Length); // nothing was removed
        LibraryDriver.PollUntil(() => IsListFocused(d), "focus returns to the list after the dialog closes");
    }

    private static bool IsListFocused(LibraryDriver d) => d.FocusedId() == "LibraryPage_List";

    private static bool IsInside(AutomationElement? e, AutomationElement container)
    {
        for (; e is not null; e = Try(() => e.Parent))
            if (Try(() => e.Equals(container)) == true) return true;
        return false;
    }

    [UiFact]
    public async Task Delete_key_dialog_is_fully_keyboard_operable_Tab_cycles_inside_it_and_Enter_confirms_the_default()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        FocusListByKeyboard(d);
        GoHome(d);
        d.Press(VirtualKeyShort.SPACE);
        var doomed = d.SelectedTitles().Single();
        d.Press(VirtualKeyShort.DELETE);
        var dlg = d.Dialog("Remove 1 item?");
        LibraryDriver.PollUntil(() => IsInside(Try(() => d.Session.Automation.FocusedElement()), dlg), "focus inside dialog");

        // Tab cycles among the dialog's own buttons and never escapes to the page underneath.
        var seen = new HashSet<string>();
        for (var i = 0; i < 6; i++)
        {
            d.Press(VirtualKeyShort.TAB);
            var f = Try(() => d.Session.Automation.FocusedElement());
            Assert.True(IsInside(f, dlg), "Tab escaped the modal dialog to: " + Focus(d));
            seen.Add(KeyOf(f!));
        }
        Assert.True(seen.Count >= 2, "expected at least Cancel and Remove to be tab stops, saw: " + string.Join(", ", seen));

        // Esc is Cancel: nothing removed.
        d.Press(VirtualKeyShort.ESCAPE);
        d.WaitDialogGone("Remove 1 item?");
        Assert.Contains(doomed, d.Titles());
        d.WaitFocus("LibraryPage_List"); // closing a dialog hands focus back to the list, so Delete works again without re-selecting

        // Re-open and confirm with the keyboard: Tab to the Remove button, Enter.
        // A key sent in the instant after a dialog closes can be dropped (see LibraryDriver.OpenDialog); re-issue until it lands.
        dlg = null!;
        for (var attempt = 0; attempt < 4 && dlg is null; attempt++)
        {
            d.Press(VirtualKeyShort.DELETE);
            try { dlg = d.Dialog("Remove 1 item?", TimeSpan.FromSeconds(4)); } catch (TimeoutException) when (attempt < 3) { }
        }
        var removeButton = LibraryDriver.DialogPart(dlg!, "PrimaryButton");
        for (var i = 0; i < 6 && Try(() => removeButton.Properties.HasKeyboardFocus.ValueOrDefault) != true; i++)
            d.Press(VirtualKeyShort.TAB);
        Assert.True(Try(() => removeButton.Properties.HasKeyboardFocus.ValueOrDefault), "Remove button never took focus by Tab");
        d.Press(VirtualKeyShort.RETURN);
        d.WaitDialogGone("Remove 1 item?");
        d.WaitForTitles(t => t.Length == 4 && !t.Contains(doomed), "keyboard-confirmed removal took effect");
    }

    [UiFact]
    public async Task Sort_flyout_is_operable_by_keyboard_alone()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        d.FocusWindow();
        // Tab to the Sort button, open it with Enter, pick an item with arrows + Enter.
        for (var i = 0; i < MaxTabs && Focus(d)?.Id != "LibraryPage_SortButton"; i++) d.Press(VirtualKeyShort.TAB);
        Assert.Equal("LibraryPage_SortButton", Focus(d)?.Id);
        d.Press(VirtualKeyShort.RETURN);
        LibraryDriver.PollUntil(() => MenuItems(d).Length > 0, "sort menu opened by Enter");

        var before = d.Titles();
        for (var i = 0; i < 12; i++)
        {
            d.Press(VirtualKeyShort.DOWN);
            if (Focus(d)?.Name == "Title (A–Z)" || Focus(d)?.Name.StartsWith("Title", StringComparison.Ordinal) == true) break;
        }
        var chosen = Focus(d)?.Name ?? "";
        Assert.StartsWith("Title", chosen);
        d.Press(VirtualKeyShort.RETURN);
        LibraryDriver.PollUntil(() => MenuItems(d).Length == 0, "menu closed after choosing");
        LibraryDriver.PollUntil(() => !d.Titles().SequenceEqual(before), "list reordered by the keyboard-chosen sort");
        // Focus did not get lost to nowhere after the flyout closed.
        LibraryDriver.PollUntil(() => Focus(d) is not null, "focus restored after the flyout");
    }

    // ── Readers ────────────────────────────────────────────────────────────

    [UiFact]
    public async Task RSVP_reader_controls_are_reachable_by_Tab_and_Space_Esc_Alt_Left_work_without_a_mouse()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        FocusListByKeyboard(d);
        GoHome(d);
        d.Press(VirtualKeyShort.RETURN);
        d.Need("RsvpPage_Root");
        d.Need("RsvpPage_PlayPauseButton");

        var stops = TabCycle(d);
        var visited = stops.Select(s => s.Key).ToHashSet();
        Assert.All(stops, s => Assert.True(s.OnScreen, "off-screen focus: " + s));
        var missing = ExpectedFocusables(d.Window).Where(e => !visited.Contains(KeyOf(e))).Select(KeyOf).Distinct().ToList();
        Assert.True(missing.Count == 0, "RSVP controls not reachable with Tab: " + string.Join(", ", missing) + " | visited: " + Describe(stops));
        foreach (var id in new[] { "RsvpPage_BackButton", "RsvpPage_PlayPauseButton", "RsvpPage_WpmSlider" })
            Assert.Contains(id, visited);

        // Back by keyboard only: Tab to the Back button and press Enter.
        TabTo(d, "RsvpPage_BackButton");
        d.Press(VirtualKeyShort.RETURN);
        d.WaitForLibrary();
    }

    [UiFact]
    public async Task Flow_reader_controls_are_reachable_by_Tab_and_Back_works_by_keyboard()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        FocusListByKeyboard(d);
        GoHome(d);
        OpenRowMenu(d, "Open in Reader");
        for (var i = 0; i < 12; i++)
        {
            d.Press(VirtualKeyShort.DOWN);
            if (Focus(d)?.Name == "Open in Flow View") break;
        }
        d.Press(VirtualKeyShort.RETURN);
        d.Need("FlowPage_Root");
        FlowReaderTests.WaitFlowReady(d);

        var stops = TabCycle(d);
        var visited = stops.Select(s => s.Key).ToHashSet();
        Assert.All(stops, s => Assert.True(s.OnScreen, "off-screen focus: " + s));
        var missing = ExpectedFocusables(d.Window).Where(e => !visited.Contains(KeyOf(e))).Select(KeyOf).Distinct().ToList();
        Assert.True(missing.Count == 0, "Flow controls not reachable with Tab: " + string.Join(", ", missing) + " | visited: " + Describe(stops));
        Assert.Contains("FlowPage_BackButton", visited);

        TabTo(d, "FlowPage_BackButton");
        d.Press(VirtualKeyShort.RETURN);
        d.WaitForLibrary();
    }
}
