# GIST — Claude Code Context

GIST is a cross-platform RSVP-style reading app. The Rust core handles all parsing, persistence, and pacing logic; native UI shells (SwiftUI on macOS/iOS, WinUI 3 on Windows) call into it via FFI.

**Current state:** M1 complete and CI-gate-verified (`cargo test`/`clippy -D warnings`/`fmt --check`/`cargo deny check bans licenses sources` re-run and still genuinely green as of 2026-09-12, after the collections/search/URL-import/removal/copy-on-import work below landed — an earlier "M1 ✅" claim in this file predated the original verification and was false when written; see the M1 audit below). M2 (Library & Reading UI) is in progress: multi-format import (txt/epub/docx) and a functional RSVP reading view are wired end-to-end; the Rust-side backends for FTS search, URL-paste import, and item removal (single/bulk, now backed by real ADR-006 copy-on-import so removal never touches a user's original file) are also now wired through `gist-core`/`gist-ffi`, but none of it has a Swift call site yet. Library chrome (collections/tags/search/removal UI), theming, and OCR review are not started. None of the Swift work has been compiled — this dev environment has Xcode Command Line Tools only, not full Xcode, so `xcodebuild` cannot run here; Swift changes are careful hand-verification against generated bindings, not compiler-checked.

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
- **Copy-on-import (ADR-006):** implemented 2026-09-12. `gist-store::store_original_copy` writes a SHA-256-content-addressed copy of file-based imports to `<storage_dir>/originals/`; `Metadata.source_copy_ref` points at it. `remove_items(delete_source_files: true)` deletes that copy — never `source_ref`'s raw path (the user's real file, wherever it lives). URL imports have no copy (nothing local to copy); `source_copy_ref` stays `None` for them.

## What's done (M0 + M1)

All Rust crates are implemented:
- `gist-model`, `gist-store` (schema v3: FTS5 token index + collections/tags CRUD, no `gist-core`/`gist-ffi` wiring yet), `gist-parse-txt`, `gist-parse-epub`, `gist-parse-docx`, `gist-rsvp`, `gist-web`, `gist-imageprep` (skeleton), `gist-core`, `gist-ffi`
- `gist-parse-pdf` is a 1-line stub — pdfium build tooling deferred
- 23 fixture files in `fixtures/` with provenance in `fixtures/README.md`
- cargo-fuzz targets for txt/epub/docx/web_extract; nightly CI runs 120s each (the fuzz targets themselves had a latent bug — 3 of the 4 were missing the `stem` argument and didn't compile against their own crates' signatures — fixed 2026-09-10, unverified since `cargo-fuzz` isn't installed in this dev environment)
- Corpus test: `crates/gist-core/tests/corpus.rs`
- `gist-store::search_items` (FTS5 query) is now wired through `Core::search_items` (`gist-core`) and `GistCore::search_items` (`gist-ffi`, returns `Vec<FfiLibraryItem>`) as of 2026-09-12, with a `gist-core` test covering the full import→search round trip. Rust-side plumbing is done; there is still no Swift call site or search field in `LibraryView` — that's the remaining M2 gap (see below).
- `gist-store::remove_items(ids)` (2026-09-12) does a single-transaction, multi-id delete — an id with no matching row is silently skipped, not an error — relying on existing `ON DELETE CASCADE`/`tokens_ad` trigger for `reading_progress`/`tokens`/FTS cleanup, and returns each removed row's paths without touching the filesystem itself. `delete_item(id)` is now just `remove_items(&[id])`, replacing an older two-code-path version that deleted files before the DB row (an ordering bug: a failed DB delete could orphan a record pointing at already-deleted files). `Core::remove_items(ids, delete_source_files)` calls the store first, then best-effort deletes the `.json`/`.tokens.json` blobs and (optionally) the sandboxed source copy only after the DB commit succeeds, logging failures at `debug!`. `GistCore::remove_items` exposes this over FFI. Tested for mixed valid/invalid ids, bulk removal, and both `delete_source_files` states. Same gap as search: no Swift call site yet.
- **ADR-006 (copy-on-import) implemented 2026-09-12** — same-day fix for a gap this file's architecture review found (see `A5` in the security register). `gist-store::store_original_copy(bytes, ext)` writes a SHA-256 content-addressed copy of file-based imports to `<storage_dir>/originals/`, deduping identical re-imports. `import_txt`/`import_file` call it after a successful parse and stamp the result onto the new `Metadata.source_copy_ref` (schema v4 column `source_copy_path`) alongside the pre-existing, informational-only `source_ref`. `remove_items(delete_source_files: true)` now deletes `source_copy_path`, never `source_path` — the bug where removal could delete the user's real file at its real location is fixed. `import_url` has no copy step (nothing local to copy); `source_copy_ref` stays `None` for URL imports, which is correct, not a gap.

## What's next (M2)

SwiftUI macOS app — status as of 2026-09-12:
1. App skeleton + `CoreClient` wrapping FFI — **done** (`CoreClient` is a `@MainActor final class`, not literally a Swift `actor`, but serves the same single-touch-point role)
2. Library grid/list view (collections, tags, sort/filter, FTS search, removal) — **not started.** `LibraryView.swift` is a flat, unsorted `List`; `SidebarView.swift` is an 11-line placeholder. The collections/tags data model now exists at the `gist-store` schema/CRUD layer only (schema v3, 2026-09-12: `collections`/`item_collections`/`tags`/`item_tags` tables, all join rows cascade-deleted from `library_items`; `Store` methods `create_collection`/`list_collections`/`add_item_to_collection`/`remove_item_from_collection`/`list_items_in_collection`/`add_tag`/`remove_tag`/`list_tags_for_item`, tested including the cascade case) — there is still no `gist-core`/`gist-ffi` wiring and no UI, both deliberately left as follow-up work. FTS search and item removal both now have a full Rust-side path (`GistCore::search_items` and `GistCore::remove_items` in `gist-ffi`, see the two notes above), but `apps/apple/Generated/gist_ffi.swift` hasn't been regenerated since either landed (still dated 2026-09-10, no `search_items`/`searchItems`/`remove_items`/`removeItems` symbols in it — `./tools/gen-bindings.sh` needs a run before Swift can even see the new exports), and there's no UI entry point regardless — `LibraryView` has no search field, no multi-select/removal affordance, and `CoreClient` has no wrapper method for either.
3. Import flows + OCR review screen + DRM error presentation — **partially done.** txt/epub/docx import is wired end-to-end (`GistCore.import_file` FFI export → `CoreClient.importFile` → `LibraryView` file picker) with a dedicated DRM alert (`ImportError::DrmProtected` / `GistError::DrmProtected`, structured across the FFI boundary, not string-matched). URL-paste import now has an FFI export (`Core::import_url` in `gist-core`, `GistCore::import_url` in `gist-ffi`, wired 2026-09-12, covered by a `gist-core` test that exercises the non-HTTPS rejection path without a live network call) but still no UI — `apps/apple/Generated/gist_ffi.swift` hasn't been regenerated since, and `CoreClient`/`LibraryView` have no call site for it. OCR review screen does not exist; `Core::import_image_with_ocr` is still `todo!("OCR import pipeline — Phase M3")` — calling it over FFI panics cleanly (caught by `ffi_catch!`, surfaces as `GistError::InternalPanic`) but that's indistinguishable from a real bug in the UI, so don't wire a button to it before M3.
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
| A5 | Architecture | ✅ Closed same day | Found and fixed 2026-09-12. ADR-006 (copy-on-import) was unimplemented, meaning `Core::remove_items(delete_source_files: true)` deleted the user's real file at its real location. Fixed: `gist-store::store_original_copy` (SHA-256 content-addressed copy under `<storage_dir>/originals/`) + `Metadata.source_copy_ref`/schema v4 `source_copy_path`, wired through `import_txt`/`import_file`/`remove_items`. Known low-severity residual: content-hash dedup means two items from identical bytes share one copy file, so deleting one item's copy can remove a file another surviving item's row still references (harmless today — nothing reads content from that path — but no reference counting yet). See ADR-006. |
| F13 | Informational | Accepted | TOCTOU gap in `gist-core::import_txt`/`import_file`: `fs::metadata` size check happens before a separate `fs::read`, so a file swapped between the two calls could bypass the `max_bytes` gate. Not exploitable under the current threat model (local single-user app, user picks the file via the OS panel); revisit if a multi-user or network-triggered import path is ever added. |

All other findings from Security Review v1 are closed.
