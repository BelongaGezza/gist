using Gist.Core.Theming;
using Microsoft.UI.Dispatching;
using Windows.UI.ViewManagement;

namespace Gist.App.Theming;

/// <summary>
/// Real <see cref="IThemeSystemProvider"/>: detects the OS light/dark appearance via
/// <see cref="UISettings"/> (spec §3.1 "OS-follow detection"). There is no direct "is dark mode"
/// property pre-Windows 11 24H2, so this uses the standard WinUI trick of reading the system
/// background colour's perceptive luminance — the same approach <c>Application.RequestedTheme</c>
/// itself is derived from internally.
/// </summary>
public sealed class UiSettingsThemeSystemProvider : IThemeSystemProvider, IDisposable
{
    private readonly UISettings _uiSettings = new();
    private readonly DispatcherQueue _dispatcher;
    private bool _isSystemDark;
    private bool _disposed;

    /// <param name="dispatcher">
    /// The UI thread's queue. <see cref="UISettings.ColorValuesChanged"/> is raised off the UI
    /// thread, and <see cref="Changed"/> must only ever be raised on it (doc comment on
    /// <see cref="IThemeSystemProvider.Changed"/>).
    /// </param>
    public UiSettingsThemeSystemProvider(DispatcherQueue dispatcher)
    {
        ArgumentNullException.ThrowIfNull(dispatcher);
        _dispatcher = dispatcher;
        _isSystemDark = ComputeIsSystemDark();
        _uiSettings.ColorValuesChanged += OnColorValuesChanged;
    }

    /// <inheritdoc />
    public bool IsSystemDark => _isSystemDark;

    /// <inheritdoc />
    public event EventHandler? Changed;

    private void OnColorValuesChanged(UISettings sender, object args)
    {
        _dispatcher.TryEnqueue(() =>
        {
            var next = ComputeIsSystemDark();
            if (next == _isSystemDark) return;
            _isSystemDark = next;
            Changed?.Invoke(this, EventArgs.Empty);
        });
    }

    private bool ComputeIsSystemDark()
    {
        var background = _uiSettings.GetColorValue(UIColorType.Background);
        var luminance = (0.299 * background.R + 0.587 * background.G + 0.114 * background.B) / 255.0;
        return luminance < 0.5;
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        _uiSettings.ColorValuesChanged -= OnColorValuesChanged;
    }
}
