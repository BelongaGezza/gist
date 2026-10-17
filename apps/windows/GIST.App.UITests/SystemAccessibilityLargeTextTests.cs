using System.Drawing;
using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using FlaUI.Core.WindowsAPI;

namespace Gist.App.UITests;

/// <summary>
/// Large-text pass over the Flow reader and every Library dialog (companion to the Library/RSVP/Remove-dialog checks in
/// <c>SystemAccessibilityTests.cs</c>). Same opt-in and same restoring helper: these change the machine-wide Windows
/// Text size for their duration. Geometry is read through UI Automation; what UIA cannot show (a label trimmed with an
/// ellipsis) is covered by saving screenshots with <c>GIST_A11Y_SHOTS</c> for a human to look at.
/// </summary>
public partial class SystemAccessibilityTests
{
    private static readonly string[] DialogButtonIds = { "PrimaryButton", "SecondaryButton", "CloseButton" };

    /// <summary>Everything the dialog actually paints on screen, excluding the full-window overlay element itself.</summary>
    private static List<(string Id, string Name, ControlType Type, Rectangle Rect)> DialogParts(AutomationElement dlg)
    {
        var overlay = dlg.BoundingRectangle;
        return dlg.FindAllDescendants()
            .Select(e => (Id: Try(() => e.AutomationId) ?? "", Name: Try(() => e.Name) ?? "", Type: Try(() => e.ControlType),
                Rect: Try(() => e.BoundingRectangle), Off: Try(() => e.Properties.IsOffscreen.ValueOrDefault)))
            .Where(p => !p.Off && p.Rect.Width > 0 && p.Rect.Height > 0 && p.Rect.Width < overlay.Width)
            .Select(p => (p.Id, p.Name, p.Type, p.Rect))
            .ToList();
    }

    /// <summary>The dialog's content and buttons are on screen, its buttons are reachable and do not overlap, and nothing spills out of the window.</summary>
    private static void AssertDialogFits(Ctx c, AutomationElement dlg, string what)
    {
        var win = c.S.Window.BoundingRectangle;
        var parts = DialogParts(dlg);
        Assert.NotEmpty(parts);

        var outside = parts.Where(p => p.Rect.Left < win.Left - 2 || p.Rect.Top < win.Top - 2 || p.Rect.Right > win.Right + 2 || p.Rect.Bottom > win.Bottom + 2)
            .Select(p => $"{p.Type}:{p.Id}:{p.Name} {R(p.Rect)}").ToList();
        Assert.True(outside.Count == 0, $"{what}: parts outside the window {R(win)}: {string.Join("; ", outside)}");

        var buttons = parts.Where(p => DialogButtonIds.Contains(p.Id)).ToDictionary(p => p.Id, p => p.Rect);
        Assert.True(buttons.Count > 0, $"{what}: no dialog button is on screen (a dialog the user cannot close)");
        AssertNoOverlap(buttons, buttons.Keys, what + " buttons");

        // Body text must not be drawn over a button.
        foreach (var text in parts.Where(p => p.Type == ControlType.Text && p.Name.Length > 0))
        foreach (var b in buttons)
        {
            var inter = Rectangle.Intersect(text.Rect, b.Value);
            Assert.True(inter.Width <= 2 || inter.Height <= 2, $"{what}: text '{text.Name}' ({R(text.Rect)}) overlaps {b.Key} ({R(b.Value)})");
        }
        SystemAccessibility.Log($"dialog {what}: {parts.Count} parts, buttons {string.Join(", ", buttons.Select(b => b.Key + "=" + R(b.Value)))}");
    }

    private static async Task<Ctx> LaunchLargeAsync(SystemAccessibility sys, int scale, Size? size)
    {
        sys.SetTextScale(scale);
        var c = await LaunchAsync();
        if (size is { } s)
        {
            SetWindowPos(c.S.Window.Properties.NativeWindowHandle.Value, IntPtr.Zero, 0, 0, s.Width, s.Height, 0x0004 | 0x0010 | 0x0002);
            Thread.Sleep(1000);
        }
        return c;
    }

    // ── Library dialogs ────────────────────────────────────────────────────

    [SystemA11yTheory]
    [InlineData(200, 0, 0)]
    [InlineData(225, 0, 0)]
    [InlineData(225, 1500, 1000)] // largest text in the smallest realistic window
    public async Task Library_dialogs_fit_the_window_at_large_text(int scale, int width, int height)
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();
        using var c = await LaunchLargeAsync(sys, scale, width > 0 ? new Size(width, height) : null);
        var d = c.D;
        string tag = $"{scale}%{(width > 0 ? " small" : "")}";

        // Import URL, then the error dialog it leads to (a non-https URL is rejected before any connection).
        var dlg = d.OpenDialog("LibraryPage_ImportUrlButton", "Import URL");
        AssertDialogFits(c, dlg, "Import URL @" + tag);
        Shot(c, $"dlg-{tag}-importurl".Replace(' ', '-').Replace('%', 'p'));
        var box = LibraryDriver.Poll(() => dlg.FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit)), "URL text box").AsTextBox();
        box.Text = "http://127.0.0.1:9/never-contacted";
        LibraryDriver.PollUntil(() => LibraryDriver.DialogPart(dlg, "PrimaryButton").Properties.IsEnabled.Value, "Import enabled");
        d.ClickPart(dlg, "PrimaryButton");
        var err = d.Dialog("Something went wrong");
        AssertDialogFits(c, err, "Error @" + tag);
        d.ClickPart(err, "CloseButton");
        d.WaitDialogGone("Something went wrong");

        // New Collection.
        d.Select(Apple);
        d.ChooseFromFlyout("LibraryPage_AddToCollectionButton", "New Collection…");
        var nc = d.Dialog("New Collection");
        AssertDialogFits(c, nc, "New Collection @" + tag);
        d.ClickPart(nc, "CloseButton");
        d.WaitDialogGone("New Collection");

        // Tag editor (titled with the item).
        d.Select("mango-diary");
        var tags = d.OpenDialog("LibraryPage_TagsButton", "mango-diary");
        AssertDialogFits(c, tags, "Tag editor @" + tag);
        d.ClickPart(tags, "CloseButton");
        d.WaitDialogGone("mango-diary");

        // Item info, from the row's context menu (keyboard: the menu key on the focused row).
        d.ClickRow(Apple);
        AutomationElement? info = null;
        for (var attempt = 0; attempt < 4 && info is null; attempt++)
        {
            d.Press(VirtualKeyShort.APPS);
            try
            {
                info = LibraryDriver.Poll(() => d.Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem).And(cf.ByName("Get Info…"))).FirstOrDefault(),
                    "Get Info menu item", TimeSpan.FromSeconds(4));
            }
            catch (TimeoutException) when (attempt < 3) { d.Press(VirtualKeyShort.ESCAPE); d.ClickRow(Apple); }
        }
        LibraryDriver.Activate(info!);
        var infoDlg = d.Dialog(Apple);
        AssertDialogFits(c, infoDlg, "Item info @" + tag);
        d.ClickPart(infoDlg, "CloseButton");
        d.WaitDialogGone(Apple);

        // Encrypt confirmation, then its result summary.
        d.Select(Apple);
        var enc = d.OpenDialog("LibraryPage_EncryptButton", "Encrypt 1 item?");
        AssertDialogFits(c, enc, "Encrypt confirm @" + tag);
        Shot(c, $"dlg-{tag}-encrypt".Replace(' ', '-').Replace('%', 'p'));
        d.ClickPart(enc, "PrimaryButton");
        var result = d.Dialog("Encryption finished");
        AssertDialogFits(c, result, "Encrypt result @" + tag);
        d.ClickPart(result, "CloseButton");
        d.WaitDialogGone("Encryption finished");

        // Appearance (the footer nav item).
        LibraryDriver.Activate(LibraryDriver.Poll(() => d.ByName("Appearance"), "Appearance footer item"));
        var look = d.Dialog("Appearance");
        AssertDialogFits(c, look, "Appearance @" + tag);
        Shot(c, $"dlg-{tag}-appearance".Replace(' ', '-').Replace('%', 'p'));
        d.Press(VirtualKeyShort.ESCAPE);
        d.WaitDialogGone("Appearance");
    }

    // ── Flow reader ────────────────────────────────────────────────────────

    private static readonly string[] FlowBarIds =
    {
        "FlowPage_BackButton", "FlowPage_Title", "FlowPage_ContentsButton", "FlowPage_FindButton", "FlowPage_TypographyButton",
    };

    private static Dictionary<string, Rectangle> VisibleRects(LibraryDriver d, IEnumerable<string> ids)
    {
        var map = new Dictionary<string, Rectangle>();
        foreach (var id in ids)
            if (d.ById(id) is { } e && !Try(() => e.Properties.IsOffscreen.ValueOrDefault)) map[id] = e.BoundingRectangle;
        return map;
    }

    private sealed record FlowMetrics(Rectangle Window, int BodyBlockHeight, Dictionary<string, Rectangle> Bar, bool BarOverflow,
        Dictionary<string, Rectangle> Typography, Dictionary<string, Rectangle> Find, Dictionary<string, Rectangle> Toc);

    private static async Task<FlowMetrics> MeasureFlowAsync(SystemAccessibility sys, int scale, Size? size)
    {
        using var c = await LaunchLargeAsync(sys, scale, size);
        var d = c.D;
        FlowReaderTests.OpenFlowFromRow(d, "Multi Chapter");
        var win = c.S.Window.BoundingRectangle;
        Thread.Sleep(800);
        Shot(c, $"flow-{scale}{(size is null ? "" : "-small")}");

        var body = FlowReaderTests.VisibleBlocks(d).FirstOrDefault()?.BoundingRectangle.Height ?? 0;
        var bar = VisibleRects(d, FlowBarIds.Append("FlowPage_ProgressText"));
        var overflow = d.Window.FindFirstDescendant(cf => cf.ByAutomationId("MoreButton")) is not null;

        FlowReaderTests.OpenFlyout(d, "FlowPage_TypographyButton", "FlowPage_FontLarger");
        var typography = VisibleRects(d, new[]
        {
            "FlowPage_FontSmaller", "FlowPage_FontSizeText", "FlowPage_FontLarger", "FlowPage_Font_Default", "FlowPage_Font_Serif",
            "FlowPage_Spacing_Compact", "FlowPage_Spacing_Regular", "FlowPage_Spacing_Relaxed",
        });
        Shot(c, $"flow-typography-{scale}{(size is null ? "" : "-small")}");
        FlowReaderTests.CloseFlyout(d);

        var toc = new Dictionary<string, Rectangle>();
        if (d.Need("FlowPage_ContentsButton").Properties.IsEnabled.Value)
        {
            FlowReaderTests.OpenFlyout(d, "FlowPage_ContentsButton", "FlowPage_TocList");
            toc = VisibleRects(d, new[] { "FlowPage_TocList" });
            FlowReaderTests.CloseFlyout(d);
        }

        FlowReaderTests.ShowFind(d);
        var find = VisibleRects(d, new[]
        {
            "FlowPage_FindBox", "FlowPage_FindPrevious", "FlowPage_FindNext", "FlowPage_FindClose", "FlowPage_FindStatus",
        });
        Shot(c, $"flow-find-{scale}{(size is null ? "" : "-small")}");

        return new FlowMetrics(win, body, bar, overflow, typography, find, toc);
    }

    [SystemA11yTheory]
    [InlineData(200, 0, 0)]
    [InlineData(225, 0, 0)]
    [InlineData(225, 1500, 1000)]
    public async Task Flow_reader_text_grows_and_its_controls_stay_on_screen_at_large_text(int scale, int width, int height)
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();
        Size? size = width > 0 ? new Size(width, height) : null;

        var baseline = await MeasureFlowAsync(sys, 100, size);
        var scaled = await MeasureFlowAsync(sys, scale, size);
        SystemAccessibility.Log($"flow {scale}% {(size is null ? "default" : size.ToString())}: body {baseline.BodyBlockHeight} -> {scaled.BodyBlockHeight}; "
            + "bar " + string.Join(" ", scaled.Bar.Select(kv => kv.Key.Replace("FlowPage_", "") + "=" + R(kv.Value)))
            + " | typography " + string.Join(" ", scaled.Typography.Select(kv => kv.Key.Replace("FlowPage_", "") + "=" + R(kv.Value)))
            + " | find " + string.Join(" ", scaled.Find.Select(kv => kv.Key.Replace("FlowPage_", "") + "=" + R(kv.Value))));

        foreach (var (label, m) in new[] { ("100%", baseline), (scale + "%", scaled) })
        {
            AssertInside(m.Window, m.Bar, label + " Flow bar");
            AssertInside(m.Window, m.Typography, label + " Flow typography flyout");
            AssertInside(m.Window, m.Find, label + " Flow find bar");
            AssertInside(m.Window, m.Toc, label + " Flow contents flyout");
            AssertNoOverlap(m.Bar, FlowBarIds, label + " Flow bar");
            AssertNoOverlap(m.Typography, m.Typography.Keys, label + " Flow typography flyout");
            AssertNoOverlap(m.Find, m.Find.Keys, label + " Flow find bar");

            // Everything the bar offers is either on screen or behind the overflow menu.
            var hidden = new[] { "FlowPage_ContentsButton", "FlowPage_FindButton", "FlowPage_TypographyButton", "FlowPage_BackButton" }
                .Where(id => !m.Bar.ContainsKey(id)).ToList();
            Assert.True(hidden.Count == 0 || m.BarOverflow, label + ": Flow commands hidden with no overflow: " + string.Join(", ", hidden));

            // The flyouts really opened with their controls (an empty map would make the checks above vacuous).
            Assert.True(m.Typography.Count >= 6, label + ": typography flyout showed only " + m.Typography.Count + " controls");
            Assert.True(m.Find.ContainsKey("FlowPage_FindBox") && m.Find.ContainsKey("FlowPage_FindClose"), label + ": find bar controls missing");
        }

        Assert.True(baseline.BodyBlockHeight > 0 && scaled.BodyBlockHeight > 0, "no Flow body block was on screen to measure");
        Assert.True(scaled.BodyBlockHeight >= baseline.BodyBlockHeight * 1.05,
            $"Flow body text did not grow with the system text size: {baseline.BodyBlockHeight} -> {scaled.BodyBlockHeight}");
    }
}

public partial class SystemAccessibilityTests
{
    /// <summary>
    /// At 225 % in a small window the Encrypt dialog's body is taller than the room it has, so it scrolls. A keyboard-only
    /// user must be able to reach and scroll it, or the "cannot be recovered" warning below the fold is unreadable.
    /// </summary>
    [SystemA11yFact]
    public async Task Scrollable_dialog_content_can_be_reached_and_scrolled_by_keyboard()
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();
        using var c = await LaunchLargeAsync(sys, 225, new Size(1500, 1000));
        var d = c.D;
        d.Select(Apple);
        var dlg = d.OpenDialog("LibraryPage_EncryptButton", "Encrypt 1 item?");

        AutomationElement? Viewer() => dlg.FindAllDescendants().FirstOrDefault(e =>
            Try(() => e.Patterns.Scroll.IsSupported) && Try(() => e.Patterns.Scroll.Pattern.VerticallyScrollable.Value));
        var viewer = LibraryDriver.Poll(Viewer, "a vertically scrollable dialog body (the test needs content taller than the dialog)");
        double Pct() => viewer.Patterns.Scroll.Pattern.VerticalScrollPercent.Value;
        Assert.Equal(0, Pct(), 1);

        // Tab until focus is on the scrollable body (it precedes the buttons in the dialog's tab order).
        for (var i = 0; i < 6; i++)
        {
            var f = Try(() => d.Session.Automation.FocusedElement());
            if (f is not null && (Try(() => f.Name) ?? "").EndsWith(", scrollable content", StringComparison.Ordinal)) break;
            d.Press(VirtualKeyShort.TAB);
        }
        var focused = Try(() => d.Session.Automation.FocusedElement());
        Assert.True(focused is not null && (Try(() => focused.Name) ?? "").EndsWith(", scrollable content", StringComparison.Ordinal),
            "Tab never reached the dialog's scrollable body; focus is on " + Try(() => focused?.ControlType) + ":" + Try(() => focused?.Name));

        d.Press(VirtualKeyShort.NEXT); // Page Down
        LibraryDriver.PollUntil(() => Pct() > 0, "Page Down scrolls the dialog body");
        d.Press(VirtualKeyShort.END);
        LibraryDriver.PollUntil(() => Pct() > 95, "End scrolls to the bottom of the dialog body");
        d.Press(VirtualKeyShort.HOME);
        LibraryDriver.PollUntil(() => Pct() < 1, "Home scrolls back to the top");

        d.Press(VirtualKeyShort.ESCAPE);
        d.WaitDialogGone("Encrypt 1 item?");
    }
}
