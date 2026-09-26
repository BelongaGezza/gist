namespace Gist.Core.Theming;

/// <summary>
/// Persists the user's <see cref="ThemeSelection"/>. Windows counterpart of Apple's
/// <c>UserDefaults</c>-backed persistence (spec §3.1: <c>ApplicationData.LocalSettings</c>) — kept
/// behind an interface, like <see cref="IThemeSystemProvider"/>, purely for the test seam.
/// </summary>
public interface IThemeSettingsStore
{
    /// <summary>The persisted selection, or <see langword="null"/> when nothing has been saved yet or it could not be read.</summary>
    ThemeSelection? Load();

    /// <summary>Persists <paramref name="selection"/>. Best-effort: a write failure must not crash the app.</summary>
    void Save(ThemeSelection selection);
}
