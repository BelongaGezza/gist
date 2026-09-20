using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;

namespace WinUiHello;

public sealed partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        var path = Path.Combine(AppContext.BaseDirectory, "gist_ffi.dll");
        var ok = NativeLibrary.TryLoad(path, out var h);
        var msg = ok ? "gist_ffi.dll: loaded" : "gist_ffi.dll: NOT loaded (" + path + ")";
        if (ok) NativeLibrary.Free(h);
        FfiStatus.Text = msg;
        // Headless verification hook: write the result so a script can read it.
        var outFile = Environment.GetEnvironmentVariable("GIST_HELLO_OUT");
        if (!string.IsNullOrEmpty(outFile)) File.WriteAllText(outFile, msg);
    }
}
