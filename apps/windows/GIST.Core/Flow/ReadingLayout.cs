namespace Gist.Core.Flow;

/// <summary>
/// Requests a jump to a section (from the Contents flyout). The container sets the request, the hosting layout
/// reacts in whatever way suits its rendering and calls <see cref="TryConsume"/> so the same request is not
/// replayed. Requesting the same section twice is two requests (a user may jump back to where they were).
/// </summary>
public sealed class SectionNavigator
{
    private readonly FlowDocument? _document;

    public SectionNavigator(FlowDocument? document = null) => _document = document;

    public event EventHandler? Requested;

    public string? PendingSectionId { get; private set; }

    /// <summary>Asks for <paramref name="sectionId"/>. Unknown ids (when a document is known) are ignored.</summary>
    public void Request(string? sectionId)
    {
        if (string.IsNullOrEmpty(sectionId)) return;
        if (_document is not null && _document.FindSectionIndex(sectionId) < 0) return;
        PendingSectionId = sectionId;
        Requested?.Invoke(this, EventArgs.Empty);
    }

    /// <summary>Takes the pending request (clearing it); false when there is none.</summary>
    public bool TryConsume(out string sectionId)
    {
        if (PendingSectionId is { } id)
        {
            PendingSectionId = null;
            sectionId = id;
            return true;
        }
        sectionId = string.Empty;
        return false;
    }
}

/// <summary>Everything a reading layout is driven by; the container owns it and hands it to the layout.</summary>
public sealed record ReadingLayoutContext(
    FlowDocument Document,
    TypographyState Typography,
    FlowSearchState Search,
    SectionNavigator Navigation,
    ReadingProgress Progress);

/// <summary>
/// Seam between the flow container and a way of laying a document out. The scrolling flow view is the only
/// conformer today; the paginated view (Q3, open for a later release) is the plausible second one. Kept
/// UI-framework-free so the contract is testable and a conformer can be a WinUI control or anything else.
/// A layout renders <see cref="ReadingLayoutContext.Document"/> at the typography, reacts to the search cursor
/// and navigation requests, restores to <see cref="ReadingProgress.InitialFraction"/> once, and keeps
/// <see cref="ReadingProgress.Update"/> in sync with where the reader is.
/// </summary>
public interface IReadingLayout
{
    void Bind(ReadingLayoutContext context);
}
