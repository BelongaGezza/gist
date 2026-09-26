using CommunityToolkit.Mvvm.ComponentModel;

namespace Gist.Core.Theming;

/// <summary>
/// The single source of truth for the app's appearance. Windows counterpart of Apple's
/// <c>ThemeManager</c> (<c>apps/apple/Shared/Theme.swift</c>): tracks the persisted
/// <see cref="Selection"/>, resolves it against the OS appearance, and republishes
/// <see cref="Resolved"/> whenever either changes.
/// </summary>
/// <remarks>
/// <para>
/// <b>UI-free.</b> This assembly must not reference WinUI or <c>Windows.UI.ViewManagement</c>;
/// <see cref="IThemeSystemProvider"/> and <see cref="IThemeSettingsStore"/> are the seams the real
/// app supplies WinRT-backed implementations for, and tests supply deterministic ones — the same
/// <c>initialSystemIsDark</c> pattern Apple's <c>ThemeManagerTests</c> uses.
/// </para>
/// <para>
/// Selection is persisted on every change, applied instantly (there is no "Apply" step — the
/// shell reacts to <see cref="PropertyChanged"/>), and Sepia/OLED never move when the OS appearance
/// changes (spec §3.1) because <see cref="GistThemes.Resolve"/> ignores <c>isSystemDark</c> for
/// them; only re-resolving while <see cref="Selection"/> is <see cref="ThemeSelection.System"/>
/// would change anything, so <see cref="OnSystemThemeChanged"/> is a no-op otherwise.
/// </para>
/// </remarks>
public sealed class ThemeManager : ObservableObject, IDisposable
{
    private readonly IThemeSettingsStore _store;
    private readonly IThemeSystemProvider _system;
    private ThemeSelection _selection;
    private GistTheme _resolved;
    private bool _disposed;

    /// <param name="store">Where the selection is persisted.</param>
    /// <param name="system">Reports and announces the OS light/dark appearance.</param>
    public ThemeManager(IThemeSettingsStore store, IThemeSystemProvider system)
    {
        ArgumentNullException.ThrowIfNull(store);
        ArgumentNullException.ThrowIfNull(system);
        _store = store;
        _system = system;
        _selection = store.Load() ?? ThemeSelection.System;
        _resolved = GistThemes.Resolve(_selection, system.IsSystemDark);
        _system.Changed += OnSystemThemeChanged;
    }

    /// <summary>The user's chosen appearance. Setting it persists immediately and re-resolves.</summary>
    public ThemeSelection Selection
    {
        get => _selection;
        set
        {
            if (!SetProperty(ref _selection, value)) return;
            _store.Save(value);
            Resolve();
        }
    }

    /// <summary>The concrete colours/backdrop rule the shell should be showing right now.</summary>
    public GistTheme Resolved
    {
        get => _resolved;
        private set => SetProperty(ref _resolved, value);
    }

    private void OnSystemThemeChanged(object? sender, EventArgs e)
    {
        if (Selection == ThemeSelection.System) Resolve();
    }

    private void Resolve() => Resolved = GistThemes.Resolve(Selection, _system.IsSystemDark);

    /// <summary>Detaches from <see cref="IThemeSystemProvider.Changed"/>.</summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _system.Changed -= OnSystemThemeChanged;
    }
}
