# ADR 016 — Windows key custody: DPAPI-protected key file

**Date:** 2026-09-20
**Status:** Accepted (prototype implemented and tested; not yet exercised inside an MSIX package)

## Context
ADR-011 encrypts document content at rest with AES-256-GCM in `gist-store`, with key custody delegated to the platform through the `KeyProvider` callback interface (`gist-ffi`, mirroring ADR-009's `OcrEngine`). On Apple this is `KeychainKeyProvider`. ADR-014 makes the app open its store with `new_with_read_key(KeyProvider)` from first launch so that a user-encrypted item stays readable. `docs/windows-development-plan.md` §3/§4.5 requires an equivalent `DpapiKeyProvider` for the WinUI 3 shell.

The contract the Windows provider must satisfy, taken from the Rust side and from `KeychainKeyProvider.swift`:
- `GetOrCreateKey()` returns **exactly 32 bytes**. Any other length surfaces as `GistError.InternalPanic` (verified in the ADR-015 spike), so the provider must never return a wrong-length value.
- **The same key for the life of the data.** The key is generated once and persisted; generating a replacement silently orphans every item encrypted under the old one.
- **Creation is race-safe.** The Apple version had a real bug (fixed 2026-09-18) where the loser of a concurrent create returned its own unpersisted key; the Windows version must not repeat it. Two near-simultaneous callers (two launches, or two threads during startup and an Encrypt action) must converge on one key.
- Key bytes are never logged and never reach Rust logs or exception text.

## Decision
`DpapiKeyProvider` (prototype: `apps/windows/spikes/keyprovider/`, to move into `GIST.Core` in W1):

- **Key**: 32 bytes from `RandomNumberGenerator`.
- **Protection**: `ProtectedData.Protect(key, entropy, DataProtectionScope.CurrentUser)` (Windows DPAPI). Entropy is an app-specific constant (`GIST.KeyProvider.v1`) unless the caller supplies one. Entropy is not a secret; it only scopes the blob to this app and format so another DPAPI consumer running as the same user cannot trivially unwrap it by calling `Unprotect` without knowing it.
- **Storage**: one file, `content-key.dpapi`, in a caller-supplied directory. In the packaged app this is `ApplicationData.Current.LocalFolder` (LocalState); unpackaged, `%LOCALAPPDATA%\GIST`. File layout: 4-byte magic `GKP1` + DPAPI blob. The magic gives a cheap, explicit "not our file" failure and a version hook.
- **Race-safe create**: write the protected blob to a uniquely named temp file in the same directory (`FileMode.CreateNew`, flushed to disk), then publish with `File.Move(tmp, target, overwrite: false)`, which fails atomically if the target exists. The loser deletes its temp file, zeroes its candidate key and **re-reads the winner's file**. The temp file is always cleaned up. Readers can never observe a half-written key file because it only appears via the atomic move.
- **Fail closed on any doubt.** If the key file exists but is unreadable as a key (bad header, empty, truncated, tampered, wrong user, wrong entropy, DPAPI master key unavailable, decrypted length != 32) the provider throws `KeyStoreCorruptException` and **does not create a new key and does not modify the file**. Plain I/O problems (permissions, disk) throw `KeyStoreIoException`. Both derive from `KeyProviderException`; messages contain no key material.
- Temporary key buffers (unused candidate, DPAPI blob, decrypted buffer on the error path) are zeroed with `CryptographicOperations.ZeroMemory`. The key returned to the caller is necessarily a managed array and is not zeroed by the provider; the caller (uniffi glue) copies it across the FFI and it lives in managed memory for the callback's duration. This is a limitation, not a guarantee.

The `IKeyProvider` interface in the prototype maps 1:1 to the uniffi-generated `KeyProvider.GetOrCreateKey()`; the W1 adapter is a trivial wrapper.

### App behaviour on `KeyStoreCorruptException`
Because the callback interface has no error channel (a throw inside the callback becomes a Rust-side failure, in the worst case `InternalPanic`), `CoreClient` must **not** hand a possibly-failing provider straight to `NewWithReadKey`. Required W1 behaviour:
1. At startup, if the store contains any `content_encrypted = 1` item or the key file exists, call `GetOrCreateKey()` once eagerly inside `CoreClient` init, in managed code where the typed exception can be caught.
2. On `KeyStoreCorruptException`: do not open the store with a substitute key. Show an explicit blocking state ("Your encryption key could not be recovered. Encrypted items cannot be opened. Unencrypted items are still available.") with options to continue with encrypted items disabled (open with plain `New`, which returns `MissingKeyProvider` for encrypted rows) or quit. Never offer "reset key" without warning that it permanently orphans encrypted items; if offered at all it is a deliberate, confirmed action that moves the old file aside.
3. On `KeyStoreIoException`: retryable error dialog.

## Threat model
Protects against:
- **Other local users** on the same machine (DPAPI CurrentUser keys derive from the user's logon credentials).
- **Offline disk theft / disk imaging** of a machine whose user profile is not otherwise decryptable (the key file is useless without the user's DPAPI master key, which is itself protected by the user's password or, on Microsoft/Entra-joined accounts, by the credential/TPM-backed mechanisms Windows uses). Note that the strength here depends on the account: a passwordless local account gives DPAPI little to bind to.
- Casual file copying: copying `content-key.dpapi` and the encrypted content to another machine or user does not yield the key.

Does **not** protect against:
- **Malware or any process running as the same user**: it can call `Unprotect` (entropy is not a secret) or read process memory. DPAPI CurrentUser is not an app-isolation boundary. MSIX package identity does not change this for DPAPI.
- A user (or admin who can reset the user's password with knowledge of the old one / an attacker who has the user's logon credentials).
- Memory inspection while the app is running, and key material in managed memory after return.

Availability risk (inverse of the above):
- **Profile reset, Windows reinstall, password reset by an admin without the old password, domain/account change, or a restored backup on another machine/user** can lose the DPAPI master key. The key file is then permanently undecryptable and **all items encrypted via "Encrypt" are unrecoverable**. This is by design (ADR-011) and there is no recovery escrow. Unencrypted items and the original files are unaffected. The app must say so at the point of first encryption (plan §4.5 UX) and use the corrupt-key state above rather than failing opaquely.
- Backup/restore of `LocalState` to a different machine restores the encrypted content but not a usable key.

## File location and permissions
- Packaged: LocalState, which is per-user and per-package (other packages cannot read it, standard users cannot read other users' profiles). Unpackaged: `%LOCALAPPDATA%\GIST`, inheriting the profile's default ACL (owner, SYSTEM, Administrators). The prototype sets no custom ACL; DPAPI, not the ACL, is the confidentiality control, and the ACL is defence in depth only.
- Kept in the same directory tree as the store so that "delete the app's data" removes both (orphaning nothing) and so backups treat them together. (Trade-off: a backup of the directory contains everything needed on the same user account; that is the same exposure profile as Apple's device-only Keychain item and acceptable for this threat model.)
- Roaming: must be **local** (LocalState / LocalAppData), never Roaming, so the key does not follow the profile to machines where the DPAPI master key differs or where the encrypted content does not exist.

## Alternatives considered
- **Windows Credential Locker (`PasswordVault`)**: closest analogue to Keychain, but it is a WinRT API (awkward outside the packaged app, and under unit tests), stores strings not bytes (needs base64 round-tripping), gives no better isolation from same-user malware, and is itself DPAPI-backed. No security gain for extra API surface; rejected.
- **Credential Manager (`CredRead`/`CredWrite`) via P/Invoke**: same underlying protection, needs a native shim or hand-written P/Invoke, no atomic create-if-absent primitive comparable to file `CreateNew`. Rejected.
- **CNG with the Microsoft Platform Crypto Provider (TPM-backed non-exportable key)**: a materially stronger property (key never leaves the TPM). Rejected for v1.0 because the AES data key must be handed to Rust as 32 raw bytes (the KeyProvider contract), so we would need a TPM-wrapped key-encryption key around a stored blob, TPM availability varies (VMs, older hardware), and TPM clears/hardware replacement make loss more likely. **Recommended as a v1.1 hardening**: wrap the 32-byte key with a TPM-resident RSA/ECDH key, falling back to DPAPI where unavailable. That requires a new file version (`GKP2`, see migration).
- **Windows Hello-gated keys (`KeyCredentialManager`)**: prompts or biometric on use are incompatible with a silent callback invoked on every read, and availability depends on Hello enrolment. Rejected as the default; a possible opt-in "require Hello to open encrypted items" feature would sit above this provider, not replace it.
- **DPAPI with `LocalMachine` scope**: any user/process on the machine can decrypt; rejected.
- **App-embedded or derived-from-machine-id keys**: not secrets; rejected.
- **Passphrase-derived key**: would need user interaction on every launch; out of scope, but would enable cross-machine recovery. Noted as a future option.

## Migration and rotation
- **No rotation in v1.0.** Rotation means re-encrypting every encrypted item's blobs (ADR-014's per-item machinery could be reused) and is deliberately not implemented, as in ADR-011.
- **Format versioning**: the `GKP1` magic allows a future `GKP2` (e.g. TPM-wrapped) to be added; the provider must continue to read `GKP1` and upgrade by writing a new file through the same temp-file-then-move path (using `File.Replace`, not delete-then-create, to avoid a window with no key).
- **Entropy or magic changes** are breaking for existing files; the constant must not change without a migration reading the old value.
- **Cross-platform**: Apple and Windows stores are not interchangeable; a library moved between platforms cannot read its encrypted items (different key custody). Export/import of an encrypted library is out of scope.

## Consequences
- Windows reaches ADR-011/014 parity with a small, dependency-light (`System.Security.Cryptography.ProtectedData` only) implementation that is unit-testable without WinUI.
- The provider is Windows-only by construction (the assembly targets `net10.0-windows`); Apple code and the Rust core are unchanged.
- The "unrecoverable key" state is a first-class UX case that W1's `CoreClient` must implement (see above), and W2's Encrypt flow must warn about it.

## Verification (what was actually run, 2026-09-20)
`apps/windows/spikes/keyprovider/DpapiKeyProvider.Tests`, .NET SDK 10.0.401, Windows 11, real DPAPI and filesystem, temp directory per test. `dotnet test`: **13 passed, 0 failed**; re-run 10 more times without rebuilding, 13/13 each time (no flakiness observed in the concurrency tests). Coverage: 32-byte creation and persistence; identical key on second call and second instance; 32 threads (mixed shared and separate instances, released simultaneously) and 5 rounds of 24 parallel creators each converge on one key and exactly one file (no leaked temp files); tampered blob, garbage/bad-header file, truncated file, header-only file and empty file all throw `KeyStoreCorruptException` and leave the file byte-for-byte unchanged with no new key generated; a different entropy throws and leaves the file intact, while the original entropy still reads it; a valid DPAPI blob of the wrong length (16 bytes) throws; neither the key nor its first 8 bytes appears in the stored file; exception text does not contain the key hex.

**Not verified:** behaviour inside an MSIX-packaged process (LocalState path, virtualisation); roaming/domain-joined profiles and DPAPI master key loss scenarios (the wrong-entropy and tamper tests simulate the failure signal, not a real master key loss); non-admin/AV-locked file behaviour beyond simple I/O errors; ARM64; integration with the uniffi `KeyProvider` callback (bindings not used in this task; the ADR-015 spike separately proved a C# callback provider works); a crash between temp-file write and move leaves a stray `.tmp` file, which is harmless but is not reaped by a later run (not implemented).
