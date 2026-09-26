using Gist.Core.Theming;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Gist.App.Dialogs;

/// <summary>
/// The "Appearance" dialog (<c>docs/windows-ui-spec.md</c> §7.3): a set of radio buttons over the
/// five <see cref="ThemeSelection"/> cases, applying live, Close-only.
/// </summary>
public static class AppearanceDialog
{
    public static async Task ShowAsync(ThemeManager theme, XamlRoot xamlRoot)
    {
        ArgumentNullException.ThrowIfNull(theme);
        ArgumentNullException.ThrowIfNull(xamlRoot);

        var panel = new StackPanel { Spacing = 8 };
        var buttons = new (ThemeSelection Selection, string Label)[]
        {
            (ThemeSelection.System, "Follow System"),
            (ThemeSelection.Light, "Light"),
            (ThemeSelection.Dark, "Dark"),
            (ThemeSelection.Sepia, "Sepia"),
            (ThemeSelection.Oled, "OLED (True Black)"),
        };

        foreach (var (selection, label) in buttons)
        {
            var radio = new RadioButton
            {
                Content = label,
                GroupName = "GistAppearance",
                Tag = selection,
                IsChecked = theme.Selection == selection,
            };
            AutomationProperties.SetAutomationId(radio, "AppearanceDialog_" + selection);
            radio.Checked += (_, _) => theme.Selection = selection;
            panel.Children.Add(radio);
        }

        panel.Children.Add(new TextBlock
        {
            Text = "When Windows is set to a high-contrast theme, these colours are not used.",
            TextWrapping = TextWrapping.Wrap,
            Style = (Style)Application.Current.Resources["CaptionTextBlockStyle"],
            Foreground = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["TextFillColorSecondaryBrush"],
            Margin = new Thickness(0, 8, 0, 0),
        });

        var dialog = new ContentDialog
        {
            Title = "Appearance",
            Content = panel,
            CloseButtonText = "Close",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = xamlRoot,
        };
        AutomationProperties.SetAutomationId(dialog, "AppearanceDialog");

        if (xamlRoot.Content is FrameworkElement root)
        {
            dialog.RequestedTheme = root.RequestedTheme;
        }

        await dialog.ShowAsync();
    }
}
