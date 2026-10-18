using Gist.Core.Theming;

namespace Gist.Core.Tests.Theming;

/// <summary>
/// W6 contrast audit of the fixed palettes (WCAG 2.x relative-luminance ratio). Themes that use
/// the Windows system accent (Light/Dark/OLED) have no fixed accent to audit; the system picks
/// accents that are legible on its own surfaces.
/// </summary>
public sealed class ThemeContrastTests
{
    private static readonly GistTheme[] Fixed =
    {
        GistThemes.Light, GistThemes.Dark, GistThemes.Sepia, GistThemes.Oled,
    };

    public static IEnumerable<object[]> Themes() => Fixed.Select(t => new object[] { t.Selection });

    private static GistTheme For(ThemeSelection s) => Fixed.Single(t => t.Selection == s);

    [Theory]
    [MemberData(nameof(Themes))]
    public void Body_text_meets_WCAG_AAA_7_to_1(ThemeSelection selection)
    {
        var t = For(selection);
        Assert.True(Ratio(t.Foreground, t.Background) >= 7.0,
            $"{selection}: {t.Foreground} on {t.Background} = {Ratio(t.Foreground, t.Background):F2}:1");
    }

    [Fact]
    public void Sepia_fixed_accent_meets_AA_for_text()
    {
        var t = GistThemes.Sepia;
        Assert.NotNull(t.AccentHex);
        Assert.True(Ratio(t.AccentHex!, t.Background) >= 4.5,
            $"Sepia accent {t.AccentHex} on {t.Background} = {Ratio(t.AccentHex!, t.Background):F2}:1");
    }

    [Fact]
    public void Oled_background_is_true_black()
    {
        Assert.Equal("#000000", GistThemes.Oled.Background, ignoreCase: true);
    }

    [Fact]
    public void Ratio_matches_known_values()
    {
        Assert.Equal(21.0, Ratio("#000000", "#FFFFFF"), 1);
        Assert.Equal(1.0, Ratio("#777777", "#777777"), 3);
    }

    private static double Ratio(string a, string b)
    {
        var (hi, lo) = (Luminance(a), Luminance(b));
        if (hi < lo) (hi, lo) = (lo, hi);
        return (hi + 0.05) / (lo + 0.05);
    }

    private static double Luminance(string hex)
    {
        double Channel(int shift)
        {
            var c = Convert.ToInt32(hex.TrimStart('#'), 16) >> shift & 0xFF;
            var s = c / 255.0;
            return s <= 0.03928 ? s / 12.92 : Math.Pow((s + 0.055) / 1.055, 2.4);
        }

        return 0.2126 * Channel(16) + 0.7152 * Channel(8) + 0.0722 * Channel(0);
    }
}
