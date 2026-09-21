namespace Gist.Core.ViewModels;

/// <summary>
/// Everything the Encrypt confirmation dialog needs (<c>docs/windows-ui-spec.md</c> §4.5, ADR-014).
/// </summary>
/// <remarks>
/// <para>
/// <b>Maintainer decision, 2026-09-21: there is no key recovery in v1.</b> The content key is
/// DPAPI-protected for the current Windows user (ADR-016), so a lost or reset Windows profile takes
/// the key with it, and an encrypted item's text is then gone for good — GIST keeps no escrow copy
/// and the plan's optional recovery-key export (risk WR7) is a v1.1 item at best. That consequence
/// has to be stated in the dialog, in plain words, <em>before</em> the user commits, which is why
/// <see cref="RecoveryWarning"/> is a required part of this record rather than a nicety the XAML
/// might drop.
/// </para>
/// <para>
/// The other two lines exist because the feature is easy to misread as something it is not:
/// encryption is opt-in per item (<see cref="OptionalNote"/>), and it never reaches the user's own
/// files or GIST's ADR-006 stored copy (<see cref="OriginalsNote"/>) — only the item's
/// <c>.json</c>/<c>.tokens.json</c> blobs.
/// </para>
/// </remarks>
/// <param name="Count">How many items the confirmation covers.</param>
/// <param name="Title">Dialog title, e.g. <c>"Encrypt 3 items?"</c>.</param>
/// <param name="Message">What encryption does, and that the items stay readable in the app.</param>
/// <param name="RecoveryWarning">The no-recovery warning. Never empty.</param>
/// <param name="OptionalNote">That encryption is per item and optional.</param>
/// <param name="OriginalsNote">That originals and stored copies are untouched.</param>
public sealed record EncryptPreview(
    int Count,
    string Title,
    string Message,
    string RecoveryWarning,
    string OptionalNote,
    string OriginalsNote)
{
    /// <summary>
    /// The no-recovery warning shown on every Encrypt confirmation. Deliberately concrete about the
    /// two realistic ways a Windows user loses a DPAPI key — a reset PC and a lost or recreated
    /// profile — rather than hiding behind "key material".
    /// </summary>
    public const string NoRecoveryWarning =
        "There is no way to recover encrypted items if your Windows account is lost or reset. "
        + "The key is tied to this Windows user profile on this PC. If the profile is deleted, "
        + "the PC is reset, or you move to another machine, encrypted items can no longer be opened, "
        + "and GIST cannot restore them. There is no recovery code and no backup key in this version.";

    /// <summary>That encryption is per item and entirely optional.</summary>
    public const string OptionalPerItemNote =
        "Encrypting is optional and applies only to the items you select. You can leave the rest of "
        + "your library unencrypted, and nothing is encrypted automatically.";

    /// <summary>That neither the user's file nor GIST's stored copy is touched.</summary>
    public const string OriginalsUntouchedNote =
        "Your original files are never touched, and neither is the copy GIST stored when importing.";

    /// <summary>What encryption does, and that the items remain readable in this app.</summary>
    public const string WhatItDoes =
        "Encrypts the selected items' text on disk. They stay readable in GIST on this PC — both "
        + "readers keep working — this only protects the files at rest.";

    /// <summary>Builds the preview for a selection of <paramref name="count"/> items.</summary>
    public static EncryptPreview For(int count) => new(
        count,
        count == 1 ? "Encrypt 1 item?" : $"Encrypt {count} items?",
        WhatItDoes,
        NoRecoveryWarning,
        OptionalPerItemNote,
        OriginalsUntouchedNote);
}
