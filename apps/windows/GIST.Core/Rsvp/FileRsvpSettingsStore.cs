namespace Gist.Core.Rsvp;

/// <summary>
/// Remembers the reader's last WPM in a one-line text file under the app's data root (same
/// trade-off as <c>FileThemeSettingsStore</c>: no WinRT reference, so it stays headless-testable).
/// An unreadable or out-of-range value is not worth an error state: <see cref="LoadWpm"/> returns
/// <see langword="null"/> and the reader falls back to <see cref="RsvpWpm.Default"/>.
/// </summary>
public sealed class FileRsvpSettingsStore
{
    private readonly string _path;

    /// <param name="path">Absolute file path. The parent directory must already exist.</param>
    public FileRsvpSettingsStore(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        _path = path;
    }

    /// <summary>The saved WPM clamped into the engine's range, or null when absent or unparseable.</summary>
    public uint? LoadWpm()
    {
        try
        {
            if (!File.Exists(_path)) return null;
            var text = File.ReadAllText(_path).Trim();
            return uint.TryParse(text, System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out var wpm)
                ? RsvpWpm.Clamp(wpm)
                : null;
        }
        catch (IOException) { return null; }
        catch (UnauthorizedAccessException) { return null; }
    }

    /// <summary>Best-effort: a write failure must not interrupt reading.</summary>
    public void SaveWpm(uint wpm)
    {
        try
        {
            File.WriteAllText(_path, wpm.ToString(System.Globalization.CultureInfo.InvariantCulture));
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}
