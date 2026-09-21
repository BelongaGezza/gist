using Microsoft.UI.Xaml;
using Windows.Storage.Pickers;
using WinRT.Interop;

namespace Gist.App.Dialogs;

/// <summary>File picker for the Import command (spec §4.1).</summary>
public static class ImportFilePicker
{
    /// <summary>
    /// Lets the user pick a .txt, .epub or .docx file. Initialised with the window handle so it
    /// works both unpackaged and packaged.
    /// </summary>
    /// <returns>The chosen path, or null if cancelled or the picker failed.</returns>
    public static async Task<string?> PickImportFileAsync(Window window)
    {
        ArgumentNullException.ThrowIfNull(window);
        try
        {
            var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary };
            picker.FileTypeFilter.Add(".txt");
            picker.FileTypeFilter.Add(".epub");
            picker.FileTypeFilter.Add(".docx");
            InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(window));
            var file = await picker.PickSingleFileAsync();
            return file?.Path;
        }
        catch (Exception)
        {
            return null;
        }
    }
}
