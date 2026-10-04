using System.Globalization;
using Gist.Core.Flow;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Storage.Streams;

namespace Gist.App.Views;

/// <summary>One find hit inside one block, in that block's <see cref="FlowBlock.PlainText"/> UTF-16 offsets.</summary>
internal readonly record struct BlockHighlight(int Start, int Length);

/// <summary>
/// Everything a block needs to render that is not the block itself. Rebuilt by <see cref="FlowPage"/> whenever
/// typography, theme or the find state changes; realised blocks are re-rendered from it.
/// </summary>
internal sealed class FlowRenderContext
{
    public static readonly FontFamily DefaultFont = new("Segoe UI Variable Text");

    public static readonly FontFamily SerifFont = new("Georgia");

    public static readonly FontFamily CodeFont = new("Cascadia Mono");

    public required TypographySettings Typography { get; init; }

    /// <summary>True on a Windows high-contrast theme: no GIST palette, system brushes only.</summary>
    public required bool HighContrast { get; init; }

    public Brush? Foreground { get; init; }

    public Brush? SecondaryForeground { get; init; }

    public Brush? Border { get; init; }

    public Brush? HeaderFill { get; init; }

    public required Brush MatchBackground { get; init; }

    public required Brush CurrentMatchBackground { get; init; }

    public required Brush MatchForeground { get; init; }

    /// <summary>
    /// Highlights per flat entry index; absent means none. Built once per query and shared by every re-render,
    /// including find-next, which only moves <see cref="CurrentEntryIndex"/>/<see cref="CurrentStart"/> (F61).
    /// </summary>
    public IReadOnlyDictionary<int, List<BlockHighlight>> Highlights { get; init; } = new Dictionary<int, List<BlockHighlight>>();

    /// <summary>Entry index of the current find match, or -1.</summary>
    public int CurrentEntryIndex { get; set; } = -1;

    /// <summary>UTF-16 start (within that entry's PlainText) of the current find match.</summary>
    public int CurrentStart { get; set; } = -1;

    public double FontSize => Typography.FontSize;

    /// <summary>Roughly 70 characters at the average glyph width (about half an em).</summary>
    public double ColumnMaxWidth => FontSize * 35;

    public double LineHeight => (FontSize * 1.3) + Typography.LineSpacing.ExtraPoints();

    /// <summary>"Rounded" is never offered on Windows (spec 7.2); a persisted <c>rounded</c> renders as Default.</summary>
    public FontFamily BodyFont => Typography.FontDesign == ReadingFontDesign.Serif ? SerifFont : DefaultFont;
}

/// <summary>
/// Builds the visual for one flat block entry. Everything is constructed in code (rebuilt per realised list
/// container on every recycle) because paragraph runs and find highlights are data-dependent.
/// </summary>
internal static class FlowBlockRenderer
{
    /// <summary>Decoded inline images above this size are not shown (the placeholder is).</summary>
    internal const int MaxInlineImageBytes = 4 * 1024 * 1024;

    public static string AutomationNameFor(FlowBlock block) => block switch
    {
        ImageBlock image => string.IsNullOrWhiteSpace(image.Alt) ? "Image" : image.Alt,
        _ => block.PlainText,
    };

    public static UIElement Build(int entryIndex, FlowBlock block, FlowRenderContext ctx)
    {
        ctx.Highlights.TryGetValue(entryIndex, out var highlights);
        var cur = ctx.CurrentEntryIndex == entryIndex ? ctx.CurrentStart : -1;
        UIElement content = block switch
        {
            HeadingBlock h => BuildHeading(h, highlights, ctx, cur),
            ParagraphBlock p => BuildParagraph(p, highlights, ctx, cur),
            ImageBlock i => BuildImage(i, ctx),
            ListBlock l => BuildList(l, highlights, ctx, cur),
            TableBlock t => BuildTable(t, highlights, ctx, cur),
            _ => new Border(),
        };

        // Centred column capped at ~70 characters; the host stretches, the column does not.
        return new Border
        {
            MaxWidth = ctx.ColumnMaxWidth,
            HorizontalAlignment = HorizontalAlignment.Center,
            Margin = new Thickness(24, 0, 24, ctx.FontSize * 0.8),
            Child = content,
        };
    }

    // ── Text ───────────────────────────────────────────────────────────────

    private static RichTextBlock NewText(FlowRenderContext ctx, double fontSize, Windows.UI.Text.FontWeight weight)
    {
        var rtb = new RichTextBlock
        {
            IsTextSelectionEnabled = true,
            TextWrapping = TextWrapping.Wrap,
            FontSize = fontSize,
            FontFamily = ctx.BodyFont,
            LineHeight = fontSize / ctx.FontSize * ctx.LineHeight,
            LineStackingStrategy = LineStackingStrategy.BlockLineHeight,
            FontWeight = weight,
        };
        if (!ctx.HighContrast && ctx.Foreground is not null) rtb.Foreground = ctx.Foreground;
        return rtb;
    }

    private static List<TextHighlighter> BuildHighlighters(IEnumerable<BlockHighlight>? highlights, FlowRenderContext ctx, int offset, int limit, int currentStart)
    {
        var result = new List<TextHighlighter>(2);
        if (highlights is null) return result;
        TextHighlighter? others = null, current = null;
        foreach (var h in highlights)
        {
            var start = h.Start - offset;
            if (start < 0 || start + h.Length > limit) continue;
            var range = new TextRange { StartIndex = start, Length = h.Length };
            if (h.Start == currentStart)
            {
                current ??= new TextHighlighter { Background = ctx.CurrentMatchBackground, Foreground = ctx.MatchForeground };
                current.Ranges.Add(range);
            }
            else
            {
                others ??= new TextHighlighter { Background = ctx.MatchBackground, Foreground = ctx.MatchForeground };
                others.Ranges.Add(range);
            }
        }
        if (others is not null) result.Add(others);
        if (current is not null) result.Add(current);
        return result;
    }

    private static void ApplyHighlights(RichTextBlock rtb, IEnumerable<BlockHighlight>? highlights, FlowRenderContext ctx, int currentStart, int offset = 0, int limit = int.MaxValue)
    {
        foreach (var h in BuildHighlighters(highlights, ctx, offset, limit, currentStart)) rtb.TextHighlighters.Add(h);
    }

    private static void ApplyHighlights(TextBlock tb, IEnumerable<BlockHighlight>? highlights, FlowRenderContext ctx, int offset, int limit, int currentStart)
    {
        foreach (var h in BuildHighlighters(highlights, ctx, offset, limit, currentStart)) tb.TextHighlighters.Add(h);
    }

    private static UIElement BuildHeading(HeadingBlock h, List<BlockHighlight>? highlights, FlowRenderContext ctx, int cur)
    {
        var scale = h.Level switch { 1 => 1.8, 2 => 1.5, 3 => 1.25, _ => 1.1 };
        var rtb = NewText(ctx, ctx.FontSize * scale, FontWeights.SemiBold);
        var p = new Paragraph();
        p.Inlines.Add(new Run { Text = h.Text });
        rtb.Blocks.Add(p);
        rtb.Margin = new Thickness(0, ctx.FontSize * 0.8, 0, 0);
        ApplyHighlights(rtb, highlights, ctx, cur);
        AutomationProperties.SetHeadingLevel(rtb, h.Level switch
        {
            1 => AutomationHeadingLevel.Level1,
            2 => AutomationHeadingLevel.Level2,
            3 => AutomationHeadingLevel.Level3,
            4 => AutomationHeadingLevel.Level4,
            5 => AutomationHeadingLevel.Level5,
            _ => AutomationHeadingLevel.Level6,
        });
        return rtb;
    }

    private static UIElement BuildParagraph(ParagraphBlock block, List<BlockHighlight>? highlights, FlowRenderContext ctx, int cur)
    {
        var rtb = NewText(ctx, ctx.FontSize, FontWeights.Normal);
        var p = new Paragraph();
        foreach (var run in block.Runs)
        {
            var r = new Run { Text = run.Text };
            if (run.Bold) r.FontWeight = FontWeights.Bold;
            if (run.Italic) r.FontStyle = Windows.UI.Text.FontStyle.Italic;
            // Code spans are literal code, a content distinction: always monospaced whatever the font choice.
            if (run.Code) r.FontFamily = FlowRenderContext.CodeFont;
            p.Inlines.Add(r);
        }
        rtb.Blocks.Add(p);
        ApplyHighlights(rtb, highlights, ctx, cur);
        return rtb;
    }

    // ── List ───────────────────────────────────────────────────────────────

    private static UIElement BuildList(ListBlock list, List<BlockHighlight>? highlights, FlowRenderContext ctx, int cur)
    {
        var stack = new StackPanel { Spacing = ctx.FontSize * 0.3 };
        var offset = 0; // PlainText joins items with one space.
        for (var i = 0; i < list.Items.Count; i++)
        {
            var item = list.Items[i];
            var row = new Grid { ColumnSpacing = 8 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(ctx.FontSize * 2) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

            var prefix = new TextBlock
            {
                Text = list.Ordered ? string.Create(CultureInfo.InvariantCulture, $"{i + 1}.") : "•",
                FontSize = ctx.FontSize,
                FontFamily = ctx.BodyFont,
                HorizontalAlignment = HorizontalAlignment.Right,
                LineHeight = ctx.LineHeight,
                LineStackingStrategy = LineStackingStrategy.BlockLineHeight,
            };
            if (!ctx.HighContrast && ctx.SecondaryForeground is not null) prefix.Foreground = ctx.SecondaryForeground;
            AutomationProperties.SetAccessibilityView(prefix, AccessibilityView.Raw);

            var text = NewText(ctx, ctx.FontSize, FontWeights.Normal);
            var p = new Paragraph();
            p.Inlines.Add(new Run { Text = item });
            text.Blocks.Add(p);
            ApplyHighlights(text, highlights, ctx, cur, offset, item.Length);
            Grid.SetColumn(text, 1);

            row.Children.Add(prefix);
            row.Children.Add(text);
            stack.Children.Add(row);
            offset += item.Length + 1;
        }
        return stack;
    }

    // ── Table ──────────────────────────────────────────────────────────────

    private static UIElement BuildTable(TableBlock table, List<BlockHighlight>? highlights, FlowRenderContext ctx, int cur)
    {
        var columns = table.Rows.Count == 0 ? 0 : table.Rows.Max(r => r.Count);
        var grid = new Grid();
        for (var c = 0; c < columns; c++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 96 });
        }
        for (var r = 0; r < table.Rows.Count; r++) grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var offset = 0; // PlainText: tab between cells, newline between rows, one char each.
        for (var r = 0; r < table.Rows.Count; r++)
        {
            var row = table.Rows[r];
            var isHeader = table.HeaderRow && r == 0;
            for (var c = 0; c < row.Count; c++)
            {
                var cellText = row[c];
                var tb = new TextBlock
                {
                    Text = cellText,
                    TextWrapping = TextWrapping.Wrap,
                    IsTextSelectionEnabled = true,
                    FontSize = ctx.FontSize * 0.95,
                    FontFamily = ctx.BodyFont,
                    FontWeight = isHeader ? FontWeights.SemiBold : FontWeights.Normal,
                };
                if (!ctx.HighContrast && ctx.Foreground is not null) tb.Foreground = ctx.Foreground;
                ApplyHighlights(tb, highlights, ctx, offset, cellText.Length, cur);
                var cell = new Border
                {
                    BorderThickness = new Thickness(1),
                    Padding = new Thickness(10, 6, 10, 6),
                    Child = tb,
                };
                if (ctx.HighContrast)
                {
                    cell.BorderBrush = (Brush)Application.Current.Resources["SystemControlForegroundBaseHighBrush"];
                }
                else
                {
                    if (ctx.Border is not null) cell.BorderBrush = ctx.Border;
                    if (isHeader && ctx.HeaderFill is not null) cell.Background = ctx.HeaderFill;
                }
                Grid.SetRow(cell, r);
                Grid.SetColumn(cell, c);
                grid.Children.Add(cell);
                offset += cellText.Length + 1;
            }
        }

        AutomationProperties.SetName(grid, string.Create(CultureInfo.InvariantCulture,
            $"Table, {table.Rows.Count} rows by {columns} columns"));

        // A wide table scrolls sideways inside the column rather than squashing its cells.
        return new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollMode = ScrollMode.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollMode = ScrollMode.Disabled,
            Content = grid,
        };
    }

    // ── Image ──────────────────────────────────────────────────────────────

    private static UIElement BuildImage(ImageBlock image, FlowRenderContext ctx)
    {
        var stack = new StackPanel { Spacing = 6 };
        var name = AutomationNameFor(image);

        // Src policy: the IR stores a path/URL the importer saw, never image bytes, so the only source this
        // reader will draw is an inline data: image of a raster type, size-capped. http(s), file, relative and
        // every other scheme are NOT fetched or opened (no network, no local path taken from document content);
        // they get the placeholder, which still carries the alt text as its Narrator name.
        var picture = TryBuildInlineImage(image.Src, name);
        if (picture is not null)
        {
            stack.Children.Add(picture);
        }
        else
        {
            var icon = new FontIcon { Glyph = "", FontSize = 24 };
            var label = new TextBlock
            {
                Text = string.IsNullOrWhiteSpace(image.Alt) ? "Image not shown" : image.Alt,
                TextWrapping = TextWrapping.Wrap,
                VerticalAlignment = VerticalAlignment.Center,
                FontFamily = ctx.BodyFont,
                FontSize = ctx.FontSize,
            };
            var placeholder = new Border
            {
                Padding = new Thickness(16),
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(4),
                Child = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12, Children = { icon, label } },
            };
            if (ctx.HighContrast)
            {
                placeholder.BorderBrush = (Brush)Application.Current.Resources["SystemControlForegroundBaseHighBrush"];
            }
            else
            {
                if (ctx.Border is not null) placeholder.BorderBrush = ctx.Border;
                if (ctx.SecondaryForeground is not null) icon.Foreground = ctx.SecondaryForeground;
                if (ctx.Foreground is not null) label.Foreground = ctx.Foreground;
            }
            AutomationProperties.SetName(placeholder, name);
            stack.Children.Add(placeholder);
        }

        if (!string.IsNullOrWhiteSpace(image.Caption))
        {
            var caption = new TextBlock
            {
                Text = image.Caption,
                TextWrapping = TextWrapping.Wrap,
                FontStyle = Windows.UI.Text.FontStyle.Italic,
                FontSize = ctx.FontSize * 0.9,
                FontFamily = ctx.BodyFont,
                IsTextSelectionEnabled = true,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
            if (!ctx.HighContrast && ctx.SecondaryForeground is not null) caption.Foreground = ctx.SecondaryForeground;
            stack.Children.Add(caption);
        }
        return stack;
    }

    private static Image? TryBuildInlineImage(string src, string altName)
    {
        try
        {
            if (!TryDecodeDataImage(src, out var bytes)) return null;

            var bitmap = new BitmapImage();
            var image = new Image { Source = bitmap, Stretch = Stretch.Uniform, HorizontalAlignment = HorizontalAlignment.Center };
            AutomationProperties.SetName(image, altName);
            _ = SetSourceAsync(bitmap, bytes);
            return image;
        }
        catch (Exception)
        {
            return null; // fall back to the placeholder; never surface exception text
        }
    }

    private static async Task SetSourceAsync(BitmapImage bitmap, byte[] bytes)
    {
        try
        {
            using var stream = new InMemoryRandomAccessStream();
            using (var writer = new DataWriter(stream))
            {
                writer.WriteBytes(bytes);
                await writer.StoreAsync();
                writer.DetachStream();
            }
            stream.Seek(0);
            await bitmap.SetSourceAsync(stream);
        }
        catch (Exception)
        {
            // Undecodable image data: the Image stays empty (its name still reads the alt text).
        }
    }

    /// <summary>
    /// Accepts only <c>data:image/(png|jpeg|gif|bmp);base64,…</c> whose decoded size is within
    /// <see cref="MaxInlineImageBytes"/>.
    /// </summary>
    internal static bool TryDecodeDataImage(string? src, out byte[] bytes)
    {
        bytes = [];
        if (string.IsNullOrEmpty(src) || !src.StartsWith("data:image/", StringComparison.OrdinalIgnoreCase)) return false;
        var comma = src.IndexOf(',');
        if (comma < 0 || comma > 64) return false;
        var header = src[..comma].ToLowerInvariant();
        var typeOk = header.StartsWith("data:image/png;", StringComparison.Ordinal)
                     || header.StartsWith("data:image/jpeg;", StringComparison.Ordinal)
                     || header.StartsWith("data:image/gif;", StringComparison.Ordinal)
                     || header.StartsWith("data:image/bmp;", StringComparison.Ordinal);
        if (!typeOk || !header.EndsWith(";base64", StringComparison.Ordinal)) return false;
        var payloadLength = src.Length - comma - 1;
        if (payloadLength / 4 * 3 > MaxInlineImageBytes + 3) return false;
        try
        {
            bytes = Convert.FromBase64String(src[(comma + 1)..]);
            return bytes.Length is > 0 and <= MaxInlineImageBytes;
        }
        catch (FormatException)
        {
            bytes = [];
            return false;
        }
    }
}
