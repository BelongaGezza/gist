using System.ComponentModel;
using Gist.Core.Filtering;
using Gist.Core.Models;
using Gist.Core.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Navigation;
using Windows.System;

namespace Gist.App.Views;

/// <summary>
/// One collection's contents (<c>docs/windows-ui-spec.md</c> §5) — same row template as
/// <see cref="LibraryPage"/>, driven by <see cref="CollectionViewModel"/>. Navigated to with a
/// <see cref="CollectionVM"/> as the <see cref="Frame"/> parameter.
/// </summary>
public sealed partial class CollectionPage : Page
{
    private CollectionViewModel? _vm;
    private List<LibraryRow> _rows = new();
    private List<LibraryItemVM> _shownItems = new();
    private bool _syncingSelection;
    private bool _updatePending;
    private bool _handlingDialog;

    public CollectionPage()
    {
        InitializeComponent();
        Loaded += OnPageLoaded;
        Unloaded += OnPageUnloaded;
        ItemsList.AddHandler(KeyDownEvent, new KeyEventHandler(OnListKeyDown), true);
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (e.Parameter is not CollectionVM collection) return;

        DetachVm();
        _vm = new CollectionViewModel(AppServices.Core, collection);
        _vm.PropertyChanged += OnStateChanged;
        TitleText.Text = _vm.Title;
        SyncSortChecks();
        UpdateView();
        _ = SafeAsync(_vm.LoadAsync);
    }

    private void OnPageLoaded(object sender, RoutedEventArgs e)
    {
        // Re-render on navigation back to an already-loaded instance (Frame caching), and covers the
        // ordinary case where OnNavigatedTo already ran before Loaded fires.
        if (_vm is not null) UpdateView();
    }

    private void OnPageUnloaded(object sender, RoutedEventArgs e) => DetachVm();

    private void DetachVm()
    {
        if (_vm is null) return;
        _vm.PropertyChanged -= OnStateChanged;
        _vm.Dispose();
        _vm = null;
    }

    private void OnStateChanged(object? sender, PropertyChangedEventArgs e)
    {
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

    private static Visibility Vis(bool visible) => visible ? Visibility.Visible : Visibility.Collapsed;

    private void UpdateView()
    {
        var vm = _vm;
        if (vm is null) return;

        var loading = vm.IsLoading && _shownItems.Count == 0 && vm.EmptyState == LibraryEmptyState.None;
        var emptyState = loading ? LibraryEmptyState.None : vm.EmptyState;

        LoadingPanel.Visibility = Vis(loading);
        Commands.Visibility = Vis(!loading);
        ItemsList.Visibility = Vis(!loading && emptyState == LibraryEmptyState.None);
        EmptyPanel.Visibility = Vis(!loading && emptyState != LibraryEmptyState.None);

        if (!loading)
        {
            RebuildList(vm);
            if (emptyState != LibraryEmptyState.None)
            {
                EmptyTitle.Text = vm.EmptyTitle;
                EmptyMessage.Text = vm.EmptyMessage;
            }

            UpdateCommands(vm);
        }

        if (vm.PendingDialog != LibraryDialog.None && !_handlingDialog)
        {
            _ = SafeAsync(() => RunPendingDialogAsync(vm));
        }
    }

    private void UpdateCommands(CollectionViewModel vm)
    {
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

    private void RebuildList(CollectionViewModel vm)
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
        // Same "LibraryItem_" prefix as LibraryPage (not "CollectionItem_"): FlaUI's LibraryDriver.Rows()
        // finds rows by this prefix regardless of which page is showing, and there is exactly one
        // list on screen at a time, so reusing it here needs no driver changes.
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(args.ItemContainer, "LibraryItem_" + row.Id);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(args.ItemContainer, row.AutomationName);
    }

    private void OnRowMenuOpening(object? sender, object e)
    {
        if (_vm is null || _vm.SelectionCount == 0) RowMenu.Hide();
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

    private void OnSortClick(object sender, RoutedEventArgs e)
    {
        if (_vm is null || sender is not FrameworkElement { Tag: string tag }) return;
        if (Enum.TryParse<LibrarySortOrder>(tag, out var order)) _vm.SortOrder = order;
    }

    private void OnRemove(object sender, RoutedEventArgs e)
    {
        if (_vm is { CanRemove: true }) _vm.RequestDialog(LibraryDialog.RemoveConfirm);
    }

    private void OnTags(object sender, RoutedEventArgs e)
    {
        if (_vm is { CanEditTags: true }) _vm.RequestDialog(LibraryDialog.TagEditor);
    }

    // TODO(W4/W5): open the RSVP/Flow reader. Until then this is a notice, same as LibraryPage.
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

    private async Task RunPendingDialogAsync(CollectionViewModel vm)
    {
        if (_handlingDialog || XamlRoot is null) return;
        _handlingDialog = true;
        try
        {
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
            if (_vm is { PendingDialog: not LibraryDialog.None }) DispatcherQueue.TryEnqueue(() => UpdateView());
        }
    }

    private static async Task SafeAsync(Func<Task> work)
    {
        try
        {
            await work();
        }
        catch (Exception)
        {
            // Deliberately swallowed: view-model operations report failures through PendingDialog.
        }
    }
}
