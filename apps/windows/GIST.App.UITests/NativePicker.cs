using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using FlaUI.Core.Input;
using FlaUI.Core.WindowsAPI;

namespace Gist.App.UITests;

/// <summary>Drives the Windows common "Open" dialog that <c>FileOpenPicker</c> shows.</summary>
public static class NativePicker
{
    /// <summary>Finds the picker: a top-level "Open" dialog window (class #32770) on the desktop.</summary>
    private static Window? Find(GistAppSession s)
    {
        var desktop = s.Automation.GetDesktop();
        foreach (var w in desktop.FindAllChildren(cf => cf.ByControlType(ControlType.Window)))
        {
            string? cls = null, name = null;
            try { cls = w.ClassName; name = w.Name; } catch (Exception) { continue; }
            if (cls == "#32770" && name == "Open") return w.AsWindow();
        }
        // Owned dialogs can also appear as modal children of the app window.
        try
        {
            var modal = s.Window.ModalWindows.FirstOrDefault(m => m.ClassName == "#32770");
            if (modal is not null) return modal;
        }
        catch (Exception) { }
        return null;
    }

    public static Window WaitForPicker(GistAppSession s) =>
        LibraryDriver.Poll(() => Find(s), "native Open dialog", TimeSpan.FromSeconds(20));

    public static void WaitGone(GistAppSession s) =>
        LibraryDriver.PollUntil(() => Find(s) is null, "native Open dialog closed", TimeSpan.FromSeconds(15));

    public static void Cancel(Window picker)
    {
        var cancel = picker.FindFirstDescendant(cf => cf.ByControlType(ControlType.Button).And(cf.ByName("Cancel")));
        if (cancel is not null) LibraryDriver.Activate(cancel);
        else { picker.Focus(); Keyboard.Type(VirtualKeyShort.ESCAPE); }
    }

    /// <summary>Types a full path into the "File name" box and presses Open.</summary>
    public static void PickFile(Window picker, string path)
    {
        var name = LibraryDriver.Poll(() =>
            picker.FindFirstDescendant(cf => cf.ByAutomationId("1148"))   // the standard File name edit control id
            ?? picker.FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit).And(cf.ByName("File name:"))),
            "picker file name box");
        var edit = name.ControlType == ControlType.Edit ? name : name.FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit)) ?? name;
        edit.Focus();
        edit.AsTextBox().Text = path;
        var open = LibraryDriver.Poll(() => picker.FindFirstDescendant(cf => cf.ByControlType(ControlType.Button).And(cf.ByAutomationId("1"))),
            "picker Open button");
        LibraryDriver.Activate(open);
    }
}
