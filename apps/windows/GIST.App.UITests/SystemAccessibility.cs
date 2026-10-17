using System.Drawing;
using System.Runtime.InteropServices;
using FlaUI.Core.AutomationElements;
using Microsoft.Win32;

namespace Gist.App.UITests;

/// <summary>
/// A Fact that runs only when <c>GIST_RUN_SYSTEM_A11Y_TESTS=1</c> as well as <c>GIST_RUN_UI_TESTS=1</c>. These tests
/// change real, machine-wide Windows accessibility settings (text size, high-contrast theme) for the duration of a
/// test, so they are a separate opt-in from the ordinary UI suite.
/// </summary>
public sealed class SystemA11yFactAttribute : FactAttribute
{
    public const string EnvVar = "GIST_RUN_SYSTEM_A11Y_TESTS";

    public SystemA11yFactAttribute()
    {
        if (Environment.GetEnvironmentVariable(UiFactAttribute.EnvVar) != "1" || Environment.GetEnvironmentVariable(EnvVar) != "1")
        {
            Skip = $"Changes system-wide accessibility settings: set {UiFactAttribute.EnvVar}=1 and {EnvVar}=1 (interactive desktop).";
        }
    }
}

/// <summary>The same gate for data-driven tests.</summary>
public sealed class SystemA11yTheoryAttribute : TheoryAttribute
{
    public SystemA11yTheoryAttribute()
    {
        if (Environment.GetEnvironmentVariable(UiFactAttribute.EnvVar) != "1" || Environment.GetEnvironmentVariable(SystemA11yFactAttribute.EnvVar) != "1")
        {
            Skip = $"Changes system-wide accessibility settings: set {UiFactAttribute.EnvVar}=1 and {SystemA11yFactAttribute.EnvVar}=1 (interactive desktop).";
        }
    }
}

/// <summary>
/// Scoped, restoring control of the two Windows accessibility settings the W6 pass needs: the text size
/// (<c>HKCU\Software\Microsoft\Accessibility\TextScaleFactor</c>, what Settings > Accessibility > Text size writes and
/// <c>UISettings.TextScaleFactor</c> reads) and the high-contrast switch (<c>SystemParametersInfo(SPI_SETHIGHCONTRAST)</c>).
/// Both are restored in <see cref="Dispose"/>; the original values are also written to
/// <see cref="MarkerPath"/> first so <c>restore-system-accessibility.ps1</c> can undo them if the test process dies mid-test.
/// </summary>
public sealed class SystemAccessibility : IDisposable
{
    public static readonly string MarkerPath = Path.Combine(Path.GetTempPath(), "gist-a11y-restore.txt");

    private const string KeyPath = @"Software\Microsoft\Accessibility";
    private const string ValueName = "TextScaleFactor";
    private const int SpiGetHighContrast = 0x42, SpiSetHighContrast = 0x43, SpifUpdateAndBroadcast = 3, HcfOn = 1;

    [StructLayout(LayoutKind.Sequential)]
    private struct HighContrast { public int cbSize; public int dwFlags; public IntPtr lpszDefaultScheme; }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SystemParametersInfo(int action, int param, ref HighContrast hc, int flags);

    [DllImport("user32.dll")]
    private static extern IntPtr OpenInputDesktop(int flags, bool inherit, int access);

    [DllImport("user32.dll")]
    private static extern bool CloseDesktop(IntPtr desktop);

    [DllImport("user32.dll")]
    private static extern bool SetProcessDpiAwarenessContext(IntPtr context);

    [DllImport("user32.dll")]
    private static extern int GetSysColor(int index);

    private readonly object? _originalScale;
    private readonly int _originalHcFlags;
    private bool _scaleChanged, _hcChanged, _disposed;

    private SystemAccessibility()
    {
        using var key = Registry.CurrentUser.OpenSubKey(KeyPath);
        _originalScale = key?.GetValue(ValueName);
        _originalHcFlags = ReadHc().dwFlags;
        // Written before anything is changed, so a crash between here and Dispose is recoverable.
        File.WriteAllLines(MarkerPath, new[]
        {
            "textScale=" + (_originalScale is null ? "absent" : Convert.ToString(_originalScale)),
            "hcFlags=" + _originalHcFlags,
        });
    }

    /// <summary>Makes this process per-monitor DPI aware so UIA rectangles and screen captures are in physical pixels (an unaware process sees a scaled, cropped view of a scaled window).</summary>
    private static void EnsureDpiAware() { try { SetProcessDpiAwarenessContext(new IntPtr(-4)); } catch (Exception) { } }

    /// <summary>Pixels and synthetic keys both need the interactive desktop; a locked workstation (secure desktop) gives black captures and rejected input.</summary>
    public static void RequireInteractiveDesktop()
    {
        var h = OpenInputDesktop(0, false, 0x100);
        if (h == IntPtr.Zero) throw new Xunit.Sdk.XunitException("The workstation is locked (secure desktop active): unlock it and re-run; captures would be black.");
        CloseDesktop(h);
    }

    public static SystemAccessibility Begin()
    {
        EnsureDpiAware();
        return new();
    }

    private static HighContrast ReadHc()
    {
        var hc = new HighContrast();
        hc.cbSize = Marshal.SizeOf(hc);
        SystemParametersInfo(SpiGetHighContrast, hc.cbSize, ref hc, 0);
        return hc;
    }

    private static bool WriteHc(int flags)
    {
        var hc = new HighContrast { dwFlags = flags, lpszDefaultScheme = Marshal.StringToHGlobalUni("High Contrast Black") };
        hc.cbSize = Marshal.SizeOf(hc);
        try { return SystemParametersInfo(SpiSetHighContrast, hc.cbSize, ref hc, SpifUpdateAndBroadcast); }
        finally { Marshal.FreeHGlobal(hc.lpszDefaultScheme); }
    }

    /// <summary>Sets Settings > Accessibility > Text size to <paramref name="percent"/> (100-225). Takes effect for apps started afterwards.</summary>
    public void SetTextScale(int percent)
    {
        using var key = Registry.CurrentUser.CreateSubKey(KeyPath);
        key.SetValue(ValueName, percent, RegistryValueKind.DWord);
        _scaleChanged = true;
    }

    /// <summary>Turns the system high-contrast theme on or off and waits until Windows reports it.</summary>
    public void SetHighContrast(bool on)
    {
        Assert.True(WriteHc(on ? HcfOn : _originalHcFlags & ~HcfOn), "SystemParametersInfo(SPI_SETHIGHCONTRAST) failed");
        _hcChanged = true;
        LibraryDriver.PollUntil(() => ((ReadHc().dwFlags & HcfOn) != 0) == on, "high contrast " + (on ? "on" : "off"));
        Thread.Sleep(1500); // the theme change is broadcast and applied asynchronously
    }

    /// <summary>COLOR_WINDOW (5), COLOR_WINDOWTEXT (8), COLOR_HIGHLIGHT (13), COLOR_HIGHLIGHTTEXT (14), COLOR_BTNFACE (15), COLOR_BTNTEXT (18), COLOR_GRAYTEXT (17), COLOR_HOTLIGHT (26).</summary>
    public static Color SysColor(int index)
    {
        var c = GetSysColor(index); // 0x00BBGGRR
        return Color.FromArgb(c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF);
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        try
        {
            if (_hcChanged) WriteHc(_originalHcFlags);
            if (_scaleChanged)
            {
                using var key = Registry.CurrentUser.CreateSubKey(KeyPath);
                if (_originalScale is null) key.DeleteValue(ValueName, throwOnMissingValue: false);
                else key.SetValue(ValueName, _originalScale, RegistryValueKind.DWord);
            }
        }
        finally
        {
            try { File.Delete(MarkerPath); } catch (IOException) { }
        }
    }

    // ── Pixels ─────────────────────────────────────────────────────────────

    /// <summary>The element's on-screen pixels, or throws if the capture is blank (a black capture is a harness failure, not a result).</summary>
    public static Bitmap Capture(AutomationElement e)
    {
        RequireInteractiveDesktop();
        Thread.Sleep(300);
        using var img = FlaUI.Core.Capturing.Capture.Rectangle(e.BoundingRectangle);
        return new Bitmap(img.Bitmap);
    }

    /// <summary>The screen pixels of a rectangle (physical pixels; the process is DPI aware).</summary>
    public static Bitmap Capture(Rectangle r)
    {
        RequireInteractiveDesktop();
        using var img = FlaUI.Core.Capturing.Capture.Rectangle(r);
        return new Bitmap(img.Bitmap);
    }

    /// <summary>Colours covering at least <paramref name="minShare"/> of the bitmap, most common first, as (colour, share).</summary>
    public static List<(Color Color, double Share)> DominantColors(Bitmap bmp, double minShare = 0.01)
    {
        var counts = new Dictionary<int, int>();
        var total = 0;
        for (var y = 0; y < bmp.Height; y++)
        for (var x = 0; x < bmp.Width; x++)
        {
            var key = bmp.GetPixel(x, y).ToArgb() & 0xFFFFFF;
            counts[key] = counts.GetValueOrDefault(key) + 1;
            total++;
        }
        return counts.Where(kv => (double)kv.Value / total >= minShare)
            .OrderByDescending(kv => kv.Value)
            .Select(kv => (Color.FromArgb(255, (kv.Key >> 16) & 0xFF, (kv.Key >> 8) & 0xFF, kv.Key & 0xFF), (double)kv.Value / total))
            .ToList();
    }

    public static string Hex(Color c) => $"#{c.R:X2}{c.G:X2}{c.B:X2}";

    /// <summary>WCAG contrast ratio between two colours.</summary>
    public static double Contrast(Color a, Color b)
    {
        static double Lin(byte v) { var s = v / 255.0; return s <= 0.03928 ? s / 12.92 : Math.Pow((s + 0.055) / 1.055, 2.4); }
        static double L(Color c) => 0.2126 * Lin(c.R) + 0.7152 * Lin(c.G) + 0.0722 * Lin(c.B);
        var (hi, lo) = (Math.Max(L(a), L(b)), Math.Min(L(a), L(b)));
        return (hi + 0.05) / (lo + 0.05);
    }

    /// <summary>Appends a line to the file named by GIST_A11Y_LOG, if set (measurements for the verification record).</summary>
    public static void Log(string line)
    {
        var path = Environment.GetEnvironmentVariable("GIST_A11Y_LOG");
        if (!string.IsNullOrEmpty(path)) File.AppendAllText(path, line + Environment.NewLine);
    }
}
