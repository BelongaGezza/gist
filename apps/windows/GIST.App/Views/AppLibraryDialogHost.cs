using Gist.App.Dialogs;
using Gist.Core.ViewModels;
using Microsoft.UI.Xaml;

namespace Gist.App.Views;

/// <summary>Adapts the real <see cref="LibraryDialogHost"/> to the page's <see cref="ILibraryDialogHost"/>.</summary>
internal sealed class AppLibraryDialogHost : ILibraryDialogHost
{
    private readonly LibraryDialogHost _inner = new();

    public Task HandleAsync(LibraryViewModel vm, XamlRoot xamlRoot) => _inner.HandleAsync(vm, xamlRoot);

    public Task<string?> PickImportFileAsync(Window window) => LibraryDialogHost.PickImportFileAsync(window);
}
