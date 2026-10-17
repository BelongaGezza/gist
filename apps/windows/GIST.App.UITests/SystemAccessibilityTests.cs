using System.Drawing;
using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;

namespace Gist.App.UITests;

/// <summary>
/// W6 text-size and high-contrast passes (docs/windows-ui-spec.md §3.1, §11). These change machine-wide Windows
/// accessibility settings for their duration (see <see cref="SystemAccessibility"/>) and are a separate opt-in
/// (<c>GIST_RUN_SYSTEM_A11Y_TESTS=1</c>). Text-size geometry is read through UI Automation only; the high-contrast
/// checks read real pixels and so need an unlocked interactive desktop.
/// </summary>
[Trait("Category", "UI")]
public partial class SystemAccessibilityTests
{
    private const string Apple = "apple-orchard";

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

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

    private static T? Try<T>(Func<T> f) { try { return f(); } catch (Exception) { return default; } }

    private static void Shot(Ctx c, string name)
    {
        var dir = Environment.GetEnvironmentVariable("GIST_A11Y_SHOTS");
        if (!string.IsNullOrEmpty(dir)) c.S.TrySaveScreenshot(Path.Combine(dir, name + ".png"));
    }

    // ── Text size: geometry ────────────────────────────────────────────────

    private static readonly string[] LibraryControls =
    {
        "LibraryPage_SearchBox", "LibraryPage_ImportFileButton", "LibraryPage_ImportUrlButton", "LibraryPage_SortButton",
        "LibraryPage_FilterButton", "LibraryPage_AddToCollectionButton", "LibraryPage_EncryptButton", "LibraryPage_RemoveButton",
        "LibraryPage_OpenButton", "LibraryPage_TagsButton", "LibraryPage_List",
    };

    private static readonly string[] RsvpControls =
    {
        "RsvpPage_BackButton", "RsvpPage_Word", "RsvpPage_PlayPauseButton", "RsvpPage_Scrubber", "RsvpPage_WpmSlider",
        "RsvpPage_WpmBox", "RsvpPage_PunctuationToggle", "RsvpPage_ReadWordButton",
    };

    private sealed record Metrics(
        Rectangle Window, int RowHeight, int SearchHeight, int WordHeight,
        Dictionary<string, Rectangle> Library, Dictionary<string, Rectangle> Rsvp,
        Dictionary<string, Rectangle> Dialog, bool OverflowPresent);

    private static Dictionary<string, Rectangle> Visible(LibraryDriver d, IEnumerable<string> ids)
    {
        var map = new Dictionary<string, Rectangle>();
        foreach (var id in ids)
            if (d.ById(id) is { } e && !Try(() => e.Properties.IsOffscreen.ValueOrDefault)) map[id] = e.BoundingRectangle;
        return map;
    }

    private static async Task<Metrics> MeasureAsync(SystemAccessibility sys, int scale, Size? windowSize = null)
    {
        sys.SetTextScale(scale);
        using var c = await LaunchAsync();
        var d = c.D;
        if (windowSize is { } size)
        {
            // The WinUI window does not expose UIA TransformPattern, so resize at the Win32 level (physical pixels).
            SetWindowPos(c.S.Window.Properties.NativeWindowHandle.Value, IntPtr.Zero, 0, 0, size.Width, size.Height, 0x0004 | 0x0010 | 0x0002);
            Thread.Sleep(1000); // layout settles after the resize
        }
        var win = c.S.Window.BoundingRectangle;

        var lib = Visible(d, LibraryControls);
        var overflow = d.Window.FindFirstDescendant(cf => cf.ByAutomationId("MoreButton")) is not null;
        var rowHeight = d.Rows()[0].BoundingRectangle.Height; // the first row: later rows are virtualised away in a short window
        var searchHeight = d.Need("LibraryPage_SearchBox").BoundingRectangle.Height;
        Shot(c, $"scale{scale}-library");

        // Remove dialog: both buttons must be reachable on screen.
        d.Select(Apple);
        var dlg = d.OpenDialog("LibraryPage_RemoveButton", "Remove 1 item?");
        var dialog = new Dictionary<string, Rectangle>
        {
            ["Dialog"] = dlg.BoundingRectangle,
            ["PrimaryButton"] = LibraryDriver.DialogPart(dlg, "PrimaryButton").BoundingRectangle,
            ["CloseButton"] = LibraryDriver.DialogPart(dlg, "CloseButton").BoundingRectangle,
        };
        Shot(c, $"scale{scale}-dialog");
        d.ClickPart(dlg, "CloseButton");
        d.WaitDialogGone("Remove 1 item?");

        // RSVP reader, entered with UIA only (no synthetic keys).
        d.Select(Apple);
        d.InvokeCommand("LibraryPage_OpenButton");
        d.Need("RsvpPage_PlayPauseButton");
        Thread.Sleep(800);
        var rsvp = Visible(d, RsvpControls);
        Shot(c, $"scale{scale}-rsvp");
        var word = rsvp.GetValueOrDefault("RsvpPage_Word").Height;

        return new Metrics(win, rowHeight, searchHeight, word, lib, rsvp, dialog, overflow);
    }

    private static string R(Rectangle r) => $"{r.Width}x{r.Height}@{r.Left},{r.Top}";

    private static string Fmt(Metrics m) =>
        $"win {m.Window.Width}x{m.Window.Height} row {m.RowHeight} search {m.SearchHeight} word {m.WordHeight} overflow {m.OverflowPresent} "
        + "lib[" + string.Join(" ", m.Library.Select(kv => kv.Key.Replace("LibraryPage_", "") + "=" + R(kv.Value))) + "] "
        + "rsvp[" + string.Join(" ", m.Rsvp.Select(kv => kv.Key.Replace("RsvpPage_", "") + "=" + R(kv.Value))) + "] "
        + "dlg[" + string.Join(" ", m.Dialog.Select(kv => kv.Key + "=" + R(kv.Value))) + "]";

    // ── Text size: assertions ──────────────────────────────────────────────

    private static readonly string[] CommandIds =
    {
        "LibraryPage_ImportFileButton", "LibraryPage_ImportUrlButton", "LibraryPage_SortButton", "LibraryPage_FilterButton",
        "LibraryPage_AddToCollectionButton", "LibraryPage_EncryptButton", "LibraryPage_RemoveButton", "LibraryPage_OpenButton",
        "LibraryPage_TagsButton",
    };

    private static void AssertInside(Rectangle win, IReadOnlyDictionary<string, Rectangle> rects, string what)
    {
        var bad = rects.Where(kv => kv.Value.Width <= 0 || kv.Value.Height <= 0
                || kv.Value.Left < win.Left - 2 || kv.Value.Top < win.Top - 2 || kv.Value.Right > win.Right + 2 || kv.Value.Bottom > win.Bottom + 2)
            .Select(kv => $"{kv.Key} {R(kv.Value)} vs window {R(win)}").ToList();
        Assert.True(bad.Count == 0, what + " outside the window: " + string.Join("; ", bad));
    }

    private static void AssertNoOverlap(IReadOnlyDictionary<string, Rectangle> rects, IEnumerable<string> ids, string what)
    {
        var present = ids.Where(rects.ContainsKey).ToList();
        for (var i = 0; i < present.Count; i++)
        for (var j = i + 1; j < present.Count; j++)
        {
            var (a, b) = (rects[present[i]], rects[present[j]]);
            var inter = Rectangle.Intersect(a, b);
            Assert.True(inter.Width <= 2 || inter.Height <= 2, $"{what}: {present[i]} ({R(a)}) overlaps {present[j]} ({R(b)})");
        }
    }

    private static void AssertLayout(Metrics m, string label)
    {
        AssertInside(m.Window, m.Library, label + " library controls");
        AssertInside(m.Window, m.Rsvp, label + " RSVP controls");
        AssertInside(m.Window, m.Dialog.Where(kv => kv.Key != "Dialog").ToDictionary(kv => kv.Key, kv => kv.Value), label + " Remove-dialog buttons");
        AssertNoOverlap(m.Library, CommandIds.Append("LibraryPage_SearchBox"), label + " library");
        AssertNoOverlap(m.Rsvp, RsvpControls.Where(id => id != "RsvpPage_Word"), label + " RSVP");
        // The word is the content of the reader: it must never be pushed up under the header (it once was, at 225 % in a short window).
        if (m.Rsvp.TryGetValue("RsvpPage_Word", out var word) && m.Rsvp.TryGetValue("RsvpPage_BackButton", out var back))
            Assert.True(word.Top >= back.Bottom - 2, $"{label}: RSVP word ({R(word)}) is hidden under the header ({R(back)})");
        AssertNoOverlap(m.Dialog, new[] { "PrimaryButton", "CloseButton" }, label + " dialog");

        // Every command is either on screen or reachable through the overflow ("...") menu; none may just disappear.
        var hidden = CommandIds.Where(id => !m.Library.ContainsKey(id)).ToList();
        Assert.True(hidden.Count == 0 || m.OverflowPresent, label + ": commands hidden with no overflow button: " + string.Join(", ", hidden));
    }

    [SystemA11yTheory]
    [InlineData(150, 0, 0)]
    [InlineData(200, 0, 0)]
    [InlineData(225, 0, 0)]
    [InlineData(200, 1500, 1000)]  // a small laptop window at 200 %
    [InlineData(225, 1500, 1000)]  // the worst case: largest text, smallest realistic window
    public async Task Text_size_grows_the_text_without_clipping_or_overlap(int scale, int width, int height)
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();
        Size? size = width > 0 ? new Size(width, height) : null;

        var baseline = await MeasureAsync(sys, 100, size);
        AssertLayout(baseline, "100%");
        var scaled = await MeasureAsync(sys, scale, size);
        SystemAccessibility.Log($"{scale}% {(size is null ? "default" : size.ToString())}: " + Fmt(scaled));

        AssertLayout(scaled, scale + "%");

        // The text really did grow: library rows scale with the text size...
        var wanted = baseline.RowHeight * (1 + 0.5 * (scale / 100.0 - 1));
        Assert.True(scaled.RowHeight >= wanted, $"row height {scaled.RowHeight}px at {scale}% vs {baseline.RowHeight}px at 100% (wanted >= {wanted:F0})");
        Assert.True(scaled.SearchHeight > baseline.SearchHeight, $"search box did not grow: {baseline.SearchHeight} -> {scaled.SearchHeight}");
        // ...and so does the RSVP word, on top of its own size. Windows tapers text scaling for large fonts, so the
        // bar is "visibly larger" rather than proportional.
        Assert.True(scaled.WordHeight >= baseline.WordHeight * 1.05, $"RSVP word did not grow: {baseline.WordHeight} -> {scaled.WordHeight}");
    }

    // ── High contrast ──────────────────────────────────────────────────────

    /// <summary>Every GetSysColor index 0-30: the whole palette a high-contrast theme can legitimately paint with.</summary>
    private static HashSet<int> SystemPalette() =>
        Enumerable.Range(0, 31).Select(i => SystemAccessibility.SysColor(i)).Select(c => c.ToArgb() & 0xFFFFFF).ToHashSet();

    /// <summary>True if <paramref name="c"/> is a system colour or an alpha blend of two (WinUI draws borders, hover and pressed states as translucent system colours, which land on the line between two palette entries).</summary>
    private static bool IsSystemDerived(Color c, IReadOnlyList<Color> palette)
    {
        const double tol = 8;
        foreach (var a in palette)
        foreach (var b in palette)
        {
            double dr = b.R - a.R, dg = b.G - a.G, db = b.B - a.B;
            var len2 = dr * dr + dg * dg + db * db;
            var t = len2 == 0 ? 0 : Math.Clamp(((c.R - a.R) * dr + (c.G - a.G) * dg + (c.B - a.B) * db) / len2, 0, 1);
            if (Math.Abs(a.R + t * dr - c.R) <= tol && Math.Abs(a.G + t * dg - c.G) <= tol && Math.Abs(a.B + t * db - c.B) <= tol) return true;
        }
        return false;
    }

    private static List<string> NonSystemColours(Bitmap bmp, double minShare)
    {
        var palette = Enumerable.Range(0, 31).Select(SystemAccessibility.SysColor).Select(c => c.ToArgb() & 0xFFFFFF).Distinct()
            .Select(v => Color.FromArgb(255, (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF)).ToList();
        return SystemAccessibility.DominantColors(bmp, minShare)
            .Where(x => !IsSystemDerived(x.Color, palette))
            .Select(x => $"{SystemAccessibility.Hex(x.Color)} {x.Share:P1}").ToList();
    }

    /// <summary>The dialog box itself: the union of its content, excluding the full-window dim overlay WinUI draws behind every ContentDialog (that overlay is the framework's translucent black, not GIST's palette).</summary>
    private static Rectangle DialogBox(AutomationElement dlg)
    {
        var parts = dlg.FindAllDescendants()
            .Select(e => Try(() => e.BoundingRectangle))
            .Where(r => r.Width > 0 && r.Height > 0 && r.Width < dlg.BoundingRectangle.Width)
            .ToList();
        var box = parts.Aggregate(Rectangle.Union);
        return Rectangle.Inflate(box, 24, 24);
    }

    private static void AssertSystemColoursOnly(AutomationElement e, string what) => AssertSystemColoursOnly(e.BoundingRectangle, what);

    private static void AssertSystemColoursOnly(Rectangle r, string what)
    {
        using var bmp = SystemAccessibility.Capture(r);
        var stray = NonSystemColours(bmp, 0.003);
        Assert.True(stray.Count == 0, $"{what} paints with non-system colours in high contrast: {string.Join(", ", stray)}");
    }

    /// <summary>The two dominant colours of a rectangle are its background and its text; require this contrast between them.</summary>
    private static void AssertLegible(Rectangle r, double minRatio, string what)
    {
        using var bmp = SystemAccessibility.Capture(r);
        // The most common colour is the background; the text colour is the one furthest from it among colours that
        // cover a meaningful number of pixels (anti-aliasing smears a glyph across many faint intermediates).
        var dom = SystemAccessibility.DominantColors(bmp, 0.0005);
        Assert.True(dom.Count >= 2, $"{what}: only {dom.Count} distinct colour(s) - no visible text?");
        var bg = dom[0].Color;
        var fg = dom.Skip(1).OrderByDescending(x => SystemAccessibility.Contrast(bg, x.Color)).First().Color;
        var ratio = SystemAccessibility.Contrast(bg, fg);
        SystemAccessibility.Log($"HC contrast {what}: bg {SystemAccessibility.Hex(bg)} ({dom[0].Share:P0}) text {SystemAccessibility.Hex(fg)} = {ratio:F1}:1");
        Assert.True(ratio >= minRatio, $"{what}: contrast {ratio:F1}:1 ({SystemAccessibility.Hex(fg)} on {SystemAccessibility.Hex(bg)}) < {minRatio}:1");
    }

    [SystemA11yFact]
    public async Task High_contrast_paints_only_system_colours()
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();

        // Detector self-test: it must flag a colour that is not derived from the system palette (GIST's brand accent),
        // and accept a system colour and a translucent blend of two, or a clean result below would mean nothing.
        var palette = new List<Color> { SystemAccessibility.SysColor(5), SystemAccessibility.SysColor(8), SystemAccessibility.SysColor(13) };
        Assert.False(IsSystemDerived(Color.FromArgb(255, 0x43, 0x61, 0xEE), palette), "brand accent should not look system-derived");
        Assert.True(IsSystemDerived(palette[2], palette));
        Assert.True(IsSystemDerived(Color.FromArgb(255, (palette[0].R + palette[2].R) / 2, (palette[0].G + palette[2].G) / 2, (palette[0].B + palette[2].B) / 2), palette));

        sys.SetHighContrast(true);
        using var c = await LaunchAsync();
        var d = c.D;
        AssertSystemColoursOnly(c.S.Window, "Library");

        d.Select(Apple);
        var dlg = d.OpenDialog("LibraryPage_RemoveButton", "Remove 1 item?");
        AssertSystemColoursOnly(DialogBox(dlg), "Remove dialog");
        d.ClickPart(dlg, "CloseButton");
        d.WaitDialogGone("Remove 1 item?");

        d.Select(Apple);
        d.InvokeCommand("LibraryPage_OpenButton");
        d.Need("RsvpPage_PlayPauseButton");
        Thread.Sleep(800);
        AssertSystemColoursOnly(c.S.Window, "RSVP reader");
    }

    [SystemA11yFact]
    public async Task High_contrast_text_meets_a_7_to_1_contrast_in_the_library_dialog_and_reader()
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();
        sys.SetHighContrast(true);
        using var c = await LaunchAsync();
        var d = c.D;

        AssertLegible(d.Row(Apple).BoundingRectangle, 7, "library row");
        AssertLegible(d.Need("LibraryPage_ImportFileButton").BoundingRectangle, 7, "Import File button");

        d.Select(Apple);
        var dlg = d.OpenDialog("LibraryPage_RemoveButton", "Remove 1 item?");
        AssertLegible(LibraryDriver.DialogPart(dlg, "PrimaryButton").BoundingRectangle, 7, "Remove dialog primary button");
        AssertLegible(LibraryDriver.DialogPart(dlg, "CloseButton").BoundingRectangle, 7, "Remove dialog cancel button");
        d.ClickPart(dlg, "CloseButton");
        d.WaitDialogGone("Remove 1 item?");

        d.Select(Apple);
        d.InvokeCommand("LibraryPage_OpenButton");
        d.Need("RsvpPage_PlayPauseButton");
        Thread.Sleep(800);
        AssertLegible(d.Need("RsvpPage_Word").BoundingRectangle, 7, "RSVP word");
        AssertLegible(d.Need("RsvpPage_PlayPauseButton").BoundingRectangle, 7, "RSVP Play button");
    }

    [SystemA11yFact]
    public async Task High_contrast_flow_reader_uses_system_colours_and_legible_text()
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();
        sys.SetHighContrast(true);
        using var c = await LaunchAsync();
        var d = c.D;

        FlowReaderTests.OpenFlowFromRow(d, "Multi Chapter");
        var list = d.Need("FlowPage_List");
        AssertSystemColoursOnly(list, "Flow reading surface");
        AssertLegible(list.BoundingRectangle, 7, "Flow reading surface");
    }

    [SystemA11yFact]
    public async Task High_contrast_keyboard_focus_is_drawn_on_the_focused_button()
    {
        using var sys = SystemAccessibility.Begin();
        SystemAccessibility.RequireInteractiveDesktop();
        sys.SetHighContrast(true);
        using var c = await LaunchAsync();
        var d = c.D;
        d.FocusWindow();

        var sort = d.Need("LibraryPage_SortButton");
        var area = Rectangle.Inflate(sort.BoundingRectangle, 8, 8);
        // Focus is not on the Sort button yet: park it on the search box (Ctrl+F), take the "before" picture.
        d.Chord(FlaUI.Core.WindowsAPI.VirtualKeyShort.CONTROL, FlaUI.Core.WindowsAPI.VirtualKeyShort.KEY_F);
        d.WaitFocus("LibraryPage_SearchBox");
        Thread.Sleep(400);
        using var before = SystemAccessibility.Capture(area);

        for (var i = 0; i < 12 && d.FocusedId() != "LibraryPage_SortButton"; i++) d.Press(FlaUI.Core.WindowsAPI.VirtualKeyShort.TAB);
        d.WaitFocus("LibraryPage_SortButton");
        Thread.Sleep(400);
        using var after = SystemAccessibility.Capture(area);

        var changed = 0;
        var w = Math.Min(before.Width, after.Width);
        var h = Math.Min(before.Height, after.Height);
        for (var y = 0; y < h; y++)
        for (var x = 0; x < w; x++)
            if (before.GetPixel(x, y).ToArgb() != after.GetPixel(x, y).ToArgb()) changed++;
        SystemAccessibility.Log($"HC focus: {changed} pixels changed around the Sort button when it took keyboard focus");
        Assert.True(changed >= 50, $"no visible focus indicator on the focused Sort button in high contrast ({changed} pixels differ)");
    }
}
