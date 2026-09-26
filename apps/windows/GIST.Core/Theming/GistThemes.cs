namespace Gist.Core.Theming;

/// <summary>
/// The fixed palette table (<c>docs/windows-ui-spec.md</c> §3.1), and resolution of
/// <see cref="ThemeSelection.System"/> against the current OS appearance.
/// </summary>
public static class GistThemes
{
    /// <summary>Light: Apple's default light appearance.</summary>
    public static readonly GistTheme Light = new(
        ThemeSelection.Light, Background: "#FFFFFF", Foreground: "#1A1A1A",
        AccentHex: null, Base: GistBaseTheme.Light, UseMica: true);

    /// <summary>Dark: Apple's default dark appearance.</summary>
    public static readonly GistTheme Dark = new(
        ThemeSelection.Dark, Background: "#1C1C1E", Foreground: "#F2F2F7",
        AccentHex: null, Base: GistBaseTheme.Dark, UseMica: true);

    /// <summary>Sepia: warm paper tones, fixed brown accent, never follows the OS.</summary>
    public static readonly GistTheme Sepia = new(
        ThemeSelection.Sepia, Background: "#F4ECD8", Foreground: "#5B4636",
        AccentHex: "#8B5A2B", Base: GistBaseTheme.Light, UseMica: false);

    /// <summary>OLED: true black so OLED panels can turn pixels fully off. Never follows the OS.</summary>
    public static readonly GistTheme Oled = new(
        ThemeSelection.Oled, Background: "#000000", Foreground: "#F2F2F7",
        AccentHex: null, Base: GistBaseTheme.Dark, UseMica: false);

    /// <summary>
    /// Resolves a selection to a concrete theme. <see cref="ThemeSelection.System"/> resolves to
    /// <see cref="Light"/> or <see cref="Dark"/> from <paramref name="isSystemDark"/>; every other
    /// case is explicit and ignores it (spec: "Sepia and OLED never participate in OS-follow").
    /// </summary>
    public static GistTheme Resolve(ThemeSelection selection, bool isSystemDark) => selection switch
    {
        ThemeSelection.System => isSystemDark ? Dark : Light,
        ThemeSelection.Light => Light,
        ThemeSelection.Dark => Dark,
        ThemeSelection.Sepia => Sepia,
        ThemeSelection.Oled => Oled,
        _ => isSystemDark ? Dark : Light,
    };
}
