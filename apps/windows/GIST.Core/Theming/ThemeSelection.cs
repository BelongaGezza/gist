namespace Gist.Core.Theming;

/// <summary>
/// The user's chosen appearance (<c>docs/windows-ui-spec.md</c> §3.1, §7.3). Windows counterpart of
/// Apple's <c>ThemeSelection</c> (<c>apps/apple/Shared/Theme.swift</c>) — same five cases, same
/// persisted string values, so a value written by one platform is never misread by the other.
/// </summary>
public enum ThemeSelection
{
    /// <summary>Resolves to Light or Dark from the OS setting (the default).</summary>
    System = 0,
    Light,
    Dark,
    Sepia,
    Oled,
}

/// <summary>Persisted-string conversions for <see cref="ThemeSelection"/>.</summary>
public static class ThemeSelectionExtensions
{
    /// <summary>The exact strings Apple persists (<c>system|light|dark|sepia|oled</c>).</summary>
    public static string ToStorageString(this ThemeSelection selection) => selection switch
    {
        ThemeSelection.System => "system",
        ThemeSelection.Light => "light",
        ThemeSelection.Dark => "dark",
        ThemeSelection.Sepia => "sepia",
        ThemeSelection.Oled => "oled",
        _ => "system",
    };

    /// <summary>
    /// Parses a persisted value. Anything unrecognised — including a blank string, a value written
    /// by a future version, or a corrupted file — resolves to <see cref="ThemeSelection.System"/>
    /// rather than failing: an unreadable appearance preference is not worth blocking the app over.
    /// </summary>
    public static ThemeSelection ParseStorageString(string? value) => value switch
    {
        "light" => ThemeSelection.Light,
        "dark" => ThemeSelection.Dark,
        "sepia" => ThemeSelection.Sepia,
        "oled" => ThemeSelection.Oled,
        _ => ThemeSelection.System,
    };
}
