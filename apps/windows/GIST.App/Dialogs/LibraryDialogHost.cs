using Gist.Core.Client;
using Gist.Core.ViewModels;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Markup;
using Windows.System;

namespace Gist.App.Dialogs;

/// <summary>
/// Shows the Library's <c>ContentDialog</c>s (<c>docs/windows-ui-spec.md</c> §4.5, §6) for whatever
/// <see cref="LibraryViewModelBase.PendingDialog"/> asks for.
/// </summary>
/// <remarks>
/// <para>
/// All wording comes from <see cref="DialogContent"/>, <see cref="RemovePreview"/> and
/// <see cref="EncryptPreview"/> in GIST.Core; this class only lays it out with standard Fluent
/// controls and theme resources (no hardcoded colours), so the dialogs follow the active theme.
/// </para>
/// <para>
/// WinUI throws if two <c>ContentDialog</c>s are open in one window, so every call is serialised
/// through <see cref="Gate"/>, and a dialog whose action raises a follow-up (Encrypt → result, any
/// failure → error/DRM) is shown after the first has closed. Every path ends in
/// <see cref="LibraryViewModelBase.DismissDialogAsync"/> so <c>PendingDialog</c> never sticks. No
/// dialog ever shows raw exception text or a path.
/// </para>
/// </remarks>
public sealed class LibraryDialogHost
{
    private const int MaxChainedDialogs = 4;

    /// <summary>
    /// How long <see cref="ShowDialogAsync"/> keeps retrying while WinUI refuses to open a
    /// <c>ContentDialog</c> because another one is still closing.
    /// </summary>
    /// <remarks>
    /// WinUI allows exactly one open <c>ContentDialog</c> per window, and a dialog stays "open" for
    /// a short tail <em>after</em> it has disappeared from the UI Automation tree — roughly half a
    /// second. A request made inside that window used to throw, get swallowed, and then be
    /// dismissed, silently dropping whatever the user had just asked for (found by the FlaUI
    /// click-through suite, which had to sleep 600 ms after every dialog to avoid it). Retrying is
    /// the fix: the wait is bounded so a genuinely broken show still fails rather than hanging.
    /// </remarks>
    private static readonly TimeSpan ShowRetryWindow = TimeSpan.FromSeconds(2);

    /// <summary>Gap between <see cref="ShowRetryWindow"/> attempts — short enough to feel instant.</summary>
    private static readonly TimeSpan ShowRetryDelay = TimeSpan.FromMilliseconds(50);

    private static readonly SemaphoreSlim Gate = new(1, 1);

    private const string DestructiveButtonStyleXaml =
        "<Style xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' TargetType='Button'>"
        + "<Setter Property='Foreground' Value='{ThemeResource SystemFillColorCriticalBrush}'/></Style>";

    /// <summary>Convenience passthrough to <see cref="ImportFilePicker.PickImportFileAsync"/>.</summary>
    public static Task<string?> PickImportFileAsync(Window window) => ImportFilePicker.PickImportFileAsync(window);

    /// <summary>
    /// Shows the dialog(s) the view model is waiting on, until nothing is pending.
    /// </summary>
    public async Task HandleAsync(LibraryViewModel vm, XamlRoot xamlRoot)
    {
        ArgumentNullException.ThrowIfNull(vm);
        ArgumentNullException.ThrowIfNull(xamlRoot);

        await Gate.WaitAsync();
        try
        {
            for (var i = 0; i < MaxChainedDialogs && vm.PendingDialog != LibraryDialog.None; i++)
            {
                var requested = vm.PendingDialog;
                bool displayed;
                try
                {
                    await ShowAsync(vm, xamlRoot, requested);
                    displayed = true;
                }
                catch (Exception)
                {
                    // A dialog failure must never surface exception text (spec §4.5).
                    displayed = false;
                }

                if (!displayed)
                {
                    // The user never saw this dialog, so they never answered it. Dismissing here
                    // would throw their action away silently — the bug this method used to have.
                    // ShowDialogAsync has already retried for ShowRetryWindow, so leave the request
                    // standing for the next pass (LibraryPage re-runs the host on the next state
                    // change) rather than pretending it was handled.
                    return;
                }

                if (vm.PendingDialog == requested)
                {
                    await vm.DismissDialogAsync();
                }
            }

            if (vm.PendingDialog != LibraryDialog.None)
            {
                // Shown but still pending after the chain limit: state must not stick.
                await vm.DismissDialogAsync();
            }
        }
        finally
        {
            Gate.Release();
        }
    }

    /// <summary>
    /// The Collection screen's counterpart of <see cref="HandleAsync(LibraryViewModel, XamlRoot)"/>.
    /// Kept separate rather than generalised over <see cref="LibraryViewModelBase"/>: the two
    /// screens support different dialogs (no Import/New Collection/Encrypt here) and, per §5, removal
    /// itself is a different operation with different wording — the same "two view models sharing a
    /// row template, not a removal" split <see cref="Gist.Core.ViewModels.CollectionViewModel"/>'s
    /// own doc comment describes.
    /// </summary>
    public async Task HandleAsync(CollectionViewModel vm, XamlRoot xamlRoot)
    {
        ArgumentNullException.ThrowIfNull(vm);
        ArgumentNullException.ThrowIfNull(xamlRoot);

        await Gate.WaitAsync();
        try
        {
            for (var i = 0; i < MaxChainedDialogs && vm.PendingDialog != LibraryDialog.None; i++)
            {
                var requested = vm.PendingDialog;
                bool displayed;
                try
                {
                    await ShowAsync(vm, xamlRoot, requested);
                    displayed = true;
                }
                catch (Exception)
                {
                    displayed = false;
                }

                if (!displayed)
                {
                    return;
                }

                if (vm.PendingDialog == requested)
                {
                    await vm.DismissDialogAsync();
                }
            }

            if (vm.PendingDialog != LibraryDialog.None)
            {
                await vm.DismissDialogAsync();
            }
        }
        finally
        {
            Gate.Release();
        }
    }

    /// <summary>
    /// Opens <paramref name="dialog"/>, retrying briefly while WinUI refuses because another
    /// <c>ContentDialog</c> in the same window has not finished closing.
    /// </summary>
    /// <remarks>
    /// Every dialog in this class goes through here rather than calling <c>ShowAsync</c> directly,
    /// so the retry covers only the opening of the dialog: an exception from the action a dialog's
    /// result triggers (an import, a removal) is not a "could not be shown" and must never cause
    /// the dialog to reappear. See <see cref="ShowRetryWindow"/> for why this is needed at all.
    /// </remarks>
    private static async Task<ContentDialogResult> ShowDialogAsync(ContentDialog dialog)
    {
        var deadline = DateTime.UtcNow + ShowRetryWindow;
        while (true)
        {
            try
            {
                return await dialog.ShowAsync();
            }
            catch (Exception) when (DateTime.UtcNow < deadline)
            {
                await Task.Delay(ShowRetryDelay);
            }
        }
    }

    private Task ShowAsync(LibraryViewModel vm, XamlRoot xamlRoot, LibraryDialog which) => which switch
    {
        LibraryDialog.ImportUrl => ShowImportUrlAsync(vm, xamlRoot),
        LibraryDialog.NewCollection => ShowNewCollectionAsync(vm, xamlRoot),
        LibraryDialog.RemoveConfirm => ShowRemoveAsync(vm, xamlRoot),
        LibraryDialog.EncryptConfirm => ShowEncryptAsync(vm, xamlRoot),
        LibraryDialog.EncryptResult => ShowEncryptResultAsync(vm, xamlRoot),
        LibraryDialog.DrmProtected => ShowMessageAsync(
            xamlRoot, DialogContent.DrmTitle, DialogContent.DrmMessage),
        LibraryDialog.Error => ShowMessageAsync(
            xamlRoot, DialogContent.ErrorTitle, DialogContent.ErrorMessage(vm.LastError?.Kind)),
        LibraryDialog.TagEditor => ShowTagEditorAsync(vm, xamlRoot),
        _ => Task.CompletedTask,
    };

    private Task ShowAsync(CollectionViewModel vm, XamlRoot xamlRoot, LibraryDialog which) => which switch
    {
        LibraryDialog.RemoveConfirm => ShowCollectionRemoveAsync(vm, xamlRoot),
        LibraryDialog.TagEditor => ShowTagEditorAsync(vm, xamlRoot),
        LibraryDialog.Error => ShowMessageAsync(
            xamlRoot, DialogContent.ErrorTitle, DialogContent.ErrorMessage(vm.LastError?.Kind)),
        _ => Task.CompletedTask,
    };

    // ── Collection remove (§5: detach, not delete) ────────────────────────

    private static async Task ShowCollectionRemoveAsync(CollectionViewModel vm, XamlRoot xamlRoot)
    {
        var preview = vm.RemovePreview();

        var panel = new StackPanel { Spacing = 8 };
        foreach (var title in preview.Titles)
        {
            panel.Children.Add(new TextBlock
            {
                Text = title,
                TextWrapping = TextWrapping.NoWrap,
                TextTrimming = TextTrimming.CharacterEllipsis,
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            });
        }

        if (preview.MoreText is { } more)
        {
            panel.Children.Add(Body(more));
        }

        panel.Children.Add(Body(preview.Message));

        var dialog = NewDialog(xamlRoot, preview.Title);

        // Not destructive (items stay in the library — only their membership of this collection
        // goes), so unlike the Library screen's Remove this gets an ordinary primary button, not
        // the destructive style.
        dialog.PrimaryButtonText = DialogContent.CollectionRemoveButton;
        dialog.CloseButtonText = DialogContent.CancelButton;
        dialog.DefaultButton = ContentDialogButton.Close;
        dialog.Content = new ScrollViewer { Content = panel, MaxHeight = 360 };

        if (await ShowDialogAsync(dialog) == ContentDialogResult.Primary)
        {
            await vm.RemoveAsync();
        }
    }

    private static ContentDialog NewDialog(XamlRoot xamlRoot, string title)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = xamlRoot,
            Title = title,
            DefaultButton = ContentDialogButton.Primary,
        };

        // Follow the active GIST theme: the dialog's own theme resources then resolve for it.
        if (xamlRoot.Content is FrameworkElement root)
        {
            dialog.RequestedTheme = root.RequestedTheme;
        }

        return dialog;
    }

    private static TextBlock Body(string text) => new()
    {
        Text = text,
        TextWrapping = TextWrapping.Wrap,
    };

    // ── Import URL ─────────────────────────────────────────────────────────

    private static async Task ShowImportUrlAsync(LibraryViewModel vm, XamlRoot xamlRoot)
    {
        var box = new TextBox { PlaceholderText = DialogContent.ImportUrlPlaceholder };
        AutomationProperties.SetName(box, DialogContent.ImportUrlTitle);

        var dialog = NewDialog(xamlRoot, DialogContent.ImportUrlTitle);
        dialog.PrimaryButtonText = DialogContent.ImportUrlPrimary;
        dialog.CloseButtonText = DialogContent.CancelButton;
        dialog.IsPrimaryButtonEnabled = false;
        box.TextChanged += (_, _) => dialog.IsPrimaryButtonEnabled = DialogContent.CanImportUrl(box.Text);
        dialog.Content = Stack(Body(DialogContent.ImportUrlBody), box);
        dialog.Opened += (_, _) => box.Focus(FocusState.Programmatic);

        if (await ShowDialogAsync(dialog) == ContentDialogResult.Primary)
        {
            await vm.ImportUrlAsync(box.Text);
        }
    }

    // ── New Collection ─────────────────────────────────────────────────────

    private static async Task ShowNewCollectionAsync(LibraryViewModel vm, XamlRoot xamlRoot)
    {
        var box = new TextBox { PlaceholderText = DialogContent.NewCollectionPlaceholder };
        AutomationProperties.SetName(box, DialogContent.NewCollectionPlaceholder);

        var dialog = NewDialog(xamlRoot, DialogContent.NewCollectionTitle);
        dialog.PrimaryButtonText = DialogContent.NewCollectionPrimary;
        dialog.CloseButtonText = DialogContent.CancelButton;
        dialog.IsPrimaryButtonEnabled = false;
        box.TextChanged += (_, _) => dialog.IsPrimaryButtonEnabled = DialogContent.CanCreateCollection(box.Text);
        dialog.Content = box;
        dialog.Opened += (_, _) => box.Focus(FocusState.Programmatic);

        if (await ShowDialogAsync(dialog) == ContentDialogResult.Primary)
        {
            await vm.CreateCollectionAndAddAsync(box.Text);
        }
    }

    // ── Remove ─────────────────────────────────────────────────────────────

    private static async Task ShowRemoveAsync(LibraryViewModel vm, XamlRoot xamlRoot)
    {
        var preview = vm.RemovePreview();

        var panel = new StackPanel { Spacing = 8 };
        foreach (var title in preview.Titles)
        {
            panel.Children.Add(new TextBlock
            {
                Text = title,
                TextWrapping = TextWrapping.NoWrap,
                TextTrimming = TextTrimming.CharacterEllipsis,
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            });
        }

        if (preview.MoreText is { } more)
        {
            panel.Children.Add(Body(more));
        }

        panel.Children.Add(Body(preview.Message));

        var dialog = NewDialog(xamlRoot, preview.Title);

        // One destructive button (maintainer decision, 2026-09-21): removal always deletes
        // everything GIST holds, so there is no second, safer choice to offer. Cancel is the Enter
        // default, as for Encrypt — the irreversible action must never be the one a stray Return
        // key triggers.
        dialog.PrimaryButtonText = DialogContent.RemoveButton;
        dialog.PrimaryButtonStyle = (Style)XamlReader.Load(DestructiveButtonStyleXaml);
        dialog.CloseButtonText = DialogContent.CancelButton;
        dialog.DefaultButton = ContentDialogButton.Close;
        dialog.Content = new ScrollViewer { Content = panel, MaxHeight = 360 };

        if (await ShowDialogAsync(dialog) == ContentDialogResult.Primary)
        {
            await vm.RemoveAsync();
        }
    }

    // ── Encrypt ────────────────────────────────────────────────────────────

    private static async Task ShowEncryptAsync(LibraryViewModel vm, XamlRoot xamlRoot)
    {
        var preview = vm.EncryptPreview();

        // The no-recovery warning is required and must be prominent: a warning InfoBar that the
        // user cannot close, above the explanatory text.
        var warning = new InfoBar
        {
            Severity = InfoBarSeverity.Warning,
            IsOpen = true,
            IsClosable = false,
            Message = preview.RecoveryWarning,
        };
        AutomationProperties.SetName(warning, "Warning: encrypted items cannot be recovered");

        var panel = new StackPanel { Spacing = 12 };
        panel.Children.Add(Body(preview.Message));
        panel.Children.Add(warning);
        panel.Children.Add(Body(preview.OptionalNote));
        panel.Children.Add(Body(preview.OriginalsNote));

        var dialog = NewDialog(xamlRoot, preview.Title);
        dialog.PrimaryButtonText = DialogContent.EncryptPrimary;
        dialog.CloseButtonText = DialogContent.CancelButton;
        // Irreversible on key loss: make Cancel, not Encrypt, the Enter default.
        dialog.DefaultButton = ContentDialogButton.Close;
        dialog.Content = new ScrollViewer { Content = panel, MaxHeight = 420 };

        if (await ShowDialogAsync(dialog) == ContentDialogResult.Primary)
        {
            await vm.EncryptAsync();
        }
    }

    private static async Task ShowEncryptResultAsync(LibraryViewModel vm, XamlRoot xamlRoot)
    {
        var panel = new StackPanel { Spacing = 6 };
        foreach (var line in DialogContent.EncryptResultLines(vm.LastEncryptSummary))
        {
            panel.Children.Add(Body(line));
        }

        var dialog = NewDialog(xamlRoot, DialogContent.EncryptResultTitle);
        dialog.CloseButtonText = DialogContent.OkButton;
        dialog.DefaultButton = ContentDialogButton.Close;
        dialog.Content = panel;
        await ShowDialogAsync(dialog);
    }

    // ── DRM / Error ────────────────────────────────────────────────────────

    private static async Task ShowMessageAsync(XamlRoot xamlRoot, string title, string message)
    {
        var dialog = NewDialog(xamlRoot, title);
        dialog.CloseButtonText = DialogContent.OkButton;
        dialog.DefaultButton = ContentDialogButton.Close;
        dialog.Content = Body(message);
        await ShowDialogAsync(dialog);
    }

    // ── Tag editor (§6) ────────────────────────────────────────────────────

    private static async Task ShowTagEditorAsync(LibraryViewModelBase vm, XamlRoot xamlRoot)
    {
        var item = vm.SingleSelectedItem;
        if (item is null)
        {
            return;
        }

        var itemId = item.Id;
        var dialog = NewDialog(xamlRoot, item.Title);
        dialog.CloseButtonText = DialogContent.TagEditorDoneButton;
        dialog.DefaultButton = ContentDialogButton.Close;

        var tagPanel = new ItemsRepeater
        {
            Layout = new UniformGridLayout { MinItemWidth = 120, MinColumnSpacing = 8, MinRowSpacing = 8 },
        };
        var empty = Body(DialogContent.TagEditorNoTags);
        var box = new TextBox { PlaceholderText = DialogContent.TagEditorAddPlaceholder, HorizontalAlignment = HorizontalAlignment.Stretch };
        AutomationProperties.SetName(box, DialogContent.TagEditorAddPlaceholder);
        var add = new Button { Content = DialogContent.TagEditorAddButton, IsEnabled = false };

        async Task ReloadAsync()
        {
            var tags = await vm.TagsForAsync(itemId);
            tagPanel.ItemsSource = tags.Select(t => BuildTagChip(t, async () =>
            {
                await vm.RemoveTagAsync(itemId, t);
                await AfterEditAsync();
            })).ToList();
            empty.Visibility = tags.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        }

        async Task AfterEditAsync()
        {
            // A failed edit raises the error dialog; close this one so it can be shown.
            if (vm.PendingDialog != LibraryDialog.TagEditor)
            {
                dialog.Hide();
                return;
            }

            await ReloadAsync();
        }

        async Task AddAsync()
        {
            if (!DialogContent.CanAddTag(box.Text))
            {
                return;
            }

            var name = box.Text;
            box.Text = string.Empty;
            await vm.AddTagAsync(itemId, name);
            await AfterEditAsync();
            box.Focus(FocusState.Programmatic);
        }

        box.TextChanged += (_, _) => add.IsEnabled = DialogContent.CanAddTag(box.Text);
        box.KeyDown += async (_, e) =>
        {
            if (e.Key == VirtualKey.Enter)
            {
                e.Handled = true;
                await AddAsync();
            }
        };
        add.Click += async (_, _) => await AddAsync();

        var addRow = new Grid { ColumnSpacing = 8 };
        addRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        addRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(add, 1);
        addRow.Children.Add(box);
        addRow.Children.Add(add);

        dialog.Content = new ScrollViewer
        {
            MaxHeight = 380,
            Content = Stack(empty, tagPanel, addRow),
        };
        dialog.Opened += async (_, _) =>
        {
            await ReloadAsync();
            box.Focus(FocusState.Programmatic);
        };

        await ShowDialogAsync(dialog);
    }

    private static FrameworkElement BuildTagChip(string tag, Func<Task> remove)
    {
        var text = new TextBlock
        {
            Text = tag,
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };
        var button = new Button
        {
            Content = new FontIcon { Glyph = "", FontSize = 12 },
            Padding = new Thickness(6),
            Background = null,
        };
        AutomationProperties.SetName(button, DialogContent.RemoveTagAutomationName(tag));
        ToolTipService.SetToolTip(button, DialogContent.RemoveTagAutomationName(tag));
        button.Click += async (_, _) => await remove();

        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
        row.Children.Add(text);
        row.Children.Add(button);
        AutomationProperties.SetName(row, tag);
        return row;
    }

    private static StackPanel Stack(params UIElement[] children)
    {
        var panel = new StackPanel { Spacing = 12 };
        foreach (var c in children)
        {
            panel.Children.Add(c);
        }

        return panel;
    }
}
