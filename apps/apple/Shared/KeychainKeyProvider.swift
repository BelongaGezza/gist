import Foundation
import Security

/// Keychain-backed implementation of the `KeyProvider` callback interface
/// generated from `gist-ffi`'s `#[uniffi::export(callback_interface)]`
/// `KeyProvider` trait (ADR-011). Rust calls `getOrCreateKey()` whenever
/// `gist-store` needs the AES-256 key it encrypts document content at rest
/// with; this class is where that key actually lives — in the macOS/iOS
/// Keychain, never in Rust, never on disk in plaintext.
///
/// **Not yet wired into `CoreClient`'s production `init()`** — see
/// `docs/adr/011-encryption-at-rest.md`'s "What was not implemented"
/// section. This class exists and is ready to use
/// (`GistCore.newEncrypted(dbPath:storageDir:keyProvider:)`), but switching
/// `CoreClient.shared` over to it is a deliberate follow-up step, not done
/// here, since it changes the on-disk format for every real user's existing
/// data and deserves its own verified rollout rather than being a side
/// effect of this security-hardening pass.
final class KeychainKeyProvider: KeyProvider {
    /// Keychain item identifiers. `service` scopes this to GIST specifically
    /// (Keychain items are shared across every app that uses the same
    /// service/account pair otherwise); `account` names which key this is,
    /// in case a future ADR needs more than one (e.g. per-library-profile
    /// keys) without colliding with this one.
    private let service = "com.gist.macos.encryption-at-rest"
    private let account = "document-content-key"

    /// Key length in bytes for AES-256 — must match `gist_store`'s
    /// `KeyProvider::get_or_create_key`'s `[u8; 32]` exactly, or the Rust
    /// side panics (caught by `ffi_catch!`, surfacing as
    /// `GistError.InternalPanic`) rather than silently truncating/padding.
    private let keyLengthBytes = 32

    func getOrCreateKey() -> Data {
        if let existing = readKey() {
            return existing
        }

        var newKey = Data(count: keyLengthBytes)
        let result = newKey.withUnsafeMutableBytes { buffer in
            SecRandomCopyBytes(kSecRandomDefault, keyLengthBytes, buffer.baseAddress!)
        }
        guard result == errSecSuccess else {
            // Matches this codebase's existing panic-at-a-well-defined-
            // boundary convention (see ffi_catch!'s doc comment on the Rust
            // side) rather than returning a zeroed/predictable key, which
            // would silently defeat the whole point of this feature.
            fatalError("KeychainKeyProvider: SecRandomCopyBytes failed with status \(result)")
        }

        if store(key: newKey) {
            return newKey
        }

        // Lost the race: another call's key is the one actually persisted,
        // not `newKey` — returning `newKey` here would silently encrypt
        // under a key that's never in Keychain, making that content
        // unreadable the moment a later launch reads the *other* call's
        // winning key back. Re-read the now-guaranteed-to-exist winner
        // instead. `fatalError` (not a fallback to `newKey`) if it's
        // somehow still missing, since that would mean SecItemAdd reported
        // a duplicate for an item that isn't actually there — a Keychain
        // state this code has no safe way to reason about.
        guard let winning = readKey() else {
            fatalError("KeychainKeyProvider: lost SecItemAdd race but no key found on re-read")
        }
        return winning
    }

    private func readKey() -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess, let data = item as? Data else {
            return nil
        }
        return data
    }

    /// Attempts to persist `key` as the one and only stored key. Returns
    /// `true` if `key` is now (or already was, byte-for-byte can't be
    /// assumed — see below) the persisted value, `false` if this call lost a
    /// race to another `getOrCreateKey()` call that stored first — in which
    /// case `key` was never written and the caller must not use it.
    private func store(key: Data) -> Bool {
        // `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` is a deliberate
        // starting point, not a final decision: it never leaves this device
        // (no iCloud Keychain sync), which is the safer default for a key
        // that, if it ever did sync, would need this app's whole
        // encryption-at-rest threat model reconsidered. Revisit only as part
        // of a real product decision about cross-device library access — see
        // ADR-011's "Harder / to watch" section.
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecValueData as String: key,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        ]

        let addStatus = SecItemAdd(query as CFDictionary, nil)
        if addStatus == errSecDuplicateItem {
            // Lost a race with another call that created the key first
            // (e.g. two near-simultaneous launches). `key` was never
            // written — the caller must re-read to get the actual winner,
            // not assume `key` itself is now valid.
            return false
        }
        guard addStatus == errSecSuccess else {
            fatalError("KeychainKeyProvider: SecItemAdd failed with status \(addStatus)")
        }
        return true
    }
}
