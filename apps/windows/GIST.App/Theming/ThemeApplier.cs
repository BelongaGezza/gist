using Gist.Core.Theming;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Windows.UI;
using Windows.UI.ViewManagement;

namespace Gist.App.Theming;

/// <summary>
/// Applies a resolved <see cref="GistTheme"/> to the app's brushes and to one window's chrome
/// (<c>docs/windows-ui-spec.md</c> §3.1, §2 "Window material"). The only place that touches
/// <c>Application.Current.Resources</c> or a <see cref="Window.SystemBackdrop"/> for theming.
/// </summary>
public static class ThemeApplier
{
    /// <summary>
    /// <c>GistBackground</c>/<c>GistForeground</c>/<c>GistAccent</c>/<c>GistSecondaryText</c> — the
    /// four brushes the spec says a theme exposes. Foreground at 60% opacity mirrors Apple's RSVP
    /// captions (<c>0.6</c> → alpha <c>153</c>).
    /// </summary>
    private const byte SecondaryTextAlpha = 153;

    /// <summary>
    /// Applies <paramref name="theme"/> to <paramref name="window"/>, unless Windows is currently in
    /// a high-contrast theme, in which case every GIST palette is bypassed (spec §3.1 "High
    /// contrast [W, required]") and the window falls back to the OS's own high-contrast resources
    /// and no backdrop.
    /// </summary>
    public static void Apply(GistTheme theme, Window window)
    {
        ArgumentNullException.ThrowIfNull(theme);
        ArgumentNullException.ThrowIfNull(window);

        var root = window.Content as FrameworkElement;

        if (new AccessibilitySettings().HighContrast)
        {
            if (root is not null) root.RequestedTheme = ElementTheme.Default;
            window.SystemBackdrop = null;
            return;
        }

        var app = Application.Current;
        var foreground = ParseHexColor(theme.Foreground);

        app.Resources["GistBackground"] = new SolidColorBrush(ParseHexColor(theme.Background));
        app.Resources["GistForeground"] = new SolidColorBrush(foreground);
        app.Resources["GistAccent"] = new SolidColorBrush(ResolveAccent(theme));
        app.Resources["GistSecondaryText"] = new SolidColorBrush(
            Color.FromArgb(SecondaryTextAlpha, foreground.R, foreground.G, foreground.B));

        if (root is not null)
        {
            root.RequestedTheme = theme.Base == GistBaseTheme.Dark ? ElementTheme.Dark : ElementTheme.Light;
        }

        window.SystemBackdrop = theme.UseMica ? PreferredBackdrop() : null;
    }

    private static Color ResolveAccent(GistTheme theme)
    {
        if (theme.AccentHex is { } hex) return ParseHexColor(hex);

        // Light/Dark/OLED follow the user's system accent colour (spec §3.1 "Accent [W]").
        return Application.Current.Resources.TryGetValue("SystemAccentColor", out var value) && value is Color c
            ? c
            : Color.FromArgb(255, 0x43, 0x61, 0xEE); // brand fallback token, per docs/iconspecification.md
    }

    private static SystemBackdrop? PreferredBackdrop()
    {
        if (MicaController.IsSupported()) return new MicaBackdrop();
        if (DesktopAcrylicController.IsSupported()) return new DesktopAcrylicBackdrop();
        return null;
    }

    /// <summary>Parses a <c>#RRGGBB</c> string. Malformed input (a coding error, not user input) throws.</summary>
    private static Color ParseHexColor(string hex)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(hex);
        var span = hex.AsSpan().TrimStart('#');
        if (span.Length != 6)
        {
            throw new FormatException($"Expected a #RRGGBB colour, got '{hex}'.");
        }

        var r = byte.Parse(span[..2], System.Globalization.NumberStyles.HexNumber);
        var g = byte.Parse(span[2..4], System.Globalization.NumberStyles.HexNumber);
        var b = byte.Parse(span[4..6], System.Globalization.NumberStyles.HexNumber);
        return Color.FromArgb(255, r, g, b);
    }
}
