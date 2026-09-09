# GIST — Claude Code Context

GIST is a cross-platform RSVP-style reading app. The Rust core handles all parsing, persistence, and pacing logic; native UI shells (SwiftUI on macOS/iOS, WinUI 3 on Windows) call into it via FFI.

**Current state:** M1 complete. M2 (Library & Reading UI) is next.

## Project name & identity

The app is named **GIST**. All crates use the `gist-*` prefix. The older name "Readrrr" appears only in `docs/development-plan-v1.md` — ignore it; v2 supersedes it.

## Repository layout

```
gist/
├── Cargo.toml                  # workspace root
├── rust-toolchain.toml         # pinned to 1.87.0
├── deny.toml                   # cargo-deny: licences, advisories, bans, sources
├── crates/
│   ├── gist-model/             # Document model, IR types, token stream, errors
│   ├── gist-parse-txt/         # Plain text import
│   ├── gist-parse-epub/        # ePub (DRM detection + ParseLimits) ✅
│   ├── gist-parse-docx/        # DOCX (style-resolution + tracked-changes) ✅
│   ├── gist-parse-pdf/         # PDF — stub only (pdfium build tooling deferred)
│   ├── gist-imageprep/         # OCR pre-processing skeleton
│   ├── gist-web/               # URL fetch (ureq/rustls + robots.txt + readability)
│   ├── gist-rsvp/              # Pure RSVP pacing engine (no I/O, no timers)
│   ├── gist-store/             # SQLite + FTS5 via rusqlite
│   ├── gist-core/              # Import pipeline facade + ImportObserver
│   └── gist-ffi/               # uniffi scaffolding → .xcframework
├── apps/
│   ├── apple/                  # SwiftUI macOS + iOS (generated Xcode project)
│   └── windows/                # Placeholder
├── fixtures/                   # 24 synthetic public-domain test fixtures
├── fuzz/                       # cargo-fuzz workspace (separate from root)
├── tools/                      # build-core-xcframework.sh, notarize.sh, gen-bindings.sh
└── docs/
    ├── ARCHITECTURE.md
    ├── adr/                    # ADRs 001–010
    ├── development-plan-v1.md  # Superseded — use v2
    ├── development-plan-v2.md  # Current plan (security hardening integrated)
    ├── product-spec-reader-app-v3.md
    └── security-review-v1.md   # M0 security review; all findings closed or tracked
```

## Key architectural decisions (ADRs in docs/adr/)

| ADR | Decision |
|-----|----------|
| 001 | FFI = uniffi proc-macro mode (not hand-rolled C ABI) |
| 002 | PDF backend = `pdfium-render` behind a swappable trait |
| 003 | Annotation anchoring: `(block_id, start, len, prefix_hash, quote_hash)` |
| 004 | DRM: detect via `META-INF/encryption.xml`; never circumvent |
| 005 | Web fetch: TLS-only via rustls, max 5 redirects, robots.txt |
| 006 | Copy-on-import (not reference-in-place) |
| 007 | IR storage: `<id>.json` + `<id>.tokens.json` on disk; SQLite holds metadata |
| 008 | FTS5: external-content table + tokens shadow table + sync triggers |
| 009 | OcrEngine: uniffi callback interface (Swift implements, Rust orchestrates) |
| 010 | HTTP: ureq + rustls (no tokio, no system OpenSSL) |

## Security policies — must not be relaxed

All are enforced in code (M1 ✅). See `docs/security-review-v1.md` for full detail.

- **FFI panic safety:** every `#[uniffi::export]` function is wrapped in `catch_unwind`. Panics map to `GistError::InternalPanic`, never propagate across the C ABI.
- **Mutex discipline:** all `gist-store` lock acquisitions use poison-tolerant recovery (`.unwrap_or_else(|p| p.into_inner())`).
- **ParseLimits:** every parser accepts a `ParseLimits` struct and enforces `max_bytes`, `max_pages`, `max_nesting_depth`, `max_expanded_bytes` before allocation.
- **DRM:** `gist-parse-epub` detects DRM via `META-INF/encryption.xml` and returns `ParseError::DrmProtected`. No decryption is ever attempted.
- **Supply chain:** all GitHub Actions pinned to immutable commit SHAs. `cargo-deny` blocks on RUSTSEC advisories.
- **Document IDs:** UUIDv7 (not timestamps).
- **Schema migrations:** transactional; version ceiling check (`SchemaTooNew`).
- **Source paths:** logged at `debug!` level only, never `info!`.

## What's done (M0 + M1)

All Rust crates are implemented:
- `gist-model`, `gist-store` (FTS5 schema v2), `gist-parse-txt`, `gist-parse-epub`, `gist-parse-docx`, `gist-rsvp`, `gist-web`, `gist-imageprep` (skeleton), `gist-core`, `gist-ffi`
- `gist-parse-pdf` is a 1-line stub — pdfium build tooling deferred
- 24 fixture files in `fixtures/` with provenance in `fixtures/README.md`
- cargo-fuzz targets for txt/epub/docx/web_extract; nightly CI runs 120s each
- Corpus test: `crates/gist-core/tests/corpus.rs`

## What's next (M2)

SwiftUI macOS app:
1. App skeleton + `CoreClient` actor wrapping FFI
2. Library grid/list view (collections, tags, sort/filter, FTS search)
3. All import flows + OCR review screen + DRM error presentation
4. Theme engine (light/dark/sepia/OLED, OS-follow)
5. Flow reading view (virtualised, typography controls, TOC, in-document search)

Exit criterion: a team member can use it as their daily reader.

## Open questions (decide before M2/M3/M4)

| Q | Must decide by |
|---|----------------|
| Q3: Paginated view — v1.0 or v1.1? (plan says v1.1; flow view uses ReadingLayout abstraction) | M2 start |
| Q8: SwiftUI Text vs TextKit 2 for flow view? (prototype both in M2) | M2 end |
| Q10: Schema/IR versioning + forward compatibility policy | M4 start |
| Q11: At-rest document integrity — BLAKE3 checksums on stored blobs? | M4 start |

## Build & toolchain

```bash
# Rust toolchain (pinned in rust-toolchain.toml)
rustup show   # should say 1.87.0

# Run all tests
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings
cargo fmt --check

# cargo-deny
cargo deny check

# Build xcframework (macOS + iOS slices)
./tools/build-core-xcframework.sh

# Regenerate Swift bindings
./tools/gen-bindings.sh

# Corpus test
cargo test --test corpus --package gist-core
```

XcodeGen generates the Xcode project — do NOT commit `apps/apple/*.xcodeproj`. Run `xcodegen generate` inside `apps/apple/` after cloning.

## Crate conventions

- No `unwrap()` on parsed/external data. Use `?` or `unwrap_or_else`.
- No bare `.lock().unwrap()` in `gist-store` — always poison-tolerant.
- Every `#[uniffi::export]` fn body wrapped in the `ffi_catch!` macro (defined in `gist-ffi`).
- `source_ref` paths at `debug!` level only.
- `gist-model` must compile to `wasm32-unknown-unknown` (no I/O deps).
- Document IDs: `Uuid::now_v7().to_string()`.
- `ParseLimits` passed to every parser — never skip limit checks.

## CI pipelines (.github/workflows/)

| Pipeline | Trigger | What it does |
|----------|---------|--------------|
| core-test | every PR | `cargo test`, `clippy -D warnings`, `fmt --check` on ubuntu + macos |
| core-quality | PR + nightly | `cargo deny`, `cargo audit`, `cargo udeps` |
| parser-corpus | nightly + parse-crate push | corpus test; uploads results artifact |
| fuzz | nightly | 120s per target; uploads crash artifacts |
| apple-build | PR touching apps/apple or FFI | XcodeGen → xcodebuild macOS debug + unit tests |
| release-macos | tag v* | **Guard step exits non-zero until M4 implemented** |

All third-party actions pinned to commit SHAs (see workflow files).

## Milestone register

| Milestone | Status | Goal |
|-----------|--------|------|
| M0 | ✅ Done | Architecture + vertical slice (txt → RSVP in macOS app) |
| M1 | ✅ Done | Security hardening + parser breadth (epub/docx/web/imageprep) |
| M2 | ⏳ Next | Library & Reading UI (SwiftUI macOS) |
| M3 | — | RSVP view, annotations, accessibility pass |
| M4 | — | Hardening, entitlements review, notarised DMG pipeline |
| M5 | — | v1.0 public release |

## Security register (open items)

| ID | Severity | Open | Notes |
|----|----------|------|-------|
| F10 | Low | M4 | Release pipeline — add real signing + notarisation |
| F12 | Info | Accepted | Document source_ref paths in PRIVACY.md before M4 |
| A3 | Architecture | M3 | App Sandbox entitlements review + explicit .entitlements file |
| A4 | Architecture | M4 | Decide at-rest integrity (BLAKE3 checksums) — record ADR |

All other findings from Security Review v1 are closed.
