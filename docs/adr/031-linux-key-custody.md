# ADR 031 — Linux key custody: Secret Service

**Date:** 2026-10-17
**Status:** Proposed — implemented as a spike; real-daemon verification outstanding

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
  policy and is tested with in-memory fakes. `SecretServiceStore` implements it with a purpose-built client
  on **zbus 6** (`backend.rs`, about six D-Bus calls: `OpenSession`, `ReadAlias`, `Unlock`/`Prompt`,
  `SearchItems`, `GetSecret`, `CreateItem`) on the default collection, items tagged
  `xdg:schema=org.gist.ContentKey`. Every method call has a 15 s timeout so a wedged key service becomes
  `Unavailable`, not a hung launch; waiting for the user to answer an unlock prompt is unbounded by design.
  Call it from a worker thread, never the GTK main loop.
* **Session crypto (`dh.rs`).** `dh-ietf1024-sha256-aes128-cbc-pkcs7`: DH over the RFC 2409 Second Oakley
  Group, HKDF-SHA256 (no salt, empty info) to an AES-128 key, AES-128-CBC/PKCS#7 with a fresh IV per
  message (`num-bigint`, `hkdf`, `sha2`, `aes`, `cbc`, all RustCrypto/permissive). `plain` is never used.
  Degenerate peer values (0, 1, p-1, >= p) are rejected. Accepted limits: 1024-bit group and a
  non-constant-time `modpow`, because the exchange is ephemeral, local and per connection, and it is what
  every Secret Service implements.
* **Race-safe creation.** Secret Service has no put-if-absent (`CreateItem` with `replace=false` makes a
  duplicate), so creation is serialised across processes with an advisory `flock` on
  `<data>/keys/content-key.lock` (std `File::lock`, no new dependency) and the entry is re-read afterwards.
  If a non-cooperating writer still leaves duplicates, the oldest entry (then lowest object path) wins
  deterministically; duplicates are never deleted.
* **Fail closed.** A wrong-length entry is `Corrupt` even if a newer valid entry exists; nothing is
  regenerated, deleted or overwritten. A failed write yields an error, never an unpersisted key. No
  keyring means unencrypted items stay usable and the shell disables new encryption with an actionable
  message (shell responsibility, L1).

## Dependency decision (resolved 2026-10-17: option 3)

The ready-made crates (`secret-service` 5.2.0, `oo7` 0.7.0-beta) depend on `zbus` 5, whose `zvariant`,
`zvariant_derive` and `zbus_names` crates carry RustSec *unmaintained* advisories RUSTSEC-2026-0370/0371/0372
(merged into `zbus` 6.0 upstream, not vulnerabilities). `cargo deny check advisories` is a blocking CI gate
with `ignore = []`, and `oo7` also fails the licence allow-list. The maintainer chose to write the client
directly on zbus 6 rather than add advisory ignores or wait. `cargo deny check bans licenses sources
advisories` is clean for the spike's shipping dependency tree (zbus 6.0.0 + RustCrypto; no `tokio`, no
OpenSSL, no `secret-service`). Cost: we now own the protocol code and the DH session handling. zbus 6.0.0 is
a new major version; revisit when `secret-service` itself moves to zbus 6.

## Verification (Linux, 2026-10-17)

28 headless tests pass (`GIST_REQUIRE_DBUS=1` makes the wire tests fail instead of skip when `dbus-daemon` is
missing); `clippy --all-targets -D warnings` and `fmt --check` clean.

* **Policy (fakes):** create/reuse, distinct keys, no write when a key exists, corrupt entry untouched,
  corrupt-older-beats-valid-newer, duplicate resolution, typed propagation of Locked/Denied/Unavailable,
  failed write, 8-thread first-open convergence (mutation-checked: 8 entries without the lock, 1 with it),
  bad lock path, Debug redaction, and an end-to-end `gist-core` run (encrypt; no plaintext in the IR blobs;
  readable after reopen with the stored key; unreadable with a different key).
* **Crypto:** same key both sides, fresh IV, wrong peer fails, degenerate peers rejected, malformed input is
  an error not a panic, and a known-answer test whose expected key was computed independently in Python
  (mutation-checked: changing the HKDF info makes it fail). The prime matches the `secret-service` crate's
  constant byte for byte.
* **Wire (fake service on a private `dbus-daemon` the tests start; never the user's bus):** stored secret is
  decrypted correctly by the service and read back in a fresh session; existing key reused; wrong-length
  entry fails closed with nothing added or removed; locked without prompt -> `Locked`; accepted prompt ->
  key created; dismissed prompt -> `Denied` and nothing created; no service on the bus -> `Unavailable`;
  unreachable bus -> `Unavailable`; a service that never answers -> `Unavailable` within the timeout (found
  by a hang during mutation testing; the timeout is the fix).

**Not verified:** the client against a real daemon. The wire tests prove conformance to the Secret Service
spec *as read here*, with a fake written by the same author, and the shared `dh` module means a
self-consistent mistake would not show on the wire (only the Python known-answer test guards that). The
`#[ignore]` test needs GNOME Keyring, KWallet's bridge or KeePassXC on a disposable bus. Also unverified:
real unlock prompts, Flatpak access. `originals/` stays plaintext (ADR-014, tracked gap `A5`).
