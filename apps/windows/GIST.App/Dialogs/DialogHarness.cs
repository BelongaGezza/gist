using Gist.Core.ViewModels;
using Microsoft.UI.Xaml;

namespace Gist.App.Dialogs;

/// <summary>
/// Dev/test only. When both <c>GIST_DATA_ROOT</c> (scratch store) and <c>GIST_DIALOG_HARNESS</c>
/// (a <see cref="LibraryDialog"/> name) are set, opens that dialog on the first library item at
/// startup so UI Automation can verify it without the Library page. Inert otherwise; the product
/// never sets either variable.
/// </summary>
internal static class DialogHarness
{
    public const string EnvVar = "GIST_DIALOG_HARNESS";

    public static async void RunIfRequested(Window window)
    {
        var name = Environment.GetEnvironmentVariable(EnvVar);
        if (string.IsNullOrWhiteSpace(name)
            || string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable(AppServices.DataRootEnvVar))
            || !Enum.TryParse<LibraryDialog>(name, out var dialog)
            || dialog == LibraryDialog.None)
        {
            return;
        }

        try
        {
            await AppServices.StartupTask;
            using var vm = new LibraryViewModel(AppServices.Core);
            await vm.LoadAsync();
            if (vm.DisplayedItems.Count > 0)
            {
                vm.SetSelection(new[] { vm.DisplayedItems[0].Id });
            }

            XamlRoot? root = null;
            for (var i = 0; i < 50 && root is null; i++)
            {
                root = window.Content?.XamlRoot;
                if (root is null)
                {
                    await Task.Delay(100);
                }
            }

            if (root is null)
            {
                return;
            }

            vm.RequestDialog(dialog);
            await new LibraryDialogHost().HandleAsync(vm, root);
        }
        catch (Exception)
        {
            // Dev-only hook: swallow, never surface.
        }
    }
}
