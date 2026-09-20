using System.Runtime.CompilerServices;
using System.Security.Cryptography;

// Exposes the classifier (and only it) to the test assembly: a transient DPAPI failure cannot be
// provoked from outside the process, so the mapping is tested directly rather than not at all.
[assembly: InternalsVisibleTo("GIST.Core.Tests")]

namespace Gist.Core.Keys;

/// <summary>
/// Decides whether a DPAPI failure is <b>proven corruption</b> or a <b>transient</b> condition
/// (review finding Q9: the spike classed every <see cref="CryptographicException"/> as corruption,
/// which would tell a user their encrypted library is unrecoverable because, say, their profile
/// was not loaded yet).
/// </summary>
/// <remarks>
/// The classification is an allow-list: only codes whose documented meaning is "these bytes are not
/// a valid, intact blob" are corruption. Everything else — including codes we have never seen — is
/// unavailable/retryable, because guessing "corrupt" is the destructive direction of the two.
/// Neither outcome ever modifies the key file.
/// <para>
/// Codes observed on Windows 11 with .NET 10 (measured, not assumed — see the ADR-016 addendum):
/// tampered blob, truncated blob and wrong entropy all give <c>ERROR_INVALID_DATA</c> (0x8007000D);
/// a payload that is not a DPAPI blob at all (garbage, empty) gives <c>ERROR_INVALID_PARAMETER</c>
/// (0x80070057). A missing DPAPI master key surfaces as <c>ERROR_FILE_NOT_FOUND</c> (0x80070002)
/// or an <c>NTE_*</c> code, none of which are in the corruption list.
/// </para>
/// </remarks>
internal static class KeyStoreFailureClassifier
{
    /// <summary>ERROR_INVALID_DATA — the blob failed its integrity check (tampered, truncated, wrong entropy).</summary>
    internal const int ErrorInvalidData = unchecked((int)0x8007000D);

    /// <summary>ERROR_INVALID_PARAMETER — the payload is not a DPAPI blob (garbage, empty, wrong format).</summary>
    internal const int ErrorInvalidParameter = unchecked((int)0x80070057);

    /// <summary>
    /// Maps a DPAPI <see cref="CryptographicException"/> onto the typed hierarchy.
    /// Never returns <c>null</c>; never has side effects.
    /// </summary>
    internal static KeyProviderException Classify(CryptographicException error) =>
        IsProvenCorruption(error)
            ? new KeyStoreCorruptException(
                "The stored key file is damaged (failed its integrity check or is not a valid key blob). " +
                "It has been left untouched; items encrypted with it cannot be opened.", error)
            : new KeyStoreUnavailableException(
                "The stored key could not be unwrapped right now (the Windows data-protection master key " +
                "is unavailable — for example the user profile is not fully loaded). Nothing was changed; retry.",
                error);

    internal static bool IsProvenCorruption(CryptographicException error) =>
        error.HResult is ErrorInvalidData or ErrorInvalidParameter;
}
