using System.Globalization;

namespace Gist.Core.Flow;

/// <summary>Font family choice. The enum name is the persisted value (<see cref="TypographySettings.Serialize"/>).</summary>
public enum ReadingFontDesign
{
    Default,
    Serif,
    Rounded,
}

/// <summary>Extra spacing between lines, in added points (not a multiplier).</summary>
public enum LineSpacingOption
{
    Compact,
    Regular,
    Relaxed,
}

public static class LineSpacingOptionExtensions
{
    public static double ExtraPoints(this LineSpacingOption option) => option switch
    {
        LineSpacingOption.Compact => 2,
        LineSpacingOption.Regular => 6,
        LineSpacingOption.Relaxed => 12,
        _ => 6,
    };
}

/// <summary>
/// The "Aa" panel's three controls. Immutable: <c>with</c> a new value and the size is clamped on construction
/// into <see cref="MinFontSize"/>..<see cref="MaxFontSize"/> (a non-finite size becomes <see cref="DefaultFontSize"/>).
/// Code spans always render monospaced regardless of <see cref="FontDesign"/>; that is content, not preference.
/// </summary>
public sealed record TypographySettings
{
    public const double MinFontSize = 13;
    public const double MaxFontSize = 28;
    public const double DefaultFontSize = 17;

    private readonly double _fontSize = DefaultFontSize;

    public static TypographySettings Default { get; } = new();

    public double FontSize
    {
        get => _fontSize;
        init => _fontSize = double.IsFinite(value) ? Math.Clamp(value, MinFontSize, MaxFontSize) : DefaultFontSize;
    }

    public ReadingFontDesign FontDesign { get; init; } = ReadingFontDesign.Default;

    public LineSpacingOption LineSpacing { get; init; } = LineSpacingOption.Regular;

    public TypographySettings WithFontSize(double size) => this with { FontSize = size };

    /// <summary>Stable one-line form: <c>size=17;font=Default;spacing=Regular</c> (invariant culture).</summary>
    public string Serialize() =>
        string.Create(CultureInfo.InvariantCulture, $"size={FontSize};font={FontDesign};spacing={LineSpacing}");

    /// <summary>
    /// Lenient inverse of <see cref="Serialize"/>: any missing, unknown or malformed part falls back to its
    /// default, so an old or hand-edited file never blocks reading. Never throws.
    /// </summary>
    public static TypographySettings Parse(string? text)
    {
        var result = Default;
        if (string.IsNullOrWhiteSpace(text) || text.Length > 256) return result;
        foreach (var part in text.Split(';'))
        {
            var eq = part.IndexOf('=');
            if (eq <= 0) continue;
            var key = part[..eq].Trim();
            var value = part[(eq + 1)..].Trim();
            switch (key)
            {
                case "size" when double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var size):
                    result = result with { FontSize = size };
                    break;
                case "font" when Enum.TryParse<ReadingFontDesign>(value, ignoreCase: false, out var font) && Enum.IsDefined(font):
                    result = result with { FontDesign = font };
                    break;
                case "spacing" when Enum.TryParse<LineSpacingOption>(value, ignoreCase: false, out var spacing) && Enum.IsDefined(spacing):
                    result = result with { LineSpacing = spacing };
                    break;
            }
        }
        return result;
    }
}

/// <summary>
/// Observable holder for the live <see cref="TypographySettings"/>, shared between the "Aa" menu and the reading
/// layout. Raises <see cref="Changed"/> only when the value actually changes.
/// </summary>
public sealed class TypographyState
{
    private TypographySettings _settings;

    public TypographyState(TypographySettings? initial = null) => _settings = initial ?? TypographySettings.Default;

    public event EventHandler? Changed;

    public TypographySettings Settings
    {
        get => _settings;
        set
        {
            ArgumentNullException.ThrowIfNull(value);
            if (_settings == value) return;
            _settings = value;
            Changed?.Invoke(this, EventArgs.Empty);
        }
    }
}

/// <summary>
/// Remembers typography in a one-line text file (same best-effort trade-off as
/// <c>FileRsvpSettingsStore</c>): an unreadable file yields defaults, a failed write is swallowed.
/// </summary>
public sealed class FileTypographySettingsStore
{
    private const int MaxFileBytes = 1024;
    private readonly string _path;

    /// <param name="path">Absolute file path. The parent directory must already exist.</param>
    public FileTypographySettingsStore(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        _path = path;
    }

    public TypographySettings Load()
    {
        try
        {
            if (!File.Exists(_path)) return TypographySettings.Default;
            using var stream = new FileStream(_path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
            var buffer = new byte[MaxFileBytes];
            var read = stream.Read(buffer, 0, buffer.Length);
            return TypographySettings.Parse(System.Text.Encoding.UTF8.GetString(buffer, 0, read));
        }
        catch (IOException) { return TypographySettings.Default; }
        catch (UnauthorizedAccessException) { return TypographySettings.Default; }
    }

    public void Save(TypographySettings settings)
    {
        ArgumentNullException.ThrowIfNull(settings);
        try
        {
            File.WriteAllText(_path, settings.Serialize());
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}
