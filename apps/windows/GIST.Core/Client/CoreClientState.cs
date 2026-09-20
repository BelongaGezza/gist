namespace Gist.Core.Client;

/// <summary>
/// Lifecycle state of a <see cref="CoreClient"/>. Everything except <see cref="Ready"/> means no
/// <c>GistCore</c> exists and every operation fails fast without touching the FFI.
/// </summary>
public enum CoreClientState
{
    /// <summary><see cref="CoreClient.InitializeAsync"/> has not run yet.</summary>
    Uninitialized = 0,

    /// <summary>The store is open and usable.</summary>
    Ready,

    /// <summary>
    /// <b>Blocking.</b> The key could not be recovered or is not a valid 32-byte key
    /// (ADR-016). Encrypted items are unreadable; unencrypted ones would be readable but the
    /// client deliberately does not open the store with a substitute key, and
    /// <b>never generates a replacement key</b> — that would orphan every encrypted item.
    /// <see cref="CoreClient.RetryAsync"/> is still offered (review Q9): it only re-reads, and
    /// deletes or recreates nothing.
    /// </summary>
    KeyStoreCorrupt,

    /// <summary>
    /// The key store was temporarily unreachable (I/O, permissions, transient platform failure).
    /// Retryable via <see cref="CoreClient.RetryAsync"/>.
    /// </summary>
    KeyStoreUnavailable,

    /// <summary>
    /// The key was obtained but the store itself would not open (bad path, locked database, …).
    /// Retryable.
    /// </summary>
    StoreUnavailable,
}
