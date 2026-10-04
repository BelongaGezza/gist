using System.ComponentModel;
using Gist.Core.Rsvp;
using Gist.Core.Theming;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Windows.System;
using Windows.UI.ViewManagement;

namespace Gist.App.Views;

/// <summary>Frame parameter for <see cref="RsvpPage"/>.</summary>
public sealed record RsvpNavigationArgs(string ItemId, string Title);

/// <summary>
/// The RSVP reader (<c>docs/windows-ui-spec.md</c> §7.1). All behaviour lives in
/// <see cref="RsvpReaderViewModel"/> (and, below it, the Rust pacing engine); this page renders
/// state, forwards input, and owns the one WinUI-specific piece: a <see cref="DispatcherQueueTimer"/>
/// the playback loop re-anchors to a monotonic clock.
/// </summary>
public sealed partial class RsvpPage : Page
{
    private RsvpNavigationArgs? _args;
    private RsvpReaderViewModel? _vm;
    private bool _left;
    private bool _syncing;
    private bool _scrubbing;
    private bool _themeHooked;

    public RsvpPage()
    {
        InitializeComponent();
        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
        // Dragging the scrubber must not fight the playback loop that keeps moving its value.
        Scrubber.AddHandler(PointerPressedEvent, new PointerEventHandler((_, _) => _scrubbing = true), true);
        Scrubber.AddHandler(PointerReleasedEvent, new PointerEventHandler((_, _) => _scrubbing = false), true);
        Scrubber.AddHandler(PointerCaptureLostEvent, new PointerEventHandler((_, _) => _scrubbing = false), true);
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _args = e.Parameter as RsvpNavigationArgs;
    }

    protected override void OnNavigatingFrom(NavigatingCancelEventArgs e)
    {
        base.OnNavigatingFrom(e);
        _ = LeaveAsync();
    }

    // ── Lifetime ───────────────────────────────────────────────────────────

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        if (_vm is not null || _args is null) return;

        TitleText.Text = _args.Title;
        var core = AppServices.Core;
        var itemId = _args.ItemId;
        _vm = new RsvpReaderViewModel(
            itemId,
            _args.Title,
            wpm => core.OpenRsvpEngineAsync(itemId, wpm),
            index => core.SaveProgressAsync(itemId, index),
            new StopwatchRsvpClock(),
            new DispatcherRsvpTimer(DispatcherQueue));
        _vm.PropertyChanged += OnVmChanged;

        if (!_themeHooked)
        {
            AppServices.Theme.PropertyChanged += OnThemeChanged;
            _themeHooked = true;
        }

        ApplyTheme();
        UpdateView();
        await _vm.LoadAsync();
        UpdateView();
        FocusPrimary();
    }

    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        if (_themeHooked)
        {
            AppServices.Theme.PropertyChanged -= OnThemeChanged;
            _themeHooked = false;
        }

        _ = LeaveAsync();
    }

    /// <summary>Pauses, persists the position (spec §7.1 item 2) and releases the engine. Runs once.</summary>
    private async Task LeaveAsync()
    {
        if (_left || _vm is null) return;
        _left = true;
        _vm.PropertyChanged -= OnVmChanged;
        try
        {
            await _vm.LeaveAsync();
        }
        catch (Exception)
        {
            // Leaving must never throw into navigation; the periodic saves already bound the loss.
        }
    }

    private void FocusPrimary()
    {
        // Focus lands on Play so Space/Enter start reading immediately (Space is also page-level).
        if (_vm is { LoadState: RsvpLoadState.Ready }) PlayPauseButton.Focus(FocusState.Programmatic);
        else BackButton.Focus(FocusState.Programmatic);
    }

    // ── View state ─────────────────────────────────────────────────────────

    private void OnVmChanged(object? sender, PropertyChangedEventArgs e) => UpdateView();

    private void UpdateView()
    {
        var vm = _vm;
        if (vm is null) return;

        LoadingPanel.Visibility = vm.LoadState == RsvpLoadState.Loading ? Visibility.Visible : Visibility.Collapsed;
        ReaderPanel.Visibility = vm.LoadState == RsvpLoadState.Ready ? Visibility.Visible : Visibility.Collapsed;
        var message = vm.LoadState switch
        {
            RsvpLoadState.Empty => "This item has no readable text.",
            RsvpLoadState.Failed => "This item couldn't be opened for reading.",
            _ => null,
        };
        MessagePanel.Visibility = message is null ? Visibility.Collapsed : Visibility.Visible;
        MessageText.Text = message ?? string.Empty;
        ReadWordButton.IsEnabled = vm.LoadState == RsvpLoadState.Ready;

        if (vm.LoadState != RsvpLoadState.Ready) return;

        _syncing = true;
        try
        {
            WordText.Text = vm.Word;
            ProgressText.Text = vm.ProgressText;
            WpmLabel.Text = vm.WpmLabel;

            var label = vm.PlayPauseLabel;
            PlayPauseText.Text = label;
            PlayPauseGlyph.Glyph = vm.IsPlaying ? "" : "";
            AutomationProperties.SetName(PlayPauseButton, label);
            ToolTipService.SetToolTip(PlayPauseButton, label + " (Space)");

            var wpm = (double)vm.Wpm;
            if (WpmSlider.Value != wpm) WpmSlider.Value = wpm;
            if (WpmBox.Value != wpm) WpmBox.Value = wpm;

            Scrubber.Maximum = Math.Max(1, vm.MaxPosition);
            Scrubber.IsEnabled = vm.MaxPosition > 0;
            if (!_scrubbing && Scrubber.Value != vm.Position) Scrubber.Value = vm.Position;
        }
        finally
        {
            _syncing = false;
        }
    }

    // ── Input ──────────────────────────────────────────────────────────────

    private void OnPlayPause(object sender, RoutedEventArgs e) => _vm?.TogglePlayPause();

    /// <summary>
    /// Space toggles playback from anywhere on the page (spec §9) — except where Space already
    /// means something: on a Button (it clicks) or in the number box's text field (it types).
    /// Handled here, in the tunnelling pass, so a focused Slider does not swallow it.
    /// </summary>
    private void OnPreviewKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key != VirtualKey.Space || _vm is not { LoadState: RsvpLoadState.Ready } vm) return;
        if (FocusManager.GetFocusedElement(XamlRoot) is ButtonBase or TextBox or NumberBox) return;
        e.Handled = true;
        vm.TogglePlayPause();
    }

    private void OnStepBack(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e) => Step(e, () => _vm?.StepWord(-1));

    private void OnStepForward(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e) => Step(e, () => _vm?.StepWord(1));

    private void OnJumpBack(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e) => Step(e, () => _vm?.JumpWords(-1));

    private void OnJumpForward(KeyboardAccelerator a, KeyboardAcceleratorInvokedEventArgs e) => Step(e, () => _vm?.JumpWords(1));

    /// <summary>Arrow accelerators must not steal caret movement from the number box.</summary>
    private void Step(KeyboardAcceleratorInvokedEventArgs e, Action action)
    {
        if (FocusManager.GetFocusedElement(XamlRoot) is TextBox or NumberBox) return;
        e.Handled = true;
        action();
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

    private void OnScrubberChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_syncing || _vm is null) return;
        _vm.SeekTo((ulong)Math.Max(0, Math.Round(e.NewValue)));
    }

    private void OnWpmSliderChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_syncing || _vm is null) return;
        _vm.SetWpm(e.NewValue);
    }

    private void OnWpmBoxChanged(NumberBox sender, NumberBoxValueChangedEventArgs args)
    {
        if (_syncing || _vm is null) return;
        if (double.IsNaN(args.NewValue))
        {
            UpdateView(); // cleared/invalid text: put the real value back
            return;
        }

        _vm.SetWpm(args.NewValue);
    }

    /// <summary>
    /// The accessible "read current word" action (spec §8): word changes are never live-announced
    /// (it would flood Narrator); this announces the current word once, on request.
    /// </summary>
    private void OnReadWord(object sender, RoutedEventArgs e)
    {
        if (_vm is not { LoadState: RsvpLoadState.Ready } vm || string.IsNullOrEmpty(vm.Word)) return;
        var peer = FrameworkElementAutomationPeer.FromElement(WordText)
                   ?? FrameworkElementAutomationPeer.CreatePeerForElement(WordText);
        peer?.RaiseNotificationEvent(
            AutomationNotificationKind.ActionCompleted,
            AutomationNotificationProcessing.MostRecent,
            vm.Word,
            "RsvpReadCurrentWord");
    }

    // ── Theme (spec §3.1, §7.1: "all controls in theme colours") ────────────

    private void OnThemeChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(ThemeManager.Resolved)) return;
        // Enqueued behind MainWindow's own handler, which replaces the Gist* brushes first.
        DispatcherQueue.TryEnqueue(ApplyTheme);
    }

    private void ApplyTheme()
    {
        if (new AccessibilitySettings().HighContrast)
        {
            // Spec §3.1: high contrast bypasses every GIST palette; fall back to system theme brushes.
            RootGrid.ClearValue(Panel.BackgroundProperty);
            RootGrid.ClearValue(Control.ForegroundProperty);
            foreach (var text in new[] { WordText, TitleText, ProgressText, WpmLabel, LoadingText, MessageText })
            {
                text.ClearValue(TextBlock.ForegroundProperty);
            }

            PlayPauseButton.Resources.Clear();
            PlayPauseButton.ClearValue(Control.BackgroundProperty);
            PlayPauseButton.ClearValue(Control.ForegroundProperty);
            return;
        }

        var resources = Application.Current.Resources;
        var background = (Brush)resources["GistBackground"];
        var foreground = (Brush)resources["GistForeground"];
        var accent = (SolidColorBrush)resources["GistAccent"];

        RootGrid.Background = background;
        WordText.Foreground = foreground;
        TitleText.Foreground = (Brush)resources["GistSecondaryText"];
        ProgressText.Foreground = (Brush)resources["GistSecondaryText"];
        WpmLabel.Foreground = foreground;
        LoadingText.Foreground = foreground;
        MessageText.Foreground = foreground;

        // Play/Pause: accent fill in every Button visual state (overriding Background alone would
        // flatten hover/pressed), with the page background as the glyph/label colour.
        var res = PlayPauseButton.Resources;
        res["ButtonBackground"] = accent;
        res["ButtonBackgroundPointerOver"] = new SolidColorBrush(accent.Color) { Opacity = 0.88 };
        res["ButtonBackgroundPressed"] = new SolidColorBrush(accent.Color) { Opacity = 0.72 };
        res["ButtonForeground"] = background;
        res["ButtonForegroundPointerOver"] = background;
        res["ButtonForegroundPressed"] = background;
        res["ButtonBorderBrush"] = accent;
        res["ButtonBorderBrushPointerOver"] = accent;
        res["ButtonBorderBrushPressed"] = accent;
    }
}

/// <summary>
/// <see cref="IRsvpTimer"/> over a <see cref="DispatcherQueueTimer"/> (one-shot). The callback runs
/// on the UI thread. Replacing a pending wake-up is just re-arming.
/// </summary>
internal sealed class DispatcherRsvpTimer : IRsvpTimer
{
    private readonly Microsoft.UI.Dispatching.DispatcherQueueTimer _timer;
    private Action? _callback;

    public DispatcherRsvpTimer(Microsoft.UI.Dispatching.DispatcherQueue queue)
    {
        _timer = queue.CreateTimer();
        _timer.IsRepeating = false;
        _timer.Tick += (_, _) =>
        {
            var cb = _callback;
            _callback = null;
            cb?.Invoke();
        };
    }

    public void Schedule(TimeSpan delay, Action callback)
    {
        _timer.Stop();
        _callback = callback;
        _timer.Interval = delay;
        _timer.Start();
    }

    public void Cancel()
    {
        _timer.Stop();
        _callback = null;
    }
}
