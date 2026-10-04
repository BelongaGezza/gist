using System.ComponentModel;
using Gist.App.Dialogs;
using Gist.App.Theming;
using Gist.App.Views;
using Gist.Core.Models;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Gist.App;

public sealed partial class MainWindow : Window
{
    private readonly List<NavigationViewItem> _collectionNavItems = new();
    private NavigationViewItem? _lastContentSelection;
    private bool _syncingSelection;

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

        SetWindowIcon();

        AppWindow.Resize(new Windows.Graphics.SizeInt32(1100, 720));
        Nav.SelectedItem = LibraryItem;
        _lastContentSelection = LibraryItem;

        ApplyTheme();
        AppServices.Theme.PropertyChanged += OnThemePropertyChanged;
        AppServices.Core.PropertyChanged += OnCorePropertyChanged;
        RebuildCollectionsNav();
    }

    /// <summary>
    /// Gives the window a real Win32 icon from <c>Assets\GIST.ico</c>.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <c>&lt;ApplicationIcon&gt;</c> in the csproj only embeds the icon as the executable's Win32
    /// resource, which is what Explorer shows for the <em>file</em>. It does not give the window an
    /// <c>HICON</c>: WinUI 3 never sets one on its own. Verified on 2026-10-04 by querying a running
    /// debug build — <c>WM_GETICON</c> (SMALL/BIG/SMALL2) and <c>GetClassLongPtr</c>
    /// (<c>GCLP_HICON</c>/<c>GCLP_HICONSM</c>) all returned NULL. Without this call the taskbar and
    /// Alt+Tab fall back to the process image's icon, which happens to look right but is a fallback,
    /// not the window's own icon, and Task Manager and some shell surfaces do not apply it.
    /// </para>
    /// <para>
    /// The custom title bar renders <c>StoreLogo.png</c> itself (see MainWindow.xaml), so this is
    /// specifically about the taskbar/Alt+Tab/Task Manager icon, not the title-bar strip.
    /// </para>
    /// <para>
    /// Best-effort by design: a missing or unreadable icon degrades to the pre-existing
    /// process-image fallback rather than taking the whole window down on launch. The asset is
    /// copied next to the executable by the csproj, so the normal path is for it to be present.
    /// </para>
    /// </remarks>
    private void SetWindowIcon()
    {
        try
        {
            var path = Path.Combine(AppContext.BaseDirectory, "Assets", "GIST.ico");
            if (File.Exists(path))
            {
                AppWindow.SetIcon(path);
            }
        }
        catch (Exception)
        {
            // Cosmetic only — never block startup over the window icon.
        }
    }

    // ── Theme (W3, spec §3.1) ──────────────────────────────────────────────

    private void OnThemePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(Gist.Core.Theming.ThemeManager.Resolved)) return;
        DispatcherQueue.TryEnqueue(ApplyTheme);
    }

    private void ApplyTheme() => ThemeApplier.Apply(AppServices.Theme.Resolved, this);

    // ── Sidebar / navigation (spec §2, §5) ──────────────────────────────────

    private void OnCorePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(Gist.Core.Client.CoreClient.Collections)) return;
        DispatcherQueue.TryEnqueue(RebuildCollectionsNav);
    }

    /// <summary>
    /// Rebuilds the "Collections" section from <see cref="AppServices.Core"/>'s current list,
    /// hiding the header when there are none (spec §2). Never disturbs the current selection or
    /// navigated content — only the Library/Collection rows this method itself owns.
    /// </summary>
    private void RebuildCollectionsNav()
    {
        foreach (var item in _collectionNavItems)
        {
            Nav.MenuItems.Remove(item);
        }

        _collectionNavItems.Clear();

        var collections = AppServices.Core.Collections;
        CollectionsHeader.Visibility = collections.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

        var insertAt = Nav.MenuItems.IndexOf(CollectionsHeader) + 1;
        foreach (var collection in collections)
        {
            var item = new NavigationViewItem
            {
                Content = collection.Name,
                Icon = new SymbolIcon(Symbol.Folder),
                Tag = collection,
            };
            AutomationProperties.SetAutomationId(item, "Sidebar_Collection_" + collection.Id);
            Nav.MenuItems.Insert(insertAt, item);
            _collectionNavItems.Add(item);
            insertAt++;
        }
    }

    /// <summary>
    /// While the reader is open, clicking the sidebar entry that is already selected (Library or
    /// the collection the reader was opened from) is "go back to it": a selection change would not
    /// fire, so without this the click would silently do nothing.
    /// </summary>
    private void OnItemInvoked(NavigationView sender, NavigationViewItemInvokedEventArgs args)
    {
        if (ContentFrame.Content is RsvpPage
            && ReferenceEquals(args.InvokedItemContainer, Nav.SelectedItem)
            && ContentFrame.CanGoBack)
        {
            ContentFrame.GoBack();
        }
    }

    private async void OnSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (_syncingSelection) return;
        if (args.SelectedItemContainer is not NavigationViewItem item) return;

        if (ReferenceEquals(item, LibraryItem))
        {
            _lastContentSelection = item;
            ContentFrame.Navigate(typeof(LibraryPage));
            ContentFrame.BackStack.Clear();
            return;
        }

        if (item.Tag is CollectionVM collection)
        {
            _lastContentSelection = item;
            ContentFrame.Navigate(typeof(CollectionPage), collection);
            ContentFrame.BackStack.Clear();
            return;
        }

        if (item.Tag as string == "appearance")
        {
            // Appearance is a dialog, not a navigation destination (spec §2): the sidebar selection
            // visually moves to it on click, so put it back once the dialog closes rather than
            // leaving "Appearance" highlighted with stale content still showing underneath.
            await AppearanceDialog.ShowAsync(AppServices.Theme, Content.XamlRoot);
            _syncingSelection = true;
            try
            {
                Nav.SelectedItem = _lastContentSelection;
            }
            finally
            {
                _syncingSelection = false;
            }
        }
    }
}
