namespace Gist.Core.Client;

/// <summary>
/// The kinds of failure the UI has to tell apart. Matched by <b>type</b> across the FFI boundary
/// (docs/windows-development-plan.md §4.5) — never by parsing a message string.
/// </summary>
public enum CoreErrorKind
{
    /// <summary>The file is DRM-protected. GIST never circumvents DRM (ADR-004).</summary>
    DrmProtected,

    /// <summary>The file the user picked is not there any more. Detected in managed code.</summary>
    SourceFileMissing,

    /// <summary>
    /// The encryption key exists but cannot be trusted or used. Blocking: encrypted items cannot
    /// be opened and no replacement key is ever generated (ADR-016).
    /// </summary>
    KeyStoreCorrupt,

    /// <summary>The key store could not be reached right now. Retryable (ADR-016 / review Q9).</summary>
    KeyStoreUnavailable,

    /// <summary>
    /// The client has no usable core: it was never initialised, initialisation failed, or it is
    /// in a blocking key state. Every operation fails fast with this instead of touching FFI.
    /// </summary>
    NotInitialized,

    /// <summary>The store itself could not be opened (bad path, locked database, …).</summary>
    StoreUnavailable,

    /// <summary>
    /// A Rust-side panic was caught at the FFI boundary. The panic text is deliberately
    /// <b>never</b> propagated into <see cref="CoreError.Message"/>.
    /// </summary>
    InternalPanic,

    /// <summary>Any other error reported by the core.</summary>
    Core,

    /// <summary>An unexpected managed-side exception.</summary>
    Unexpected,
}

/// <summary>
/// A failure, in a shape the UI can present directly: a kind to branch on and a message that is
/// safe to show. Deliberately tiny — GIST has no error-code catalogue and does not need one.
/// </summary>
/// <remarks>
/// <para>
/// <see cref="Message"/> is user-facing prose. It never contains a Rust panic payload, and the
/// messages produced here never embed a file path or a document title, so it is safe to show
/// anywhere. (Where the UI needs the path — the DRM dialog — it reads
/// <see cref="CoreClient.DrmProtectedSource"/>, which is not part of any message.)
/// </para>
/// <para>
/// Nothing in this layer logs. Source paths and titles must never be logged above
/// <c>debug</c> (CLAUDE.md), and the simplest way to guarantee that is to emit no log records at
/// all from the client layer.
/// </para>
/// </remarks>
/// <param name="Kind">What went wrong, for branching.</param>
/// <param name="Message">A user-presentable sentence.</param>
public sealed record CoreError(CoreErrorKind Kind, string Message)
{
    /// <summary>Whether retrying the same operation could plausibly succeed.</summary>
    public bool IsRetryable => Kind is CoreErrorKind.KeyStoreUnavailable or CoreErrorKind.StoreUnavailable;

    /// <summary>
    /// Whether this failure blocks the whole client rather than one operation — the state where
    /// encrypted items are unreadable and the shell must show a blocking banner (ADR-016).
    /// </summary>
    public bool IsBlocking => Kind is CoreErrorKind.KeyStoreCorrupt or CoreErrorKind.NotInitialized;

    /// <summary>For callers that would rather throw than inspect a property.</summary>
    public CoreClientException ToException() => new(this);
}

/// <summary>
/// Exception form of <see cref="CoreError"/>, for call sites that prefer exceptions. The client
/// itself does not throw these: it publishes <see cref="CoreClient.LastError"/> so a view can bind
/// to it, exactly as Apple's <c>CoreClient</c> publishes <c>error</c>.
/// </summary>
public sealed class CoreClientException : Exception
{
    public CoreClientException(CoreError error)
        : base(error.Message)
    {
        Error = error;
    }

    public CoreError Error { get; }

    public CoreErrorKind Kind => Error.Kind;
}
