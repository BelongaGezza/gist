using System.ComponentModel;
using Gist.Core.Client;
using Gist.Core.Filtering;
using Gist.Core.Models;
using Gist.Core.ViewModels;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.System;

namespace Gist.App.Views;

/// <summary>Display projection of a library item (title, authors, encrypted glyph). Never carries paths.</summary>
public sealed record LibraryRow(string Id, string Title, string Authors, bool Encrypted)
{
    public Visibility AuthorsVisibility => Authors.Length > 0 ? Visibility.Visible : Visibility.Collapsed;

    public Visibility LockVisibility => Encrypted ? Visibility.Visible : Visibility.Collapsed;

    /// <summary>Accessible name of the list row.</summary>
    public string AutomationName =>
        (Authors.Length > 0 ? $"{Title}, {Authors}" : Title) + (Encrypted ? ", encrypted" : string.Empty);

    public static LibraryRow From(LibraryItemVM item) => new(
        item.Id,
        string.IsNullOrWhiteSpace(item.Title) ? "Untitled" : item.Title,
        item.Authors.Count > 0 ? string.Join(", ", item.Authors) : string.Empty,
        item.ContentEncrypted);
}

public sealed partial class LibraryPage : Page
{
    private readonly CoreClient _core = AppServices.Core;
    private LibraryViewModel? _vm;
    private List<LibraryRow> _rows = new();
    private List<LibraryItemVM> _shownItems = new();
    private bool _sawLoadStart;
    private bool _loadedOnce;
    private bool _vmLoadRequested;
    private bool _syncingSelection;
    private bool _syncingSearch;
    private bool _updatePending;
    private bool _handlingDialog;

    public LibraryPage()
    {
        InitializeComponent();
        Loaded += OnPageLoaded;
        Unloaded += OnPageUnloaded;
        ItemsList.RightTapped += OnListRightTapped;
        // Escape must be seen even when the AutoSuggestBox's own text box handles it.
        SearchBox.AddHandler(KeyDownEvent, new KeyEventHandler(OnSearchKeyDown), true);
    }

    // ── Lifetime ───────────────────────────────────────────────────────────

    private void OnPageLoaded(object sender, RoutedEventArgs e)
    {
        _vm = new LibraryViewModel(_core);
        _vm.PropertyChanged += OnStateChanged;
        _core.PropertyChanged += OnStateChanged;
        SyncSortChecks();
        UpdateView();
    }

    private void OnPageUnloaded(object sender, RoutedEventArgs e)
    {
        _core.PropertyChanged -= OnStateChanged;
        if (_vm is null) return;
        _vm.PropertyChanged -= OnStateChanged;
        _vm.Dispose();
        _vm = null;
    }

    private void OnStateChanged(object? sender, PropertyChangedEventArgs e)
    {
        // View-model notifications can arrive on a thread-pool thread (search debounce), so marshal and coalesce.
        if (_updatePending) return;
        _updatePending = true;
        if (!DispatcherQueue.TryEnqueue(() =>
            {
                _updatePending = false;
                UpdateView();
            }))
        {
            _updatePending = false;
        }
    }

    // ── View state ─────────────────────────────────────────────────────────

    private async void OnRetry(object sender, RoutedEventArgs e)
    {
        _sawLoadStart = false;
        _loadedOnce = false;
        _vmLoadRequested = false;
        UpdateView();
        await AppServices.RetryAsync();
    }

    /// <summary>All UI text here is fixed or view-model text; raw error/exception text and paths are never shown.</summary>
    private void UpdateView()
    {
        var vm = _vm;
        if (vm is null) return;

        // The first list load counts as done once IsLoading has gone true and back to false while Ready.
        if (_core.State == CoreClientState.Ready)
        {
            if (!_vmLoadRequested)
            {
                _vmLoadRequested = true;
                _ = SafeAsync(vm.LoadAsync);
            }

            if (_core.IsLoading) _sawLoadStart = true;
            else if (_sawLoadStart) _loadedOnce = true;
        }

        var loading = false;
        var main = false;
        var corrupt = false;
        var error = false;

        switch (_core.State)
        {
            case CoreClientState.KeyStoreCorrupt:
                corrupt = true;
                break;
            case CoreClientState.KeyStoreUnavailable:
                error = true;
                ErrorTitle.Text = "Your encryption key is temporarily unavailable";
                ErrorDetail.Text = "Nothing has been changed. Try again.";
                break;
            case CoreClientState.StoreUnavailable:
                error = true;
                ErrorTitle.Text = "Your library couldn't be opened";
                ErrorDetail.Text = "Nothing has been changed. Try again.";
                break;
            case CoreClientState.Ready when !_loadedOnce:
                if (_core.LastError is not null && !_core.IsLoading && vm.PendingDialog == LibraryDialog.None)
                {
                    error = true;
                    ErrorTitle.Text = "Something went wrong";
                    ErrorDetail.Text = "Your library couldn't be loaded. Try again.";
                }
                else
                {
                    loading = true;
                }
                break;
            case CoreClientState.Ready:
                main = true;
                break;
            default:
                loading = true;
                break;
        }

        var emptyState = main ? vm.EmptyState : LibraryEmptyState.None;

        LoadingPanel.Visibility = Vis(loading);
        CorruptPanel.Visibility = Vis(corrupt);
        ErrorPanel.Visibility = Vis(error);
        SearchBox.Visibility = Vis(main);
        Commands.Visibility = Vis(main);
        ItemsList.Visibility = Vis(main && emptyState == LibraryEmptyState.None);
        EmptyPanel.Visibility = Vis(main && emptyState != LibraryEmptyState.None);

        if (main)
        {
            RebuildList(vm);
            UpdateEmptyState(vm, emptyState);
            UpdateCommands(vm);
            SyncSearchBox(vm);
        }

        if (vm.PendingDialog != LibraryDialog.None && !_handlingDialog)
        {
            _ = SafeAsync(() => RunPendingDialogAsync(vm));
        }
    }

    private static Visibility Vis(bool visible) => visible ? Visibility.Visible : Visibility.Collapsed;

    private void UpdateEmptyState(LibraryViewModel vm, LibraryEmptyState state)
    {
        if (state == LibraryEmptyState.None) return;
        EmptyTitle.Text = vm.EmptyTitle;
        EmptyMessage.Text = vm.EmptyMessage;
        EmptyIcon.Glyph = state switch
        {
            LibraryEmptyState.NoSearchHits => "",
            LibraryEmptyState.NoTagHits => "",
            _ => "",
        };
        EmptyImportButton.Visibility = Vis(state == LibraryEmptyState.NoItems);
    }

    private void UpdateCommands(LibraryViewModel vm)
    {
        ImportFileButton.IsEnabled = vm.IsImportEnabled;
        ImportUrlButton.IsEnabled = vm.IsImportEnabled;
        EmptyImportButton.IsEnabled = vm.IsImportEnabled;
        AddToCollectionButton.IsEnabled = vm.CanAddToCollection;
        EncryptButton.IsEnabled = vm.CanEncrypt;
        RemoveButton.IsEnabled = vm.CanRemove;
        OpenButton.IsEnabled = vm.CanOpen;
        TagsButton.IsEnabled = vm.CanEditTags;
        BusyRing.IsActive = vm.IsBusy;
        BusyRing.Visibility = Vis(vm.IsBusy);
        SyncSortChecks();
    }

    private void SyncSortChecks()
    {
        if (_vm is null) return;
        SortNewest.IsChecked = _vm.SortOrder == LibrarySortOrder.DateAddedNewest;
        SortOldest.IsChecked = _vm.SortOrder == LibrarySortOrder.DateAddedOldest;
        SortTitleAz.IsChecked = _vm.SortOrder == LibrarySortOrder.TitleAZ;
        SortTitleZa.IsChecked = _vm.SortOrder == LibrarySortOrder.TitleZA;
        SortAuthorAz.IsChecked = _vm.SortOrder == LibrarySortOrder.AuthorAZ;
    }

    private void SyncSearchBox(LibraryViewModel vm)
    {
        if (string.Equals(SearchBox.Text, vm.SearchText, StringComparison.Ordinal)) return;
        _syncingSearch = true;
        try { SearchBox.Text = vm.SearchText; }
        finally { _syncingSearch = false; }
    }

    // ── List and selection ─────────────────────────────────────────────────

    private void RebuildList(LibraryViewModel vm)
    {
        var items = vm.DisplayedItems;
        if (_shownItems.Count == items.Count && _shownItems.SequenceEqual(items)) return;

        _shownItems = items.ToList();
        _rows = _shownItems.Select(LibraryRow.From).ToList();

        _syncingSelection = true;
        try
        {
            ItemsList.ItemsSource = _rows;
            var wanted = new HashSet<string>(vm.SelectedIds, StringComparer.Ordinal);
            foreach (var row in _rows.Where(r => wanted.Contains(r.Id)))
            {
                ItemsList.SelectedItems.Add(row);
            }
        }
        finally
        {
            _syncingSelection = false;
        }

        PushSelection();
    }

    private void OnSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_syncingSelection) PushSelection();
    }

    private void PushSelection() =>
        _vm?.SetSelection(ItemsList.SelectedItems.OfType<LibraryRow>().Select(r => r.Id).ToList());

    private void OnContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (args.Phase != 0 || args.Item is not LibraryRow row) return;
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(args.ItemContainer, "LibraryItem_" + row.Id);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(args.ItemContainer, row.AutomationName);
    }

    private void OnListRightTapped(object sender, RightTappedRoutedEventArgs e)
    {
        // Right-click on an unselected row selects it first (spec 4.1).
        for (var node = e.OriginalSource as DependencyObject; node is not null; node = VisualTreeHelper.GetParent(node))
        {
            if (node is not ListViewItem { Content: LibraryRow row } container) continue;
            if (!container.IsSelected)
            {
                ItemsList.SelectedItems.Clear();
                ItemsList.SelectedItems.Add(row);
            }
            return;
        }
    }

    private void OnRowMenuOpening(object? sender, object e)
    {
        var vm = _vm;
        if (vm is null || vm.SelectionCount == 0)
        {
            RowMenu.Hide();
            return;
        }
        RowEncrypt.Visibility = Vis(vm.SelectedItems.Any(i => !i.ContentEncrypted));
    }

    private void OnListDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if (e.OriginalSource is FrameworkElement { DataContext: LibraryRow } && _vm is { CanOpen: true })
        {
            OnOpen(sender, e);
        }
    }

    private void OnListKeyDown(object sender, KeyRoutedEventArgs e)
    {
        var vm = _vm;
        if (vm is null) return;
        switch (e.Key)
        {
            case VirtualKey.Enter when vm.CanOpen:
                e.Handled = true;
                OnOpen(sender, e);
                break;
            case VirtualKey.Delete when vm.CanRemove:
                e.Handled = true;
                vm.RequestDialog(LibraryDialog.RemoveConfirm);
                break;
        }
    }

    // ── Search ─────────────────────────────────────────────────────────────

    private void OnSearchTextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
    {
        if (_syncingSearch || args.Reason != AutoSuggestionBoxTextChangeReason.UserInput || _vm is null) return;
        _vm.SearchText = sender.Text;
    }

    private void OnSearchKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key != VirtualKey.Escape || _vm is null) return;
        e.Handled = true;
        _syncingSearch = true;
        try { SearchBox.Text = string.Empty; }
        finally { _syncingSearch = false; }
        _vm.SearchText = string.Empty;
        ItemsList.Focus(FocusState.Keyboard);
    }

    private void OnFocusSearch(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        if (SearchBox.Visibility == Visibility.Visible) SearchBox.Focus(FocusState.Keyboard);
    }

    // ── Commands ───────────────────────────────────────────────────────────

    private async void OnImportFile(object sender, RoutedEventArgs e) => await SafeAsync(async () =>
    {
        var vm = _vm;
        var window = AppServices.MainWindow;
        if (vm is null || window is null || !vm.IsImportEnabled) return;
        var path = await AppServices.DialogHost.PickImportFileAsync(window);
        if (!string.IsNullOrWhiteSpace(path)) await vm.ImportFileAsync(path);
        // Any failure dialog (DRM / error) is raised through vm.PendingDialog and shown by UpdateView.
    });

    private void OnImportUrl(object sender, RoutedEventArgs e) => _vm?.RequestDialog(LibraryDialog.ImportUrl);

    private void OnEncrypt(object sender, RoutedEventArgs e)
    {
        if (_vm is { CanEncrypt: true }) _vm.RequestDialog(LibraryDialog.EncryptConfirm);
    }

    private void OnRemove(object sender, RoutedEventArgs e)
    {
        if (_vm is { CanRemove: true }) _vm.RequestDialog(LibraryDialog.RemoveConfirm);
    }

    private void OnTags(object sender, RoutedEventArgs e)
    {
        if (_vm is { CanEditTags: true }) _vm.RequestDialog(LibraryDialog.TagEditor);
    }

    // TODO(W4): open the RSVP reader. Until then Open / Open in Reader / Open in Flow View are a notice.
    private async void OnOpen(object sender, RoutedEventArgs e) => await ShowComingSoonAsync();

    private async void OnOpenFlow(object sender, RoutedEventArgs e) => await ShowComingSoonAsync();

    private async Task ShowComingSoonAsync() => await SafeAsync(async () =>
    {
        if (_handlingDialog || XamlRoot is null) return;
        _handlingDialog = true;
        try
        {
            await new ContentDialog
            {
                Title = "Coming in a later update",
                Content = "The reader isn't available yet.",
                CloseButtonText = "OK",
                XamlRoot = XamlRoot,
            }.ShowAsync();
        }
        finally
        {
            _handlingDialog = false;
        }
    });

    private void OnSortClick(object sender, RoutedEventArgs e)
    {
        if (_vm is null || sender is not FrameworkElement { Tag: string tag }) return;
        if (Enum.TryParse<LibrarySortOrder>(tag, out var order)) _vm.SortOrder = order;
    }

    private void OnFilterFlyoutOpening(object? sender, object e)
    {
        var vm = _vm;
        if (vm is null) return;
        FilterFlyout.Items.Clear();

        var all = new RadioMenuFlyoutItem { Text = "All Tags", GroupName = "TagFilter", IsChecked = vm.TagFilter is null };
        all.Click += (_, _) => vm.TagFilter = null;
        FilterFlyout.Items.Add(all);

        foreach (var tag in vm.AllTags)
        {
            var item = new RadioMenuFlyoutItem
            {
                Text = tag,
                GroupName = "TagFilter",
                IsChecked = string.Equals(vm.TagFilter, tag, StringComparison.Ordinal),
            };
            var captured = tag;
            item.Click += (_, _) => vm.TagFilter = captured;
            FilterFlyout.Items.Add(item);
        }
    }

    private void OnCollectionFlyoutOpening(object? sender, object e)
    {
        var vm = _vm;
        if (vm is null) return;
        CollectionFlyout.Items.Clear();

        foreach (var collection in vm.Collections)
        {
            var item = new MenuFlyoutItem { Text = collection.Name };
            var captured = collection;
            item.Click += async (_, _) => await SafeAsync(() => vm.AddToCollectionAsync(captured));
            CollectionFlyout.Items.Add(item);
        }

        if (vm.Collections.Count > 0) CollectionFlyout.Items.Add(new MenuFlyoutSeparator());
        var create = new MenuFlyoutItem { Text = "New Collection…" };
        create.Click += (_, _) => vm.RequestDialog(LibraryDialog.NewCollection);
        CollectionFlyout.Items.Add(create);
    }

    // ── Dialogs ────────────────────────────────────────────────────────────

    private async Task RunPendingDialogAsync(LibraryViewModel vm)
    {
        if (_handlingDialog || XamlRoot is null) return;
        _handlingDialog = true;
        try
        {
            // The host shows the dialog, drives the VM operation and dismisses. Re-check once in case the
            // operation it ran raised a follow-up (e.g. Encrypt -> result), but never loop on a stuck dialog.
            for (var i = 0; i < 3 && vm.PendingDialog != LibraryDialog.None; i++)
            {
                var before = vm.PendingDialog;
                await AppServices.DialogHost.HandleAsync(vm, XamlRoot);
                if (vm.PendingDialog == before) break;
            }
        }
        finally
        {
            _handlingDialog = false;
        }
    }

    /// <summary>Runs UI-thread async work; failures must never crash the app and their text is never shown.</summary>
    private static async Task SafeAsync(Func<Task> work)
    {
        try
        {
            await work();
        }
        catch (Exception)
        {
            // Deliberately swallowed: view-model operations report failures through PendingDialog/LastError.
        }
    }
}
