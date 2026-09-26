using Gist.Core.ViewModels;
using Microsoft.UI.Xaml;

namespace Gist.App.Views;

/// <summary>
/// The Library page's view of the dialog layer. Same members as the real
/// <c>Gist.App.Dialogs.LibraryDialogHost</c> (delivered separately); the page depends only on this.
/// </summary>
internal interface ILibraryDialogHost
{
    /// <summary>Shows whatever <c>vm.PendingDialog</c> asks for, drives the VM operations, and dismisses.</summary>
    Task HandleAsync(LibraryViewModel vm, XamlRoot xamlRoot);

    /// <summary>The Collection screen's counterpart (§5): fewer dialogs, different removal wording.</summary>
    Task HandleAsync(CollectionViewModel vm, XamlRoot xamlRoot);

    /// <summary>Shows the file picker (.txt .epub .docx); null when cancelled.</summary>
    Task<string?> PickImportFileAsync(Window window);
}
