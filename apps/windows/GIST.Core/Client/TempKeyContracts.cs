// ─────────────────────────────────────────────────────────────────────────────
// TEMPORARY — DELETE THIS FILE.
//
// These are the key-custody contracts (ADR-016) that the key-custody workstream
// owns and is moving out of apps/windows/spikes/keyprovider into
// GIST.Core/Keys/ under this same namespace. They are duplicated here only so
// the CoreClient workstream could be developed in parallel, against the agreed
// shape, before that PR landed.
//
// On rebase onto the merged key-custody work: delete this whole file. Nothing
// else has to change — namespace, type names and members are identical to the
// agreed contract, and CoreClient only ever refers to IKeyProvider and the
// exception types, never to DpapiKeyProvider.
//
// Agreed contract (see docs/adr/016-windows-key-custody.md):
//   IKeyProvider { byte[] GetOrCreateKey(); }
//   KeyProviderException
//     ├── KeyStoreCorruptException
//     ├── KeyStoreIoException
//     └── KeyStoreUnavailableException
//   DpapiKeyProvider(string directory, byte[]? entropy = null)   (not duplicated here)
// ─────────────────────────────────────────────────────────────────────────────

namespace Gist.Core.Keys;

/// <summary>
/// Plain-C# mirror of uniffi's generated <c>KeyProvider.GetOrCreateKey()</c> callback interface
/// (gist-ffi, ADR-011). Contract: exactly 32 bytes, the same key for the life of the data.
/// </summary>
public interface IKeyProvider
{
    byte[] GetOrCreateKey();
}

/// <summary>Base type for all key-custody failures. Messages never contain key material.</summary>
public class KeyProviderException : Exception
{
    public KeyProviderException(string message, Exception? inner = null) : base(message, inner)
    {
    }
}

/// <summary>
/// The stored key exists but cannot be trusted (bad header, truncated, tampered, wrong
/// user/entropy, DPAPI master key lost, wrong decrypted length). A new key is NEVER generated in
/// this state: doing so would orphan every item encrypted under the old one.
/// </summary>
public sealed class KeyStoreCorruptException : KeyProviderException
{
    public KeyStoreCorruptException(string message, Exception? inner = null) : base(message, inner)
    {
    }
}

/// <summary>I/O problem (permissions, disk) that is not evidence of corruption — retryable.</summary>
public sealed class KeyStoreIoException : KeyProviderException
{
    public KeyStoreIoException(string message, Exception? inner = null) : base(message, inner)
    {
    }
}

/// <summary>
/// The key store cannot be reached right now for a reason that is expected to clear on its own
/// (transient platform failure, store temporarily locked). Retryable; never evidence of corruption.
/// </summary>
public sealed class KeyStoreUnavailableException : KeyProviderException
{
    public KeyStoreUnavailableException(string message, Exception? inner = null) : base(message, inner)
    {
    }
}
