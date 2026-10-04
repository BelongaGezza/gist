using System.ComponentModel;
using Gist.Core.Flow;
using Gist.Core.Theming;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using VirtualKey = Windows.System.VirtualKey;
using Windows.UI;
using Windows.UI.ViewManagement;

namespace Gist.App.Views;

/// <summary>Frame parameter for <see cref="FlowPage"/>.</summary>
public sealed record FlowNavigationArgs(string ItemId, string Title);

/// <summary>One Contents row: the heading text and its indent (16 epx per level below h1).</summary>
public sealed record TocRow(string Title, Thickness Indent, int EntryIndex);

/// <summary>
/// The flow reader (<c>docs/windows-ui-spec.md</c> §7.2). The document model, find, typography, progress and the
/// position store are UI-free (<c>Gist.Core.Flow</c>); this page owns only rendering, input and focus. Blocks are
/// shown in a virtualising <see cref="ListView"/> (an <see cref="ItemsStackPanel"/>: only on/near-screen blocks
/// have containers) and built in code per container, so a very long document costs what is on screen.
/// </summary>
public sealed partial class FlowPage : Page, IReadingLayout
{
    private const double LineScrollEpx = 48;
    private const double PageScrollViewportFraction = 0.9;

    private FlowNavigationArgs? _args;
    private FlowDocument? _doc;
    private List<FlowBlockEntry> _items = [];
    private TypographyState _typography = new();
    private FlowSearchState _search = new();
    private ReadingProgress _progress = new();
    private FlowRenderContext? _ctx;
    private ScrollViewer? _scroll;
    private DispatcherQueueTimer? _findTimer;
    private DispatcherQueueTimer? _saveTimer;
    private bool _themeHooked;
    private bool _left;
    private bool _bound;
    private bool _restoring;
    private bool _progressQueued;
    private int _scrollToken;

    public FlowPage()
    {
        InitializeComponent();
        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _args = e.Parameter as FlowNavigationArgs;
    }

    protected override void OnNavigatingFrom(NavigatingCancelEventArgs e)
    {
        base.OnNavigatingFrom(e);
        Leave();
    }

    // ── Lifetime ───────────────────────────────────────────────────────────

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        if (_bound || _args is null) return;
        _bound = true;

        var title = string.IsNullOrWhiteSpace(_args.Title) ? "Untitled" : _args.Title;
        TitleText.Text = title;
        AutomationProperties.SetName(RootGrid, title); // the Narrator name for the page is the item title

        if (!_themeHooked)
        {
            AppServices.Theme.PropertyChanged += OnThemeChanged;
            _themeHooked = true;
        }
        ApplyTheme();

        try
        {
            var json = await AppServices.Core.GetDocumentJsonAsync(_args.ItemId);
            if (_left) return;
            if (json is null)
            {
                ShowMessage("This item couldn't be opened for reading.");
                return;
            }

            var result = await Task.Run(() => FlowDocumentDecoder.Decode(json));
            if (_left) return;
            if (result.Document is null)
            {
                ShowMessage("This item couldn't be opened for reading.");
                return;
            }
            if (result.Document.Entries.Count == 0)
            {
                ShowMessage("This item has no readable text.");
                return;
            }

            _typography = new TypographyState(AppServices.FlowTypography.Load());
            var progress = new ReadingProgress(AppServices.FlowPositions.Load(_args.ItemId));
            var search = new FlowSearchState();
            search.SetDocument(result.Document);
            Bind(new ReadingLayoutContext(result.Document, _typography, search, new SectionNavigator(result.Document), progress));
            await RestoreAsync();
        }
        catch (Exception)
        {
            // Every failure path shows fixed text; exception text is never surfaced (F50).
            if (!_left) ShowMessage("This item couldn't be opened for reading.");
        }
    }

    private void OnUnloaded(object sender, RoutedEventArgs e) => Leave();

    /// <summary>Persists the position and unhooks everything. Runs once.</summary>
    private void Leave()
    {
        if (_left) return;
        _left = true;
        if (_themeHooked)
        {
            AppServices.Theme.PropertyChanged -= OnThemeChanged;
            _themeHooked = false;
        }
        _findTimer?.Stop();
        _saveTimer?.Stop();
        if (_scroll is not null) _scroll.ViewChanged -= OnViewChanged;
        _typography.Changed -= OnTypographyChanged;

        // A position is only worth keeping once the restore finished; saving mid-restore would store a
        // half-scrolled value and lose the real one.
        if (_args is not null && _doc is not null && !_restoring)
        {
            AppServices.FlowPositions.Save(_args.ItemId, _progress.Fraction);
        }
    }

    private void ShowMessage(string text)
    {
        LoadingPanel.Visibility = Visibility.Collapsed;
        FlowList.Visibility = Visibility.Collapsed;
        MessageText.Text = text;
        MessagePanel.Visibility = Visibility.Visible;
        BackButton.Focus(FocusState.Programmatic);
    }

    // ── IReadingLayout ─────────────────────────────────────────────────────

    /// <summary>Binds the scrolling flow layout to its context (the seam a paginated layout would also implement).</summary>
    public void Bind(ReadingLayoutContext context)
    {
        _doc = context.Document;
        _typography = context.Typography;
        _search = context.Search;
        _progress = context.Progress;
        _items = [.. context.Document.Entries];

        _typography.Changed += OnTypographyChanged;
        _progress.Changed += OnProgressChanged;

        TocList.ItemsSource = context.Document.TableOfContents
            .Select(t => new TocRow(t.Title, new Thickness(16 * t.IndentLevel, 0, 0, 0), t.EntryIndex))
            .ToList();
        ContentsButton.IsEnabled = context.Document.TableOfContents.Count > 0;
        TypographyButton.IsEnabled = true;
        FindToggle.IsEnabled = true;
        UpdateTypographyUi();
        RebuildContext();

        LoadingPanel.Visibility = Visibility.Collapsed;
        // Kept in the tree (so it realises and can scroll) but invisible until the saved position is in place:
        // the reader never sees the document at the top and then jump.
        FlowList.Opacity = 0;
        FlowList.Visibility = Visibility.Visible;
        FlowList.ItemsSource = _items;
    }

    private async Task RestoreAsync()
    {
        _restoring = true;
        try
        {
            FlowList.UpdateLayout();
            _scroll = FindScrollViewer(FlowList);
            var target = ReadingProgress.BlockForFraction(_progress.InitialFraction, _items.Count);
            if (target > 0) await ScrollToEntryCoreAsync(target, ++_scrollToken);
        }
        finally
        {
            _restoring = false;
            FlowList.Opacity = 1;
            ProgressRow.Visibility = Visibility.Visible;
            if (_scroll is not null) _scroll.ViewChanged += OnViewChanged;
            UpdateProgressFromScroll();
            UpdateProgressUi();
            FocusDocument();
        }
    }

    /// <summary>
    /// Puts keyboard focus in the document (the first on-screen block) so Home/End/PgUp/PgDn, Ctrl+F and F3 work
    /// straight away; the page-level key handlers only run when focus is somewhere on the page.
    /// </summary>
    private void FocusDocument()
    {
        var first = FirstVisibleIndex;
        if (first < 0) first = 0;
        if (FlowList.ContainerFromIndex(first) is Control container) container.Focus(FocusState.Programmatic);
        else BackButton.Focus(FocusState.Programmatic);
    }

    private static ScrollViewer? FindScrollViewer(DependencyObject root)
    {
        var count = VisualTreeHelper.GetChildrenCount(root);
        for (var i = 0; i < count; i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is ScrollViewer sv) return sv;
            if (FindScrollViewer(child) is { } nested) return nested;
        }
        return null;
    }

    // ── Rendering ──────────────────────────────────────────────────────────

    private void OnFlowContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (args.ItemContainer is not ListViewItem container) return;
        if (args.InRecycleQueue)
        {
            if (container.ContentTemplateRoot is Border recycled) recycled.Child = null;
            return;
        }
        RenderContainer(container, args.ItemIndex);
    }

    private void RenderContainer(ListViewItem container, int index)
    {
        if (_ctx is null || index < 0 || index >= _items.Count) return;
        if (container.ContentTemplateRoot is not Border host) return;
        var block = _items[index].Block;
        host.Child = FlowBlockRenderer.Build(index, block, _ctx);
        AutomationProperties.SetAutomationId(container, "FlowBlock_" + index.ToString(System.Globalization.CultureInfo.InvariantCulture));
        AutomationProperties.SetName(container, FlowBlockRenderer.AutomationNameFor(block));
    }

    /// <summary>Re-renders every realised block (typography, theme or highlights changed).</summary>
    private void RefreshRealized()
    {
        if (FlowList.ItemsPanelRoot is not Panel panel) return;
        foreach (var child in panel.Children)
        {
            if (child is ListViewItem container)
            {
                var index = FlowList.IndexFromContainer(container);
                if (index >= 0) RenderContainer(container, index);
            }
        }
    }

    private void RebuildContext()
    {
        var hc = new AccessibilitySettings().HighContrast;
        var resources = Application.Current.Resources;

        Brush? foreground = null, secondary = null, border = null, header = null;
        if (!hc)
        {
            foreground = (Brush)resources["GistForeground"];
            secondary = (Brush)resources["GistSecondaryText"];
            border = secondary is SolidColorBrush s ? new SolidColorBrush(s.Color) { Opacity = 0.5 } : secondary;
            header = resources["GistAccent"] is SolidColorBrush a ? new SolidColorBrush(a.Color) { Opacity = 0.15 } : null;
        }

        Brush matchBg, currentBg, matchFg;
        if (hc)
        {
            matchBg = new SolidColorBrush(SystemColor("SystemColorHighlightColor", Colors.Yellow));
            matchFg = new SolidColorBrush(SystemColor("SystemColorHighlightTextColor", Colors.Black));
            currentBg = new SolidColorBrush(SystemColor("SystemColorHighlightTextColor", Colors.Black));
        }
        else
        {
            // Warm yellow for matches and orange for the current one, dark text on both: legible on every
            // GIST theme (the highlight sits on its own opaque swatch, not on the page background).
            matchBg = new SolidColorBrush(Color.FromArgb(255, 255, 224, 138));
            currentBg = new SolidColorBrush(Color.FromArgb(255, 255, 159, 67));
            matchFg = new SolidColorBrush(Colors.Black);
        }

        var highlights = new Dictionary<int, List<BlockHighlight>>();
        var matches = _search.Matches;
        for (var i = 0; i < matches.Count; i++)
        {
            var m = matches[i];
            if (!highlights.TryGetValue(m.EntryIndex, out var list)) highlights[m.EntryIndex] = list = [];
            list.Add(new BlockHighlight(m.Utf16Start, m.Utf16Length));
        }

        _ctx = new FlowRenderContext
        {
            Typography = _typography.Settings,
            HighContrast = hc,
            Foreground = foreground,
            SecondaryForeground = secondary,
            Border = border,
            HeaderFill = header,
            MatchBackground = matchBg,
            CurrentMatchBackground = currentBg,
            MatchForeground = matchFg,
            Highlights = highlights,
        };
        SetCurrentMatchOnContext();
    }

    private void SetCurrentMatchOnContext()
    {
        if (_ctx is null) return;
        if (_search.CurrentMatch is { } m)
        {
            _ctx.CurrentEntryIndex = m.EntryIndex;
            _ctx.CurrentStart = m.Utf16Start;
        }
        else
        {
            _ctx.CurrentEntryIndex = -1;
            _ctx.CurrentStart = -1;
        }
    }

    /// <summary>Re-renders one entry if it currently has a container (F61: find-next touches two blocks, not all).</summary>
    private void RefreshEntry(int index)
    {
        if (index < 0 || index >= _items.Count) return;
        if (FlowList.ContainerFromIndex(index) is ListViewItem container) RenderContainer(container, index);
    }

    private static Color SystemColor(string key, Color fallback)
    {
        try
        {
            if (Application.Current.Resources.TryGetValue(key, out var value) && value is Color c) return c;
        }
        catch (Exception)
        {
            // Fall through to the fallback.
        }
        return fallback;
    }

    // ── Typography ─────────────────────────────────────────────────────────

    private void OnSmaller(object sender, RoutedEventArgs e) =>
        _typography.Settings = _typography.Settings.WithFontSize(_typography.Settings.FontSize - 1);

    private void OnLarger(object sender, RoutedEventArgs e) =>
        _typography.Settings = _typography.Settings.WithFontSize(_typography.Settings.FontSize + 1);

    private void OnFontClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { Tag: string tag } && Enum.TryParse<ReadingFontDesign>(tag, out var font))
        {
            _typography.Settings = _typography.Settings with { FontDesign = font };
        }
    }

    private void OnSpacingClick(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { Tag: string tag } && Enum.TryParse<LineSpacingOption>(tag, out var spacing))
        {
            _typography.Settings = _typography.Settings with { LineSpacing = spacing };
        }
    }

    private void OnTypographyChanged(object? sender, EventArgs e)
    {
        AppServices.FlowTypography.Save(_typography.Settings);
        UpdateTypographyUi();
        RebuildContext();
        // Block heights change with size/spacing: keep the reader on the same block afterwards.
        var anchor = (FlowList.ItemsPanelRoot as ItemsStackPanel)?.FirstVisibleIndex ?? -1;
        RefreshRealized();
        if (anchor >= 0) _ = ScrollToEntryAsync(anchor);
    }

    private void UpdateTypographyUi()
    {
        var s = _typography.Settings;
        SizeText.Text = string.Create(System.Globalization.CultureInfo.InvariantCulture, $"{s.FontSize:0} pt");
        AutomationProperties.SetName(SizeText, SizeText.Text);
        SmallerButton.IsEnabled = s.FontSize > TypographySettings.MinFontSize;
        LargerButton.IsEnabled = s.FontSize < TypographySettings.MaxFontSize;
        // A persisted "rounded" has no Windows face and renders as Default, so Default is what shows as chosen.
        FontDefaultRadio.IsChecked = s.FontDesign != ReadingFontDesign.Serif;
        FontSerifRadio.IsChecked = s.FontDesign == ReadingFontDesign.Serif;
        SpacingCompactRadio.IsChecked = s.LineSpacing == LineSpacingOption.Compact;
        SpacingRegularRadio.IsChecked = s.LineSpacing == LineSpacingOption.Regular;
        SpacingRelaxedRadio.IsChecked = s.LineSpacing == LineSpacingOption.Relaxed;
    }

    // ── Contents ───────────────────────────────────────────────────────────

    private void OnTocContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (args.InRecycleQueue || args.Item is not TocRow row) return;
        AutomationProperties.SetName(args.ItemContainer, row.Title);
        AutomationProperties.SetAutomationId(args.ItemContainer, "FlowToc_" + args.ItemIndex.ToString(System.Globalization.CultureInfo.InvariantCulture));
    }

    private void OnTocItemClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not TocRow row) return;
        TocFlyout.Hide();
        _ = ScrollToEntryAsync(row.EntryIndex);
    }

    // ── Find ───────────────────────────────────────────────────────────────

    private bool FindOpen => FindBar.Visibility == Visibility.Visible;

    private void OnFindToggle(object sender, RoutedEventArgs e)
    {
        if (FindToggle.IsChecked == true) ShowFind(); else HideFind();
    }

    private void OnFindAccelerator(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e)
    {
        e.Handled = true;
        if (_doc is not null) ShowFind();
    }

    private void ShowFind()
    {
        FindBar.Visibility = Visibility.Visible;
        FindToggle.IsChecked = true;
        FindBox.Focus(FocusState.Programmatic);
        FindBox.SelectAll();
    }

    private void HideFind()
    {
        _findTimer?.Stop();
        FindBar.Visibility = Visibility.Collapsed;
        FindToggle.IsChecked = false;
        FindBox.Text = string.Empty;
        ApplyQuery(scroll: false);
    }

    private void OnFindCloseClick(object sender, RoutedEventArgs e) => HideFind();

    private void OnFindTextChanged(object sender, TextChangedEventArgs e)
    {
        // Coalesce keystrokes: a search walks every block.
        _findTimer ??= CreateTimer(250, () => ApplyQuery(scroll: true));
        _findTimer.Stop();
        _findTimer.Start();
    }

    private DispatcherQueueTimer CreateTimer(int milliseconds, Action tick)
    {
        var timer = DispatcherQueue.CreateTimer();
        timer.IsRepeating = false;
        timer.Interval = TimeSpan.FromMilliseconds(milliseconds);
        timer.Tick += (_, _) =>
        {
            timer.Stop();
            tick();
        };
        return timer;
    }

    private void ApplyQuery(bool scroll)
    {
        if (_doc is null || _left) return;
        _findTimer?.Stop();
        _search.SetQuery(FindBox.Text);
        AfterSearchChanged(scroll);
    }

    private void AfterSearchChanged(bool scroll)
    {
        UpdateFindStatus();
        RebuildContext();
        RefreshRealized();
        if (scroll && _search.CurrentMatch is { } match) _ = ScrollToEntryAsync(match.EntryIndex);
    }

    private void UpdateFindStatus()
    {
        string text;
        if (_search.Query.Length == 0) text = string.Empty;
        else if (_search.MatchCount == 0) text = "No matches";
        else
        {
            var plus = _search.Truncated ? "+" : string.Empty;
            text = string.Create(System.Globalization.CultureInfo.InvariantCulture,
                $"{_search.CurrentMatchIndex + 1} of {_search.MatchCount}{plus}");
        }
        FindStatus.Text = text; // a TextBlock's automation name is its text; no separate name to go stale
    }

    private void StepFind(bool forward)
    {
        if (_doc is null) return;
        if (!FindOpen)
        {
            ShowFind();
            return;
        }
        // A query typed a moment ago may still be waiting on the debounce: apply it first.
        if (_findTimer is { IsRunning: true }) ApplyQuery(scroll: false);
        if (_search.MatchCount == 0) return;
        if (forward) _search.FindNext(); else _search.FindPrevious();

        // Only the previous and the new current match change appearance: reuse the per-query highlight data
        // and re-render just those two entries instead of rebuilding the context and every realised block (F61).
        if (_ctx is null)
        {
            AfterSearchChanged(scroll: true);
            return;
        }
        var oldEntry = _ctx.CurrentEntryIndex;
        SetCurrentMatchOnContext();
        UpdateFindStatus();
        RefreshEntry(oldEntry);
        if (_ctx.CurrentEntryIndex != oldEntry) RefreshEntry(_ctx.CurrentEntryIndex);
        if (_search.CurrentMatch is { } match) _ = ScrollToEntryAsync(match.EntryIndex);
    }

    private void OnFindNextClick(object sender, RoutedEventArgs e) => StepFind(forward: true);

    private void OnFindPreviousClick(object sender, RoutedEventArgs e) => StepFind(forward: false);

    private void OnFindNextAccelerator(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e)
    {
        e.Handled = true;
        StepFind(forward: true);
    }

    private void OnFindPreviousAccelerator(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e)
    {
        e.Handled = true;
        StepFind(forward: false);
    }

    private void OnFindKeyDown(object sender, KeyRoutedEventArgs e)
    {
        switch (e.Key)
        {
            case VirtualKey.Enter:
                e.Handled = true;
                StepFind(forward: !IsShiftDown());
                break;
            case VirtualKey.Escape:
                e.Handled = true;
                HideFind();
                break;
        }
    }

    private static bool IsShiftDown() =>
        Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift)
            .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);

    // ── Scrolling, progress and position ───────────────────────────────────

    private int FirstVisibleIndex => (FlowList.ItemsPanelRoot as ItemsStackPanel)?.FirstVisibleIndex ?? -1;

    private bool AtBottom => _scroll is { ScrollableHeight: > 1 } s && s.VerticalOffset >= s.ScrollableHeight - 1;

    private Task ScrollToEntryAsync(int index) => ScrollToEntryCoreAsync(index, ++_scrollToken);

    /// <summary>
    /// Brings block <paramref name="index"/> to the top. Heights of unrealised blocks are estimates, so a long jump
    /// can land short; re-issue until the block is first (or the end of the document stops it moving).
    /// </summary>
    private async Task ScrollToEntryCoreAsync(int index, int token)
    {
        if (index < 0 || index >= _items.Count) return;
        try
        {
            for (var attempt = 0; attempt < 6 && token == _scrollToken && !_left; attempt++)
            {
                FlowList.ScrollIntoView(_items[index], ScrollIntoViewAlignment.Leading);
                await Task.Delay(30);
                if (_left) return;
                FlowList.UpdateLayout();
                if (FirstVisibleIndex == index || AtBottom) break;
            }
        }
        catch (Exception)
        {
            // A scroll request must never fault the page.
        }
        if (token == _scrollToken && !_restoring) UpdateProgressFromScroll();
    }

    private void OnViewChanged(object? sender, ScrollViewerViewChangedEventArgs e)
    {
        if (_restoring || _left || _progressQueued) return;
        // The panel's FirstVisibleIndex lags the scroll offset until layout runs, and ViewChanged fires per frame:
        // read it once per idle dispatcher turn instead (also the F49 coalescing lesson).
        _progressQueued = true;
        if (!DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
            {
                _progressQueued = false;
                if (!_left && !_restoring) UpdateProgressFromScroll();
            }))
        {
            _progressQueued = false;
        }
    }

    /// <summary>Block-count based (as Apple): which block the reader is at, not a pixel fraction.</summary>
    private void UpdateProgressFromScroll()
    {
        var first = FirstVisibleIndex;
        if (first < 0 || _items.Count == 0) return;
        var fraction = AtBottom ? 1.0 : ReadingProgress.FractionForBlock(first, _items.Count);
        _progress.Update(fraction);
    }

    private void OnProgressChanged(object? sender, EventArgs e)
    {
        UpdateProgressUi();
        // Persist shortly after scrolling settles (and again on leave) rather than on every frame.
        _saveTimer ??= CreateTimer(500, SavePosition);
        _saveTimer.Stop();
        _saveTimer.Start();
    }

    private void SavePosition()
    {
        if (_args is null || _left || _restoring) return;
        AppServices.FlowPositions.Save(_args.ItemId, _progress.Fraction);
    }

    private void UpdateProgressUi()
    {
        FlowProgressBar.Value = _progress.Fraction;
        ProgressText.Text = _progress.PercentText;
        AutomationProperties.SetName(ProgressText, _progress.PercentText);
    }

    private void ScrollBy(double delta)
    {
        if (_scroll is null) return;
        _scroll.ChangeView(null, Math.Clamp(_scroll.VerticalOffset + delta, 0, _scroll.ScrollableHeight), null, true);
    }

    private async Task ScrollToEndAsync()
    {
        if (_items.Count == 0 || _scroll is null) return;
        var token = ++_scrollToken;
        for (var attempt = 0; attempt < 6 && token == _scrollToken && !_left; attempt++)
        {
            FlowList.ScrollIntoView(_items[^1], ScrollIntoViewAlignment.Default);
            await Task.Delay(30);
            if (_left) return;
            FlowList.UpdateLayout();
            _scroll.ChangeView(null, _scroll.ScrollableHeight, null, true);
            await Task.Delay(30);
            if (AtBottom) break;
        }
        UpdateProgressFromScroll();
    }

    // ── Input ──────────────────────────────────────────────────────────────

    /// <summary>
    /// Home/End/PgUp/PgDn and the arrow keys scroll the document from wherever focus is on the page (a button,
    /// the text), except inside the find box where they edit text. Handled in the tunnelling pass so the
    /// ListView's own item-by-item focus movement does not fight them. PgUp/PgDn page by the real viewport.
    /// </summary>
    private void OnPreviewKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (_scroll is null || _restoring || _doc is null) return;
        if (FocusManager.GetFocusedElement(XamlRoot) is TextBox) return;
        switch (e.Key)
        {
            case VirtualKey.Home:
                e.Handled = true;
                _scrollToken++;
                _scroll.ChangeView(null, 0, null, true);
                break;
            case VirtualKey.End:
                e.Handled = true;
                _ = ScrollToEndAsync();
                break;
            case VirtualKey.PageDown:
                e.Handled = true;
                ScrollBy(_scroll.ViewportHeight * PageScrollViewportFraction);
                break;
            case VirtualKey.PageUp:
                e.Handled = true;
                ScrollBy(-_scroll.ViewportHeight * PageScrollViewportFraction);
                break;
            case VirtualKey.Down:
                e.Handled = true;
                ScrollBy(LineScrollEpx);
                break;
            case VirtualKey.Up:
                e.Handled = true;
                ScrollBy(-LineScrollEpx);
                break;
        }
    }

    private void OnBackAccelerator(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e)
    {
        e.Handled = true;
        GoBack();
    }

    private void OnBackClick(object sender, RoutedEventArgs e) => GoBack();

    private void GoBack()
    {
        if (Frame is { CanGoBack: true } frame) frame.GoBack();
    }

    // ── Theme (spec §3.1: all controls in theme colours; high contrast bypasses the palette) ───────────

    private void OnThemeChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(ThemeManager.Resolved)) return;
        // Enqueued behind MainWindow's own handler, which replaces the Gist* brushes first.
        DispatcherQueue.TryEnqueue(() =>
        {
            if (_left) return;
            ApplyTheme();
            if (_doc is null) return;
            RebuildContext();
            RefreshRealized();
        });
    }

    private void ApplyTheme()
    {
        if (new AccessibilitySettings().HighContrast)
        {
            RootGrid.ClearValue(Panel.BackgroundProperty);
            foreach (var text in new[] { TitleText, LoadingText, MessageText, ProgressText, FindStatus })
            {
                text.ClearValue(TextBlock.ForegroundProperty);
            }
            FlowProgressBar.ClearValue(Control.ForegroundProperty);
            return;
        }

        var resources = Application.Current.Resources;
        var foreground = (Brush)resources["GistForeground"];
        var secondary = (Brush)resources["GistSecondaryText"];
        RootGrid.Background = (Brush)resources["GistBackground"];
        TitleText.Foreground = secondary;
        LoadingText.Foreground = foreground;
        MessageText.Foreground = foreground;
        ProgressText.Foreground = secondary;
        FindStatus.Foreground = secondary;
        FlowProgressBar.Foreground = (Brush)resources["GistAccent"];
    }
}
