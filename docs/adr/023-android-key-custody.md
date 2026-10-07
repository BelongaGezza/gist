# ADR 023 — Android key custody: Hardware-backed Android Keystore wrapping

**Date:** 2026-10-07  
**Status:** Proposed  

## Context

GIST implements opt-in, per-item encryption at rest using AES-256-GCM in `gist-store` (ADR-011, ADR-014). Key custody is delegated to the host platform via the `KeyProvider` callback interface:
```rust
pub trait KeyProvider: Send + Sync {
    fn get_or_create_key(&self) -> Vec<u8>; // Exactly 32 bytes
}
```
On macOS/iOS, custody is implemented via the Apple Keychain (`kSecClassGenericPassword`, `KeychainKeyProvider.swift`). On Windows, it uses DPAPI CurrentUser (`content-key.dpapi`, ADR-016).

On Android, we need to satisfy the same security contract:
1. Returns **exactly 32 bytes**.
2. **Persistent:** Returns the identical key across process launches for the life of the data.
3. **Race-safe creation:** Two concurrent threads or background processes attempting initial key generation must converge on the exact same 32-byte key without deadlocks or corrupted writes.
4. **Hardware-backed protection:** Protect against extraction by other apps, physical storage inspection, and offline disk dumps.
5. **Fail closed on corruption:** If the stored key is tampered with or corrupted, fail cleanly with a typed exception rather than silently generating a new key (which would permanently orphan previously encrypted content).

## Decision

Implement `AndroidKeystoreKeyProvider` in `apps/android/core/` implementing the generated UniFFI `KeyProvider` interface using the **Android Keystore System** (`AndroidKeyStore`).

### Architecture & Storage Design

1. **Master Key (Key Encryption Key - KEK):**
   - Generated inside the Android Keystore using `KeyGenerator` with algorithm `AES` and block mode `GCM`, key size 256 bits, alias `"gist_master_key"`.
   - Backed by hardware security modules: Trusted Execution Environment (TEE) or StrongBox Keymaster (`KeyGenParameterSpec.Builder.setIsStrongBoxBacked(true)` attempted first with fallback to standard TEE).
   - Non-exportable: The private master key material never leaves the secure hardware.
2. **Content Key (Data Encryption Key - DEK):**
   - 32 cryptographically secure random bytes generated via `SecureRandom`.
   - Encrypted on-device using the Android Keystore master key with `AES/GCM/NoPadding` (authenticated encryption, with a 12-byte initialization vector and 128-bit authentication tag).
   - Stored in a single file `content-key.keystore` under the app's private files directory (`context.filesDir/keys/content-key.keystore`).
   - File format: 4-byte magic `GAK1` (GIST Android Key v1) + 12-byte IV + GCM ciphertext + 16-byte authentication tag.
3. **Atomic, Race-Safe Creation:**
   - On initial creation, write the wrapped key to a temporary file (`content-key.keystore.<uuid>.tmp`).
   - Atomically rename to `content-key.keystore` using `File.renameTo()` or `AtomicFile`. If another thread or process won the race and created the target file first, delete the temporary file, zero candidate buffers, and re-read/decrypt the winning file.
4. **Memory Hygiene:**
   - Candidate and intermediate byte buffers are overwritten with zeros (`Arrays.fill(bytes, 0.toByte())`) immediately after use.
   - The returned 32-byte array is passed directly across JNA/UniFFI to Rust's memory.
5. **Fail Closed:**
   - If the key file exists but decryption fails (e.g. tag mismatch, invalid format, corrupted file, or master key missing/invalidated), throw a typed `KeyStoreCorruptException` or `KeyStoreUnavailableException`.
   - **Never delete or regenerate the key file on error.**

## Threat Model

Protects against:
- **Other apps on the device:** Android's Linux sandbox UID separation prevents other apps from accessing `context.filesDir`, and Android Keystore aliases are strictly isolated per app UID.
- **Offline disk imaging / extraction:** Even if the physical flash storage is extracted from the device or imaged via bootloader exploits, the `content-key.keystore` file cannot be decrypted without the TEE/StrongBox hardware key.
- **Rooted devices without lockscreen unlock:** The Keystore requires hardware-level cryptographic authorization.

Does not protect against:
- Root exploits or runtime debuggers operating within the app's own process memory while the key is loaded into RAM.
- Devices subjected to factory resets (which purge the hardware Keystore keys, permanently rendering encrypted data unreadable — an intended property of local-first encryption at rest).

## Consequences

- Direct behavioral parity with macOS Keychain and Windows DPAPI.
- Zero plaintext key material stored on persistent flash storage.
- If Android Auto Backup is enabled, `content-key.keystore` must **not** be backed up to Google Drive (governed by ADR-024), because the Keystore master key cannot be exported or restored onto another device.
