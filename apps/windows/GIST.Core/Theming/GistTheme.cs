namespace Gist.Core.Theming;

/// <summary>
/// The base Fluent theme (Light or Dark) a <see cref="GistTheme"/> forces on chrome — WinUI's
/// <c>ElementTheme</c>, kept as a Core-neutral enum since this assembly must not reference WinUI.
/// </summary>
public enum GistBaseTheme
{
    Light,
    Dark,
}

/// <summary>
/// One resolved appearance: the concrete colours a <see cref="ThemeSelection"/> maps to, plus
/// whether the shell may show Mica behind it. Windows counterpart of Apple's resolved <c>Theme</c>
/// (<c>apps/apple/Shared/Theme.swift</c>) — same background/foreground/accent-per-case table
/// (<c>docs/windows-ui-spec.md</c> §3.1).
/// </summary>
/// <param name="Selection">Which case this is (never <see cref="ThemeSelection.System"/> — see <see cref="GistThemes.Resolve"/>).</param>
/// <param name="Background">Hex background colour (<c>#RRGGBB</c>).</param>
/// <param name="Foreground">Hex foreground colour (<c>#RRGGBB</c>).</param>
/// <param name="AccentHex">
/// A fixed accent colour, or <see langword="null"/> when the theme should use the Windows system
/// accent colour instead (spec §3.1 "Accent [W]": Light/Dark/OLED follow the user's system accent;
/// only Sepia keeps a fixed brown so its warm palette is never broken by a stray blue control).
/// </param>
/// <param name="Base">Which Fluent base theme this forces on chrome (buttons, text boxes, scrollbars).</param>
/// <param name="UseMica">
/// Whether the shell window may show a Mica/Acrylic backdrop behind this theme. False for Sepia and
/// OLED (spec §2: "Sepia and OLED use solid theme colours everywhere. OLED must be true #000000;
/// Mica would tint it").
/// </param>
public sealed record GistTheme(
    ThemeSelection Selection,
    string Background,
    string Foreground,
    string? AccentHex,
    GistBaseTheme Base,
    bool UseMica);
