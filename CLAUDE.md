# GIST — Claude Code Context

GIST is a cross-platform RSVP-style reading app. The Rust core handles all parsing, persistence, and pacing logic; native UI shells (SwiftUI on macOS/iOS, WinUI 3 on Windows) call into it via FFI.

**Current state:** M1 complete and CI-gate-verified (`cargo test`/`clippy -D warnings`/`fmt --check`/`cargo deny check bans licenses sources` all genuinely green as of 2026-09-10 — an earlier "M1 ✅" claim in this file predated that verification and was false when written; see the M1 audit below). M2 (Library & Reading UI) is in progress: multi-format import (txt/epub/docx) and a functional RSVP reading view are wired end-to-end; library chrome (collections/tags/search UI), theming, and OCR review are not started. None of the Swift work has been compiled — this dev environment has Xcode Command Line Tools only, not full Xcode, so `xcodebuild` cannot run here; Swift changes are careful hand-verification against generated bindings, not compiler-checked.

## Project name & identity

The app is named **GIST**. All crates use the `gist-*` prefix. The older name "Readrrr" appears only in `docs/development-plan-v1.md` — ignore it; v2 supersedes it.

## Repository layout

```
gist/
├── Cargo.toml                  # workspace root
├── rust-toolchain.toml         # pinned to 1.87.0
├── deny.toml                   # cargo-deny: licences, advisories, bans, sources
├── LICENSE                     # MIT — every crate declares license = "MIT"
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
├── fixtures/                   # 23 synthetic public-domain test fixtures
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
- **Supply chain:** all GitHub Actions pinned to immutable commit SHAs. `cargo-deny` blocks on RUSTSEC advisories. License allow-list: MIT/Apache-2.0/BSD/ISC/Zlib/Unicode-3.0 plus MPL-2.0 (file-level copyleft only, allowed because it's required transitively by uniffi per ADR-001 and by scraper per ADR-005 — not optional). `cargo deny check bans licenses sources` is verified green locally; `cargo deny check advisories` currently cannot run in this dev environment — the live RustSec DB has a CVSS 4.0 entry that cargo-deny 0.18.3's bundled parser can't read, and the fix (cargo-deny 0.20+) needs `rustc 1.88+`, i.e. a `rust-toolchain.toml` bump (do that only via PR, per its own comment).
- **Document IDs:** UUIDv7 (not timestamps).
- **Schema migrations:** transactional; version ceiling check (`SchemaTooNew`).
- **Source paths:** logged at `debug!` level only, never `info!`.

## What's done (M0 + M1)

All Rust crates are implemented:
- `gist-model`, `gist-store` (FTS5 schema v2), `gist-parse-txt`, `gist-parse-epub`, `gist-parse-docx`, `gist-rsvp`, `gist-web`, `gist-imageprep` (skeleton), `gist-core`, `gist-ffi`
- `gist-parse-pdf` is a 1-line stub — pdfium build tooling deferred
- 23 fixture files in `fixtures/` with provenance in `fixtures/README.md`
- cargo-fuzz targets for txt/epub/docx/web_extract; nightly CI runs 120s each (the fuzz targets themselves had a latent bug — 3 of the 4 were missing the `stem` argument and didn't compile against their own crates' signatures — fixed 2026-09-10, unverified since `cargo-fuzz` isn't installed in this dev environment)
- Corpus test: `crates/gist-core/tests/corpus.rs`
- `gist-store::search_items` (FTS5 query) is implemented and tested at the storage layer but has **no caller** in `gist-core` or `gist-ffi` yet — search is unreachable from the app. Needs wiring before it counts as an M2 deliverable.

## What's next (M2)

SwiftUI macOS app — status as of 2026-09-10:
1. App skeleton + `CoreClient` wrapping FFI — **done** (`CoreClient` is a `@MainActor final class`, not literally a Swift `actor`, but serves the same single-touch-point role)
2. Library grid/list view (collections, tags, sort/filter, FTS search) — **not started.** `LibraryView.swift` is a flat, unsorted `List`; `SidebarView.swift` is an 11-line placeholder. No collections/tags data model exists anywhere (`gist-model`/`gist-store`). FTS search has no path to the UI (see `search_items` note above).
3. Import flows + OCR review screen + DRM error presentation — **partially done.** txt/epub/docx import is wired end-to-end (`GistCore.import_file` FFI export → `CoreClient.importFile` → `LibraryView` file picker) with a dedicated DRM alert (`ImportError::DrmProtected` / `GistError::DrmProtected`, structured across the FFI boundary, not string-matched). URL-paste import has no FFI export or UI despite `gist-web` being fully implemented. OCR review screen does not exist; `Core::import_image_with_ocr` is still `todo!("OCR import pipeline — Phase M3")` — calling it over FFI panics cleanly (caught by `ffi_catch!`, surfaces as `GistError::InternalPanic`) but that's indistinguishable from a real bug in the UI, so don't wire a button to it before M3.
4. Theme engine (light/dark/sepia/OLED, OS-follow) — **not started.**
5. Flow reading view (virtualised, typography controls, TOC, in-document search) — **not started**, file doesn't exist. Q8 (SwiftUI Text vs TextKit 2) can't be decided until this starts.

Not on the original M2 list but now functionally real: **RSVP playback** (`apps/apple/macOS/RsvpView.swift`) was a hardcoded placeholder through 2026-09-10; it's now wired to `gist_rsvp` via `CoreClient.startRsvp`/`saveProgress`, with a hand-ported client-side pacing loop (see `RsvpPlayer` — there's no per-tick FFI call, so `token_duration_ms` and its punctuation helpers are manually kept in sync with `crates/gist-rsvp/src/lib.rs`; the Rust engine is wall-clock-anchored via `token_at_elapsed`, the Swift port is a `Task.sleep`-per-token loop that doesn't re-sync to wall clock, so timing drift can accumulate over a long session — a known fidelity gap, not a bug). Full RSVP polish (ORP highlighting, precise `CVDisplayLink` timing, annotations) is still M3 scope per the milestone register.

Exit criterion: a team member can use it as their daily reader. Not met yet — no compiled build exists to try (see Current state above), and the library/theme/flow-view gaps above are real blockers even once it compiles.

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

# cargo-deny — advisories may not run in every environment, see note above
cargo deny check bans licenses sources
cargo deny check advisories   # best-effort; known to fail on some cargo-deny/rustc pairings

# Build xcframework (macOS + iOS slices) — needs full Xcode, not just Command
# Line Tools (`xcode-select -p` should point at an Xcode.app, not CommandLineTools)
./tools/build-core-xcframework.sh

# Regenerate Swift bindings
./tools/gen-bindings.sh

# Corpus test
cargo test --test corpus --package gist-core
```

XcodeGen generates the Xcode project — do NOT commit `apps/apple/*.xcodeproj`. Run `xcodegen generate` inside `apps/apple/` after cloning. `apps/apple/Generated/` (uniffi-generated Swift bindings) is also never committed — if `git status` ever shows it as untracked-but-not-ignored, check `.gitignore` for a pattern broken by a trailing inline comment (`.gitignore` only treats `#` as a comment at the start of a line; this exact bug shipped once, 2026-09-10).

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
| M1 | ✅ Done, CI-verified | Security hardening + parser breadth (epub/docx/web/imageprep) |
| M2 | 🔶 In progress | Library & Reading UI (SwiftUI macOS) — import + RSVP wired; library/theme/flow-view not started; nothing compiler-verified yet |
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
| F13 | Informational | Accepted | TOCTOU gap in `gist-core::import_txt`/`import_file`: `fs::metadata` size check happens before a separate `fs::read`, so a file swapped between the two calls could bypass the `max_bytes` gate. Not exploitable under the current threat model (local single-user app, user picks the file via the OS panel); revisit if a multi-user or network-triggered import path is ever added. |

All other findings from Security Review v1 are closed.
