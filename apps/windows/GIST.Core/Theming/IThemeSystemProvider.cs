namespace Gist.Core.Theming;

/// <summary>
/// Whatever <see cref="ThemeManager"/> needs to know about the OS appearance, kept behind an
/// interface so this UI-free assembly never references <c>Windows.UI.ViewManagement</c> and tests
/// can drive it deterministically — the same seam as Apple's <c>initialSystemIsDark</c>
/// (<c>ThemeManagerTests</c>).
/// </summary>
public interface IThemeSystemProvider
{
    /// <summary>Whether the OS is currently in a dark appearance.</summary>
    bool IsSystemDark { get; }

    /// <summary>Raised when <see cref="IsSystemDark"/> changes. Never raised off the UI thread by the real provider.</summary>
    event EventHandler? Changed;
}
