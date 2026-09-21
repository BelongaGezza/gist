using System.Diagnostics;
using FlaUI.Core.AutomationElements;
using FlaUI.Core.Definitions;
using FlaUI.Core.Input;
using FlaUI.Core.WindowsAPI;

namespace Gist.App.UITests;

/// <summary>
/// FlaUI helpers over the Library page: everything is located by AutomationId or accessible name and every wait
/// polls with a deadline (no fixed sleeps except where commented).
/// </summary>
public sealed class LibraryDriver
{
    public static readonly TimeSpan Wait = TimeSpan.FromSeconds(20);

    public GistAppSession Session { get; }
    public Window Window => Session.Window;

    public LibraryDriver(GistAppSession session)
    {
        Session = session;
        try { Window.Patterns.Window.Pattern.SetWindowVisualState(WindowVisualState.Maximized); }
        catch (Exception) { /* keep the default size; overflow handling below copes */ }
        FocusWindow();
    }

    public void FocusWindow()
    {
        try { Window.SetForeground(); } catch (Exception) { }
        try { Window.Focus(); } catch (Exception) { }
    }

    // ── Polling ────────────────────────────────────────────────────────────

    public static T Poll<T>(Func<T?> probe, string what, TimeSpan? timeout = null) where T : class
    {
        var deadline = DateTime.UtcNow + (timeout ?? Wait);
        while (true)
        {
            try { var v = probe(); if (v is not null) return v; } catch (Exception) { /* tree changing */ }
            if (DateTime.UtcNow >= deadline) throw new TimeoutException($"Timed out waiting for: {what}");
            Thread.Sleep(100);
        }
    }

    public static void PollUntil(Func<bool> probe, string what, TimeSpan? timeout = null)
    {
        var deadline = DateTime.UtcNow + (timeout ?? Wait);
        while (true)
        {
            try { if (probe()) return; } catch (Exception) { /* tree changing */ }
            if (DateTime.UtcNow >= deadline) throw new TimeoutException($"Timed out waiting for: {what}");
            Thread.Sleep(100);
        }
    }

    /// <summary>Asserts something stays true for a short window (used for "nothing happened" checks).</summary>
    public static void StaysTrue(Func<bool> probe, string what, TimeSpan window)
    {
        var deadline = DateTime.UtcNow + window;
        while (DateTime.UtcNow < deadline)
        {
            Assert.True(probe(), what);
            Thread.Sleep(100);
        }
    }

    // ── Elements ───────────────────────────────────────────────────────────

    public AutomationElement? ById(string id) =>
        Window.FindFirstDescendant(cf => cf.ByAutomationId(id));

    public AutomationElement Need(string id) => Poll(() => ById(id), "element " + id);

    public AutomationElement? ByName(string name, ControlType? type = null) =>
        Window.FindFirstDescendant(cf => type is { } t ? cf.ByName(name).And(cf.ByControlType(t)) : cf.ByName(name));

    public bool HasText(string name) => ByName(name) is not null;

    public void WaitForLibrary() => Need("LibraryPage_List");

    // ── Rows ───────────────────────────────────────────────────────────────

    public AutomationElement[] Rows() =>
        Window.FindAllDescendants(cf => cf.ByControlType(ControlType.ListItem))
            .Where(e => (Try(() => e.AutomationId) ?? "").StartsWith("LibraryItem_", StringComparison.Ordinal))
            .ToArray();

    public static string RowTitle(AutomationElement row) =>
        row.FindFirstDescendant(cf => cf.ByControlType(ControlType.Text))?.Name ?? "";

    public string[] Titles() => Rows().Select(RowTitle).ToArray();

    public void WaitForTitles(Func<string[], bool> ok, string what) =>
        PollUntil(() => ok(Titles()), what);

    public AutomationElement Row(string title) => Poll(() => Rows().FirstOrDefault(r => RowTitle(r) == title), "row " + title);

    public string[] SelectedTitles() =>
        Rows().Where(r => r.Patterns.SelectionItem.Pattern.IsSelected.Value).Select(RowTitle).ToArray();

    /// <summary>Selects exactly the given titles via UIA SelectionItem (independent of click geometry).</summary>
    public void Select(params string[] titles)
    {
        var first = true;
        foreach (var t in titles)
        {
            var p = Row(t).Patterns.SelectionItem.Pattern;
            if (first) p.Select(); else p.AddToSelection();
            first = false;
        }
        PollUntil(() => SelectedTitles().OrderBy(x => x).SequenceEqual(titles.OrderBy(x => x)), "selection " + string.Join("|", titles));
    }

    public void ClickRow(string title)
    {
        // UIA select + SetFocus rather than a mouse click: keeps keyboard focus on the row without needing synthetic mouse input.
        var row = Row(title);
        row.Patterns.SelectionItem.Pattern.Select();
        row.Focus();
    }

    // ── Commands ───────────────────────────────────────────────────────────

    /// <summary>Finds a command bar button, expanding the overflow menu when the bar collapsed it.</summary>
    public AutomationElement Command(string id)
    {
        var direct = ById(id);
        if (direct is not null && !direct.Properties.IsOffscreen.ValueOrDefault) return direct;
        OpenOverflow();
        return Poll(() => ById(id), "command " + id);
    }

    private void OpenOverflow()
    {
        var more = ById("MoreButton");
        if (more is null) return;
        try { more.Patterns.ExpandCollapse.Pattern.Expand(); } catch (Exception) { more.Click(); }
    }

    public void CloseOverflowIfOpen()
    {
        if (ById("MoreButton") is { } more)
        {
            try
            {
                if (more.Patterns.ExpandCollapse.IsSupported &&
                    more.Patterns.ExpandCollapse.Pattern.ExpandCollapseState.Value == ExpandCollapseState.Expanded)
                    more.Patterns.ExpandCollapse.Pattern.Collapse();
            }
            catch (Exception) { }
        }
    }

    public bool CommandEnabled(string id)
    {
        var e = Command(id);
        var enabled = e.Properties.IsEnabled.Value;
        CloseOverflowIfOpen();
        return enabled;
    }

    public void InvokeCommand(string id)
    {
        var e = Command(id);
        if (e.Patterns.Invoke.IsSupported) e.Patterns.Invoke.Pattern.Invoke(); else e.Click();
    }

    // ── Dialogs / menus ────────────────────────────────────────────────────

    /// <summary>
    /// The open ContentDialog, found as the Window element carrying its title. Scoping every dialog query to this
    /// element matters: the page underneath stays in the UIA tree, so e.g. "Encrypt" names both the command and the
    /// dialog's primary button.
    /// </summary>
    public AutomationElement Dialog(string title) =>
        Poll(() =>
        {
            var dlg = Window.FindAllDescendants(cf => cf.ByControlType(ControlType.Window).And(cf.ByName(title))).FirstOrDefault();
            // The Window element appears before its content is populated; wait for a body line and a button so
            // callers can read the dialog without racing the template.
            if (dlg is null) return null;
            var hasBody = dlg.FindAllDescendants()
                .Any(t => Try(() => t.ControlType) is ControlType.Text or ControlType.Edit
                    && (Try(() => t.Name) ?? "").Length > 0 && Try(() => t.Name) != title);
            var hasButton = dlg.FindFirstDescendant(cf => cf.ByControlType(ControlType.Button)) is not null;
            return hasBody && hasButton ? dlg : null;
        }, "dialog titled '" + title + "'");

    public void WaitDialogGone(string title)
    {
        PollUntil(() => Window.FindAllDescendants(cf => cf.ByControlType(ControlType.Window).And(cf.ByName(title))).Length == 0,
            "dialog closed: " + title);
        // Unavoidable fixed settle: WinUI finishes closing a ContentDialog asynchronously after it disappears from the
        // UIA tree, and the app's dialog host swallows the "only one ContentDialog may be open" error and drops a
        // request made in that window (found by this suite; no UIA-visible signal exists for "fully closed").
        Thread.Sleep(600);
    }

    public static AutomationElement DialogPart(AutomationElement dialog, string id) =>
        Poll(() => dialog.FindFirstDescendant(cf => cf.ByAutomationId(id)), "dialog part " + id);

    /// <summary>
    /// Activates an element through its UIA patterns (Invoke, SelectionItem, Toggle, ExpandCollapse) and only falls
    /// back to a synthetic mouse click when none applies. Patterns are deterministic and do not depend on the pointer
    /// or the element being on screen.
    /// </summary>
    public static void Activate(AutomationElement e)
    {
        if (e.Patterns.Invoke.IsSupported) e.Patterns.Invoke.Pattern.Invoke();
        else if (e.Patterns.SelectionItem.IsSupported) e.Patterns.SelectionItem.Pattern.Select();
        else if (e.Patterns.Toggle.IsSupported) e.Patterns.Toggle.Pattern.Toggle();
        else if (e.Patterns.ExpandCollapse.IsSupported) e.Patterns.ExpandCollapse.Pattern.Expand();
        else e.Click();
    }

    /// <summary>Waits until the dialog with this title shows text satisfying <paramref name="ok"/>; returns that text.</summary>
    public string[] WaitTexts(string title, Func<string[], bool> ok, string what) =>
        Poll(() =>
        {
            var dlg = Window.FindAllDescendants(cf => cf.ByControlType(ControlType.Window).And(cf.ByName(title))).FirstOrDefault();
            if (dlg is null) return null;
            var t = DialogTexts(dlg);
            return ok(t) ? t : null;
        }, $"{what} in dialog '{title}'");

    public void CloseFlyout(string buttonId)
    {
        try
        {
            var b = ById(buttonId);
            if (b is not null && b.Patterns.ExpandCollapse.IsSupported) { b.Patterns.ExpandCollapse.Pattern.Collapse(); return; }
        }
        catch (Exception) { }
        try { Keyboard.Type(VirtualKeyShort.ESCAPE); } catch (Exception) { }
    }

    /// <summary>Clicks PrimaryButton / SecondaryButton / CloseButton and returns its label (for assertions).</summary>
    public string ClickPart(AutomationElement dialog, string partId)
    {
        var b = DialogPart(dialog, partId);
        var name = b.Name;
        Activate(b);
        return name;
    }

    /// <summary>Every text-ish name inside a dialog (titles, body lines, InfoBar messages).</summary>
    public static string[] DialogTexts(AutomationElement dialog) =>
        dialog.FindAllDescendants().Select(e => Try(() => e.Name) ?? "").Where(n => n.Length > 0).ToArray();

    /// <summary>Opens a command-bar flyout button and activates one of its menu items by name.</summary>
    public void ChooseFromFlyout(string buttonId, string itemName)
    {
        InvokeCommand(buttonId);
        var item = Poll(() => Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem).And(cf.ByName(itemName)))
            .FirstOrDefault(), $"menu item '{itemName}'");
        Activate(item);
    }

    public string[] FlyoutItems(string buttonId)
    {
        InvokeCommand(buttonId);
        PollUntil(() => Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem)).Any(m => (Try(() => m.AutomationId) ?? "") == "" && (Try(() => m.Name) ?? "") != "System"),
            "flyout items");
        var names = Window.FindAllDescendants(cf => cf.ByControlType(ControlType.MenuItem))
            .Select(m => Try(() => m.Name) ?? "").Where(n => n.Length > 0 && n != "System").ToArray();
        CloseFlyout(buttonId);
        return names;
    }

    public string SearchText() =>
        Need("LibraryPage_SearchBox").FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit))?.Patterns.Value.Pattern.Value.Value ?? "";

    // ── Keyboard ───────────────────────────────────────────────────────────

    public void Chord(VirtualKeyShort modifier, VirtualKeyShort key)
    {
        FocusWindow();
        Input(() => Keyboard.TypeSimultaneously(modifier, key));
    }

    public void Press(VirtualKeyShort key)
    {
        FocusWindow();
        Input(() => Keyboard.Type(key));
    }

    public void Type(string text) => Input(() => Keyboard.Type(text));

    /// <summary>
    /// Real key events need an unlocked interactive desktop. When Windows refuses SendInput (locked workstation,
    /// disconnected RDP session, secure desktop) say so plainly instead of leaving a bare Win32 "Access is denied".
    /// </summary>
    private static void Input(Action send)
    {
        try { send(); }
        catch (System.ComponentModel.Win32Exception e) when (e.NativeErrorCode == 5)
        {
            throw new InvalidOperationException(
                "Synthetic keyboard input was rejected (Access is denied). This test needs an unlocked interactive "
                + "desktop; unlock the workstation (or use a connected console session) and re-run.", e);
        }
    }

    /// <summary>
    /// Sets the search box text through UIA (focus + ValuePattern) - the same text-changed path as typing, without
    /// synthetic keys. Used where typing is incidental; the real-typing path is covered by its own keyboard test.
    /// </summary>
    public void SetSearch(string text)
    {
        var edit = Poll(() => Need("LibraryPage_SearchBox").FindFirstDescendant(cf => cf.ByControlType(ControlType.Edit)), "search edit");
        edit.Focus();
        edit.Patterns.Value.Pattern.SetValue(text);
    }

    /// <summary>The AutomationId of the focused element or, failing that, of its nearest ancestor that has one.</summary>
    public string FocusedId()
    {
        AutomationElement? f;
        try { f = Session.Automation.FocusedElement(); }
        catch (FlaUI.Core.Exceptions.ElementNotAvailableException) { return ""; } // focus is between elements right now
        for (var e = f; e is not null; e = Try(() => e.Parent))
        {
            var id = Try(() => e.AutomationId);
            if (!string.IsNullOrEmpty(id) && id.StartsWith("LibraryPage_", StringComparison.Ordinal)) return id;
            if (!string.IsNullOrEmpty(id) && id.StartsWith("LibraryItem_", StringComparison.Ordinal)) return "LibraryPage_List";
            if (Try(() => e.ControlType) == ControlType.Window) break;
        }
        return "";
    }

    public void WaitFocus(string id)
    {
        try { PollUntil(() => FocusedId() == id, $"focus on {id}"); }
        catch (TimeoutException)
        {
            string chain;
            try
            {
                var parts = new List<string>();
                for (var e = Session.Automation.FocusedElement(); e is not null && parts.Count < 8; e = Try(() => e.Parent))
                    parts.Add($"{Try(() => e.ControlType)}:{Try(() => e.AutomationId)}:{Try(() => e.Name)}");
                chain = string.Join(" <- ", parts);
            }
            catch (Exception ex) { chain = "unavailable: " + ex.GetType().Name; }
            throw new TimeoutException($"Timed out waiting for focus on {id}; focused chain: {chain}");
        }
    }

    private static T? Try<T>(Func<T> f) { try { return f(); } catch (Exception) { return default; } }
}
