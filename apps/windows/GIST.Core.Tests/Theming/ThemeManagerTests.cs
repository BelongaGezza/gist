using Gist.Core.Tests.TestSupport;
using Gist.Core.Theming;
using Xunit;

namespace Gist.Core.Tests.Theming;

/// <summary>
/// Windows counterpart of Apple's <c>ThemeManagerTests</c>: OS-follow resolution, explicit
/// selections overriding it, persistence, and OLED-vs-Dark distinctness — all against the
/// deterministic <see cref="FakeThemeSystemProvider"/> seam rather than the real OS appearance.
/// </summary>
public sealed class ThemeManagerTests
{
    [Fact]
    public void SystemSelectionFollowsLightWhenOsIsLight()
    {
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), new FakeThemeSystemProvider(initialIsSystemDark: false));

        Assert.Equal(ThemeSelection.System, manager.Selection);
        Assert.Equal(GistThemes.Light, manager.Resolved);
    }

    [Fact]
    public void SystemSelectionFollowsDarkWhenOsIsDark()
    {
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), new FakeThemeSystemProvider(initialIsSystemDark: true));

        Assert.Equal(GistThemes.Dark, manager.Resolved);
    }

    [Fact]
    public void ResolvedTracksTheOsWhileFollowingSystem()
    {
        var system = new FakeThemeSystemProvider(initialIsSystemDark: false);
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), system);
        Assert.Equal(GistThemes.Light, manager.Resolved);

        system.SetSystemDark(true);
        Assert.Equal(GistThemes.Dark, manager.Resolved);

        system.SetSystemDark(false);
        Assert.Equal(GistThemes.Light, manager.Resolved);
    }

    [Theory]
    [InlineData(ThemeSelection.Light)]
    [InlineData(ThemeSelection.Dark)]
    [InlineData(ThemeSelection.Sepia)]
    [InlineData(ThemeSelection.Oled)]
    public void ExplicitSelectionOverridesTheOs(ThemeSelection selection)
    {
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), new FakeThemeSystemProvider())
        {
            Selection = selection,
        };

        Assert.Equal(selection, manager.Resolved.Selection);
    }

    [Theory]
    [InlineData(ThemeSelection.Sepia)]
    [InlineData(ThemeSelection.Oled)]
    public void SepiaAndOledNeverFollowTheOs(ThemeSelection selection)
    {
        var system = new FakeThemeSystemProvider(initialIsSystemDark: false);
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), system) { Selection = selection };
        var before = manager.Resolved;

        system.SetSystemDark(true);

        Assert.Equal(before, manager.Resolved);
    }

    [Fact]
    public void SelectionPersistsAcrossAFreshInstanceOnTheSameStore()
    {
        var store = new InMemoryThemeSettingsStore();
        _ = new ThemeManager(store, new FakeThemeSystemProvider()) { Selection = ThemeSelection.Sepia };

        var reopened = new ThemeManager(store, new FakeThemeSystemProvider());

        Assert.Equal(ThemeSelection.Sepia, reopened.Selection);
        Assert.Equal(1, store.SaveCount);
    }

    [Fact]
    public void OledAndDarkAreDistinctPalettes()
    {
        Assert.NotEqual(GistThemes.Oled.Background, GistThemes.Dark.Background);
        Assert.Equal("#000000", GistThemes.Oled.Background);
        Assert.False(GistThemes.Oled.UseMica);
        Assert.True(GistThemes.Dark.UseMica);
    }

    [Fact]
    public void SelectionChangeRaisesPropertyChangedForBothProperties()
    {
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), new FakeThemeSystemProvider());
        var seen = new List<string?>();
        manager.PropertyChanged += (_, e) => seen.Add(e.PropertyName);

        manager.Selection = ThemeSelection.Dark;

        Assert.Contains(nameof(ThemeManager.Selection), seen);
        Assert.Contains(nameof(ThemeManager.Resolved), seen);
    }

    [Fact]
    public void SettingTheSameSelectionDoesNotReSaveOrRenotify()
    {
        var store = new InMemoryThemeSettingsStore();
        var manager = new ThemeManager(store, new FakeThemeSystemProvider()) { Selection = ThemeSelection.Dark };
        var savesAfterFirstSet = store.SaveCount;
        var seen = new List<string?>();
        manager.PropertyChanged += (_, e) => seen.Add(e.PropertyName);

        manager.Selection = ThemeSelection.Dark;

        Assert.Equal(savesAfterFirstSet, store.SaveCount);
        Assert.Empty(seen);
    }

    [Fact]
    public void SystemThemeChangeIsIgnoredOnceAnExplicitSelectionIsChosen()
    {
        var system = new FakeThemeSystemProvider(initialIsSystemDark: false);
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), system) { Selection = ThemeSelection.Light };
        var seen = new List<string?>();
        manager.PropertyChanged += (_, e) => seen.Add(e.PropertyName);

        system.SetSystemDark(true);

        Assert.Equal(GistThemes.Light, manager.Resolved);
        Assert.Empty(seen);
    }

    [Fact]
    public void DisposeDetachesFromTheSystemProvider()
    {
        var system = new FakeThemeSystemProvider(initialIsSystemDark: false);
        var manager = new ThemeManager(new InMemoryThemeSettingsStore(), system);
        manager.Dispose();

        // Selection stays System, so if Dispose had not detached, this would re-resolve to Dark.
        system.SetSystemDark(true);

        Assert.Equal(GistThemes.Light, manager.Resolved);
    }
}
