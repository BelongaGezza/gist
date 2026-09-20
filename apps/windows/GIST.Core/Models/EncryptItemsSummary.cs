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
