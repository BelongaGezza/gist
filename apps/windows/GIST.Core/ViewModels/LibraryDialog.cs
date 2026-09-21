namespace Gist.Core.ViewModels;

/// <summary>
/// Which dialog the shell should be showing, per <c>docs/windows-ui-spec.md</c> §4.5.
/// </summary>
/// <remarks>
/// <para>
/// The view model owns <em>which</em> dialog is pending; the shell owns what a
/// <c>ContentDialog</c> looks like. Keeping the choice here means the dialog-triggering rules —
/// including the two that are easy to get wrong, "a DRM failure raises the DRM dialog, not the
/// generic one" and "a successful refresh must not silently dismiss an unacknowledged failure" —
/// are unit-testable without a XAML host.
/// </para>
/// <para>
/// Only one dialog is ever pending: every entry here is modal in the shell, so a second one could
/// not be shown anyway.
/// </para>
/// </remarks>
public enum LibraryDialog
{
    /// <summary>Nothing to show.</summary>
    None = 0,

    /// <summary>Import URL prompt (§4.5). Import is disabled while the box is blank.</summary>
    ImportUrl,

    /// <summary>New Collection name prompt (§4.5).</summary>
    NewCollection,

    /// <summary>Remove confirmation; content comes from <see cref="RemovePreview"/>.</summary>
    RemoveConfirm,

    /// <summary>Encrypt confirmation; content comes from <see cref="EncryptPreview"/>.</summary>
    EncryptConfirm,

    /// <summary>Encrypt result summary; content comes from <c>LastEncryptSummary</c>.</summary>
    EncryptResult,

    /// <summary>Tag editor for the single selected item (§6).</summary>
    TagEditor,

    /// <summary>
    /// The DRM dialog. Raised only from a structurally typed
    /// <see cref="Client.CoreErrorKind.DrmProtected"/>, never from message text (§10 item 4).
    /// </summary>
    DrmProtected,

    /// <summary>The generic error dialog, carrying a <see cref="Client.CoreError"/>.</summary>
    Error,
}

/// <summary>
/// Which of the three empty states (<c>docs/windows-ui-spec.md</c> §4.4) the list should render.
/// </summary>
public enum LibraryEmptyState
{
    /// <summary>The list has rows (or is still loading): render the list, not an empty state.</summary>
    None = 0,

    /// <summary>The library itself is empty.</summary>
    NoItems,

    /// <summary>A search is active and matched nothing.</summary>
    NoSearchHits,

    /// <summary>A tag filter is active and matched nothing.</summary>
    NoTagHits,
}
