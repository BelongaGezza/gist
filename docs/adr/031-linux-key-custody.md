# ADR 031 — Linux key custody: Secret Service

**Date:** 2026-10-17
**Status:** Proposed — blocked on one dependency-policy decision (see Open decision)

## Context

ADR-011/014 delegate the AES-256 content key to the platform through `gist_store::KeyProvider`
(Keychain on Apple, DPAPI on Windows per ADR-016, Android Keystore per ADR-026).
`docs/linux-development-plan.md` D4 requires a maintained Secret Service integration, fail-closed
behaviour, no plaintext key on disk and no silent regeneration. Linux calls `gist-core` in-process,
so the provider is plain Rust implementing the existing trait; no new FFI surface.

## Decision

`spikes/linux-keyring` (excluded from the workspace, like `spikes/linux-gtk`) prototypes it:

* **The infallible trait is kept.** `LinuxKeyProvider::open(store, lock_path)` resolves the key once and
  returns a typed `KeyError` (`Unavailable`, `Locked`, `Denied`, `Corrupt(len)`, `Lock`, `Store`). A
  constructed provider always holds a valid 32-byte key (`Zeroizing`; `Debug` prints no key material). The
  shell calls `open` before `Core::init_with_read_key` and presents the error; no `gist-store` change.
* **Two layers.** A small `SecretStore` trait (`list`, `add`; deliberately no delete/replace) carries the
  policy and is tested with in-memory fakes. `SecretServiceStore` implements it over the `secret-service`
  crate's blocking API, default collection, DH-encrypted session, items tagged
  `xdg:schema=org.gist.ContentKey`.
* **Race-safe creation.** Secret Service has no put-if-absent (`CreateItem` with `replace=false` makes a
  duplicate), so creation is serialised across processes with an advisory `flock` on
  `<data>/keys/content-key.lock` (std `File::lock`, no new dependency) and the entry is re-read afterwards.
  If a non-cooperating writer still leaves duplicates, the oldest entry (then lowest object path) wins
  deterministically; duplicates are never deleted.
* **Fail closed.** A wrong-length entry is `Corrupt` even if a newer valid entry exists; nothing is
  regenerated, deleted or overwritten. A failed write yields an error, never an unpersisted key. No
  keyring means unencrypted items stay usable and the shell disables new encryption with an actionable
  message (shell responsibility, L1).

## Open decision (needs the maintainer)

`secret-service` 5.2.0 (and `oo7` 0.7.0-beta, which also fails the licence allow-list) depend on
`zbus` 5, whose `zvariant`, `zvariant_derive` and `zbus_names` crates now carry RustSec
**unmaintained** advisories RUSTSEC-2026-0370/0371/0372. They are not vulnerabilities: upstream merged
them into `zbus` 6.0. `cargo deny check advisories` is a blocking CI gate and `deny.toml` has
`ignore = []`, so promoting this crate into the workspace needs one of:

1. **Add the three IDs to `deny.toml` `ignore` with this rationale**, revisit when `secret-service` moves
   to zbus 6. (Recommended: transitive, no vulnerability, one-line removal later.)
2. Wait for a zbus-6 `secret-service`/`oo7` release; keep the crate in `spikes/` until then.
3. Implement the Secret Service client directly on zbus 6 (more code and our own DH session handling).

Until decided the crate stays in `spikes/`, outside the workspace and outside CI's deny scope.

## Verification (Linux, 2026-10-17)

12 headless tests pass: create/reuse, distinct keys, no write when a key exists, corrupt entry untouched,
corrupt-older-beats-valid-newer, duplicate resolution, typed propagation of Locked/Denied/Unavailable,
failed write, 8-thread first-open convergence (mutation-checked: 8 entries without the lock, 1 with it),
bad lock path, Debug redaction, and an end-to-end `gist-core` run (encrypt, no plaintext in the IR blobs,
readable after reopen with the stored key, unreadable with a different key). `clippy -D warnings` and
`fmt --check` clean. The real backend returns a typed `Unavailable` within ~30 ms on a private session bus
with no Secret Service.

**Not verified:** the `SecretServiceStore` against a real daemon (one `#[ignore]` test needs
`gnome-keyring`, KWallet or KeePassXC on a disposable bus), locked-keyring unlock prompts, behaviour
inside Flatpak. `originals/` stays plaintext (ADR-014, tracked gap `A5`).
