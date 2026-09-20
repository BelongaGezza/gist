using Gist.App.Views;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Gist.App;

public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();

        // Custom title bar; falls back to the default one where unsupported.
        if (AppWindowTitleBar.IsCustomizationSupported())
        {
            ExtendsContentIntoTitleBar = true;
            SetTitleBar(AppTitleBar);
        }
        else
        {
            AppTitleBar.Visibility = Visibility.Collapsed;
        }

        // Mica, falling back to Desktop Acrylic, then to the solid theme background (no backdrop).
        // TODO(W3): Sepia/OLED must use solid colours, not Mica (spec section 2).
        if (MicaController.IsSupported())
            SystemBackdrop = new MicaBackdrop();
        else if (DesktopAcrylicController.IsSupported())
            SystemBackdrop = new DesktopAcrylicBackdrop();

        AppWindow.Resize(new Windows.Graphics.SizeInt32(1100, 720));
        Nav.SelectedItem = LibraryItem;
    }

    private void OnSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItemContainer is not NavigationViewItem item) return;
        switch (item.Tag as string)
        {
            case "library":
                ContentFrame.Navigate(typeof(LibraryPage));
                break;
            case "appearance":
                // TODO(W3): open the Appearance dialog. Placeholder for now.
                ContentFrame.Navigate(typeof(PlaceholderPage), "Appearance");
                break;
        }
    }
}
