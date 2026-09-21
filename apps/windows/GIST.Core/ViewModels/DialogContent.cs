using Gist.Core.Client;
using Gist.Core.Models;

namespace Gist.Core.ViewModels;

/// <summary>
/// All fixed wording, button labels and enabled rules for the Library dialogs
/// (<c>docs/windows-ui-spec.md</c> §4.5 and §6), kept UI-free so it is unit-tested and the XAML
/// layer holds no duplicated strings.
/// </summary>
/// <remarks>
/// Nothing here ever includes a raw exception/core message, a file path or a URL: error text is
/// chosen from <see cref="CoreErrorKind"/> alone.
/// </remarks>
public static class DialogContent
{
    // Import URL
    public const string ImportUrlTitle = "Import URL";
    public const string ImportUrlPlaceholder = "https://…";
    public const string ImportUrlBody =
        "GIST fetches the page, extracts the readable content, and adds it to your library.";
    public const string ImportUrlPrimary = "Import";

    // New Collection
    public const string NewCollectionTitle = "New Collection";
    public const string NewCollectionPlaceholder = "Collection name";
    public const string NewCollectionPrimary = "Create";

    // Remove
    public const string RemoveFromLibraryButton = "Remove from Library";
    public const string RemoveAlsoStoredCopyButton = "Also Delete Stored Copy";
    public const string CollectionRemoveButton = "Remove";

    // Encrypt
    public const string EncryptPrimary = "Encrypt";
    public const string EncryptResultTitle = "Encryption finished";

    // Shared
    public const string CancelButton = "Cancel";
    public const string OkButton = "OK";

    // DRM
    public const string DrmTitle = "Can't import this book";
    public const string DrmMessage = "This book is DRM-protected and can't be imported.";

    // Error
    public const string ErrorTitle = "Something went wrong";

    // Tag editor
    public const string TagEditorTitleFallback = "Tags";
    public const string TagEditorNoTags = "No tags yet.";
    public const string TagEditorAddPlaceholder = "Add a tag";
    public const string TagEditorAddButton = "Add";
    public const string TagEditorDoneButton = "Done";

    /// <summary>Import is enabled only for non-blank input (§4.5).</summary>
    public static bool CanImportUrl(string? text) => !string.IsNullOrWhiteSpace(text);

    /// <summary>Create is enabled only for a non-blank name.</summary>
    public static bool CanCreateCollection(string? text) => !string.IsNullOrWhiteSpace(text);

    /// <summary>Add (tag) is enabled only for a non-blank name.</summary>
    public static bool CanAddTag(string? text) => !string.IsNullOrWhiteSpace(text);

    /// <summary>Accessible name for a tag's remove button.</summary>
    public static string RemoveTagAutomationName(string tag) => $"Remove tag {tag}";

    /// <summary>
    /// A fixed, generic sentence for a failure of the given kind. Never the core's own message.
    /// </summary>
    public static string ErrorMessage(CoreErrorKind? kind) => kind switch
    {
        CoreErrorKind.DrmProtected => DrmMessage,
        CoreErrorKind.SourceFileMissing => "The file could not be found. It may have been moved or deleted.",
        CoreErrorKind.KeyStoreCorrupt => "GIST can't use its encryption key, so encrypted items can't be opened.",
        CoreErrorKind.KeyStoreUnavailable => "GIST couldn't reach its encryption key. Please try again.",
        CoreErrorKind.NotInitialized => "The library isn't ready yet. Please try again.",
        CoreErrorKind.StoreUnavailable => "The GIST library couldn't be opened.",
        CoreErrorKind.InternalPanic => "GIST hit an unexpected problem and stopped that action. Please try again.",
        _ => "That action couldn't be completed.",
    };

    /// <summary>
    /// Body lines of the Encrypt result dialog: the tally message, then any scrubbed first errors
    /// under a heading (§4.5 "+ first errors").
    /// </summary>
    public static IReadOnlyList<string> EncryptResultLines(EncryptItemsSummary? summary)
    {
        var s = summary ?? EncryptItemsSummary.Empty;
        var lines = new List<string> { s.Message };
        if (s.FailedCount > 0 && s.FirstErrors.Count > 0)
        {
            lines.Add("Why:");
            lines.AddRange(s.FirstErrors.Select(e => "• " + e));
        }

        return lines;
    }
}
