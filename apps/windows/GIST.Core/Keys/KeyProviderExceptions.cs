namespace Gist.Core.Keys;

/// <summary>Base type for all key-custody failures. Messages never contain key material.</summary>
public class KeyProviderException : Exception
{
    public KeyProviderException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>
/// The stored key file exists and is <b>provably</b> unusable: the header is not ours, the file is
/// empty or truncated, the DPAPI blob is structurally invalid or fails its integrity check, or the
/// decrypted payload is not <see cref="DpapiKeyProvider.KeyLength"/> bytes.
/// <para>
/// A new key is NEVER generated in this state and the file is never modified: doing so would orphan
/// every item encrypted under the old key. The app must surface an "encrypted items are
/// unrecoverable" state (ADR-016), and must not offer a silent reset.
/// </para>
/// <para>
/// This is deliberately <b>not</b> the catch-all: anything that is merely "the platform could not
/// decrypt right now" is <see cref="KeyStoreUnavailableException"/>, because routing a transient
/// failure to the unrecoverable state would tell a user their library is lost when it is not
/// (review finding Q9).
/// </para>
/// </summary>
public sealed class KeyStoreCorruptException : KeyProviderException
{
    public KeyStoreCorruptException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>
/// The key could not be read or created <b>right now</b>, and nothing proves it is damaged: the
/// DPAPI master key is not available (user profile not loaded, credential state in flux, roaming
/// profile still syncing), or the platform returned an error we do not recognise.
/// <para>
/// Retryable. The caller should offer Retry and must never delete, overwrite or recreate the key
/// file on the strength of this exception — the same "fail closed, change nothing" rule as
/// <see cref="KeyStoreCorruptException"/>, but with a different message to the user.
/// </para>
/// </summary>
public sealed class KeyStoreUnavailableException : KeyProviderException
{
    public KeyStoreUnavailableException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>I/O problem (permissions, disk, sharing violation) that is not evidence of corruption. Retryable.</summary>
public sealed class KeyStoreIoException : KeyProviderException
{
    public KeyStoreIoException(string message, Exception? inner = null) : base(message, inner) { }
}
