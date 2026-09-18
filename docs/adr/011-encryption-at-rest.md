# ADR 011 — Encryption at rest

**Date:** 2026-09-18
**Status:** Accepted (Rust-side crypto and migration path implemented and tested; platform-side key custody implemented but not end-to-end runtime-verified — see Verification)

## Context

Security register item `A6` (originally scoped narrower, as at-rest *integrity* only — Q11 broadened it to confidentiality) flagged that GIST's persisted content is plaintext on disk: ADR-007's IR blobs (`<id>.json`/`<id>.tokens.json`) and ADR-006's sandboxed original-file copies under `originals/` are both written via plain `std::fs::write`, with no encryption anywhere in the storage path. GIST is a personal reading app; the documents it stores are exactly the kind of content (books, articles, personal documents someone chose to read) a person would reasonably expect device-at-rest protection for, on par with what iOS/macOS Data Protection already gives most other apps' data by default at the filesystem level — GIST's own storage layer currently adds nothing on top of that baseline, and provides no protection at all on a platform without transparent filesystem encryption.

## Decision

**Encrypt at `gist-store`'s file read/write boundary, not above or below it.** Specifically: the `.json` document blob, the `.tokens.json` token-stream blob, and ADR-006's `originals/` sandboxed copy are all encrypted with **AES-256-GCM** (via the `aes-gcm` crate) before being written, and decrypted after being read. Nothing above `gist-store` (the SQLite metadata rows, `gist-core`'s facade, the FFI layer) needs to know or care that this is happening — `Document`/`Token` structs are still plain Rust values everywhere else in the pipeline.

**Key custody is a platform responsibility, not a Rust one.** Rust owns the crypto operations; it never generates, stores, or has any opinion about where the key comes from beyond receiving 32 bytes through a trait:

```rust
// gist-store/src/lib.rs
pub trait KeyProvider: Send + Sync {
    fn get_or_create_key(&self) -> [u8; 32];
}
```

This mirrors ADR-009's `OcrEngine` callback-interface pattern deliberately: Rust orchestrates, the platform layer (Swift on Apple, backed by Keychain) implements the actual OS-native mechanism, and an adapter in `gist-ffi` bridges a uniffi callback interface to the Rust-side trait. See **Architectural correction** below for exactly where each half of this lives and why.

**Nonce handling:** a fresh random 12-byte nonce (`Aes256Gcm::generate_nonce`) is generated for every write and prepended to the ciphertext (`nonce || ciphertext`, the ciphertext already carrying its own GCM authentication tag) — standard AES-GCM practice. AES-GCM's security property (indistinguishability under chosen-plaintext attack, tamper detection) depends on never reusing a nonce under the same key, which a fresh random nonce per write satisfies with overwhelming probability at this data volume.

## Architectural correction: where `KeyProvider` actually lives

The initial framing of this decision (mirroring `OcrEngine`) suggested defining `KeyProvider` in `gist-core`. That doesn't fit this codebase's actual dependency graph: `gist-core` depends on `gist-store` (`Core::init` calls `gist_store::Store::open`), not the reverse. `gist-store` is the crate that actually performs the file I/O this trait gates, so if the trait lived in `gist-core`, `gist-store` would need to depend on `gist-core` to use it — a cycle that doesn't exist today and shouldn't be introduced for this.

The trait is defined in **`gist-store`**, and **`gist-core` re-exports it** (`pub use gist_store::{KeyProvider, FakeKeyProvider};`), exactly the same pattern already established for `ParseLimits` (defined in `gist-model`, re-exported by `gist-core`, with an existing doc comment explaining the same reasoning). Callers that only ever interact with `gist-core`'s facade — like `gist-ffi` — can still write `gist_core::KeyProvider` and never need to know it's actually implemented one crate down.

On the FFI side, the intended shape (not yet implemented — see Verification) follows `OcrEngine`'s exact precedent: a **separate** `gist-ffi`-side `KeyProvider` trait annotated `#[uniffi::export(callback_interface)]` (uniffi's callback-interface macro must run in the crate that exports it over FFI), implemented in Swift against the Keychain, plus a small `CoreKeyProviderAdapter` in `gist-ffi` bridging the uniffi callback object to `gist_store::KeyProvider` — mirroring `gist-ffi`'s existing `CoreOcrAdapter`.

## Migration: existing plaintext stores must not become unreadable

Chose **(b) a per-row flag**, not **(a) an eager bulk re-encryption pass at open time.**

Schema v5 adds one column: `library_items.content_encrypted INTEGER NOT NULL DEFAULT 0`, following the exact same additive-column pattern as v4's `source_copy_path` migration. Every pre-existing row defaults to `0` (unencrypted) — correct, since nothing about those rows' on-disk content changes. `Store::open` (no key provider) is completely unchanged: it always writes `content_encrypted = 0` and plaintext, byte-for-byte the same behavior as before this ADR. `Store::open_encrypted(db_path, storage_dir, key_provider)` is a new, additive entry point: content written *through it* is encrypted and flagged `1`; reads consult the flag per-row and decrypt only when it's set, using whichever `Store` instance's configured key (erroring with a new `StoreError::MissingKeyProvider` if a row is flagged encrypted but the `Store` was opened without a key).

**Why not (a):** an eager bulk migration — reading every existing document blob and original-file copy at `Store::open_encrypted` time and rewriting them encrypted — was rejected for three concrete reasons specific to this codebase's current state:

1. **Document content is otherwise immutable.** Only `reading_progress` (a separate table, RSVP token-index only) is ever updated after import; `insert_item` is the only writer of document content, called exactly once per item at import time. There is no existing "rewrite this document" code path to piggyback a migration onto — a bulk pass would be new, bespoke machinery built solely for this one-time transition, which is more surface area to get wrong than the alternative.
2. **Partial-migration failure is a worse failure mode than "old items stay as they were."** A library can be arbitrarily large; a migration interrupted partway (app killed, disk full, crash) leaves some items re-encrypted and others not, and — worse — a *bug* in the rewrite path risks data loss on a customer's real library, which the per-row-flag approach structurally cannot cause (it never rewrites anything that already works).
3. **The blast radius of "old items aren't retroactively encrypted" is small and shrinking.** New imports are encrypted from the moment `Core::init_encrypted` is wired up; a user's existing library becomes fully encrypted over time as items are naturally re-imported (or, if the product later wants a faster transition, an explicit opt-in "re-encrypt my library" maintenance action — not built here — can be layered on top of the same per-row flag, reusing `insert_item`'s existing encrypt-on-write path rather than needing new crypto plumbing).

**Consequence to track:** without such an explicit maintenance action (not implemented in this pass), an existing user's already-imported items remain plaintext on disk indefinitely — only content imported after `Core::init_encrypted` starts being used goes through the encrypted path. This is a known, accepted gap in this decision, not an oversight: the alternative (silently forcing a bulk rewrite) was judged riskier for the reasons above.

## What was implemented

- `gist-store`: `KeyProvider` trait, `FakeKeyProvider` (test-only, fixed in-memory key), `encrypt_at_rest`/`decrypt_at_rest` (AES-256-GCM, random nonce prepended to ciphertext), schema v5 migration, `Store::open_encrypted`, and encryption wired into `insert_item`, `get_item`, `get_tokens`, and `store_original_copy`. New `StoreError` variants: `MissingKeyProvider`, `DecryptionFailed`.
- `gist-core`: re-exports `KeyProvider`/`FakeKeyProvider`; `Core::init_encrypted` as the encrypted counterpart to `Core::init`.
- Test coverage (`gist-store`, 8 new tests + `gist-core`, 1 new test): encrypt/decrypt round-trip, on-disk ciphertext genuinely doesn't contain a known plaintext canary string (for both the document/tokens blobs and an original-file copy), wrong key fails cleanly (`DecryptionFailed`, not a panic), corrupted/too-short ciphertext fails cleanly, the full migration scenario (plaintext item written under `Store::open`, then the same on-disk store reopened via `Store::open_encrypted` — old item still readable as plaintext, new item written after that point is genuinely encrypted), and reading encrypted content back through a plain `Store::open` fails with `MissingKeyProvider` rather than garbage or a panic.
- `gist-ffi`: a `#[uniffi::export(callback_interface)]` `KeyProvider` trait (returns `Vec<u8>` — uniffi has no const-generic-array support — validated to exactly 32 bytes by `CoreKeyProviderAdapter`, which panics via the normal `ffi_catch!` path on any other length) and `GistCore::newEncrypted(dbPath:storageDir:keyProvider:)`, mirroring `OcrEngine`/`CoreOcrAdapter` exactly. Swift bindings were regenerated (`apps/apple/Generated/gist_ffi.swift` exposes `KeyProvider` and `GistCore.newEncrypted`).
- `apps/apple/Shared/KeychainKeyProvider.swift`: a real `KeyProvider` implementation against `kSecClassGenericPassword`/`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`, using `SecRandomCopyBytes` to generate a new key on first use and `SecItemAdd`/`SecItemCopyMatching` to persist/retrieve it. **Post-review fix (2026-09-18, same day):** the first version of this file had a real race-condition bug — if two calls raced on `SecItemAdd` and one lost (`errSecDuplicateItem`), it returned its own locally-generated, never-persisted key instead of re-reading the key that actually won and was stored, which would have caused silent, unrecoverable decryption failure for that session's writes. Fixed by having `store()` report success/failure and having `getOrCreateKey()` re-read via `readKey()` on a lost race, rather than assuming its own candidate key was the one that got saved.

## What was not implemented (this pass)

- **Wiring `KeychainKeyProvider` into `CoreClient.shared`'s production `init()`.** `.shared` still constructs a plain `GistCore(dbPath:storageDir:)`, unchanged — every real user's on-disk data remains exactly as before this ADR. `GistCore.newEncrypted` and `KeychainKeyProvider` exist and build cleanly, but nothing in the shipped app calls them yet. This is deliberate, not an oversight: switching every real user's storage format is a bigger decision than a security-hardening pass should make unilaterally — see Consequences.
- **A real Keychain round-trip test.** No test exercises `KeychainKeyProvider`/`GistCore.newEncrypted` against an actual Keychain at runtime; its correctness rests on the `Security` framework APIs being used as documented (`SecItemAdd`/`SecItemCopyMatching`/`SecRandomCopyBytes`), not on an executed test.
- **A migration UI/decision for existing users.** If/when `CoreClient.shared` is switched to `newEncrypted`, that also needs a decision about what happens to a user's already-imported (plaintext) library — see the Migration section above; nothing product-facing was built for that here.

## Verification

- **Rust-side crypto and migration:** fully covered by `cargo test --workspace` (26 tests in `gist-store`, 1 new end-to-end test in `gist-core`), all passing. `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` both clean. `cargo deny check bans licenses sources` still passes with `aes-gcm` added (`bans ok, licenses ok, sources ok`) — no new advisory, license, or duplicate-version problem introduced by this dependency.
- **FFI + Swift: compiles and links, not runtime-verified.** `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`, CI flags) both succeed with `KeychainKeyProvider.swift` and the regenerated `KeyProvider` bindings in the tree — **43/43 tests pass**, unchanged, because none of those tests call the new code path. That confirms the Swift side compiles correctly against the FFI bindings and doesn't regress anything existing; it does **not** confirm a real Keychain read/write actually works end-to-end. Do not read "tests pass" as "Keychain integration verified" — those are different claims, and only the first one is being made here.

## Consequences

**Easier:** any future feature needing at-rest confidentiality (annotations, if they ever store quoted document text; a future sync feature staging content locally before upload) can reuse `encrypt_at_rest`/`decrypt_at_rest` and the same `KeyProvider` rather than inventing its own scheme. `gist-model` stays free of any crypto or Keychain dependency, preserving its `wasm32-unknown-unknown` compilation target — this ADR's crypto lives entirely in `gist-store`, a native-only crate that already depends on `rusqlite` and the filesystem.

**Harder / to watch:** losing the platform-held key makes all encrypted content permanently unreadable, by design — there is no recovery path other than the platform's own keychain backup/sync mechanisms, which are outside this ADR's scope and should be called out clearly to users once the UI for this exists (a warning during onboarding or in settings, not built here). The `gist-ffi`/Swift half of this (listed under "What was not implemented") is required before this protection has any real effect for an actual user — until that lands and `Core::init_encrypted` replaces `Core::init` at the app's actual startup call site, this ADR describes a capability that exists in the Rust core but is not yet switched on for anyone. Whoever picks up that follow-up should also decide the Keychain access-control flags (e.g. `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` vs. one that permits iCloud Keychain sync) as part of that work — not pre-decided here, since it's a product/security tradeoff involving cross-device access to reading material, not a pure Rust-core question.
