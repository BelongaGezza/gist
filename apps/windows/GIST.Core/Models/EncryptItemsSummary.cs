namespace Gist.Core.Models;

/// <summary>
/// Tallied outcome of a bulk <c>EncryptItemsAsync</c> call (ADR-014), for the result dialog in
/// <c>docs/windows-ui-spec.md</c> §4.5. Mirrors Apple's <c>EncryptItemsSummary</c>.
/// </summary>
/// <param name="EncryptedCount">Items newly encrypted by this call.</param>
/// <param name="AlreadyEncryptedCount">Items that were already encrypted (idempotent no-op).</param>
/// <param name="FailedCount">Items the core could not encrypt.</param>
public sealed record EncryptItemsSummary(int EncryptedCount, int AlreadyEncryptedCount, int FailedCount)
{
    /// <summary>Most first-error lines kept for the result dialog.</summary>
    public const int MaxFirstErrors = 3;

    /// <summary>
    /// Up to <see cref="MaxFirstErrors"/> fixed-text reasons for failed items (spec §4.5 "+ first
    /// errors"). Produced by <see cref="ScrubFailure"/>, so never a path, title or raw core message.
    /// </summary>
    public IReadOnlyList<string> FirstErrors { get; init; } = Array.Empty<string>();

    /// <summary>
    /// Maps a raw per-item core error string (a Rust <c>Display</c>, which can embed a path, item id
    /// or OS error text) to fixed, presentable text by its stable leading kind tag. Anything
    /// unrecognised becomes a generic line; the raw text is never returned.
    /// </summary>
    public static string ScrubFailure(string? raw)
    {
        var text = raw ?? string.Empty;
        if (text.StartsWith("item not found", StringComparison.Ordinal))
        {
            return "An item could no longer be found in the library.";
        }

        if (text.StartsWith("io:", StringComparison.Ordinal))
        {
            return "An item's files could not be read or written.";
        }

        if (text.StartsWith("sqlite:", StringComparison.Ordinal))
        {
            return "The library database reported an error.";
        }

        if (text.StartsWith("decryption failed", StringComparison.Ordinal)
            || text.StartsWith("checksum mismatch", StringComparison.Ordinal))
        {
            return "An item's stored data appears to be damaged.";
        }

        return "An item could not be encrypted.";
    }

    public static EncryptItemsSummary Empty { get; } = new(0, 0, 0);

    public bool IsEmpty => EncryptedCount == 0 && AlreadyEncryptedCount == 0 && FailedCount == 0;

    /// <summary>
    /// A short human-readable line ("2 items encrypted, 1 was already encrypted."). Only mentions
    /// the parts that actually happened. Deliberately carries no item titles or paths, so it is
    /// safe to show anywhere without leaking library contents.
    /// </summary>
    public string Message
    {
        get
        {
            var parts = new List<string>(3);
            if (EncryptedCount > 0)
            {
                parts.Add($"{EncryptedCount} item{(EncryptedCount == 1 ? "" : "s")} encrypted");
            }

            if (AlreadyEncryptedCount > 0)
            {
                parts.Add($"{AlreadyEncryptedCount} {(AlreadyEncryptedCount == 1 ? "was" : "were")} already encrypted");
            }

            if (FailedCount > 0)
            {
                parts.Add($"{FailedCount} failed");
            }

            return parts.Count == 0 ? "No items were selected." : string.Join(", ", parts) + ".";
        }
    }
}
