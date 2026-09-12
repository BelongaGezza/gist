# GIST — Development Plan v2

**Scope:** empty repo → notarised public v1.0 on macOS, with the Rust core built so iOS is a shell-only follow-on and Windows is a C-ABI follow-on.

**Supersedes:** development-plan-v1.md  
**Changes from v1:** Security review v1 (2026-09-08) findings fully integrated. All finding references are tagged `[Fx]` or `[Ax]` for traceability. Crate names updated to `gist-*` throughout (matching the live codebase). Milestone plan restructured to make security hardening explicit work, not assumed.

**Last updated:** 2026-09-12 — M2 milestone section rewritten from a generic feature list to an accurate progress snapshot (import + RSVP wired; search/URL-import/removal/library/theme/flow-view not yet wired or not started); verified against the current codebase, not just prior notes. Same-day architect review re-ran `cargo test`/`clippy -D warnings`/`fmt --check`/`cargo deny check bans licenses sources` against the latest commit (all still green), found one new architecture-vs-code gap — ADR-006 (copy-on-import) was unimplemented, recorded as `[A5]` — and then implemented it the same day: `gist-store::store_original_copy`, `Metadata.source_copy_ref`, schema v3→v4, `remove_items` now deletes the sandboxed copy instead of the user's real file. `[A5]`/`R12` closed; see §2.2/§2.10 and ADR-006. Also corrected a stale fixture count (24 → 23, matching `fixtures/`).

**Two deviations from spec v3 noted:**
1. **Paginated view** deferred to v1.1 (spec §11 position maintained). Flow view built on a layout abstraction from day one so paginated is a second implementation, not a rewrite.
2. **Full-text search** committed to v1.0 (spec §10.4 leaves open). Retrofitting FTS5 over an existing library requires schema migration + full re-index — worse to defer than to design in at M1.

---

## 0. Security Baseline

Security is a first-class architectural concern. This section defines the standing commitments the codebase must meet at every milestone. It does not replace the per-finding actions in later sections; it is the policy those actions implement.

### 0.1 FFI Safety Policy
- Every function exported via `#[uniffi::export]` **must** wrap its body in `std::panic::catch_unwind`. No exception. Panics must map to a `GistError::InternalPanic` variant — never propagate across the C ABI.
- The FFI crate's `[profile.release]` in `Cargo.toml` **must** set `panic = "abort"` as a belt-and-braces measure.
- Rationale: a panic crossing a C ABI boundary is undefined behaviour on all targets. On Apple Silicon it typically produces SIGABRT, but there is no guarantee in release builds, LTO builds, or future iOS targets. `[F1]` ✅ Closed

### 0.2 Mutex Discipline
- All `Mutex` lock acquisitions on the `Store` connection **must** use poison-tolerant recovery (`unwrap_or_else(|p| p.into_inner())` or an explicit `map_err` path). No bare `.lock().unwrap()` is permitted in `gist-store`. `[F2]` ✅ Closed
- This policy becomes self-consistent once `[F1]` is resolved (panics are caught at the FFI boundary before they poison anything), but both fixes are required independently.

### 0.3 Supply Chain Policy
- All third-party GitHub Actions **must** be pinned to an immutable commit SHA, not a mutable version tag. `[F3]` ✅ Closed
- `rust-toolchain.toml` **must** specify an exact stable version (`channel = "1.8x.y"`), not `"stable"`. Updates are made via an explicit PR so the change is visible in git history. `[F9]` ✅ Closed (pinned to 1.87.0)
- `deny.toml` **must** include `[advisories]`, `[bans]`, and `[sources]` sections in addition to `[licenses]`. `cargo-deny` must block on published RUSTSEC advisories. `[F5]` ✅ Closed

### 0.4 Parser Resource Limit Policy
- Before any parser moves from stub to real implementation, it **must** accept a `ParseLimits` struct and enforce all limits before allocation. `[F4]` ✅ Closed
- Limits for all parsers (enforced in `gist-core`):

```rust
pub struct ParseLimits {
    pub max_bytes: usize,          // 256 MB default
    pub max_pages: usize,          // 2 000 default
    pub max_nesting_depth: usize,  // for DOCX/ePub XML
    pub max_expanded_bytes: usize, // zip decompression limit (zip-bomb guard)
}
```

- For zip-based formats (ePub, DOCX), the expanded-bytes limit is enforced during **streaming decompression**, not after reading the full output. Return `ParseError::ResourceLimitExceeded` before allocation.

### 0.5 DRM Policy
- The ePub parser **must** detect DRM before attempting to parse content. Presence of `META-INF/encryption.xml` with non-obfuscation encryption methods returns `ParseError::DrmProtected`. `[F8]` ✅ Closed
- No decryption is attempted under any circumstances. IDPF font-obfuscation entries must not false-positive as DRM.
- ADR-004 written and implemented.

### 0.6 Identifier Policy
- All document IDs **must** use UUIDv7 (via the `uuid` crate already declared in `Cargo.toml`). Timestamp-derived hex IDs are prohibited. `[F6]` ✅ Closed

### 0.7 Schema Migration Policy
- Every schema migration **must** be wrapped in a transaction so a partial migration cannot leave the database in an intermediate state. `[F11]` ✅ Closed
- A version ceiling check **must** be present: if `user_version > SCHEMA_VERSION`, return `StoreError::SchemaTooNew` rather than silently operating on a future-schema database.

### 0.8 Privacy Policy for Stored Paths
- `source_ref` stores full filesystem paths. Any future diagnostic, telemetry, or sync feature **must** strip or hash these before transmission. This is a documented architectural constraint, not a fix required now. `[F12]`
- Log lines that emit source paths should use `debug!` level, not `info!`, so they do not appear in default logging configurations.

---

## 1. Repository & Project Structure

### 1.1 Mono-repo layout

```
gist/
├── Cargo.toml                  # workspace root
├── rust-toolchain.toml         # pinned stable, exact version [F9] ✅
├── deny.toml                   # cargo-deny: licence, advisories, bans, sources [F5] ✅
├── crates/
│   ├── gist-model/             # document model, IR types, serde, errors ✅
│   ├── gist-parse-txt/         # ✅
│   ├── gist-parse-epub/        # ✅ DRM detection + ParseLimits
│   ├── gist-parse-docx/        # ✅ style-resolution + tracked-changes
│   ├── gist-parse-pdf/         # ⏳ stub — pdfium build tooling deferred
│   ├── gist-imageprep/         # ✅ greyscale + resize + PNG re-encode
│   ├── gist-web/               # ✅ ureq/rustls + robots.txt + readability
│   ├── gist-rsvp/              # ✅ pacing engine, pure, no I/O
│   ├── gist-store/             # ✅ SQLite + FTS5 migration v1→v2
│   ├── gist-core/              # ✅ import pipeline + ImportObserver + OcrEngine
│   └── gist-ffi/               # ✅ uniffi scaffolding + OcrEngine callback interface
├── apps/
│   ├── apple/
│   │   ├── project.yml         # XcodeGen — do NOT commit .pbxproj ✅
│   │   ├── Shared/             # SwiftUI views, view models, theme engine
│   │   ├── macOS/              # AppKit bridges, menus, window mgmt, DMG plist
│   │   ├── iOS/                # share ext, camera, BackgroundTasks
│   │   ├── Generated/          # uniffi Swift bindings — gitignored, built
│   │   └── Tests/
│   └── windows/                # placeholder until Milestone 6+
├── assets/                     # fonts (verify OFL), icons, default cover templates
├── fixtures/                   # ✅ 23 synthetic public-domain fixtures: txt/epub/docx/web
│   ├── README.md               # provenance + expected behaviour per fixture
│   ├── txt/                    # 4 normal + 2 adversarial (null bytes, max lines)
│   ├── epub/                   # 3 normal + 4 adversarial (DRM, IDPF, empty spine, missing file)
│   ├── docx/                   # 3 normal + 2 adversarial (no document.xml, deep nesting)
│   └── web/                    # 3 normal + 2 adversarial HTML (for future net-capable tests)
├── fuzz/                       # ✅ cargo-fuzz workspace (NOT in main workspace members)
│   ├── Cargo.toml
│   ├── README.md
│   ├── corpus/                 # seed corpus from valid fixtures
│   └── fuzz_targets/           # fuzz_parse_txt, fuzz_parse_epub, fuzz_parse_docx, fuzz_web_extract
├── tools/                      # build-core-xcframework.sh, notarize.sh, gen-bindings.sh
├── docs/
│   ├── ARCHITECTURE.md
│   ├── BUILDING-macos.md
│   ├── FFI.md
│   ├── PRIVACY.md              # documents source_ref path storage [F12] ⏳ M4
│   └── adr/                    # Architecture Decision Records ✅ ADRs 001–010
│       ├── 001-ffi-uniffi.md
│       ├── 002-pdf-backend-pdfium.md
│       ├── 003-annotation-anchoring.md
│       ├── 004-drm-detection.md
│       ├── 005-web-fetch-policy.md
│       ├── 006-copy-on-import.md
│       ├── 007-ir-storage-format.md
│       ├── 008-fts5-schema.md
│       ├── 009-ocr-callback-interface.md
│       └── 010-http-ureq-rustls.md
└── .github/workflows/
    ├── core-test.yml           # ✅ SHA-pinned
    ├── core-quality.yml        # ✅ SHA-pinned, cargo-deny
    ├── parser-corpus.yml       # ✅ nightly + on parse-crate push; ubuntu+macos matrix
    ├── fuzz.yml                # ✅ nightly; 120s per target; crash artifact upload
    └── release-macos.yml       # ✅ guard step (exits non-zero until implemented)
```

**Crate boundary rule:** `gist-model` has no I/O dependencies. It must compile to `wasm32-unknown-unknown` — cheapest possible insurance on the future web-client option.

### 1.2 Workspace configuration

- `[workspace.dependencies]` for every shared dep — single-point version bumps, no diamond-version drift.
- Pinned toolchain via `rust-toolchain.toml` — exact stable version `1.87.0`, updated via PR. `[F9]` ✅
- `gist-ffi` builds `crate-type = ["staticlib", "cdylib"]`. macOS/iOS link the **staticlib** into an `.xcframework`.
- **Xcode project is generated, not committed.** `project.yml` + XcodeGen.
- `apps/apple/Generated/` is gitignored; bindings are generated as an Xcode build phase.
- Binding generation failures are **not** silenced. The pre-build script uses a graceful fallback: fail only when no generated files already exist. `[F7]` ✅
- `fuzz/` is a separate cargo workspace (not in the root `members` list) as required by cargo-fuzz.

### 1.3 CI skeleton

| Pipeline | Trigger | Does | Status |
|---|---|---|---|
| `core-test` | every PR | `cargo test --workspace` on ubuntu + macos; `clippy -D warnings`; `cargo fmt --check` | ✅ |
| `core-quality` | PR + nightly | `cargo-deny` (licence + advisories + bans + sources) `[F5]`, `cargo-audit`, `cargo-udeps` | ✅ |
| `parser-corpus` | nightly + on `crates/gist-parse-*` / `fixtures/` push | `cargo test --test corpus --package gist-core`; uploads `target/corpus-results/` as artifact; ubuntu + macos matrix | ✅ commit `a5aaf03` |
| `fuzz` | nightly | `cargo-fuzz` targets (txt, epub, docx, web_extract), 120s each, nightly toolchain, uploads crash artifacts on failure | ✅ commit `a5aaf03` |
| `apple-build` | PR touching `apps/apple/**` or FFI | XcodeGen → `xcodebuild` macOS Debug + unit tests | ⏳ M2 |
| `release-macos` | tag `v*` | **Fails loudly if pipeline not fully implemented** `[F10]`; when implemented: universal xcframework, sign, notarise, staple, produce DMG | ⏳ M4 |
| `docs-check` | PR | link-check `docs/`, verify BUILDING-*.md | ⏳ M4 |

**Action pinning:** all third-party GitHub Actions are pinned to immutable commit SHAs. `[F3]` ✅
```yaml
- uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683  # v4.2.2
- uses: dtolnay/rust-toolchain@4305c38b25d97ef35a8ad1f985ccf33d0b9d0787  # stable 2024-10
- uses: EmbarkStudios/cargo-deny-action@34899fc28e28a96b4e8215ecdfdee2e2dbde5706  # v1
```

**Note:** `release-macos` is tag-triggered on the main repo only — fork PRs must not have access to signing secrets.

### 1.4 Fixtures ✅ Complete

23 synthetically generated public-domain fixtures in `fixtures/`. Provenance recorded in `fixtures/README.md`. `tools/gen-fixtures` produces adversarial cases on demand (zip-bomb candidates, etc.).

Fuzz seed corpus populated from valid fixture files: `fuzz/corpus/fuzz_parse_epub/` (3 files), `fuzz/corpus/fuzz_parse_docx/` (2 files), `fuzz/corpus/fuzz_parse_txt/` (2 files).

---

## 2. Rust Core — Phased Build Plan

**Complexity:** S ≤ 3 days · M 1–2 weeks · L 3–5 weeks · XL 6+ weeks (one engineer)

### Build order (dependency-sequenced)

```
Phase A:  model ──┬──> store ──┐
                  │            │
Phase B:          ├──> txt     ├──> core (facade) ──> ffi ──> [Swift shell]
                  ├──> epub    │
                  ├──> docx    │
                  ├──> rsvp ───┘
Phase C:          ├──> web
                  ├──> pdf
                  └──> imageprep
```

**Thinnest vertical slice that proves the architecture:** `model` + `txt` + `rsvp` + minimal `store` + minimal `ffi` → SwiftUI window displaying RSVP over a `.txt` file. Build this before writing a second parser. ✅ Done

---

### 2.1 `gist-model` — Document model · **M** ✅ Complete

**Does:** owns the §7 semantic schema: `Document`, `Section`, `Block` (Paragraph/Image/List/Table), `TextRun` with inline emphasis, `Metadata`, `OcrConfidence`. Plus: stable block IDs, a flat **token stream** for RSVP/TTS/search, and character-offset anchors for annotations.

**Crates:** `serde` + `serde_json`, `thiserror`, `uuid` (v7, v4), `unicode-segmentation`

**Document IDs:** `Uuid::now_v7().to_string()` — UUIDv7 implemented. `[F6]` ✅

**Key decision — annotation anchoring:** store `(block_id, start, len, prefix_hash, quote_hash)` and re-anchor by search when hashes mismatch. Costs ~2 days now; prevents a data-loss bug class later. *Not deferrable.* ADR-003 ✅

**Key decision — token stream:** compute once at import and persist. Shared substrate for RSVP, TTS, FTS indexing, and reading-time estimation.

---

### 2.2 `gist-store` — SQLite persistence · **L** ✅ Complete (M1 scope)

**Does:** schema + migrations, library CRUD, progress, FTS5 index, source-file management. Collections/tags/smart views deferred to M2.

**Crates:** `rusqlite` (features: `bundled`, `fts5`, `backup`, `serde_json`), hand-rolled `PRAGMA user_version` migrations, `directories`

**Implemented:**
- WAL mode; one writer behind a `Mutex`; poison-tolerant recovery on all `.lock()` calls. `[F2]` ✅
- `SCHEMA_VERSION = 4`. Migration v1→v2: `tokens` shadow table + `fts_index` FTS5 virtual table (`porter unicode61`) + sync triggers. `[A1]` ✅. Migration v2→v3 (2026-09-12): `collections`/`item_collections`/`tags`/`item_tags` tables, all join rows `ON DELETE CASCADE` from `library_items`, with `create_collection`/`list_collections`/`add_item_to_collection`/`remove_item_from_collection`/`list_items_in_collection`/`add_tag`/`remove_tag`/`list_tags_for_item` CRUD — schema/store layer only, no `gist-core`/`gist-ffi` wiring yet (see M2 milestone section). Migration v3→v4 (2026-09-12): adds `library_items.source_copy_path`, the sandboxed-copy path ADR-006 requires (see `store_original_copy` below and `[A5]`).
- Version ceiling check: returns `StoreError::SchemaTooNew` if `user_version > SCHEMA_VERSION`. `[F11]` ✅
- `insert_item`: writes `.tokens.json` + indexes Word tokens in same transaction; now also persists `Metadata.source_copy_ref` into the new `source_copy_path` column.
- `store_original_copy(bytes: &[u8], ext: &str) -> Result<String, StoreError>`: **implemented 2026-09-12, closes `[A5]`.** Writes `bytes` to `<storage_dir>/originals/<sha256-hex>.<ext>`, skipping the write if a file with that hash already exists (content-addressed dedup — re-importing identical bytes reuses the copy). Returns the copy's path for the caller to stamp onto `Metadata.source_copy_ref` before `insert_item`. `gist-core::import_txt`/`import_file` call this after a successful parse (never for input GIST rejects — unsupported type, DRM, resource limits) and before persisting. `import_url` does not call this — there's no local file for a URL import to copy (see ADR-006's scope note). **Known limitation:** two items imported from identical bytes share one file on disk; removing one with `delete_source_files: true` deletes it out from under the other's (unused-for-reads) reference — no reference counting yet, tracked in ADR-006.
- `remove_items(ids: &[String]) -> Result<Vec<RemovedItem>, StoreError>`: **implemented 2026-09-12.** Multi-id removal in a single `unchecked_transaction`; all-or-nothing on the DB side (an error partway through rolls back, nothing is removed). An id with no matching row is silently skipped rather than erroring — bulk-remove is idempotent against ids already gone. Returns a `RemovedItem { id, doc_path, source_path, source_copy_path }` for each row actually removed, **but never touches the filesystem itself** — callers (`gist-core`) delete the `.json`/`.tokens.json` blobs (and, optionally, the sandboxed source copy) only *after* this call returns `Ok`, which fixes a file-before-DB ordering bug an earlier version of this doc had flagged in the old single-item `delete_item`: an interruption can now only leave an orphaned file, never an orphaned DB row. `reading_progress`/`tokens`/`fts_index` cleanup still rides on the existing `ON DELETE CASCADE` + `tokens_ad` trigger — `remove_items` doesn't hand-roll deletes for those tables. Covered by `gist-store` tests for the mixed valid/invalid-id case, single-item delegation, and the `source_copy_path` round trip through insert→remove.
- `delete_item(id)`: now a thin wrapper — `remove_items(&[id.to_string()])` — so there is one deletion code path instead of two that could drift apart.
- There are still no `annotations`/`bookmarks` tables (M3 features, not built), so there's nothing there to cascade regardless.

**Architecture decision `[A1]`:** ✅ Resolved — external-content FTS5 table with `tokens` shadow table updated on insert/delete via triggers. Documented in ADR-008.

---

### 2.3 `gist-parse-txt` — Plain text · **S** ✅ Complete

**Crates:** `encoding_rs`, `chardetng`

`ParseLimits.max_bytes` enforced. `[F4]` ✅

---

### 2.4 `gist-parse-epub` — ePub · **M** ✅ Complete

**Crates:** `zip`, `quick-xml`, `roxmltree`, `url`, `percent-encoding`, `gist-core`, `gist-model`

**Implemented:**
- `check_drm()`: parses `META-INF/encryption.xml`; exempts IDPF font obfuscation (`http://www.idpf.org/2008/embedding`); returns `ParseError::DrmProtected` for any commercial algorithm. `[F8]` ✅ ADR-004 ✅
- All four `ParseLimits` fields enforced (max_bytes, max_pages, max_nesting_depth, max_expanded_bytes during streaming decompression). `[F4]` ✅
- OPF/container.xml parsing via roxmltree; spine iteration; XHTML→block mapping.
- XHTML: h1–h6→Heading, p/div→Paragraph, ul/ol/li→List, em/strong/i/b/code→TextRun marks.

---

### 2.5 `gist-parse-docx` — DOCX · **M–L** ✅ Complete

**Crates:** `zip`, `quick-xml`, `gist-model`, `gist-core`

**Implemented:**
- `parse_styles()`: walks `styles.xml`; resolves `w:basedOn` chains (depth-limited to 20) to detect `Heading N` styles.
- `parse_numbering()`: detects ordered/unordered from `w:numFmt`.
- `parse_document()`: `pStyle` → style lookup; `numPr` → list detection; `w:ins` accepted, `w:del` skipped; `has_tracked_changes` flag set.
- All four `ParseLimits` fields enforced. `[F4]` ✅
- Tables: parse and persist (M1); flatten at render (M2). `[Q2]` ✅ Resolved

---

### 2.6 `gist-parse-pdf` — PDF · **XL** ⚠️ Deferred

**Crates:** `pdfium-render` — deferred due to `libpdfium` per-arch binary build tooling requirement.

**Status:** stub only (1 line). Parser is kept behind a swappable trait so the backend decision can change without an API break.

**Pre-implementation requirements:** `ParseLimits` struct defined ✅. ADR-002 written ✅. Build tooling for `libpdfium` per arch (arm64, x86_64) still needed.

---

### 2.7 `gist-imageprep` — OCR pre/post-processing · **M** ✅ Skeleton complete

**Does:** pre-processing (greyscale, resize-to-2048, PNG re-encode); post-processing of native OCR results deferred to M3.

**Crates:** `image` (png + jpeg features), `gist-core`, `gist-model`

**Implemented:**
- `prepare_image(page_index, raw_bytes, limits)`: decode PNG/JPEG → luma8 → resize if >2048px (Lanczos3) → re-encode PNG.
- Pixel-count guard via `limits.max_expanded_bytes / 4`.
- `OcrEngine` trait defined in `gist-core` (not `gist-ffi`) to avoid circular deps. `[A2]` ✅ ADR-009 ✅

**Remaining M3 work:** `imageproc` (Hough deskew, Otsu threshold), `rayon` parallel multi-page, post-processing of OCR results into ordered blocks with `ocrConfidence[]`.

---

### 2.8 `gist-web` — URL fetch + extraction · **M** ✅ Complete

**Crates:** `ureq` (rustls feature, no default features), `scraper`, `url`, `gist-core`, `gist-model`, `thiserror`

**Implemented per ADR-005 and ADR-010:** `[F13]` ✅
- HTTPS-only; `ParseError::InvalidInput` on HTTP.
- `robots.txt` pre-fetch with `GIST/1.0` UA; prefix-match check for `*` and `GIST` agent blocks; `ParseError::RobotsDisallowed` if disallowed.
- ureq agent: rustls backend, max 5 redirects, 30s connect / 60s read, no cookies.
- Body streaming capped at `min(limits.max_bytes, 50 MB)`.
- Readability: `<article>` → `<main>` → `<body>` fallback; strips script/style/nav/header/footer/noscript/aside/iframe subtrees.
- Uses `gist_core::ParseLimits` (not a local duplicate). `[Q12]` ✅ Resolved

---

### 2.9 `gist-rsvp` — RSVP pacing engine · **M** ✅ Complete

**Does:** token stream → timed presentation schedule. WPM 100–1000 (default 250), punctuation-aware pauses, word-group chunking, play/pause/seek state machine, ORP index per word, session stats.

**Crates:** none beyond `gist-model`. Pure `std`. No I/O. No `unsafe`.

**Design rule:** core does not own a timer. Exposes a pure `(state, elapsed) -> current token` function. SwiftUI shell drives from `CVDisplayLink`.

---

### 2.10 `gist-core` — Facade / orchestration · **L** ✅ M1 complete

**Does:** import pipeline (type sniff → dispatch → normalise → persist → index), library operations, reading-session management, structured error taxonomy.

**Crates:** `infer`, `thiserror`, `tracing`, `gist-parse-txt`, `gist-parse-epub`, `gist-parse-docx`, `gist-rsvp`, `gist-store`, `gist-model`

**Implemented:**
- `ParseLimits` (shared across all parsers). `[F4]` ✅
- `ParseError { InvalidInput(String), ResourceLimitExceeded }`.
- `OcrEngine: Send + Sync` trait (defined here, not in `gist-ffi`, to avoid circular dep). `[A2]` ✅
- `OcrPageResult { page_index, text, confidence }`.
- `ImportError { Io, Epub, Docx, Txt, ImagePrep, Web, Store, UnsupportedType, Cancelled }`.
- `ImportObserver { on_progress(bytes_read, total); is_cancelled() -> bool }` + `NullObserver`.
- `import_file(path, observer)`: magic-byte type sniff via `infer`, extension fallback; dispatches to correct parser; cancellation checked at two points; stamps `source_ref`; copies into sandboxed storage and stamps `source_copy_ref` (see `[A5]` below); inserts into store.
- `import_image_with_ocr`: stub (Phase M3 pipeline).
- **`[A5]` ADR-006 (copy-on-import) — implemented 2026-09-12, closing the gap found in that day's architecture review.** `import_txt`/`import_file` now call `store.store_original_copy(&bytes, &ext)` after a successful parse (never for input GIST rejects — unsupported type, DRM, resource limits) and stamp the returned path onto the new `Metadata.source_copy_ref` field, alongside the existing informational-only `source_ref`. `Core::remove_items(delete_source_files: true)` now deletes `item.source_copy_path` (the sandboxed copy) and never touches `item.source_path` (the user's real file at its real location) — the exact bug this finding described is fixed; see the updated `remove_items_with_delete_source_files_true_deletes_the_sandboxed_copy_not_the_original` test, which explicitly asserts the original survives. `import_url` deliberately does **not** create a copy — there's no local file for a URL import to copy, only fetched content — so `source_copy_ref` stays `None` for those items and `delete_source_files` has nothing to do for them. See ADR-006 (updated 2026-09-12) for the full design, a naming clarification (`Metadata.doc_path` in the original ADR text is now `Metadata.source_copy_ref`, to avoid colliding with `doc_path`'s existing meaning as the serialised-IR-blob path), and a known limitation around content-hash dedup vs. per-item deletion (harmless today, needs reference counting before anything depends on a shared copy surviving).
- `search_items(query, limit)`: ✅ wired 2026-09-12 — calls `store.search_items` for ranked ids, resolves each via a new `Store::get_item_by_id`, silently omits an id that no longer resolves (e.g. deleted between the FTS match and the lookup) rather than failing the whole search. Covered by a `gist-core` test that imports a fixture-style document and searches for a word it contains.
- `remove_items(ids: &[String], delete_source_files: bool) -> Result<(), CoreError>`: **implemented 2026-09-12, corrected same day per `[A5]`.** Calls `store.remove_items(ids)` first (transactional DB delete), then — only after that succeeds — best-effort deletes each removed item's `.json`/`.tokens.json` blobs, and its sandboxed source copy too if `delete_source_files` is true. File-deletion failures are logged at `debug!` (per the `source_ref`-logging policy) and don't fail the call, since the library metadata is already gone by that point. Unknown ids are silently ignored (delegates to `Store::remove_items`'s semantics). Covered by five `gist-core` tests: single removal alongside a surviving item, bulk removal of 2–3 items, and both `delete_source_files` true/false cases — the true case asserts both that the sandboxed copy is deleted *and* that the user's original file survives untouched.

**Logging discipline `[F12]`:** `source_ref` paths at `debug!` level only.

**Corpus test:** `crates/gist-core/tests/corpus.rs` — iterates all fixtures with `catch_unwind`; asserts zero panics; writes `target/corpus-results/summary.json`. ✅ commit `a5aaf03`

---

### 2.11 `gist-ffi` — Bindings layer · **L** ✅ M1 complete

**Does:** uniffi scaffolding over `gist-core` for Swift; C ABI shim for Windows (future).

**Crates:** `uniffi = "0.32"` (proc-macro mode, not UDL)

**Implemented:**
- `catch_unwind` + `ffi_catch!` macro on all `#[uniffi::export]` functions. `[F1]` ✅
- `[profile.release] panic = "abort"`. `[F1]` ✅
- `GistError { Core(String), InternalPanic(String) }`.
- `OcrPageResult` (uniffi Record) + `OcrEngine` (`#[uniffi::export(callback_interface)]`).
- `CoreOcrAdapter`: bridges FFI `OcrEngine` → `gist_core::OcrEngine` without circular dep.
- `GistCore` uniffi object with constructor `new(db_path, storage_dir)` plus exported methods: `health`, `import_txt`, `import_file`, `import_url(url)` (✅ added 2026-09-12), `list_items(offset, limit)`, `search_items(query, limit)` (✅ added 2026-09-12, returns `Vec<FfiLibraryItem>`), `start_rsvp(item_id, wpm)`, `save_progress(item_id, token_index)`, `remove_items(ids, delete_source_files)` (✅ added 2026-09-12, wraps `gist_core::Core::remove_items`, `ffi_catch!`-wrapped like every other export), `import_image_with_ocr(path, engine)`.
- `apps/apple/Generated/gist_ffi.swift` hasn't been regenerated since `search_items`, `import_url`, or `remove_items` were added — `./tools/gen-bindings.sh` needs a run (requires `uniffi-bindgen`, not installed in this dev environment) before Swift can see any of them.

---

## 3. macOS UI Shell — Phased Build Plan

### 3.1 App skeleton + core bridge · **M** ⏳ M2
`GistApp` scene, window management, `CoreClient` actor wrapping FFI (single touch-point), app-support directory setup, error-presentation surface.
**FFI:** `Core.init(dbPath:storageDir:)`, `Core.health()`, error enum bridging.

### 3.2 Library view (grid + list) · **L** ⏳ M2
`NavigationSplitView` with sidebar (collections, tags, smart views), `LazyVGrid` + `Table`.

**Sort:** sort control (key selector + direction toggle) always visible in the toolbar. Five sort keys: name (title), source type, date added, date last read, reading progress. Active key and direction persisted independently per view (grid/list) and survive restarts. Default: date added descending. Source-type sort groups items by format with secondary sort by date added descending.

**Filter and search:** filter bar, full-text search (FTS5 via `gist-core`), cover thumbnails, progress rings.

**Selection and removal:**
- Multi-select via ⌘-click and checkbox mode (list view); keyboard-accessible (`Space` to toggle, `⌘A` to select all).
- Single-item removal: context menu item "Remove from Library…" and toolbar button when one item is selected.
- Bulk removal: "Remove X Items…" toolbar button when multiple items are selected.
- Both paths show a confirmation sheet stating: the count and titles of items to be removed, and whether source files will be deleted (checkbox, default on, preference-backed).
- On confirmation: calls `CoreClient.removeItems(ids:deleteSourceFiles:)`, which calls `gist-core::remove_items` (✅ backend implemented 2026-09-12, not yet wired to this UI — see §2.10/§2.11) — a single SQLite transaction covering metadata + `reading_progress`/`tokens`/FTS5 entries (via cascade/triggers; there are no `annotations`/`bookmarks` tables yet, so nothing there to include). Source file deletion follows transaction commit, best-effort. On error, surfaces a specific error sheet; never partially removes.
- If the removed item is open in the reader, close the reader and return to the library before the confirmation sheet dismisses.
- Empty state shown when all items in a collection are removed.

### 3.3 Import flows · **L** ⏳ M2
`.fileImporter` + drag-and-drop, URL paste sheet with validation, image→OCR flow with multi-page review and low-confidence highlighting, per-import progress rows, specific error presentation.
DRM error (`ParseError::DrmProtected`) must surface a distinct, user-facing explanation — not a generic import failure. `[F8]`

### 3.4 Theme engine · **S–M** ⏳ M2 (build before the reader)
`Theme` model (light/dark/sepia/OLED), OS-follow, semantic colour tokens, contrast validation, persistence.

### 3.5 Reader — flow view · **XL** ⏳ M2
Virtualised rendering, full typography controls, TOC/section jump, in-document search, progress bar, scroll-position persistence, keyboard/trackpad navigation.

**Key architecture decision (see §7 Q8):** SwiftUI `Text` per block in a `LazyVStack` vs TextKit 2 via `NSViewRepresentable`. Recommendation: SwiftUI-native for v1.0 with `AttributedString`; escalate to TextKit 2 only if selection/highlighting proves inadequate. Build on a `ReadingLayout` protocol regardless.

### 3.6 RSVP view + speed dial · **L** ⏳ M3
Fixed-position word display with ORP highlight, play/pause, rotary speed dial, numeric WPM readout, accessible stepper alternative, scrub/seek, back-5-words, punctuation-pause toggle, exit-to-flow, session stats.
Drive from `CVDisplayLink`, not `Timer` — `Timer` jitter is visible at 600+ WPM.
**Accessibility is mandatory:** `accessibilityAdjustableAction` + visible numeric stepper. `[spec §5.5]`

### 3.7 Annotation UI · **L** ⏳ M3
Selection → highlight in N colours, margin notes, bookmarks, annotations sidebar, jump-to-annotation, export via `.fileExporter`.

### 3.8 Settings / preferences · **M** ⏳ M3
`Settings` scene with tabs: Reading, Typography, RSVP, Import, Storage, About.
Storage tab includes the "Delete source files on removal" default preference (matches the removal confirmation checkbox default).

### 3.9 TTS + accessibility pass · **M** ⏳ M3
`AVSpeechSynthesizer` read-aloud; full VoiceOver audit; Dynamic Type verification; contrast checks.

### 3.10 App Sandbox Entitlements Review `[A3]` ⏳ M3
**Required before M4 release pipeline.**
- Produce an explicit `.entitlements` file. `ENABLE_HARDENED_RUNTIME: YES` is set correctly but the entitlements file and its content are not visible in the current codebase.
- Minimum entitlements for the file importer: `com.apple.security.files.user-selected.read-only`.
- Additional entitlements (camera, network for URL fetch) require justification and must be reviewed against least-privilege.
- This review must be completed before the M4 notarisation pipeline is implemented.

### 3.11 Localisation scaffolding · **S** ⏳ M3
String catalogs (`.xcstrings`) from day one, no string literals in views. English (UK) only ships in v1.0.

---

## 4. iOS Extension Plan

The Rust core requires **zero** changes for iOS (same uniffi bindings, same `.xcframework` with added `ios`/`ios-simulator` slices). [Unchanged from v1 — see full iOS breakdown in prior plan if needed.]

---

## 5. Milestone Plan

Assumes 2–3 engineers with Rust + Swift competency.

### Pre-M1 Immediate Actions ✅ Complete

| Action | Finding | Status |
|---|---|---|
| Pin all GitHub Actions to commit SHA in all three workflow files | `[F3]` | ✅ commit 10b1c1e |
| Add guard step to `release-macos.yml` that exits non-zero until pipeline is implemented | `[F10]` | ✅ commit 10b1c1e |
| Pin `rust-toolchain.toml` to exact stable version `1.87.0` | `[F9]` | ✅ commit 10b1c1e |

### M0 — Foundations & vertical slice · **3 weeks** ✅ Complete

**Goal:** prove the architecture end-to-end before investing in breadth.

All M0 deliverables complete (commit `c5f074e`):
- Workspace + crate skeleton ✅
- `gist-model` v1 ✅
- `gist-parse-txt` ✅
- `gist-rsvp` core ✅
- Minimal `gist-store` (items + progress) ✅
- uniffi bindings + `.xcframework` build script ✅
- macOS app importing a `.txt`, listing it, running RSVP on it ✅
- `docs/ARCHITECTURE.md` + ADRs 001–003 ✅

**M0 security debt — all resolved in M1:**
- `catch_unwind` at FFI boundary `[F1]` ✅
- Mutex poison-tolerance in `gist-store` `[F2]` ✅
- UUIDv7 document IDs `[F6]` ✅
- `deny.toml` `[advisories]`, `[bans]`, `[sources]` `[F5]` ✅
- Xcode pre-build script graceful fallback `[F7]` ✅
- Schema migration version ceiling + transaction `[F11]` ✅

---

### M1 — Security Hardening + Import Breadth · **7 weeks** ✅ Complete

**Security hardening:** ✅ All complete (commit `10b1c1e`)

| Task | Finding | Status |
|---|---|---|
| `catch_unwind` + `panic = "abort"` on FFI | `[F1]` | ✅ |
| Mutex poison-tolerant recovery in `gist-store` | `[F2]` | ✅ |
| UUIDv7 in `gist-model` | `[F6]` | ✅ |
| `[advisories]`, `[bans]`, `[sources]` in `deny.toml` | `[F5]` | ✅ |
| Graceful fallback in Xcode pre-build script | `[F7]` | ✅ |
| Schema migration version ceiling + transaction | `[F11]` | ✅ |
| `ParseLimits` struct in `gist-core` | `[F4]` | ✅ |
| ADR `004-drm-detection.md` | `[F8]` | ✅ |
| ADR `005-web-fetch-policy.md` | `[F13]` | ✅ |
| `OcrEngine` callback interface + ADR-009 | `[A2]` | ✅ |
| FTS5 schema + migration + ADR-008 | `[A1]` | ✅ |

**Parser + feature work:**

| Deliverable | Status |
|---|---|
| ePub parser (DRM + ParseLimits) | ✅ commit 906957e |
| DOCX parser (style-resolution + tracked-changes) | ✅ commit aa76f89 |
| gist-store FTS5 migration v1→v2 | ✅ commit 2c0b4b5 |
| gist-web (ureq + rustls + robots.txt + readability) | ✅ commit 50b2c84 |
| gist-imageprep (greyscale + resize + PNG re-encode) | ✅ commit f6b3c46 |
| OcrEngine callback interface in gist-ffi | ✅ commit f6b3c46 |
| gist-core import pipeline + ImportObserver | ✅ commit 7cce58c |
| ADRs 004–009 | ✅ commit ff27aa3 |
| Fixture corpus (24 files) + `parser-corpus` CI | ✅ commit a5aaf03 |
| `cargo-fuzz` targets per parser + seed corpus + fuzz CI | ✅ commit a5aaf03 |
| ADR-010 (HTTP ureq+rustls) + stray fixture cleanup | ✅ commit 14a0cfd |
| PDF parser | ⏳ Deferred — pdfium build tooling |

**Exit criterion met:** corpus infrastructure in place; `catch_unwind` at all FFI exports; all M0 security debt items resolved. Fuzz runs nightly on all parser entry points.

---

### M2 — Library & Reading · **5 weeks** 🔶 In progress — status as of 2026-09-12

**Goal:** it becomes a usable reading app.

**Done:**
- App skeleton + `CoreClient` wrapping FFI ✅ (`@MainActor final class`, not literally a Swift `actor` — same single-touch-point role, close enough not to relitigate)
- Multi-format import wired end-to-end: `.fileImporter` → `CoreClient.importFile` → `GistCore.import_file` (txt/epub/docx), with a dedicated DRM alert (`ParseError::DrmProtected` surfaced as a structured error, not string-matched) ✅
- RSVP playback wired to `gist_rsvp` via `CoreClient.startRsvp`/`saveProgress` ✅ — not on the original M2 list, pulled forward because the vertical slice needed it. **Known fidelity gap carried into M3:** the Swift pacing loop (`RsvpPlayer` in `RsvpView.swift`) is a hand-ported, `Task.sleep`-per-token re-implementation of `token_duration_ms` — there's no per-tick FFI call — and unlike the Rust engine (wall-clock-anchored via `token_at_elapsed`) it doesn't re-sync to wall clock, so timing can drift over a long session. Track this as an explicit M3 RSVP-polish item, not a regression to fix now.

**Not started / blocking the exit criterion:**
- Library grid/list with sort (name / source type / date added / date last read / progress), collections/tags/smart views — `LibraryView.swift` is currently a flat unsorted `List` (99 lines), `SidebarView.swift` an 11-line placeholder. The schema/CRUD half of this is now done: `gist-store` (schema v3, 2026-09-12) has `collections`/`item_collections`/`tags`/`item_tags` tables (all join rows `ON DELETE CASCADE` from `library_items`) and `Store` methods `create_collection`/`list_collections`/`add_item_to_collection`/`remove_item_from_collection`/`list_items_in_collection`/`add_tag`/`remove_tag`/`list_tags_for_item`, covered by tests including a cascade-delete check. Deliberately out of scope for that change and still not started: `gist-core`/`gist-ffi` wiring (no FFI export exists yet) and all UI — smart-views/filter query logic also deferred to whatever composes on top of these primitives.
- **Full-text search has no path to the UI.** `gist-store::search_items` (FTS5 MATCH) is now wired end-to-end through `Core::search_items` and `GistCore::search_items` (✅ 2026-09-12, tested), but there is still no Swift call site: `CoreClient` has no `searchItems` wrapper and `LibraryView` has no search field. The remaining work here is UI-only.
- **Item removal backend is now done, including the `[A5]` fix; the UI still isn't.** `Store::remove_items`, `Core::remove_items(ids, delete_source_files)`, and `GistCore::remove_items` (FFI) all landed 2026-09-12 — transactional multi-id DB delete, then best-effort file cleanup after commit, with tests covering single/bulk removal and both `delete_source_files` states (see §2.2/§2.10/§2.11). Same-day, ADR-006 (copy-on-import) was implemented for real, closing `[A5]`: `delete_source_files: true` now deletes the sandboxed copy GIST made at import time, never the user's original file at its real location. Same shape as the search-wiring gap above: `apps/apple/Generated/gist_ffi.swift` needs a `./tools/gen-bindings.sh` run before Swift sees any of this, and there's no UI call site yet — `CoreClient` has no `removeItems` wrapper, and `LibraryView`/context-menu/confirmation-sheet from §3.2 don't exist. What remains here is UI-only; the removal confirmation dialogue (spec §4) can now truthfully describe what "delete source file" does.
- URL-paste import — `gist-web` is fully implemented (fetch, robots.txt, readability) and now has an FFI export: `Core::import_url` (`gist-core`) and `GistCore::import_url` (`gist-ffi`), ✅ 2026-09-12, tested via the non-HTTPS rejection path (no live network call needed since ADR-005's HTTPS-only check runs before any fetch). Still no UI entry point — `CoreClient` has no `importUrl` wrapper and no view calls it. Note: wiring this required repointing `gist-web`'s `ParseLimits` import from `gist-core` to `gist-model` directly, since `gist-web` already depended on `gist-core` (for that one re-exported type) and `gist-core` → `gist-web` would otherwise have been a dependency cycle.
- OCR review screen does not exist. `Core::import_image_with_ocr` is still `todo!("OCR import pipeline — Phase M3")`; do not wire a UI button to it before the real pipeline lands — a `todo!` panic is caught by `ffi_catch!` and surfaces as `GistError::InternalPanic`, indistinguishable from a real bug to anyone testing the UI.
- Theme engine (light/dark/sepia/OLED, OS-follow) — not started.
- Flow reading view (virtualised, typography controls, TOC, in-document search) — not started, file doesn't exist. Q8 (SwiftUI `Text` vs TextKit 2) can't be decided until this starts.

**Environment constraint carried through M2:** this dev environment has Xcode Command Line Tools only, not full Xcode — `xcodebuild` cannot run here. Every Swift change so far has been hand-verified against the generated bindings, not compiler-checked. Rust-side work (search wiring, URL import wiring, item-removal backend) should be prioritised precisely because it *can* be verified in this environment; Swift-heavy work (library UI, theme engine, flow view) carries higher risk of an uncaught compile error until it's built on a machine with full Xcode.

**Exit criterion:** a team member can use it as their daily reader, including searching their library and removing items they no longer want. Not met yet — no compiled build exists to try, and the library/search/removal/theme/flow-view gaps above are real blockers even once it compiles.

---

### M3 — RSVP, Annotations, Accessibility · **4 weeks**

**Goal:** the differentiating features and the accessibility bar.
- RSVP view + rotary dial + accessible alternative + scrub + session stats + exit-to-flow
- Annotations (highlights/notes/bookmarks) + sidebar + Markdown/text export
- Settings (including "Delete source files on removal" preference); TTS
- **Full VoiceOver + Dynamic Type audit passed** `[spec §5.5]`
- **App Sandbox entitlements review and explicit `.entitlements` file** `[A3]`
- Localisation scaffolding

**Exit criterion:** RSVP frame timing verified stable at 1000 WPM; VoiceOver navigable end to end; entitlements file reviewed and approved.

---

### M4 — Hardening & Release Engineering · **4 weeks**

**Goal:** shippable to strangers.
- Performance against §9 targets (20-page PDF < 5s; 1,000-item library responsive) with benchmarks in CI
- Crash/error-path sweep; fuzz-corpus review; confirm no parser can panic on adversarial input
- Storage management UI
- Notarised signed DMG pipeline with full signing + notarisation (not a stub) `[F10]`
- **At-rest document integrity review `[A4]`:** decide whether the threat model warrants BLAKE3 checksums on stored document blobs. Record decision in an ADR. If warranted, implement before public release.
- `CONTRIBUTING.md`, `BUILDING-macos.md`, `PRIVACY.md` `[F12]`, licence attribution screen, `cargo-deny` clean
- Public beta (~20–50 users) and triage

**Exit criterion:** a tag produces a DMG a stranger can download and open without a Gatekeeper warning; no known high or medium severity findings open in the security register.

---

### M5 — v1.0 Public Release · **2 weeks**

Beta feedback triaged; release notes; landing/README; GitHub issue templates; v1.0 tagged and published.

**Total: ~25 weeks / ~6 months** to public macOS v1.0.
*(+2 weeks from v1 plan for security hardening sprint in M1)*

**Add ~8–10 weeks for iOS** (M6), which can start in parallel around M3 if a third engineer is available.

---

## 6. Key Technical Risks

| # | Risk | L | I | Mitigation |
|---|---|---|---|---|
| **R1** | **PDF reading-order extraction quality.** Multi-column, footnoted, or unusually-structured documents are heuristic-only. | H | H | Adopt `pdfium-render`. Build graded fixture corpus at M1 start. Keep backend behind a trait. Define explicit "acceptable" bar; willing to ship v1.0 with documented limitations. |
| **R2** | **Parser panics on malformed input reach the FFI boundary.** A panic across the C ABI is undefined behaviour. | H | H | `catch_unwind` at every FFI export `[F1]` ✅. `cargo-fuzz` per parser in nightly CI ✅ commit `a5aaf03`. `ParseLimits` enforced before allocation `[F4]` ✅. |
| **R3** | **Supply-chain compromise via mutable CI action tags.** A compromised tag has access to signing keys. | H | H | All actions pinned to commit SHA `[F3]` ✅. Automate SHA updates via dependabot/Renovate. |
| **R4** | **Licence contamination breaks the MIT promise.** | M | H | `cargo-deny` with strict allowlist including `[advisories]` and `[sources]` as a blocking CI gate `[F5]` ✅. Manual licence review of bundled fonts and fixtures. |
| **R5** | **RSVP timing jitter at high WPM.** At 1000 WPM a word is 60ms — ~4 frames. | M | H | Core is a pure `(state, elapsed) -> token` function ✅. Drive from `CVDisplayLink`. Pre-fetch a token window across FFI. Instrument frame timing. |
| **R6** | **FFI boundary friction and build tooling.** | M | M | uniffi proc-macro mode ✅. Binding generation as Xcode build phase with non-silenced failures `[F7]` ✅. |
| **R7** | **Notarisation and Gatekeeper.** First-time notarisation with `libpdfium` reliably surfaces problems. | M | M | Throwaway end-to-end notarisation during M0. Prefer staticlib. Automate and run on every tag. Review entitlements explicitly in M3 `[A3]`. |
| **R8** | **Annotation anchors break** across re-import or parser improvements. | M | H | Content-fingerprinted anchors with re-anchoring on mismatch (ADR-003). Property-test anchors against perturbed documents. |
| **R9** | **Schema downgrade corruption.** A user reverts to an older app version after a schema migration; older code silently operates on a newer schema. | M | H | Version ceiling check in `gist-store`: return `SchemaTooNew` before any operation on a future-version schema `[F11]` ✅. |
| **R10** | **Zip-bomb denial of service in ePub/DOCX parsers.** A crafted file with extreme compression ratio exhausts process memory. | M | H | `ParseLimits.max_expanded_bytes` enforced during streaming decompression `[F4]` ✅. Fuzz corpus includes zip-bomb-structure adversarial cases ✅ commit `a5aaf03`. |
| **R11** | **Scope: three UI codebases, small team.** | M | M | macOS-only for v1.0. Do not begin Windows until the iOS shell has validated the core API is genuinely platform-neutral. |
| **R12** | ~~**Copy-on-import (ADR-006) is unimplemented; item removal can delete a user's original file.**~~ **Resolved 2026-09-12.** `store_original_copy` + `source_copy_ref`/`source_copy_path` now exist end to end; `remove_items(delete_source_files: true)` deletes the sandboxed copy, never the raw `source_path`. Residual: the content-hash dedup vs. per-item-deletion limitation noted in ADR-006 (low severity, no reference counting yet). | L | M | `[A5]` ✅ Closed. Residual risk tracked in ADR-006's "known limitation" note, not release-blocking. |

---

## 7. Open Questions

**Resolved (ADRs written or decisions recorded in codebase):**
- FFI = uniffi proc-macro mode (ADR-001) ✅
- PDF backend = `pdfium-render` with a swappable trait (ADR-002) ✅
- Annotation anchoring: `(block_id, start, len, prefix_hash, quote_hash)` with content-fingerprinted re-anchoring (ADR-003) ✅
- HTTP = `ureq` + `rustls`, no tokio, no system OpenSSL (ADR-010) ✅
- MVP platform = macOS ✅
- Sync = none in v1 ✅
- Monetisation = free, MIT ✅
- DRM = detect and refuse, never circumvent (ADR-004) ✅ `[F8]`
- Q1: Full-text search in v1.0 — committed, FTS5 implemented ✅
- Q2: Tables parse+persist in model, flatten at render for v1.0 ✅ (DOCX parser)
- Q5: URL fetching on iOS — use Swift `URLSession` (system proxy, ATS, cellular-awareness) ✅
- Q6: Copy-on-import (ADR-006) ✅
- Q7: IR storage format — `<id>.json` + `<id>.tokens.json` on disk, SQLite holds metadata + paths (ADR-007) ✅
- Q9: Minimum macOS version = macOS 14 (unlocks `@Observable`, modern `NavigationSplitView`, string catalogs) ✅
- Q12: Web fetch policy — ADR-005 written and gist-web implemented ✅ `[F13]`

**Still open — decide before implementation begins:**

| Q | Question | Must decide by |
|---|---|---|
| **Q3** | Paginated view: v1.0 or v1.1? *(This plan: v1.1, but flow view built on layout abstraction regardless)* | M2 start |
| **Q4** | Windows OCR: `Windows.Media.Ocr` vs Tesseract? *(Design the `OcrEngine` trait to support both; decide implementation at Windows kickoff)* | Windows kickoff |
| **Q8** | SwiftUI `Text` vs TextKit 2 for the flow view? *(Prototype both in M2; decide before M3)* | M2 end |
| **Q10** | Schema/IR versioning and forward compatibility? *(Required before first public beta)* | M4 start |
| **Q11** | At-rest document integrity: is BLAKE3 checksumming warranted for stored document blobs? `[A4]` | M4 start |

---

## 8. Security Register

Findings from Security Review v1 (2026-09-08). Closed when fix is merged and verified.

| ID | Severity | Status | Closed in |
|---|---|---|---|
| F1 | High | ✅ Closed | commit 10b1c1e |
| F2 | High | ✅ Closed | commit 10b1c1e |
| F3 | High | ✅ Closed | commit 10b1c1e |
| F4 | Medium | ✅ Closed | commit 10b1c1e |
| F5 | Medium | ✅ Closed | commit 10b1c1e |
| F6 | Medium | ✅ Closed | commit 10b1c1e |
| F7 | Medium | ✅ Closed | commit 10b1c1e |
| F8 | Low | ✅ Closed | ADR-004 + commit 906957e |
| F9 | Low | ✅ Closed | commit 10b1c1e |
| F10 | Low | Open | M4 (release pipeline) |
| F11 | Low | ✅ Closed | commit 2c0b4b5 |
| F12 | Info | Accepted — document in PRIVACY.md | M4 |
| F13 | Info | ✅ Closed | ADR-005 + commit 50b2c84 |
| A1 | Architecture | ✅ Closed | ADR-008 + commit 2c0b4b5 |
| A2 | Architecture | ✅ Closed | ADR-009 + commit f6b3c46 |
| A3 | Architecture | Open | M3 (entitlements review) |
| A4 | Architecture | Open | M4 (integrity decision) |
| A5 | Architecture | ✅ Closed | Discovered and resolved same day, 2026-09-12. ADR-006 (copy-on-import) was unimplemented — `source_ref`/`source_path` was the raw original filesystem path, not a sandboxed copy; `delete_source_files: true` on `remove_items` deleted that raw path. Fixed: `gist-store::store_original_copy` + `Metadata.source_copy_ref`/`library_items.source_copy_path` (schema v4), wired through `import_txt`/`import_file` and `remove_items`. See §2.2/§2.10 and ADR-006 for detail, including a known low-severity dedup-vs-deletion limitation that remains open but isn't release-blocking. |

**Open items blocking release:** F10 (M4), A3 (M3), A4 (M4). F12 accepted and deferred.

---

*End of document. Security findings referenced as `[Fn]` and `[An]` map to GIST Security Review v1 (2026-09-08) findings by number.*
