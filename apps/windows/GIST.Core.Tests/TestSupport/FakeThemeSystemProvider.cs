using Gist.Core.Theming;

namespace Gist.Core.Tests.TestSupport;

/// <summary>
/// Deterministic <see cref="IThemeSystemProvider"/> for tests — the <c>initialSystemIsDark</c> seam
/// Apple's <c>ThemeManagerTests</c> uses, so OS-follow resolution never depends on the test
/// machine's real appearance.
/// </summary>
public sealed class FakeThemeSystemProvider : IThemeSystemProvider
{
    public FakeThemeSystemProvider(bool initialIsSystemDark = false)
    {
        IsSystemDark = initialIsSystemDark;
    }

    public bool IsSystemDark { get; private set; }

    public event EventHandler? Changed;

    /// <summary>Simulates the OS appearance changing, raising <see cref="Changed"/> exactly as the real provider would.</summary>
    public void SetSystemDark(bool isDark)
    {
        if (IsSystemDark == isDark) return;
        IsSystemDark = isDark;
        Changed?.Invoke(this, EventArgs.Empty);
    }
}

/// <summary>An in-memory <see cref="IThemeSettingsStore"/> for tests that don't need real file persistence.</summary>
public sealed class InMemoryThemeSettingsStore : IThemeSettingsStore
{
    private ThemeSelection? _value;

    public int SaveCount { get; private set; }

    public ThemeSelection? Load() => _value;

    public void Save(ThemeSelection selection)
    {
        SaveCount++;
        _value = selection;
    }
}
