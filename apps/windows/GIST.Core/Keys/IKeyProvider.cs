namespace Gist.Core.Keys;

/// <summary>
/// Plain-C# mirror of uniffi's generated <c>KeyProvider.GetOrCreateKey()</c> callback interface
/// (gist-ffi, ADR-011). Contract: exactly 32 bytes, the same key for the life of the data.
/// </summary>
/// <remarks>
/// The uniffi callback has no error channel, so implementations must never be handed straight to
/// <c>NewWithReadKey</c>. <c>CoreClient</c> calls <see cref="GetOrCreateKey"/> eagerly in managed
/// code first, where the typed <see cref="KeyProviderException"/> hierarchy can be caught and
/// presented (ADR-016 "App behaviour on KeyStoreCorruptException").
/// </remarks>
public interface IKeyProvider
{
    /// <summary>Returns the persisted 32-byte content key, creating it once if it does not exist.</summary>
    /// <exception cref="KeyStoreCorruptException">The stored key is provably unusable; it is left untouched.</exception>
    /// <exception cref="KeyStoreUnavailableException">A transient failure; retrying later may succeed.</exception>
    /// <exception cref="KeyStoreIoException">An I/O failure reading or writing the key file.</exception>
    byte[] GetOrCreateKey();
}
