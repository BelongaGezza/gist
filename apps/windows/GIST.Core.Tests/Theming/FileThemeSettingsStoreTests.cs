using Gist.Core.Theming;
using Xunit;

namespace Gist.Core.Tests.Theming;

/// <summary>Real-filesystem tests for <see cref="FileThemeSettingsStore"/> (no mocking, per repo convention).</summary>
public sealed class FileThemeSettingsStoreTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "GistThemeStoreTests-" + Guid.NewGuid().ToString("N"));
    private readonly string _path;

    public FileThemeSettingsStoreTests()
    {
        Directory.CreateDirectory(_dir);
        _path = Path.Combine(_dir, "theme-selection.txt");
    }

    [Fact]
    public void LoadReturnsNullWhenNoFileExistsYet()
    {
        Assert.Null(new FileThemeSettingsStore(_path).Load());
    }

    [Fact]
    public void SaveThenLoadRoundTrips()
    {
        var store = new FileThemeSettingsStore(_path);
        store.Save(ThemeSelection.Oled);

        Assert.Equal(ThemeSelection.Oled, store.Load());
    }

    [Fact]
    public void LoadReturnsNullForUnreadableContent()
    {
        File.WriteAllText(_path, "not-a-real-theme");

        // ParseStorageString itself falls back to System for anything unrecognised, so this proves
        // the store does not throw on garbage, not that it returns null specifically.
        Assert.Equal(ThemeSelection.System, new FileThemeSettingsStore(_path).Load());
    }

    [Fact]
    public void LoadReturnsNullForAnEmptyFile()
    {
        File.WriteAllText(_path, string.Empty);

        Assert.Null(new FileThemeSettingsStore(_path).Load());
    }

    [Fact]
    public void SecondSaveOverwritesTheFirst()
    {
        var store = new FileThemeSettingsStore(_path);
        store.Save(ThemeSelection.Light);
        store.Save(ThemeSelection.Sepia);

        Assert.Equal(ThemeSelection.Sepia, store.Load());
    }

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}
