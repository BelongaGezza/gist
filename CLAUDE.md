# GIST — Claude Code Context

GIST is a cross-platform RSVP-style reading app. The Rust core handles all parsing, persistence, and pacing logic; native UI shells (SwiftUI on macOS/iOS, WinUI 3 on Windows) call into it via FFI.

## Platform awareness (read first)

- **This repo is developed on several machines** (macOS M1 Pro, Windows 11, possibly Linux). A `SessionStart` hook (`.claude/settings.json` → `tools/detect-platform.sh`) prints a `[GIST ENV]` block at session start giving the current OS, buildable targets and missing tools. **That block is the only source of truth for the current machine.**
- **Every machine-specific statement in this file is historical**, written from the macOS session (Xcode version, "no Accessibility permission", "verified locally", "this dev environment"). Treat them as `[verified: macOS/M1, <date>]`, not as facts about the current machine. Record new verification results per platform in `PLATFORM_VERIFICATION.md`.
- **Architecture precedence:** this repo is a Rust core with native shells (SwiftUI, WinUI 3). Any parent-directory CLAUDE.md that describes a Flutter/Dart architecture does not apply here; where they conflict, this file wins. The parent's platform-guard, no-absolute-paths and cross-platform-check rules still apply.
- **Non-macOS sessions may not edit Apple code** (`apps/apple/**`, `ios/`, `macos/`, Xcode projects, entitlements, `Info.plist`). `tools/guard-apple-edits.sh` (a `PreToolUse` hook) is a best-effort safety net that blocks most such Write/Edit calls (not Bash-tool edits); the real enforcement is CODEOWNERS and PR review. Log the needed change in `PENDING_APPLE_CHANGES.md` instead; set `GIST_ALLOW_APPLE_EDITS=1` only when the user explicitly instructs the edit.
- **Handoff files:** `PENDING_APPLE_CHANGES.md` (Apple work deferred from other OSes), `SETUP_NOTES.md` (per-machine setup), `PLATFORM_VERIFICATION.md` (what is verified where). Update them in the same commit as the change they describe.
- **Reverse handoff:** `PENDING_WINDOWS_CHANGES.md` (heading `## Pending Windows Change`) records Windows-only work (`apps/windows/**` C#/XAML) that was edited or needs verifying from a session that could not build it. On Windows/Linux sessions `detect-platform.sh` flags unapplied entries; delete each entry once applied and verified.
- **macOS sessions may edit `apps/windows/**` as text, with a warning only:** `tools/guard-windows-edits.sh` (a warn-only `PreToolUse` hook, always exit 0) reminds you the code cannot be built or tested on macOS; log what needs Windows verification in `PENDING_WINDOWS_CHANGES.md`. `GIST_FAKE_OS` is a test-only seam in both scripts; leave it unset.
- Hook scripts are bash and run under Git Bash on Windows. Do not use `python3` in scripts (the Windows Store stub hangs).

> **Rationalised 2026-10-12.** This file is the stable, always-loaded context. Dated narrative, the full milestone register and the full security register were moved verbatim to `docs/project-history.md` and `docs/security-register.md`. Keep this file short: put new history and findings there, not here.

## Current state (summary)

- **Rust core (M0–M1):** done and CI-verified (`cargo test`/`clippy -D warnings`/`fmt --check`/`cargo deny check bans licenses sources`). PDF (M6), tables, schema v8, TXT encoding detection and import metadata (M8 first slice) are in.
- **macOS app (M2–M7):** feature-complete and compiler/test-verified; what remains is human-gated — manual click-throughs (`docs/qa-manual-clickthrough-*.md`), VoiceOver/Dynamic Type on hardware, a real macOS 14/15 run, and a signed Hardened-Runtime launch (`N8`).
- **Release:** signing/notarisation credentials are missing; see `docs/v1.0-release-checklist.md`. v1.0 waits on M7 verification.
- **Windows (WinUI 3):** W0–W5 built; W6 (accessibility/packaging) not started. Pending hand-offs are in `PENDING_WINDOWS_CHANGES.md`.
- **M8 (spec conformance, `SC-01`–`SC-24`):** first Rust/macOS slice done; plan `docs/m8-agent-roles.md` is a DRAFT (D1–D6 unresolved). Spec §12 of `docs/product-spec-reader-app-v3.md` is the live backlog. Close an `SC` row only when implemented and tested, or the spec text is amended and dated in spec §10. Check §12 before claiming a spec feature done.
- Per-milestone detail: `docs/project-history.md`. Open security items: `docs/security-register.md` (all `F14`–`F25`/`N1`–`N7`/`N9` closed; `N8`, `F28`, `F29`, `F33` (accepted), and the low-severity `F39`–`F85` remainders are open).

## Project name & identity

The app is named **GIST**. All crates use the `gist-*` prefix. The older name "Readrrr" appears only in `docs/development-plan-v1.md` — ignore it; v2 supersedes it.

## Working conventions

- **Reports:** deliverable reports (security review, audit, analysis) default to a plain Markdown file. Use the `artifact-design` skill / HTML artifact only when explicitly asked.
- **"Proceed with development"** (plain text or `/proceed-with-development`): run the next milestone's remaining backlog via the multi-agent Team Leader protocol in `.claude/commands/proceed-with-development.md`, which names the active roles doc (currently `docs/m7-agent-roles.md`; M8's is an undecided draft — ask before adopting it or inventing scope). When a roles doc's backlog is exhausted, ask the user before drafting the next. Never push or open a PR as part of it; publishing is a separate explicit ask.
- **Standing step (F30):** every such run ends with an independent security/quality review by a fresh agent before records are updated.
- **Records:** update `PENDING_APPLE_CHANGES.md`, `SETUP_NOTES.md`, `PENDING_WINDOWS_CHANGES.md`, `PLATFORM_VERIFICATION.md` and the registers in the same commit as the change they describe. `main` is the only long-lived branch; use short-lived topic branches + PRs and delete them after merge.

## Repository layout

```
gist/
├── Cargo.toml                  # workspace root
├── rust-toolchain.toml         # pinned to 1.99.0
├── deny.toml                   # cargo-deny: licences, advisories, bans, sources
├── LICENSE                     # MIT — every crate declares license = "MIT"
├── crates/
│   ├── gist-model/             # Document model, IR types, token stream, errors
│   ├── gist-parse-txt/         # Plain text import
│   ├── gist-parse-epub/        # ePub (DRM detection + ParseLimits) ✅
│   ├── gist-parse-docx/        # DOCX (style-resolution + tracked-changes) ✅
│   ├── gist-parse-pdf/         # PDF — pdfium-render on a pinned prebuilt libpdfium.dylib (ADR-002, M6)
│   ├── gist-imageprep/         # OCR pre-processing skeleton
│   ├── gist-web/               # URL fetch (ureq/rustls + robots.txt + readability)
│   ├── gist-rsvp/              # Pure RSVP pacing engine (no I/O, no timers)
│   ├── gist-store/             # SQLite + FTS5 via rusqlite
│   ├── gist-core/              # Import pipeline facade + ImportObserver
│   └── gist-ffi/               # uniffi scaffolding → .xcframework
├── apps/
│   ├── apple/                  # SwiftUI macOS + iOS (generated Xcode project)
│   └── windows/                # WinUI 3 shell (W0–W5 built: library/collections/tags/theming, RSVP and flow readers; accessibility/packaging W6 not started)
├── spikes/                     # linux-gtk (GTK4 prototype), pdf-isolation (ADR-022 measurement); excluded from the workspace
├── fixtures/                   # synthetic public-domain test fixtures (23 original + M6 table/PDF additions)
├── fuzz/                       # cargo-fuzz workspace (separate from root)
├── tools/                      # build-core-xcframework.sh, notarize.sh, gen-bindings.sh
└── docs/
    ├── ARCHITECTURE.md
    ├── FFI.md                  # FFI boundary map (was empty until 2026-10-09)
    ├── adr/                    # ADRs 001–029 (see the ADR table below for the key ones)
    ├── development-plan-v1.md  # Superseded — use v2
    ├── development-plan-v2.md  # Current plan (security hardening integrated)
    ├── product-spec-reader-app-v3.md
    ├── security-review-v1.md   # M0 security review — superseded in full by v2 (2026-09-18)
    └── security-review-v2.md   # Independent re-verification pass (2026-09-18); current source of truth for findings, alongside this file's register below
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
| 011 | Encryption at rest: AES-256-GCM at `gist-store`'s file boundary, platform-backed `KeyProvider` |
| 012 | macOS app sandbox (entitlements) |
| 013 | At-rest integrity: BLAKE3 checksum sidecar files, verified on read, missing = unverified not corrupt |
| 014 | Per-item on-demand encryption (`Store::encrypt_item`), plus decoupling read-decrypt capability from write-auto-encrypt behavior (`Store::open_with_read_key`) |
| 019 | IR/schema versioning: `ir_version` envelope at the `gist-store` storage boundary (not on `gist_model::Document`); forward-compat rejection (`StoreError::IrVersionTooNew`) mirrors `SchemaTooNew`; additive changes need no version bump |
| 021 | Reading-state model: schema v7 `library_items.last_opened_at` (stamped by both readers via `mark_item_opened`), progress derived from the RSVP `reading_progress` position (flow-only items show 0%), `source_type` exposed, index `idx_tokens_item_idx` |
| 022 | PDF parser process isolation (`F33`): options and measured spike; recommendation to accept for v1.0 and schedule a portable helper process later — pending user decision |
| 030 | Per-item `word_count`: schema v8 stored column (not `COUNT(*)` on read), definition = number of `TokenKind::Word` tokens, backfilled from the `tokens` table in the migration transaction (ADR numbers 024–029 are the Linux/Android plans) |
| 023 | Paginated reading view: pure `Paginator` over AppKit-measured line fragments, position stored as a (block, offset) anchor in its own store |

## Security policies — must not be relaxed

All enforced in code. The full findings register and history is `docs/security-register.md`.

- **FFI panic safety:** every `#[uniffi::export]` function is wrapped in `ffi_catch!` (`catch_unwind`); panics map to `GistError::InternalPanic`, never cross the C ABI.
- **Mutex discipline:** all `gist-store` locks use poison-tolerant recovery (`.unwrap_or_else(|p| p.into_inner())`).
- **ParseLimits:** every parser accepts `ParseLimits` and enforces `max_bytes`, `max_pages`, `max_nesting_depth`, `max_expanded_bytes`, `max_zip_entries` (and table limits) **before** allocation.
- **DRM:** `gist-parse-epub` detects DRM via `META-INF/encryption.xml` → `ParseError::DrmProtected`; ambiguity is treated as DRM; never decrypt. Encrypted PDFs are rejected, never bypassed.
- **Web fetch:** TLS-only, `safe_resolve` rejects non-globally-routable addresses on every hop (SSRF), max 5 redirects, robots.txt, no cookies (ADR-005).
- **Supply chain:** all GitHub Actions pinned to commit SHAs; workflows default to `permissions: contents: read`; `cargo-deny` (bans/licenses/sources/advisories) blocks in CI. Licence allow-list: MIT/Apache-2.0/BSD/ISC/Zlib/Unicode-3.0/CDLA-Permissive-2.0 plus MPL-2.0 (required transitively by uniffi and scraper). Local `cargo-deny` 0.18.3 can't run `check advisories` (CVSS 4.0); CI uses action v2.1.1.
- **Document IDs:** UUIDv7. **Schema migrations:** transactional with a version ceiling (`SchemaTooNew`). **Source paths:** `debug!` only, never `info!`.
- **Copy-on-import (ADR-006):** file imports are copied to `<storage_dir>/originals/` (SHA-256 content-addressed); `remove_items(delete_source_files: true)` deletes that copy, never `source_ref`'s raw path. URL imports have no copy.
- **At rest:** BLAKE3 checksum sidecars verified on read (ADR-013); opt-in per-item AES-256-GCM encryption with Keychain-backed key (ADR-011/014); IR blobs carry an `ir_version` envelope (ADR-019).

## Open questions

Q3 (paginated view) resolved by ADR-023; Q10 (IR versioning) closed by ADR-019; Q11 (at-rest confidentiality) addressed by ADR-011/013/014. Full text in `docs/project-history.md`.

## Build & toolchain

```bash
# Rust toolchain (pinned in rust-toolchain.toml)
rustup show   # should say 1.99.0

# Run all tests
cargo test --workspace
# PDF tests need the pinned pdfium binary: ./tools/fetch-pdfium.sh, then
# GIST_REQUIRE_PDFIUM=1 cargo test --workspace  (unset, pdfium tests skip with a notice)

# Lint
cargo clippy --workspace -- -D warnings
cargo fmt --check

# cargo-deny — this dev environment's own cargo-deny binary is 0.18.3 and
# can't parse CVSS 4.0 advisories; `cargo install cargo-deny --locked` to
# upgrade it locally. CI uses cargo-deny-action v2.1.1 (0.20.2), where this
# check is genuinely clean (see N7) — both categories below are green there.
cargo deny check bans licenses sources
cargo deny check advisories

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
- `gist-model` must compile to `wasm32-unknown-unknown` (no I/O deps). **Correction 2026-10-04:** this was false until M6 R7 — `uuid` v7 needs an RNG source on that target; fixed by a wasm32-only `uuid` `js` feature in `gist-model/Cargo.toml` (native builds and `Cargo.lock` unaffected). `cargo check -p gist-model --target wasm32-unknown-unknown` now passes.
- Document IDs: `Uuid::now_v7().to_string()`.
- `ParseLimits` passed to every parser — never skip limit checks.

## CI pipelines (.github/workflows/)

Repo is hosted at `https://github.com/BelongaGezza/gist` (public); Dependabot alerts and security updates are enabled. Workflows run on real GitHub Actions (plus `windows-build`):

| Pipeline | Trigger | What it does |
|----------|---------|--------------|
| core-test | every PR | `cargo test`, `clippy -D warnings`, `fmt --check` on ubuntu + macos |
| core-quality | PR + nightly | `cargo deny`, `cargo audit`, `cargo udeps` |
| parser-corpus | nightly + parse-crate push | corpus test; uploads results artifact |
| fuzz | nightly | 120s per target; uploads crash artifacts |
| apple-build | PR touching `apps/apple/**`, `crates/gist-ffi/**`, or the gen-bindings/xcframework tooling | XcodeGen → `xcodebuild build`/`test` on the `GISTmacOS` scheme (Debug, unsigned) |
| release-macos | tag v* | verify-secrets guard → archive → DMG → notarize → GitHub release; the signed half has never run (no credentials, `F10`) |

Treat CI failures as blocking. Windows has its own `windows-build` job (not yet a required check, `F45`). History of CI fixes (`N6`, `F25`): `docs/project-history.md`.

## Milestones (status only)

| M | Status |
|---|--------|
| M0, M1 | ✅ Done |
| M2 | 🔶 Feature-complete; manual click-through outstanding |
| M3 | 🔶 Feature-complete; manual/VoiceOver/device verification outstanding |
| M4 | 🔶 Agent roles done; signing/notarisation credentials + beta outstanding |
| M5 | 🔶 Agent roles done; human release checklist outstanding |
| M6 | 🔶 PDF + tables done; manual PDF/table QA and signed launch (`N8`) outstanding |
| M7 | 🔶 Roles done; user decisions, macOS 14/15 run, manual QA outstanding |
| M8 | 🔶 First slice (SC-01/03/06/15) done; plan is a draft |

Full per-milestone detail and verification counts: `docs/project-history.md`.

## Security register

`docs/security-register.md` is the source of truth (rows `F1`–`F85`, `A1`–`A7`, `N1`–`N9`) with `docs/security-review-v2.md` and the dated `docs/security-quality-review-*.md` write-ups. Current review files supersede `security-review-v1.md`. Never mark an item closed without re-checking source; add new findings there.
