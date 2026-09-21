using Gist.Core.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Storage.Pickers;
using WinRT.Interop;

namespace Gist.App.Views;

/// <summary>
/// TODO(W2): temporary stand-in until <c>Gist.App.Dialogs.LibraryDialogHost</c> lands; swap it in
/// <see cref="AppServices.DialogHost"/> and delete this file. Real picker, but every dialog is a bare
/// notice that just dismisses the pending request.
/// </summary>
internal sealed class StubLibraryDialogHost : ILibraryDialogHost
{
    public async Task HandleAsync(LibraryViewModel vm, XamlRoot xamlRoot)
    {
        if (vm.PendingDialog == LibraryDialog.None) return;

        var dialog = new ContentDialog
        {
            Title = "Coming in a later update",
            Content = "This dialog isn't available yet.",
            CloseButtonText = "OK",
            XamlRoot = xamlRoot,
        };
        await dialog.ShowAsync();
        await vm.DismissDialogAsync();
    }

    public async Task<string?> PickImportFileAsync(Window window)
    {
        var picker = new FileOpenPicker();
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(window));
        picker.FileTypeFilter.Add(".txt");
        picker.FileTypeFilter.Add(".epub");
        picker.FileTypeFilter.Add(".docx");
        var file = await picker.PickSingleFileAsync();
        return file?.Path;
    }
}
