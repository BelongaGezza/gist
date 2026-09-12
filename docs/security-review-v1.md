# GIST Reader — Security Review v1

**Prepared for:** Systems Architect  
**Date:** 2026-09-08  
**Codebase state:** Milestone 0 (M0) — core scaffolding; most parsers were stubs at review time  
**Current status:** All High/Medium findings closed in M1. Three items remain open (F10, A3, A4).

**⚠️ Superseded/partial as of 2026-09-12:** this document predates the collections/search/URL-import/removal/copy-on-import work, the `[A5]` copy-on-import fix, and an independent second security audit (also 2026-09-12) that found 12 new findings (`F14`–`F25`) and 2 new architecture items (`A6`, `A7`) not reflected below. Spot-checks of this doc's closed findings (F1/F2/F3/F6/F9/F11) still hold — this is a coverage gap, not a regression. `CLAUDE.md`'s security register and `docs/development-plan-v2.md` §8 are the current sources of truth; a `security-review-v2.md` pass to properly supersede this file is tracked as future work, not done here.

---

## Executive Summary

GIST's architecture is sound at its foundation. The Rust core, `rusqlite` with parameterised queries, a `Mutex`-wrapped connection, uniffi FFI, and a pure RSVP engine with no I/O are well-considered decisions that eliminate whole classes of common vulnerability.

The most consequential risk at M0 was the **absence of `catch_unwind` at the FFI boundary** — resolved in M1. The second concern was that four parsers were empty stubs with no resource limits, zip-bomb rejection, or DRM detection — all resolved. A third was supply-chain weakness via mutable GitHub Actions tags — resolved immediately (pre-M1).

**All High and Medium findings are closed. See the security register for the three remaining open items (F10, A3, A4).**

---

## Findings

### F1 — High: No `catch_unwind` at FFI Boundary ✅ Closed (commit 10b1c1e)

A panic propagating from Rust across a C ABI into Swift is undefined behaviour. Every `#[uniffi::export]` function is now wrapped in the `ffi_catch!` macro (catch_unwind + GistError::InternalPanic). `[profile.release] panic = "abort"` set in gist-ffi.

### F2 — High: Mutex Poisoning ✅ Closed (commit 10b1c1e)

`.lock().unwrap()` at four call sites in gist-store replaced with `.lock().unwrap_or_else(|p| p.into_inner())`.

### F3 — High: GitHub Actions Pinned by Mutable Tag ✅ Closed (commit 10b1c1e)

All third-party actions pinned to immutable commit SHAs across all three workflow files:
```yaml
- uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683  # v4.2.2
- uses: dtolnay/rust-toolchain@4305c38b25d97ef35a8ad1f985ccf33d0b9d0787
- uses: EmbarkStudios/cargo-deny-action@34899fc28e28a96b4e8215ecdfdee2e2dbde5706
```

### F4 — Medium: Complex Parsers Were Empty Stubs ✅ Closed (commit 10b1c1e)

`ParseLimits` struct defined in `gist-core` and enforced in all parsers (max_bytes, max_pages, max_nesting_depth, max_expanded_bytes). Streaming decompression limit for zip-based formats (ePub, DOCX).

### F5 — Medium: `deny.toml` Missing Critical Sections ✅ Closed (commit 10b1c1e)

Added `[advisories]`, `[bans]`, and `[sources]` (unknown-registry = "deny") sections. **Correction (2026-09-12 audit, finding L-8):** this entry originally claimed `[advisories]` set an explicit `vulnerability = "deny"` key; the live `deny.toml` does not have that key — it has only `version = 2` and `ignore = []`. This is harmless under current `cargo-deny` defaults (advisories are denied by default), but the original claim was inaccurate and is corrected here rather than left standing.

### F6 — Medium: Document IDs Were Predictable Nanosecond Timestamps ✅ Closed (commit 10b1c1e)

Replaced with `Uuid::now_v7().to_string()`. The `uuid` crate was already a declared dependency.

### F7 — Medium: Binding Generation Failures Silenced ✅ Closed (commit 10b1c1e)

`|| true` removed from Xcode pre-build script. Replaced with graceful fallback: fails only when no generated files already exist.

### F8 — Low: ePub DRM Detection Not Implemented ✅ Closed (ADR-004 + commit 906957e)

`check_drm()` in gist-parse-epub parses `META-INF/encryption.xml`, exempts IDPF font obfuscation (`http://www.idpf.org/2008/embedding`), returns `ParseError::DrmProtected` for commercial algorithms. ADR-004 written.

### F9 — Low: Rust Toolchain Not Pinned ✅ Closed (commit 10b1c1e)

`rust-toolchain.toml` now specifies `channel = "1.87.0"`.

### F10 — Low: Release Pipeline Is a Placeholder ⏳ Open — M4

`release-macos.yml` now has a guard step that exits non-zero (prevents accidental empty releases). Full signing + notarisation implementation deferred to M4.

### F11 — Low: Schema Migration Had No Version Ceiling ✅ Closed (commit 2c0b4b5)

`SCHEMA_VERSION = 2` ceiling check added: returns `StoreError::SchemaTooNew` if `user_version > SCHEMA_VERSION`. Migrations wrapped in transactions.

### F12 — Informational: Full File Paths Stored and Logged ⏳ Accepted — document in PRIVACY.md at M4

`source_ref` stores full filesystem paths in the SQLite database. This is intentional for provenance. Changed `tracing::info!` to `tracing::debug!` for source path log lines. Any future diagnostic/telemetry must strip/hash paths before transmission. Will be documented in `docs/PRIVACY.md` before M4.

### F13 — Informational: `gist-web` Was an Empty Stub ✅ Closed (ADR-005 + commit 50b2c84)

ADR-005 (web fetch policy) and ADR-010 (HTTP ureq+rustls) written. gist-web implemented: HTTPS-only, robots.txt, max 5 redirects, 30s/60s timeouts, response size limit, no cookie jar, readability extraction.

---

## Architecture-Level Findings

### A1: FTS5 Index Not Present in Schema ✅ Closed (ADR-008 + commit 2c0b4b5)

External-content FTS5 table with `tokens` shadow table. Schema v1→v2 migration. ADR-008 written.

### A2: OCR Callback Interface Not Implemented ✅ Closed (ADR-009 + commit f6b3c46)

`OcrEngine: Send + Sync` trait defined in `gist-core` (not gist-ffi, to avoid circular dep). `OcrPageResult` uniffi Record. `CoreOcrAdapter` bridges FFI OcrEngine → core OcrEngine. ADR-009 written.

### A3: App Sandbox Entitlements Not Visible ⏳ Open — M3

`ENABLE_HARDENED_RUNTIME: YES` is set correctly. An explicit `.entitlements` file with minimum entitlements (`com.apple.security.files.user-selected.read-only` for file importer, plus camera and network with justification) must be produced and reviewed against least-privilege before M4 notarisation pipeline.

### A4: No Integrity Verification for Stored Documents ⏳ Open — M4 decision

Document JSON blobs are written/read without checksum. Decision on whether BLAKE3 checksumming is warranted given the threat model must be made by M4 start and recorded in an ADR.

---

## Security Register Summary

| ID | Severity | Status |
|---|---|---|
| F1 | High | ✅ Closed — commit 10b1c1e |
| F2 | High | ✅ Closed — commit 10b1c1e |
| F3 | High | ✅ Closed — commit 10b1c1e |
| F4 | Medium | ✅ Closed — commit 10b1c1e |
| F5 | Medium | ✅ Closed — commit 10b1c1e |
| F6 | Medium | ✅ Closed — commit 10b1c1e |
| F7 | Medium | ✅ Closed — commit 10b1c1e |
| F8 | Low | ✅ Closed — ADR-004 + commit 906957e |
| F9 | Low | ✅ Closed — commit 10b1c1e |
| F10 | Low | **Open — M4** |
| F11 | Low | ✅ Closed — commit 2c0b4b5 |
| F12 | Info | **Accepted — PRIVACY.md at M4** |
| F13 | Info | ✅ Closed — ADR-005 + commit 50b2c84 |
| A1 | Architecture | ✅ Closed — ADR-008 + commit 2c0b4b5 |
| A2 | Architecture | ✅ Closed — ADR-009 + commit f6b3c46 |
| A3 | Architecture | **Open — M3** |
| A4 | Architecture | **Open — M4 decision** |

**Open items blocking release:** F10 (M4), A3 (M3), A4 (M4). F12 accepted and deferred.

---

## Positive Observations (unchanged from original review)

1. Parameterised SQL throughout — no string-concatenated queries, no SQL injection risk.
2. WAL mode and foreign keys enabled at connection open, before schema creation.
3. Structured `thiserror`-derived error enums throughout; `GistError` correctly flattens at FFI boundary.
4. `gist-rsvp` is pure with no I/O, no `unsafe` — trivially auditable.
5. WPM clamped to 100–1000 in Rust — defensive against absurd UI values.
6. `clippy -D warnings` + `cargo fmt --check` as blocking CI gates.
7. `cargo-deny` licence allowlist as blocking CI gate.
8. `ENABLE_HARDENED_RUNTIME: YES` correctly set.
9. Shell scripts use `set -euo pipefail`.
10. uniffi proc-macro mode — no hand-rolled C ABI.
11. `ureq` + `rustls` — avoids system OpenSSL version variance.
