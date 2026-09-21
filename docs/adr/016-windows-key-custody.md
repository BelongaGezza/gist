# ADR 016 — Windows key custody: DPAPI-protected key file

**Date:** 2026-09-20
**Status:** Accepted (implemented in `GIST.Core` and tested; not yet exercised inside an MSIX package).
Amended 2026-09-20 by the W1 addendum at the end of this file — read it alongside the decision below,
which it refines on failure classification, ACL, temp-file reaping and storage paths.

## Context
ADR-011 encrypts document content at rest with AES-256-GCM in `gist-store`, with key custody delegated to the platform through the `KeyProvider` callback interface (`gist-ffi`, mirroring ADR-009's `OcrEngine`). On Apple this is `KeychainKeyProvider`. ADR-014 makes the app open its store with `new_with_read_key(KeyProvider)` from first launch so that a user-encrypted item stays readable. `docs/windows-development-plan.md` §3/§4.5 requires an equivalent `DpapiKeyProvider` for the WinUI 3 shell.

The contract the Windows provider must satisfy, taken from the Rust side and from `KeychainKeyProvider.swift`:
- `GetOrCreateKey()` returns **exactly 32 bytes**. Any other length surfaces as `GistError.InternalPanic` (verified in the ADR-015 spike), so the provider must never return a wrong-length value.
- **The same key for the life of the data.** The key is generated once and persisted; generating a replacement silently orphans every item encrypted under the old one.
- **Creation is race-safe.** The Apple version had a real bug (fixed 2026-09-18) where the loser of a concurrent create returned its own unpersisted key; the Windows version must not repeat it. Two near-simultaneous callers (two launches, or two threads during startup and an Encrypt action) must converge on one key.
- Key bytes are never logged and never reach Rust logs or exception text.

## Decision
`DpapiKeyProvider` (now `apps/windows/GIST.Core/Keys/`, namespace `Gist.Core.Keys`; the prototype at
`apps/windows/spikes/keyprovider/` was promoted in W1 and deleted — see the addendum):

- **Key**: 32 bytes from `RandomNumberGenerator`.
- **Protection**: `ProtectedData.Protect(key, entropy, DataProtectionScope.CurrentUser)` (Windows DPAPI). Entropy is an app-specific constant (`GIST.KeyProvider.v1`) unless the caller supplies one. Entropy is not a secret; it only scopes the blob to this app and format so another DPAPI consumer running as the same user cannot trivially unwrap it by calling `Unprotect` without knowing it.
- **Storage**: one file, `content-key.dpapi`, in a caller-supplied directory. In the packaged app this is `ApplicationData.Current.LocalFolder` (LocalState); unpackaged, `%LOCALAPPDATA%\GIST`. File layout: 4-byte magic `GKP1` + DPAPI blob. The magic gives a cheap, explicit "not our file" failure and a version hook.
- **Race-safe create**: write the protected blob to a uniquely named temp file in the same directory (`FileMode.CreateNew`, flushed to disk), then publish with `File.Move(tmp, target, overwrite: false)`, which fails atomically if the target exists. The loser deletes its temp file, zeroes its candidate key and **re-reads the winner's file**. The temp file is always cleaned up. Readers can never observe a half-written key file because it only appears via the atomic move.
- **Fail closed on any doubt.** If the key file exists but is unreadable as a key (bad header, empty, truncated, tampered, wrong user, wrong entropy, DPAPI master key unavailable, decrypted length != 32) the provider throws and **does not create a new key and does not modify the file**. Plain I/O problems (permissions, disk) throw `KeyStoreIoException`. All derive from `KeyProviderException`; messages contain no key material. *(Amended: which of `KeyStoreCorruptException` and `KeyStoreUnavailableException` is thrown is set out in addendum §1 — "DPAPI master key unavailable" is now the retryable one, not corruption.)*
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
- Packaged: LocalState, which is per-user and per-package (other packages cannot read it, standard users cannot read other users' profiles). Unpackaged: `%LOCALAPPDATA%\GIST`. *(Amended: the key directory is no longer left on the profile's inherited ACL — see addendum §2. DPAPI, not the ACL, is still the confidentiality control, and the ACL remains defence in depth only.)*
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

## Verification (what was actually run, 2026-09-20, against the spike)
`apps/windows/spikes/keyprovider/DpapiKeyProvider.Tests` (since promoted, see the addendum), .NET SDK 10.0.401, Windows 11, real DPAPI and filesystem, temp directory per test. `dotnet test`: **13 passed, 0 failed**; re-run 10 more times without rebuilding, 13/13 each time (no flakiness observed in the concurrency tests). Coverage: 32-byte creation and persistence; identical key on second call and second instance; 32 threads (mixed shared and separate instances, released simultaneously) and 5 rounds of 24 parallel creators each converge on one key and exactly one file (no leaked temp files); tampered blob, garbage/bad-header file, truncated file, header-only file and empty file all throw `KeyStoreCorruptException` and leave the file byte-for-byte unchanged with no new key generated; a different entropy throws and leaves the file intact, while the original entropy still reads it; a valid DPAPI blob of the wrong length (16 bytes) throws; neither the key nor its first 8 bytes appears in the stored file; exception text does not contain the key hex.

**Not verified:** behaviour inside an MSIX-packaged process (LocalState path, virtualisation); roaming/domain-joined profiles and DPAPI master key loss scenarios (the wrong-entropy and tamper tests simulate the failure signal, not a real master key loss); non-admin/AV-locked file behaviour beyond simple I/O errors; ARM64; integration with the uniffi `KeyProvider` callback (bindings not used in this task; the ADR-015 spike separately proved a C# callback provider works); a crash between temp-file write and move leaves a stray `.tmp` file, which is harmless but is not reaped by a later run (not implemented).

---

## Addendum — W1 promotion into `GIST.Core` (2026-09-20)

Closes the design points raised as **Q9** in `docs/review-pre-w1-quality-security.md`. The code moved
from `apps/windows/spikes/keyprovider/` to `apps/windows/GIST.Core/Keys/` (namespace `Gist.Core.Keys`)
and the spike was deleted; the public surface is unchanged (`IKeyProvider.GetOrCreateKey()` -> exactly
32 bytes, `DpapiKeyProvider(string directory, byte[]? entropy = null)`, `KeyFilePath`, `KeyLength`,
`FileName`, `KeyProviderException`, `KeyStoreCorruptException`, `KeyStoreIoException`).

### 1. Transient vs corrupt, and retry semantics

The spike classed **every** `CryptographicException` as `KeyStoreCorruptException`, so a user whose
profile simply was not loaded yet would be told their encrypted library is unrecoverable. The
hierarchy gains `KeyStoreUnavailableException` (retryable, `KeyProviderException` subclass), and
`KeyStoreFailureClassifier` maps DPAPI failures onto the two:

| Condition | Outcome |
|---|---|
| Bad/absent magic, file empty, file shorter than the header | `KeyStoreCorruptException` |
| `ERROR_INVALID_DATA` (0x8007000D) — tampered, truncated or wrong-entropy blob | `KeyStoreCorruptException` |
| `ERROR_INVALID_PARAMETER` (0x80070057) — payload is not a DPAPI blob at all | `KeyStoreCorruptException` |
| Decrypted payload is not 32 bytes | `KeyStoreCorruptException` |
| Any other `CryptographicException` (master key missing, `NTE_*`, access denied, **unknown codes**) | `KeyStoreUnavailableException` |
| `Protect` failing while creating a brand-new key | `KeyStoreUnavailableException` |
| File I/O failure (permissions, disk, sharing) | `KeyStoreIoException` |

The two corruption codes are an **allow-list measured on this machine** (Windows 11, .NET 10.0.401),
not inferred from documentation: tampering a byte, truncating the blob and using the wrong entropy all
produce 0x8007000D, while garbage and empty payloads produce 0x80070057. Unknown codes deliberately
fall to *unavailable*, because "corrupt" is the destructive verdict of the two — it is the one that
tells a user to give up on their data.

**Retry semantics.** `KeyStoreUnavailableException` and `KeyStoreIoException` are retryable: the same
call may succeed later with no repair step, so `CoreClient` offers Retry and opens the store with plain
`New` (encrypted items disabled) only if the user declines. `KeyStoreCorruptException` is not
retryable and keeps the blocking state described above. Independently of the type,
**no failure path ever deletes, overwrites or recreates the key file** — the only file the provider
ever removes is its own `content-key.dpapi.<guid>.tmp` scratch file. This is now an explicit,
enumerated test over every failure shape reachable from outside the process, asserting the key file's
bytes *and* its last-write time are unchanged and that nothing new appears beside it.

Reads of the key file retry up to 5 times with a short backoff on a sharing violation (AV scanner,
search indexer, backup agent, ACL propagation) before reporting `KeyStoreIoException`; a missing file
still fails immediately.

### 2. ACL hardening

The key **directory** gets an explicit DACL: inheritance removed, one inheritable Full Control rule for
the current user. The key file and the scratch file pick this up by inheritance, and Windows propagates
it to a key file that already exists, so a directory created before this change is hardened on next use.
It is applied when the directory is created and once per provider instance, and skipped when the
directory is already protected, so the steady state costs one read-only DACL check.

Setting the key **file's** own DACL as well was implemented, tested and reverted: `SetAccessControl`
opens the file in a way that makes a concurrently racing reader fail with a sharing violation — caught
by the 24-parallel-creators test, which is exactly the race this ADR is careful about. Inheritance
achieves the same end state without the extra open.

Hardening is best-effort and never fails `GetOrCreateKey()`: DPAPI, not the ACL, is the confidentiality
control (a same-user process can unwrap the blob either way), so a filesystem without ACL support or a
refused DACL write degrades rather than blocks.

### 3. Stale temp-file reaping

A crash between writing the scratch file and publishing it used to leave a `.tmp` file forever. Once
per provider instance the directory is swept for files matching `content-key.dpapi.<32 hex>.tmp` whose
last write is older than **1 minute** (a publish takes milliseconds; anything older is debris, and the
threshold means a live racing creator's file is never taken). Only that exact pattern is reaped — a
lookalike such as `content-key.dpapi.backup.tmp` and any unrelated file are left alone, and the key
file can never match. Sweeping is best-effort; a file held open is retried on the next launch.

### 4. `GistStoragePaths` — one source of truth for locations

`apps/windows/GIST.Core/Storage/GistStoragePaths.cs` is the contract every other component codes
against instead of composing paths itself: `Root`, `DbPath` (`gist.sqlite3`), `StorageDir` (`storage`),
`KeyDir` (`keys`), built by `ForRoot(root)`, `ForUnpackaged()` (`%LOCALAPPDATA%\GIST`), `ForPackaged()`
and `Resolve()`. **Invariant, tested:** all three derive from one `Root`, so the key and the store it
unlocks always move, back up and get deleted together, and a debug build pointed at a scratch root
cannot read the production store with the wrong key. `ForPackaged()` throws rather than falling back to
the unpackaged root — a silent fallback would be that same bug in disguise.

Package identity is detected with the Win32 `GetCurrentPackageFamilyName` P/Invoke (`kernel32.dll`),
not a WinRT projection, so `GIST.Core` stays UI-free and headless-testable; the packaged root is then
`%LOCALAPPDATA%\Packages\<PackageFamilyName>\LocalState`, the documented location of
`ApplicationData.Current.LocalFolder`. **Unverified:** no MSIX package exists yet, so the packaged
branch has never been observed from inside one — only the "no package identity" result (15700) is
exercised. If it ever disagrees with what WinRT reports, the packaged host passes that folder to
`ForRoot` and nothing else changes.

### 5. Still open — recovery-key export (maintainer decision, before W2 exposes Encrypt)

Unchanged and **deliberately not implemented here.** DPAPI blobs are not portable across users or
machines, so a profile reset, password reset without the old password, or a restore onto another
machine loses every encrypted item (see "Availability risk" above). Whether v1.0 ships a recovery-key
export — and if so, what it is (a printable/downloadable wrapped copy of the 32-byte key, a
passphrase-derived secondary wrapping, or nothing at all, with the warning made louder instead) — is a
maintainer decision that must be taken **before the Encrypt action is exposed in W2**, because the
choice changes what the first-encryption dialog has to say and possibly the key file format (a `GKP2`
version, as with the TPM option). Nothing in this addendum forecloses any of those options.

### Verification (what was actually run, 2026-09-20, after promotion)

`apps/windows/GIST.Core.Tests`, .NET SDK 10.0.401, Windows 11, real DPAPI and real filesystem, a fresh
temp directory per test (nothing touches the machine's own `%LOCALAPPDATA%\GIST` key or its ACLs).
`dotnet build`: 0 warnings, 0 errors (warnings-as-errors). `dotnet test`: **50 passed, 0 failed**, run
three times for flakiness. That is the spike's 13 tests unchanged, plus 16 new key-custody tests
(classification for both corruption codes and five transient/unknown codes, the shared base type, the
"never deletes/overwrites/recreates" invariant over six damaged-file shapes, stale-tmp reaped,
fresh-tmp kept, reaping never touching the key file or lookalikes, directory ACL restricted, key file
inheriting it, hardening idempotent, an existing unhardened directory hardened on next use, key under
the same root as the store, two roots holding independent keys), plus 11 `GistStoragePaths` tests, plus
the skeleton's smoke test.

**Not verified (unchanged from the original list, minus the reaping gap which is now closed):**
behaviour inside an MSIX-packaged process, including whether `ForPackaged()`'s computed LocalState path
matches WinRT's; roaming/domain-joined profiles and real DPAPI master-key loss (the wrong-entropy and
tamper tests simulate the failure *signal*, not a real master-key loss, and no transient failure can be
provoked from outside the process — the classifier's transient mapping is tested directly instead);
ARM64; integration with the uniffi `KeyProvider` callback; behaviour on a filesystem without ACL
support.

## Decision 2026-09-21 - recovery of encrypted items
No recovery mechanism in v1.0 (maintainer decision). DPAPI (CurrentUser) keys do not survive a profile reset, a Windows reinstall or a move to another PC, and encrypted items become permanently unreadable in those cases. Mitigations: (1) Encrypt is opt-in per item and new imports stay plaintext (ADR-014); (2) the Encrypt confirmation dialog states the irrecoverability plainly; (3) originals in the user's own files are never touched, so the source material remains. A passphrase-protected key backup/export is a v1.1 candidate and would likely need a key-file format revision; nothing in the current format blocks adding it later.
