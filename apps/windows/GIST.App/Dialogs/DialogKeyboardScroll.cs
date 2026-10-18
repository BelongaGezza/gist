using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Gist.App.Dialogs;

/// <summary>
/// The scrolling body of a Library dialog. Dialog bodies are capped in height (<paramref name="maxHeight"/>, in
/// effective pixels) and scroll beyond it; at large Windows text sizes that happens for ordinary content (for Encrypt,
/// the "cannot be recovered" warning sits below the fold). A plain <see cref="ScrollViewer"/> is not a tab stop and
/// dialog focus starts on a button, so the arrow and Page keys did nothing and a keyboard-only user could not read the
/// rest. This one becomes a tab stop exactly while it has something to scroll, so Tab reaches it and the arrow,
/// Page Up/Down, Home and End keys then scroll it.
/// </summary>
internal static class DialogKeyboardScroll
{
    public static ScrollViewer Body(UIElement content, double maxHeight, string dialogTitle)
    {
        var viewer = new ScrollViewer { Content = content, MaxHeight = maxHeight };

        // Narrator reads this when focus lands on it; the title says which dialog's text it is.
        AutomationProperties.SetName(viewer, dialogTitle + ", scrollable content");

        void Update() => viewer.IsTabStop = viewer.ScrollableHeight > 1;
        viewer.SizeChanged += (_, _) => Update();
        if (content is FrameworkElement element) element.SizeChanged += (_, _) => Update();
        viewer.LayoutUpdated += OnFirstLayout;

        void OnFirstLayout(object? sender, object e)
        {
            viewer.LayoutUpdated -= OnFirstLayout;
            Update();
        }

        return viewer;
    }
}
