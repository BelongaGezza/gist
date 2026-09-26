namespace Gist.Core.Theming;

/// <summary>
/// Plain-file <see cref="IThemeSettingsStore"/>: one line, one of the storage strings from
/// <see cref="ThemeSelectionExtensions"/>, under the app's data root — not the WinRT
/// <c>ApplicationData.LocalSettings</c> registry the spec names, so this assembly stays UI-free and
/// headless-testable (the same trade this codebase already made for <c>GistStoragePaths</c>: the
/// package's <c>LocalState</c> folder is resolved without a WinRT reference).
/// </summary>
/// <remarks>
/// Unlike the DPAPI key file, an unreadable or corrupt appearance preference is not a data-loss
/// event: <see cref="Load"/> returns <see langword="null"/> for anything it cannot parse, and the
/// caller falls back to <see cref="ThemeSelection.System"/>. There is nothing here worth a blocking
/// error state over.
/// </remarks>
public sealed class FileThemeSettingsStore : IThemeSettingsStore
{
    private readonly string _path;

    /// <param name="path">Absolute file path. The parent directory must already exist.</param>
    public FileThemeSettingsStore(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        _path = path;
    }

    /// <inheritdoc />
    public ThemeSelection? Load()
    {
        try
        {
            if (!File.Exists(_path)) return null;
            var text = File.ReadAllText(_path).Trim();
            return text.Length == 0 ? null : ThemeSelectionExtensions.ParseStorageString(text);
        }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    /// <inheritdoc />
    public void Save(ThemeSelection selection)
    {
        try
        {
            File.WriteAllText(_path, selection.ToStorageString());
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}
