using System.ComponentModel;
using Gist.Core.Client;
using Gist.Core.Models;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Gist.App.Views;

/// <summary>Display projection of a library item (title, authors, encrypted glyph). Never carries paths.</summary>
public sealed record LibraryRow(string Title, string Authors, Visibility LockVisibility)
{
    public static LibraryRow From(LibraryItemVM item) => new(
        item.Title,
        item.Authors.Count > 0 ? string.Join(", ", item.Authors) : "Unknown author",
        item.ContentEncrypted ? Visibility.Visible : Visibility.Collapsed);

    // Used by UI Automation / screen readers as the list item's accessible name.
    public override string ToString() =>
        LockVisibility == Visibility.Visible ? $"{Title}, {Authors}, encrypted" : $"{Title}, {Authors}";
}

public sealed partial class LibraryPage : Page
{
    private readonly CoreClient _core = AppServices.Core;
    private bool _sawLoadStart;
    private bool _loadedOnce;

    public LibraryPage()
    {
        InitializeComponent();
        Loaded += (_, _) =>
        {
            _core.PropertyChanged += OnCorePropertyChanged;
            UpdateView();
        };
        Unloaded += (_, _) => _core.PropertyChanged -= OnCorePropertyChanged;
    }

    private void OnCorePropertyChanged(object? sender, PropertyChangedEventArgs e) => UpdateView();

    private async void OnRetry(object sender, RoutedEventArgs e)
    {
        _sawLoadStart = false;
        _loadedOnce = false;
        UpdateView();
        await AppServices.RetryAsync();
    }

    /// <summary>All UI text here is fixed; raw error/exception text and paths are never shown.</summary>
    private void UpdateView()
    {
        // The first list load counts as done once IsLoading has gone true and back to false while Ready.
        if (_core.State == CoreClientState.Ready)
        {
            if (_core.IsLoading) _sawLoadStart = true;
            else if (_sawLoadStart) _loadedOnce = true;
        }

        var loading = false;
        var list = false;
        var empty = false;
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
                if (_core.LastError is not null && !_core.IsLoading)
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
                if (_core.Items.Count > 0)
                {
                    list = true;
                    ItemsList.ItemsSource = _core.Items.Select(LibraryRow.From).ToList();
                }
                else
                {
                    empty = true;
                }
                break;
            default:
                loading = true;
                break;
        }

        LoadingPanel.Visibility = loading ? Visibility.Visible : Visibility.Collapsed;
        ItemsList.Visibility = list ? Visibility.Visible : Visibility.Collapsed;
        EmptyPanel.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        CorruptPanel.Visibility = corrupt ? Visibility.Visible : Visibility.Collapsed;
        ErrorPanel.Visibility = error ? Visibility.Visible : Visibility.Collapsed;
    }
}
