# ADR 014 — Per-item encryption, and decoupling read-decrypt from write-auto-encrypt

**Date:** 2026-09-18
**Status:** Accepted, implemented

## Context

ADR-011 added whole-store encryption at rest (`Store::open_encrypted`), but its migration story deliberately leaves every already-imported item plaintext forever unless the *entire* store is reopened via `Store::open_encrypted` and the item is re-imported — there is no way to encrypt one existing book a user cares about without a bulk re-encryption pass, which ADR-011 explicitly rejected as too risky (see its Migration section).

This ADR adds that missing per-item, opt-in capability: `Store::encrypt_item(id, key)` retroactively encrypts one already-imported item's on-disk blobs, callable independently of how the `Store` itself was opened.

**The bug this ADR also fixes.** The first implementation of `encrypt_item` took `key: &[u8; 32]` as an explicit parameter rather than reading `self.key_provider`, specifically so it would work on a `Store` opened via plain `Store::open` — which is how `CoreClient.shared`'s production instance was opened at the time. That worked for the *write* half, but nobody had separated it from the *read* half: `read_maybe_encrypted` (used by `get_item`/`get_tokens`, and everything built on them — RSVP, flow view) checked the exact same `self.key_provider` field to decide whether it had a key to decrypt with. A `Store` opened via plain `Store::open` has `key_provider = None`, so once `encrypt_item` flipped a row's `content_encrypted` flag to `1`, reading that row's actual content back through that same `Store` instance failed with `StoreError::MissingKeyProvider` — permanently, since `CoreClient.shared` never re-opens its store differently mid-session.

Translated to the running app: a user selects a book in the Library, clicks "Encrypt," and that book becomes permanently unreadable through the running app afterward — RSVP won't open it, Flow View won't open it, nothing will. This was caught by review before shipping (the original implementation's own test,
`encrypt_items_encrypts_unencrypted_and_reports_already_encrypted_idempotently`, asserted exactly this failure as "expected" behavior) — but a security feature whose one interactive entry point is a foot-gun this sharp is a data-access bug, not a documented limitation, and needed fixing before this could ship.

## Decision

### Per-item encryption

`Store::encrypt_item(id: &str, key: &[u8; 32]) -> Result<EncryptOutcome, StoreError>`:

- Looks up the item's `doc_path`/`content_encrypted` flag by id; `NotFound` if the id doesn't exist.
- If already encrypted, returns `EncryptOutcome::AlreadyEncrypted` immediately — no file I/O, no DB write. Safe to call on a mixed bulk selection without checking first.
- Otherwise: reads and BLAKE3-verifies (ADR-013) the current plaintext `<id>.json`/`<id>.tokens.json` blobs, encrypts both under `key` (AES-256-GCM, same `encrypt_at_rest` ADR-011 already established), rewrites both files plus their checksum sidecars, then sets `content_encrypted = 1` in one `UPDATE` statement. Returns `EncryptOutcome::Encrypted`.
- `key` is an explicit parameter, not read from `self`'s construction-time key provider — this is what lets any `Store`, regardless of how it was opened, encrypt a specific item on demand.

`gist-core::Core::encrypt_items(ids, key_provider)` is the bulk-friendly facade: calls `key_provider.get_or_create_key()` once (not once per id — avoids redundant Keychain round trips on the platform side), then `Store::encrypt_item` once per id, returning one `EncryptItemOutcome` per id so a UI can show an accurate summary even when some ids in a bulk selection fail (e.g. `NotFound` if an item was removed concurrently) while others succeed.

`gist-ffi::GistCore::encrypt_items` exposes the same shape over FFI, reusing the `KeyProvider` callback-interface machinery ADR-011 already defined (`CoreKeyProviderAdapter`) rather than inventing a second mechanism.

**Scope — `originals/` is deliberately untouched.** Only the two IR blobs (`<id>.json`/`<id>.tokens.json`, ADR-007) are encrypted by `encrypt_item`. ADR-006's sandboxed `originals/` copy is never read or written by this method: that file is content-addressed by a hash of its *plaintext* bytes and may be shared by more than one `library_items` row (the same dedup limitation already tracked as security register `A5`/`L-6`) — encrypting it in place would silently corrupt whatever other item still expects to find plaintext at that shared path. Nothing in the app currently reads document *content* from an original copy's path (only its existence, for deletion), so leaving it alone is safe, but it is a real, tracked gap: an item "encrypted" via `encrypt_item` can still have a plaintext copy of its original file on disk under `originals/`. Closing that gap would need per-copy reference counting or per-item (rather than content-addressed) original storage — out of scope here, tracked alongside `A5`.

### Decoupling read-decrypt from write-auto-encrypt

`Store` gains a second field:

```rust
pub struct Store {
    // ...
    /// Governs whether NEW writes are auto-encrypted (insert_item,
    /// store_original_copy). Write-path only.
    key_provider: Option<Arc<dyn KeyProvider>>,
    /// Used only by read_maybe_encrypted to decrypt content already
    /// flagged content_encrypted = 1. Read-path only.
    read_key_provider: Option<Arc<dyn KeyProvider>>,
}
```

Three constructors now exist:

| Constructor | Write path (new imports) | Read path (decrypt on load) |
|---|---|---|
| `Store::open` | plaintext (unchanged) | none — `MissingKeyProvider` on any encrypted row |
| `Store::open_encrypted(key_provider)` | encrypted under `key_provider` (unchanged, ADR-011) | `key_provider` |
| `Store::open_with_read_key(key_provider)` **(new)** | plaintext | `key_provider` |

`read_maybe_encrypted` (the single choke point `get_item`/`get_tokens` both go through) now consults a new `decryption_key()` helper — `read_key_provider`, falling back to `key_provider` — instead of the write-only `encryption_key()` it used before. `open_encrypted` sets both fields to the same provider, so its behavior is unchanged, byte-for-byte, from before this ADR (this was a hard requirement — no change was made to any existing `open_encrypted` call site or test).

`gist-core::Core::init_with_read_key` and `gist-ffi::GistCore::new_with_read_key` mirror `open_with_read_key` at their respective layers, following the exact `init`/`init_encrypted` and `new`/`new_encrypted` naming precedent already established by ADR-011.

**`CoreClient.shared`'s production `init()` now calls `GistCore.newWithReadKey(dbPath:storageDir:keyProvider:)` with a real `KeychainKeyProvider()`**, replacing the plain `GistCore(dbPath:storageDir:)` it used before. This is the actual fix, end to end: new imports through `.shared` still land plaintext by default (nothing about the default import experience changes), but an item a user encrypts via the Library's "Encrypt" action is now genuinely readable afterward through the same running app, because `.shared`'s `GistCore` finally has a real key to decrypt with.

## Consequences

**Easier:** any future feature that needs "read-only access to a key a different write path controls" (a hypothetical read-only export tool, a future migration/verification pass) can reuse the same `read_key_provider` field and `open_with_read_key` constructor rather than inventing a new mechanism. The three-constructor table above is also just documentation of what already existed implicitly — `open`/`open_encrypted` didn't change behavior, they just now sit at two ends of a spectrum `open_with_read_key` fills the middle of.

**Harder / to watch:**
- A `Store` opened via `open_with_read_key` with the *wrong* key (not matching whatever key encrypted the row) still fails correctly, with `StoreError::DecryptionFailed`, not silently — this was verified by a dedicated test (`encrypt_item_then_read_through_read_capable_store_with_wrong_key_fails`). Read capability doesn't bypass the cryptography; it only makes the *right* key reachable.
- The `originals/` scope gap above is unchanged by this ADR and remains open. A user relying on "Encrypt" to protect a document's original file bytes (not just GIST's internal copy) would be surprised — this should be called out in any future user-facing copy about what "Encrypt" actually protects (the in-app confirmation alert already says "Only GIST's internal document data is affected, never your original file").
- `CoreClient.shared`'s Keychain-backed key is now created on first launch after this change lands (via `KeychainKeyProvider.getOrCreateKey()`'s first call, triggered by `.shared`'s `init()` itself) rather than only when a user first clicks "Encrypt." This has no user-visible effect today (nothing reads the key unless a row is actually flagged encrypted) but does mean a Keychain item now exists for every user of a build with this change, not just users who used per-item encryption — worth knowing if a future feature ever wants to distinguish "has this user ever encrypted anything" from "does a key exist."

## Verification

- `gist-store`: 16 new tests total across this ADR's two pieces — `encrypt_item`'s core behavior (round trip via a separately-opened `open_encrypted` store, idempotency, unknown-id, `originals/` untouched, checksum sidecar rewrite, flag visibility in `list_items`) plus the read/write decoupling fix specifically: `encrypt_item_then_read_through_same_read_capable_store_succeeds` (the gap this ADR closes — encrypt and read back through the *same* `Store` instance, and confirms a same-instance plaintext item stays unaffected) and `encrypt_item_then_read_through_read_capable_store_with_wrong_key_fails` (wrong key still fails cleanly).
- `gist-core`: `encrypt_items_then_read_through_read_capable_core_succeeds` — the same scenario through `Core::init_with_read_key`, including confirming both real content-reading call sites (`get_document` and `start_rsvp`) succeed afterward, not just one. The original `encrypt_items_encrypts_unencrypted_and_reports_already_encrypted_idempotently` test (which exercises a genuinely keyless `Core::init`, still correct behavior) is retained with an updated comment clarifying it no longer describes `CoreClient.shared`'s actual configuration.
- `gist-ffi`: `new_with_read_key` compiles and is exercised transitively via the Swift-side integration tests below; no dedicated Rust-side FFI test was added since `gist-ffi`'s existing pattern is to keep FFI-layer tests thin (see `new_encrypted`, which has none either) and rely on `gist-core`'s tests for behavior coverage.
- Swift: `apps/apple/Tests/GISTTests.swift` gained `testEncryptItemsThenReadBackSucceedsOnAReadCapableClient`, which builds a `CoreClient` via the new `init(dbPath:storageDir:keyProvider:)` test seam (mirroring `.shared`'s real `newWithReadKey`-backed shape), encrypts a freshly-imported item, and confirms both `loadDocument` (Flow View's read path) and `startRsvp` (RSVP's read path) succeed afterward with no `client.error` set. `cargo test --workspace`/`clippy -D warnings`/`fmt --check`/`cargo deny check bans licenses sources` and `xcodebuild build`/`test` (scheme `GISTmacOS`) were all re-run clean after this change — see this ADR's entry in the `A6` security register row and the CLAUDE.md changelog for the exact counts.
