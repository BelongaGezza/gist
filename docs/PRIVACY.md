# GIST — Privacy

**Status:** current as of 2026-09-28. Closes security register item `[F12]` and
satisfies `docs/product-spec-reader-app-v3.md` §9.2. Describes what the
shipped code actually does today, not aspirational behavior — every claim
below is backed by a specific file/ADR reference so it can be re-verified
against the source directly.

GIST is a local-first reading app. There is no server component, no account
system, and no backend GIST's authors operate or can see data from. Everything
described below happens entirely on the device running the app.

## 1. What GIST stores, and where

Everything lives under one sandboxed per-app storage directory
(`storage_dir`) plus a local SQLite database. Nothing is stored anywhere
else on the device, and nothing is synced off it by GIST itself.

| What | Where | Notes |
|---|---|---|
| Library metadata (title, author, import date, tags, collections, reading progress) | SQLite (`gist-store`) | Queried locally only; never transmitted. |
| `source_ref` — the original file's full filesystem path, or the source URL | SQLite `library_items` row | Informational/provenance display only ("Imported from ~/Downloads/book.epub"). Never used for reading or deletion, and never transmitted anywhere. See §3 for its logging policy. |
| Parsed document content (the IR block tree and the flat token stream) | `<storage_dir>/<id>.json` and `<storage_dir>/<id>.tokens.json` | One pair of files per imported item (ADR-007). Plaintext by default; see §4 for the optional encrypted case. |
| A sandboxed copy of the original imported file | `<storage_dir>/originals/<content-hash>.<ext>` | File-based imports only (txt/epub/docx and, from M3, multi-page OCR scans) — content-addressed by a SHA-256 hash of the file's bytes, so identical re-imports dedupe automatically (ADR-006). **URL imports have no copy here** — there is no local file to copy; the fetched, extracted content lives only in the IR blobs above. GIST never reads back from or writes to the user's original file at its original location — only this sandboxed copy is ever touched by removal. |
| Integrity checksums | A `.blake3` sidecar file next to each checksummed file above (e.g. `<id>.json.blake3`) | Plain-text BLAKE3 digest, used only to detect on-disk corruption of GIST's own prior writes — not a security boundary, not transmitted (ADR-013). A missing sidecar (any file written before this feature existed) is treated as "unverified," never as an error. |
| Encryption-at-rest key material | The OS keychain (macOS Keychain via `kSecClassGenericPassword`, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`) | Generated on-device via `SecRandomCopyBytes`; GIST's own code never sees, stores, or transmits this key outside the platform keychain API (ADR-011). |

## 2. What ever leaves the device

**Only the URL-import fetch itself — nothing else.** When a user pastes a URL
to import an article, `gist-web::fetch_url` makes exactly one outbound
request chain: the target URL (plus, before it, a `robots.txt` pre-check
against the same host) over HTTPS. That is confirmed directly from
`crates/gist-web/src/lib.rs`, which enforces (ADR-005):

- **TLS only.** `.https_only(true)` on the HTTP agent; plain-HTTP URLs are
  rejected before any connection is attempted.
- **SSRF protection on every connection, including redirects.** A custom
  resolver (`safe_resolve`) rejects any address that isn't globally routable
  (loopback, RFC1918/private, link-local including the cloud metadata
  address, multicast, CGNAT, IPv6 unique-local/link-local, etc.) — enforced
  by the HTTP client on the initial connection *and* every redirect hop
  (closing security findings `F14`/`M-1`), so a URL import can't be used to
  reach services on the user's own machine or local network.
- **Bounded blast radius.** Maximum 5 redirects, a 50 MB response cap, a
  30 s connect timeout and a 60 s total transfer timeout.
- **No cookies.** No cookie jar exists; nothing is sent or stored, and each
  import is stateless.
- **`robots.txt` is honored** before the target URL is fetched (best-effort,
  5 s timeout — failure to fetch it doesn't block the import).
- **No image data, no personal data, no telemetry payload of any kind rides
  along with this request.** The only bytes that leave the device are the
  HTTP request itself (URL, a fixed `GIST/1.0` User-Agent, standard HTTP
  headers) — the same shape of request any web browser makes to load a page.

**Nothing else in GIST makes a network call.** File-based imports (txt,
ePub, DOCX) and RSVP/flow-view reading never touch the network at all — they
are pure local file/database operations. There is **no telemetry, no
analytics, no crash reporting, and no data collection of any kind** anywhere
in the app, per product spec §9.2. If a future release ever adds a
diagnostics or crash-reporting integration, it must strip or hash file paths
(`source_ref`, see §3) before any such integration is wired up — this
document should be updated as part of that change, not after it ships.

## 3. `source_ref` and logging

`source_ref` — the original file's full filesystem path (or, for a URL
import, the source URL) — is retained in the local database purely for
provenance display ("Imported from ~/Downloads/book.epub"). It is:

- **Never transmitted anywhere.** It plays no role in `gist-web`'s network
  calls, and nothing else in the app sends it off-device.
- **Never used for reading or deletion.** Only `source_copy_ref` (the
  sandboxed copy under `originals/`, or nothing at all for URL imports) is
  ever read from or deleted by GIST — `source_ref`'s raw path is informational
  only (ADR-006).
- **Logged at `debug!` level only, never at `info!` or above.** This is an
  explicit, enforced convention across the Rust core (see the crate
  conventions in `CLAUDE.md`, and e.g. `crates/gist-core/src/lib.rs`'s
  file-deletion logging, which logs the path only via `tracing::debug!`).
  Default log levels never include a full filesystem path. This constraint
  must be reviewed before adding any logging or crash-reporting integration
  that might run at a higher default verbosity, per product spec §9.2.

## 4. Encryption at rest

**Current state: opt-in, per item — not on by default for new imports.**
This is the accurate, as-shipped description; do not read anything below as
"GIST encrypts your library." It doesn't, unless a user explicitly asks it
to for a specific item.

- New imports land **plaintext on disk** by default, exactly as they always
  have. Nothing about the default import path changes because this
  capability exists (ADR-011/ADR-014).
- A user can select an item in the macOS Library view and choose **"Encrypt"**
  to retroactively encrypt that one item's two IR blobs
  (`<id>.json`/`<id>.tokens.json`) with AES-256-GCM, using a key generated
  and held in the macOS Keychain (`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`
  — device-local, not synced via iCloud Keychain). GIST's own code never
  handles this key directly; it only calls a platform-provided
  `KeyProvider` interface (ADR-011).
- **The sandboxed `originals/` copy (§1) is deliberately not touched by
  "Encrypt."** Only the two IR blobs are encrypted. If the original file's
  bytes were copied into `originals/` at import time (file-based imports
  only), that copy remains plaintext on disk even after the item shows as
  "encrypted" in the Library. This is a known, documented scope limit
  (ADR-014) — the in-app confirmation for "Encrypt" says only GIST's internal
  document data is affected, never the user's original file.
- Losing the platform-held key makes that item's encrypted content
  permanently unreadable through GIST — there is no recovery path other than
  the OS's own keychain backup/sync mechanisms, which are outside GIST's
  control.
- Items imported before this feature existed, and never explicitly
  encrypted afterward, remain plaintext indefinitely — there is no automatic
  bulk re-encryption pass (ADR-011 §"Migration" explains why: a bulk rewrite
  of an arbitrarily large library was judged riskier than leaving old items
  as they were).

## 5. At-rest integrity checksums

Every IR blob and every sandboxed original-file copy GIST writes gets a
BLAKE3 checksum sidecar file (ADR-013). This is a **corruption-detection**
mechanism, not a confidentiality mechanism — it answers "is this file
unchanged since GIST wrote it," not "can someone else read it."

- On every real read of document content, the checksum is verified
  automatically, before any decryption attempt. If it doesn't match, GIST
  reports a clear, typed error (`ChecksumMismatch`) rather than silently
  serving corrupted content, silently failing to parse, or crashing. **Your
  data is never silently discarded or overwritten because of a checksum
  failure** — a mismatch only ever produces an error the item failed to
  load; nothing is deleted or altered as a result. The only way to recover a
  genuinely corrupted item today is to re-import it from its original
  source, if still available.
- A file with no checksum sidecar (anything written before this feature
  existed) is treated as "unverified" — never as an error, and never
  reported as "corrupt" for the simple fact that no checksum was ever
  recorded for it.
- Checksums play no role in confidentiality and are not affected by whether
  an item is encrypted (§4) — they're computed over whatever bytes are
  actually on disk, ciphertext or plaintext.

## 6. On-device processing (OCR)

Optical character recognition for scanned/photographed pages runs entirely
on-device, using the platform's native OCR engine (Apple's `Vision`
framework on macOS/iOS) via a Rust ↔ platform callback interface (ADR-009).
No image data is ever sent off-device for recognition, and no image data
rides along with any network request GIST makes (§2) — OCR and URL import
are entirely separate code paths.

## 7. Local storage sandboxing

The macOS app runs with Apple's App Sandbox enabled, with explicitly
declared, reviewed entitlements (app-sandbox, user-selected file
read/write, network client only — see `docs/adr/012-macos-app-sandbox.md`).
GIST cannot access files outside what the user explicitly selects via the
system file picker, and has no network-listener or server capability of any
kind.

## 8. Summary

- No account, no server, no sync, no telemetry, no analytics, no crash
  reporting.
- The only network traffic GIST ever generates is the URL-import fetch a
  user explicitly initiates, over TLS, with SSRF protections on every hop.
- Personal reading material is stored locally, plaintext by default, with
  an opt-in per-item encryption action available, and on-disk integrity
  checksums that fail loudly rather than silently on corruption.
- The user's original imported files are never modified or deleted by
  GIST — only GIST's own sandboxed copy of them (for file-based imports) is
  ever touched.

This document should be kept current as encryption, checksum, or network
behavior changes — in particular, revisit §4 if/when encryption-at-rest is
ever made on-by-default for new imports, and §2/§3 before adding any
diagnostics, telemetry, or crash-reporting integration.
