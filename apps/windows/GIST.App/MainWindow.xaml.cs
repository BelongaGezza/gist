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

        AppWindow.Resize(new Windows.Graphics.SizeInt32(1100, 720));
        Nav.SelectedItem = LibraryItem;
        _lastContentSelection = LibraryItem;

        ApplyTheme();
        AppServices.Theme.PropertyChanged += OnThemePropertyChanged;
        AppServices.Core.PropertyChanged += OnCorePropertyChanged;
        RebuildCollectionsNav();
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
