using System.Text.RegularExpressions;
using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using FlaUI.Core.WindowsAPI;

namespace Gist.App.UITests;

/// <summary>
/// The flow reader (W5, docs/windows-ui-spec.md §7.2) driven through UI Automation against the real GIST.exe.
/// The document model, find, typography and position logic are proven in GIST.Core.Tests; these tests prove the
/// screen is wired to them: entry from Library and Collection, Contents, find, typography, progress and the
/// restored position. The seeded "Multi Chapter" epub has 5 chapters of h1/h2/h3 + paragraphs + lists
/// (15 headings, "subsection" occurs exactly 5 times); the seeded .txt files have no headings.
/// </summary>
[Trait("Category", "UI")]
public class FlowReaderTests
{
    private const string Multi = "Multi Chapter";
    private const string Apple = "apple-orchard";
    private static readonly Regex Percent = new(@"^(\d+)%$");
    private static readonly Regex Status = new(@"^(\d+) of (\d+)$");

    private sealed record Ctx(GistAppSession S, LibraryDriver D, SeededLibrary Lib) : IDisposable
    {
        public void Dispose() => S.Dispose();
    }

    private static async Task<Ctx> LaunchAsync(Func<string, SeededLibrary, Task>? afterSeed = null)
    {
        SeededLibrary? lib = null;
        var s = await GistAppSession.StartAsync(async root =>
        {
            lib = await LibrarySeed.SeedAsync(root);
            if (afterSeed is not null) await afterSeed(root, lib);
        });
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

    // ── Helpers ────────────────────────────────────────────────────────────

    /// <summary>Selects the row and opens its context menu from the keyboard, then picks "Open in Flow View".</summary>
    internal static void OpenFlowFromRow(LibraryDriver d, string title)
    {
        // A synthetic Menu key can land before the row has focus (or be swallowed while another popup closes):
        // like LibraryDriver.OpenDialog, re-issue it until the menu is actually up. Re-opening is harmless.
        AutomationElement? item = null;
        for (var attempt = 0; attempt < 4 && item is null; attempt++)
        {
            d.ClickRow(title);
            d.Press(VirtualKeyShort.APPS);
            try
            {
                item = LibraryDriver.Poll(
                    () => d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem).And(cf.ByName("Open in Flow View"))).FirstOrDefault(),
                    "context menu item 'Open in Flow View'", TimeSpan.FromSeconds(4));
            }
            catch (TimeoutException) when (attempt < 3)
            {
                d.Press(VirtualKeyShort.ESCAPE);
            }
        }
        LibraryDriver.Activate(item!);
        WaitFlowReady(d);
    }

    internal static void WaitFlowReady(LibraryDriver d)
    {
        d.Need("FlowPage_Root");
        LibraryDriver.PollUntil(() => d.ById("FlowPage_ProgressText") is { } p && Percent.IsMatch(p.Name), "flow reader ready (progress shown)");
    }

    internal static int ProgressPercent(LibraryDriver d)
    {
        var m = Percent.Match(d.Need("FlowPage_ProgressText").Name);
        Assert.True(m.Success, "progress text '" + d.Need("FlowPage_ProgressText").Name + "'");
        return int.Parse(m.Groups[1].Value);
    }

    /// <summary>An empty TextBlock exposes no UIA Name at all, which is how "no status" reads.</summary>
    internal static string FindStatus(LibraryDriver d) => Safe(() => d.Need("FlowPage_FindStatus").Name) ?? "";

    internal static AutomationElement[] RealizedBlocks(LibraryDriver d) =>
        d.Window.FindAllDescendants()
            .Where(e => (Safe(() => e.AutomationId) ?? "").StartsWith("FlowBlock_", StringComparison.Ordinal))
            .ToArray();

    /// <summary>Realised blocks whose rectangle intersects the document viewport, top to bottom.</summary>
    internal static AutomationElement[] VisibleBlocks(LibraryDriver d)
    {
        var view = d.Need("FlowPage_List").BoundingRectangle;
        return RealizedBlocks(d)
            .Where(b =>
            {
                var r = b.BoundingRectangle;
                return r.Height > 0 && r.Bottom > view.Top + 2 && r.Top < view.Bottom - 2;
            })
            .OrderBy(b => b.BoundingRectangle.Top)
            .ToArray();
    }

    internal static bool BlockVisible(LibraryDriver d, string name) => VisibleBlocks(d).Any(b => b.Name == name);

    /// <summary>Scrolls a flyout row into view (the list virtualises and caps its height), then clicks it.</summary>
    internal static void ActivateRow(LibraryDriver d, AutomationElement row)
    {
        // Focus scrolls the row into view; Enter is the keyboard equivalent of clicking an item-click row.
        row.Focus();
        d.Press(VirtualKeyShort.RETURN);
    }

    private static T? Safe<T>(Func<T> f) { try { return f(); } catch (Exception) { return default; } }

    private static void ClickToggleOrInvoke(AutomationElement e)
    {
        if (e.Patterns.Toggle.IsSupported) e.Patterns.Toggle.Pattern.Toggle();
        else LibraryDriver.Activate(e);
    }

    internal static void OpenFlyout(LibraryDriver d, string buttonId, string waitForId)
    {
        for (var attempt = 0; ; attempt++)
        {
            d.InvokeCommand(buttonId);
            try
            {
                LibraryDriver.Poll(() => d.ById(waitForId), "element " + waitForId, TimeSpan.FromSeconds(4));
                return;
            }
            catch (TimeoutException) when (attempt < 3)
            {
                // The invoke did not open the flyout (a popup was still closing); try again.
            }
        }
    }

    internal static void CloseFlyout(LibraryDriver d)
    {
        d.Press(VirtualKeyShort.ESCAPE);
        LibraryDriver.PollUntil(() => d.ById("FlowPage_TocList") is null && d.ById("FlowPage_FontLarger") is null, "flyout closed");
    }

    internal static void ShowFind(LibraryDriver d)
    {
        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_F);
        d.Need("FlowPage_FindBox");
    }

    internal static void SetFind(LibraryDriver d, string text)
    {
        var box = d.Need("FlowPage_FindBox");
        box.Focus();
        box.Patterns.Value.Pattern.SetValue(text);
    }

    // ── Entry and Back ─────────────────────────────────────────────────────

    [UiFact]
    public async Task Open_in_Flow_View_from_the_library_shows_the_document_and_Back_returns()
    {
        using var c = await LaunchAsync();
        var d = c.D;

        OpenFlowFromRow(d, Multi);

        Assert.Equal(Multi, d.Need("FlowPage_Title").Name);
        Assert.Equal(Multi, d.Need("FlowPage_Root").Name); // Narrator name for the page is the item title
        Assert.Equal(0, ProgressPercent(d));
        LibraryDriver.PollUntil(() => BlockVisible(d, "Chapter 1: The Beginning"), "first heading block rendered");
        Assert.Null(d.ById("RsvpPage_Root"));

        d.InvokeCommand("FlowPage_BackButton");
        d.WaitForLibrary();

        OpenFlowFromRow(d, Multi);
        d.Chord(VirtualKeyShort.ALT, VirtualKeyShort.LEFT);
        d.WaitForLibrary();
    }

    [UiFact]
    public async Task Open_in_Flow_View_from_a_collection_works_and_a_document_without_headings_disables_Contents()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        var zebra = c.Lib.ByFile("zebra-notes.txt").Title;

        LibraryDriver.Activate(LibraryDriver.Poll(() => d.ByName(LibrarySeed.CollectionName), "sidebar collection entry"));
        d.Need("CollectionPage_Root");
        LibraryDriver.PollUntil(() => d.Titles().SequenceEqual(new[] { zebra }), "collection shows its item");
        OpenFlowFromRow(d, zebra);

        Assert.Equal(zebra, d.Need("FlowPage_Title").Name);
        Assert.False(d.Need("FlowPage_ContentsButton").Properties.IsEnabled.Value, "no headings: Contents is disabled");
        Assert.True(d.Need("FlowPage_TypographyButton").Properties.IsEnabled.Value);
        LibraryDriver.PollUntil(() => VisibleBlocks(d).Any(b => b.Name.Contains("appears once", StringComparison.Ordinal) || b.Name.Contains("quokka", StringComparison.Ordinal)),
            "text paragraph rendered");

        d.InvokeCommand("FlowPage_BackButton");
        d.Need("CollectionPage_Root");
    }

    // ── Virtualisation ─────────────────────────────────────────────────────

    [UiFact]
    public async Task Only_blocks_near_the_viewport_are_realised()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenFlowFromRow(d, Multi);
        LibraryDriver.PollUntil(() => BlockVisible(d, "Chapter 1: The Beginning"), "first heading rendered");

        // The document has 5 chapters of 9 blocks each (45). Shrink the window's realised set check to: the last
        // chapter's heading is not realised while we are at the top (it would be if every block were built).
        Assert.DoesNotContain(RealizedBlocks(d), b => b.Name == "Chapter 5: The Resolution");
    }

    // ── Contents ───────────────────────────────────────────────────────────

    [UiFact]
    public async Task Contents_lists_every_heading_indented_by_level_and_clicking_scrolls_to_it()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenFlowFromRow(d, Multi);
        Assert.True(d.Need("FlowPage_ContentsButton").Properties.IsEnabled.Value);

        OpenFlyout(d, "FlowPage_ContentsButton", "FlowPage_TocList");
        var rows = LibraryDriver.Poll(() =>
        {
            var found = d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.ListItem))
                .Where(e => (Safe(() => e.AutomationId) ?? "").StartsWith("FlowToc_", StringComparison.Ordinal))
                .ToArray();
            return found.Length == 15 ? found : null;
        }, "15 contents rows");

        Assert.Equal("Chapter 1: The Beginning", rows[0].Name);
        Assert.Equal("Section 1.1 — Introduction", rows[1].Name);
        Assert.Equal("Subsection 1.1.1 — Details", rows[2].Name);

        // Indent is 16 epx x (level - 1): h2 is one step in, h3 two steps in (compare text left edges).
        double TextLeft(AutomationElement row) => row.FindFirstDescendant(cf => cf.ByControlType(ControlType.Text))!.BoundingRectangle.Left;
        var l1 = TextLeft(rows[0]);
        var l2 = TextLeft(rows[1]);
        var l3 = TextLeft(rows[2]);
        Assert.True(l2 > l1, "h2 is indented past h1");
        Assert.InRange((l3 - l1) / (l2 - l1), 1.9, 2.1);

        var target = rows.Single(r => r.Name == "Chapter 5: The Resolution");
        ActivateRow(d, target);

        LibraryDriver.PollUntil(() => BlockVisible(d, "Chapter 5: The Resolution"), "jumped to the Chapter 5 heading", TimeSpan.FromSeconds(20));
        LibraryDriver.PollUntil(() => ProgressPercent(d) > 50, "progress advanced after the jump");
    }

    // ── Find ───────────────────────────────────────────────────────────────

    [UiFact]
    public async Task Find_steps_with_buttons_F3_Shift_F3_and_Ctrl_G_and_handles_no_matches()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenFlowFromRow(d, Multi);

        ShowFind(d);
        SetFind(d, "SUBSECTION"); // case-insensitive: the headings say "Subsection"
        LibraryDriver.PollUntil(() => FindStatus(d) == "1 of 5", "five case-insensitive matches");
        LibraryDriver.PollUntil(() => BlockVisible(d, "Subsection 1.1.1 — Details"), "first match scrolled into view");

        d.Press(VirtualKeyShort.F3);
        LibraryDriver.PollUntil(() => FindStatus(d) == "2 of 5", "F3 moves to the next match");
        LibraryDriver.PollUntil(() => BlockVisible(d, "Subsection 2.1.1 — Details"), "second match scrolled into view");

        d.Chord(VirtualKeyShort.SHIFT, VirtualKeyShort.F3);
        LibraryDriver.PollUntil(() => FindStatus(d) == "1 of 5", "Shift+F3 moves to the previous match");

        d.Chord(VirtualKeyShort.CONTROL, VirtualKeyShort.KEY_G);
        LibraryDriver.PollUntil(() => FindStatus(d) == "2 of 5", "Ctrl+G moves to the next match");

        LibraryDriver.Activate(d.Need("FlowPage_FindNext"));
        LibraryDriver.PollUntil(() => FindStatus(d) == "3 of 5", "Next button");
        LibraryDriver.Activate(d.Need("FlowPage_FindPrevious"));
        LibraryDriver.PollUntil(() => FindStatus(d) == "2 of 5", "Previous button");

        // Wraps around at the end.
        for (var i = 0; i < 4; i++) LibraryDriver.Activate(d.Need("FlowPage_FindNext"));
        LibraryDriver.PollUntil(() => FindStatus(d) == "1 of 5", "Next wraps from the last match to the first");

        SetFind(d, "zzzz-not-in-the-text");
        LibraryDriver.PollUntil(() => FindStatus(d) == "No matches", "no-match status");
        Assert.True(d.Need("FlowPage_ContentsButton").Properties.IsEnabled.Value, "still responsive");

        SetFind(d, "");
        LibraryDriver.PollUntil(() => FindStatus(d) == "", "empty query clears the status");

        d.InvokeCommand("FlowPage_FindClose");
        LibraryDriver.PollUntil(() => d.ById("FlowPage_FindBox") is null || d.ById("FlowPage_FindBox")!.Properties.IsOffscreen.ValueOrDefault, "find bar closed");
    }

    // ── Typography ─────────────────────────────────────────────────────────

    [UiFact]
    public async Task Typography_menu_applies_clamps_persists_and_offers_no_Rounded()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenFlowFromRow(d, Multi);
        LibraryDriver.PollUntil(() => RealizedBlocks(d).Any(b => b.Name.StartsWith("This is chapter 1", StringComparison.Ordinal)), "first paragraph rendered");

        double Height(string idPart) => RealizedBlocks(d).First(b => b.AutomationId == "FlowBlock_" + idPart).BoundingRectangle.Height;
        var before = Height("2"); // first paragraph

        OpenFlyout(d, "FlowPage_TypographyButton", "FlowPage_FontLarger");
        Assert.Equal("17 pt", d.Need("FlowPage_FontSizeText").Name);

        // Rounded is not offered on Windows (spec 7.2).
        Assert.Null(d.ById("FlowPage_Font_Rounded"));
        Assert.Null(d.ByName("Rounded"));

        LibraryDriver.Activate(d.Need("FlowPage_FontLarger"));
        LibraryDriver.PollUntil(() => d.Need("FlowPage_FontSizeText").Name == "18 pt", "size 18");
        LibraryDriver.PollUntil(() => Height("2") > before, "paragraph grew with the larger size");

        // Line spacing: Relaxed is taller than Compact for the same paragraph.
        LibraryDriver.Activate(d.Need("FlowPage_Spacing_Compact"));
        LibraryDriver.PollUntil(() => d.Need("FlowPage_Spacing_Compact").Patterns.SelectionItem.Pattern.IsSelected.Value, "Compact selected");
        var compact = Height("2");
        LibraryDriver.Activate(d.Need("FlowPage_Spacing_Relaxed"));
        LibraryDriver.PollUntil(() => Height("2") > compact, "Relaxed is taller than Compact");

        LibraryDriver.Activate(d.Need("FlowPage_Font_Serif"));
        LibraryDriver.PollUntil(() => d.Need("FlowPage_Font_Serif").Patterns.SelectionItem.Pattern.IsSelected.Value, "Serif selected");

        // Clamp at both ends: 13..28.
        for (var i = 0; i < 6; i++)
        {
            if (!d.Need("FlowPage_FontSmaller").Properties.IsEnabled.Value) break;
            LibraryDriver.Activate(d.Need("FlowPage_FontSmaller"));
            Thread.Sleep(150);
        }
        LibraryDriver.PollUntil(() => d.Need("FlowPage_FontSizeText").Name == "13 pt", "smallest size is 13 pt");
        LibraryDriver.PollUntil(() => !d.Need("FlowPage_FontSmaller").Properties.IsEnabled.Value, "Smaller disabled at 13");

        for (var i = 0; i < 20; i++)
        {
            if (!d.Need("FlowPage_FontLarger").Properties.IsEnabled.Value) break;
            LibraryDriver.Activate(d.Need("FlowPage_FontLarger"));
            Thread.Sleep(150);
        }
        LibraryDriver.PollUntil(() => d.Need("FlowPage_FontSizeText").Name == "28 pt", "largest size is 28 pt");
        LibraryDriver.PollUntil(() => !d.Need("FlowPage_FontLarger").Properties.IsEnabled.Value, "Larger disabled at 28");

        // Set 20 pt for the persistence check, then verify the file independently of the UI.
        for (var i = 0; i < 8; i++)
        {
            LibraryDriver.Activate(d.Need("FlowPage_FontSmaller"));
            Thread.Sleep(150);
        }
        LibraryDriver.PollUntil(() => d.Need("FlowPage_FontSizeText").Name == "20 pt", "20 pt");
        var file = Path.Combine(c.S.Root, "flow-typography.txt");
        LibraryDriver.PollUntil(() => File.Exists(file) && File.ReadAllText(file) == "size=20;font=Serif;spacing=Relaxed", "typography persisted to its file");

        // And it is global: leave, reopen, the choice is still applied.
        CloseFlyout(d);
        d.InvokeCommand("FlowPage_BackButton");
        d.WaitForLibrary();
        OpenFlowFromRow(d, Multi);
        OpenFlyout(d, "FlowPage_TypographyButton", "FlowPage_FontLarger");
        Assert.Equal("20 pt", d.Need("FlowPage_FontSizeText").Name);
        Assert.True(d.Need("FlowPage_Font_Serif").Patterns.SelectionItem.Pattern.IsSelected.Value);
    }

    // ── Keyboard ───────────────────────────────────────────────────────────

    [UiFact]
    public async Task End_Home_and_PageDown_move_the_reader_and_the_progress_bar()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        OpenFlowFromRow(d, Multi);
        Assert.Equal(0, ProgressPercent(d));

        d.Press(VirtualKeyShort.NEXT); // Page Down
        LibraryDriver.PollUntil(() => ProgressPercent(d) > 0, "Page Down advanced");

        d.Press(VirtualKeyShort.END);
        LibraryDriver.PollUntil(() => ProgressPercent(d) == 100, "End reaches 100%");
        LibraryDriver.PollUntil(() => BlockVisible(d, "End of chapter 5. The next chapter continues the narrative."), "last paragraph visible");

        d.Press(VirtualKeyShort.HOME);
        LibraryDriver.PollUntil(() => ProgressPercent(d) == 0, "Home returns to 0%");
        LibraryDriver.PollUntil(() => BlockVisible(d, "Chapter 1: The Beginning"), "first heading visible");
    }

    // ── Position ───────────────────────────────────────────────────────────

    private static async Task JumpToHeadingAsync(LibraryDriver d, string heading)
    {
        OpenFlyout(d, "FlowPage_ContentsButton", "FlowPage_TocList");
        var row = LibraryDriver.Poll(
            () => d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.ListItem).And(cf.ByName(heading))).FirstOrDefault(),
            "contents row " + heading);
        row.Click();
        LibraryDriver.PollUntil(() => BlockVisible(d, heading), "jumped to " + heading);
        await Task.CompletedTask;
    }

    [UiFact]
    public async Task Progress_tracks_the_scroll_position_and_leaving_and_reopening_restores_it_without_a_jump()
    {
        using var c = await LaunchAsync();
        var d = c.D;
        var id = c.Lib.Items.Single(i => i.Title == Multi).Id;
        OpenFlowFromRow(d, Multi);
        Assert.Equal(0, ProgressPercent(d));

        await JumpToHeadingAsync(d, "Chapter 3: The Discovery");
        LibraryDriver.PollUntil(() => ProgressPercent(d) is > 20 and < 80, "progress is mid-document");
        // The bar and the text agree.
        Assert.InRange(d.Need("FlowPage_ProgressBar").Patterns.RangeValue.Pattern.Value.Value, 0.2, 0.8);
        var settled = ProgressPercent(d);
        LibraryDriver.StaysTrue(() => ProgressPercent(d) == settled, "progress is stable at rest", TimeSpan.FromSeconds(1));

        var stored = Path.Combine(c.S.Root, "flow-positions", "FlowScrollPosition." + id);
        LibraryDriver.PollUntil(() => File.Exists(stored), "position written to its own file");

        d.InvokeCommand("FlowPage_BackButton");
        d.WaitForLibrary();
        OpenFlowFromRow(d, Multi);

        // Restored before first render: the page is never shown at 0% and then jumped; so the very first
        // progress reading we can observe is already the restored one, and the heading is at the top.
        Assert.Equal(settled, ProgressPercent(d));
        LibraryDriver.PollUntil(() => BlockVisible(d, "Chapter 3: The Discovery"), "reopened at the saved heading");
        LibraryDriver.StaysTrue(() => ProgressPercent(d) == settled, "no drift after restore", TimeSpan.FromSeconds(1));
    }

    [UiFact]
    public async Task The_position_survives_an_application_restart()
    {
        string? root = null;
        try
        {
            int settled;
            using (var c = await LaunchAsync())
            {
                root = c.S.Root;
                c.S.KeepRoot = true; // the restart below needs the same data root
                var d = c.D;
                OpenFlowFromRow(d, Multi);
                await JumpToHeadingAsync(d, "Chapter 4: The Challenge");
                LibraryDriver.PollUntil(() => ProgressPercent(d) > 40, "progress advanced");
                settled = ProgressPercent(d);
                Thread.Sleep(900); // the position is saved shortly after scrolling settles
                d.InvokeCommand("FlowPage_BackButton");
                d.WaitForLibrary();
                Assert.Equal(0, c.S.CloseCleanly(TimeSpan.FromSeconds(10)));
            }

            using var again = await GistAppSession.StartAsync(existingRoot: root);
            var d2 = new LibraryDriver(again);
            d2.WaitForLibrary();
            d2.WaitForTitles(t => t.Length == 5, "five rows after restart");
            OpenFlowFromRow(d2, Multi);
            Assert.Equal(settled, ProgressPercent(d2));
            LibraryDriver.PollUntil(() => BlockVisible(d2, "Chapter 4: The Challenge"), "restored after restart");
        }
        finally
        {
            if (root is not null) GistAppSession.DeleteRoot(root);
        }
    }

    [UiFact]
    public async Task A_corrupt_stored_position_is_ignored_and_an_out_of_range_one_is_clamped()
    {
        // Corrupt: opens at the top.
        using (var c = await LaunchAsync((root, lib) => WritePositionAsync(root, lib, "not a number")))
        {
            OpenFlowFromRow(c.D, Multi);
            Assert.Equal(0, ProgressPercent(c.D));
            LibraryDriver.PollUntil(() => BlockVisible(c.D, "Chapter 1: The Beginning"), "opened at the top");
        }

        // Out of range (7.5): clamped to 1, so the reader opens at the end.
        using (var c = await LaunchAsync((root, lib) => WritePositionAsync(root, lib, "7.5")))
        {
            OpenFlowFromRow(c.D, Multi);
            LibraryDriver.PollUntil(() => ProgressPercent(c.D) == 100, "clamped to the end");
        }
    }

    private static async Task WritePositionAsync(string root, SeededLibrary lib, string content)
    {
        var id = lib.Items.Single(i => i.Title == Multi).Id;
        var dir = Path.Combine(root, "flow-positions");
        Directory.CreateDirectory(dir);
        await File.WriteAllTextAsync(Path.Combine(dir, "FlowScrollPosition." + id), content);
    }
}
