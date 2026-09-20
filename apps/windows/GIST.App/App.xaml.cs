using Microsoft.UI.Xaml;

namespace Gist.App;

public partial class App : Application
{
    private Window? _window;

    public App() => InitializeComponent();

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // Composition root: build services once, before any window exists.
        AppServices.Initialize();
        _window = new MainWindow();
        _window.Activate();
    }
}
