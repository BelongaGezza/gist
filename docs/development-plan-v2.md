# GIST — Development Plan v2

**Scope:** empty repo → notarised public v1.0 on macOS, with the Rust core built so iOS is a shell-only follow-on and Windows is a C-ABI follow-on.

**Supersedes:** development-plan-v1.md  
**Changes from v1:** Security review v1 (2026-09-08) findings fully integrated. All finding references are tagged `[Fx]` or `[Ax]` for traceability. Crate names updated to `gist-*` throughout (matching the live codebase). Milestone plan restructured to make security hardening explicit work, not assumed.

**Last updated:** 2026-09-09 — housekeeping commit `14a0cfd`: added missing ADR-010 (HTTP ureq+rustls), corrected plan ADR references (ADR-003 is annotation-anchoring, not HTTP), removed stray `fixtures/hello.txt`.

**Two deviations from spec v3 noted:**
1. **Paginated view** deferred to v1.1 (spec §11 position maintained). Flow view built on a layout abstraction from day one so paginated is a second implementation, not a rewrite.
2. **Full-text search** committed to v1.0 (spec §10.4 leaves open). Retrofitting FTS5 over an existing library requires schema migration + full re-index — worse to defer than to design in at M1.

---

## 0. Security Baseline

Security is a first-class architectural concern. This section defines the standing commitments the codebase must meet at every milestone. It does not replace the per-finding actions in later sections; it is the policy those actions implement.

### 0.1 FFI Safety Policy
- Every function exported via `#[uniffi::export]` **must** wrap its body in `std::panic::catch_unwind`. No exception. Panics must map to a `GistError::InternalPanic` variant — never propagate across the C ABI.
- The FFI crate's `[profile.release]` in `Cargo.toml` **must** set `panic = "abort"` as a belt-and-braces measure.
- Rationale: a panic crossing a C ABI boundary is undefined behaviour on all targets. `[F1]` ✅ Closed

### 0.2 Mutex Discipline
- All `Mutex` lock acquisitions on the `Store` connection **must** use poison-tolerant recovery (`unwrap_or_else(|p| p.into_inner())`). No bare `.lock().unwrap()` is permitted in `gist-store`. `[F2]` ✅ Closed

### 0.3 Supply Chain Policy
- All third-party GitHub Actions **must** be pinned to an immutable commit SHA. `[F3]` ✅ Closed
- `rust-toolchain.toml` **must** specify an exact stable version (`channel = "1.8x.y"`). `[F9]` ✅ Closed (pinned to 1.87.0)
- `deny.toml` **must** include `[advisories]`, `[bans]`, and `[sources]` sections. `[F5]` ✅ Closed

### 0.4 Parser Resource Limit Policy
- Before any parser moves from stub to real implementation, it **must** accept a `ParseLimits` struct and enforce all limits before allocation. `[F4]` ✅ Closed

```rust
pub struct ParseLimits {
    pub max_bytes: usize,          // 256 MB default
    pub max_pages: usize,          // 2 000 default
    pub max_nesting_depth: usize,  // for DOCX/ePub XML
    pub max_expanded_bytes: usize, // zip decompression limit (zip-bomb guard)
}
```

For zip-based formats (ePub, DOCX), the expanded-bytes limit is enforced during **streaming decompression**, not after reading the full output.

### 0.5 DRM Policy
- The ePub parser **must** detect DRM before attempting to parse content. Presence of `META-INF/encryption.xml` with non-obfuscation encryption methods returns `ParseError::DrmProtected`. `[F8]` ✅ Closed
- ADR-004 written and implemented.

### 0.6 Identifier Policy
- All document IDs **must** use UUIDv7. Timestamp-derived hex IDs are prohibited. `[F6]` ✅ Closed

### 0.7 Schema Migration Policy
- Every schema migration **must** be wrapped in a transaction. `[F11]` ✅ Closed
- A version ceiling check **must** be present: if `user_version > SCHEMA_VERSION`, return `StoreError::SchemaTooNew`.

### 0.8 Privacy Policy for Stored Paths
- `source_ref` stores full filesystem paths. Any future diagnostic, telemetry, or sync feature **must** strip or hash these before transmission. `[F12]`
- Log lines that emit source paths should use `debug!` level, not `info!`.

---

## 1. Repository & Project Structure

### 1.1 Mono-repo layout

```
gist/
├── Cargo.toml
├── rust-toolchain.toml         # pinned stable 1.87.0 [F9] ✅
├── deny.toml                   # cargo-deny: licence, advisories, bans, sources [F5] ✅
├── crates/
│   ├── gist-model/             ✅
│   ├── gist-parse-txt/         ✅
│   ├── gist-parse-epub/        ✅ DRM detection + ParseLimits
│   ├── gist-parse-docx/        ✅ style-resolution + tracked-changes
│   ├── gist-parse-pdf/         ⏳ stub — pdfium build tooling deferred
│   ├── gist-imageprep/         ✅ greyscale + resize + PNG re-encode
│   ├── gist-web/               ✅ ureq/rustls + robots.txt + readability
│   ├── gist-rsvp/              ✅ pacing engine, pure, no I/O
│   ├── gist-store/             ✅ SQLite + FTS5 migration v1→v2
│   ├── gist-core/              ✅ import pipeline + ImportObserver + OcrEngine
│   └── gist-ffi/               ✅ uniffi scaffolding + OcrEngine callback interface
├── apps/
│   ├── apple/
│   │   ├── project.yml         # XcodeGen — do NOT commit .pbxproj ✅
│   │   ├── Shared/
│   │   ├── macOS/
│   │   ├── iOS/
│   │   ├── Generated/          # uniffi Swift bindings — gitignored
│   │   └── Tests/
│   └── windows/
├── assets/
├── fixtures/                   ✅ 24 synthetic public-domain fixtures
│   ├── README.md
│   ├── txt/
│   ├── epub/
│   ├── docx/
│   └── web/
├── fuzz/                       ✅ cargo-fuzz workspace
│   ├── Cargo.toml
│   ├── README.md
│   ├── corpus/
│   └── fuzz_targets/
├── tools/
├── docs/
│   ├── ARCHITECTURE.md
│   ├── BUILDING-macos.md
│   ├── FFI.md
│   ├── PRIVACY.md              # documents source_ref path storage [F12] ⏳ M4
│   └── adr/                    # ADRs 001–010
└── .github/workflows/
    ├── core-test.yml           ✅ SHA-pinned
    ├── core-quality.yml        ✅ SHA-pinned, cargo-deny
    ├── parser-corpus.yml       ✅ nightly + on parse-crate push
    ├── fuzz.yml                ✅ nightly; 120s per target
    └── release-macos.yml       ✅ guard step exits non-zero until M4
```

### 1.2 CI skeleton

| Pipeline | Trigger | Status |
|---|---|---|
| `core-test` | every PR | ✅ |
| `core-quality` | PR + nightly | ✅ |
| `parser-corpus` | nightly + parse-crate push | ✅ commit `a5aaf03` |
| `fuzz` | nightly | ✅ commit `a5aaf03` |
| `apple-build` | PR touching `apps/apple/**` or FFI | ⏳ M2 |
| `release-macos` | tag `v*` | ✅ guard step in place; full impl ⏳ M4 |

---

## 2. Rust Core — Phased Build Plan

All Phase A and B crates are complete. Phase C is partially complete.

### 2.1 `gist-model` ✅ Complete
Document model, IR types, token stream, annotation anchors, UUIDv7 IDs.

### 2.2 `gist-store` ✅ Complete (M1 scope)
SQLite + WAL + FTS5 (schema v2). Poison-tolerant Mutex. SchemaTooNew ceiling check. Transactional migrations.

### 2.3 `gist-parse-txt` ✅ Complete
Encoding detection via `encoding_rs` + `chardetng`. ParseLimits enforced.

### 2.4 `gist-parse-epub` ✅ Complete
DRM detection, IDPF font obfuscation exemption, all four ParseLimits fields enforced during streaming decompression. OPF/spine parsing, XHTML→block mapping.

### 2.5 `gist-parse-docx` ✅ Complete
Style-resolution via `w:basedOn` chains (depth-limited to 20). Numbering detection. Tracked-changes handling. ParseLimits enforced.

### 2.6 `gist-parse-pdf` ⚠️ Stub only
`pdfium-render` backend selected (ADR-002). Deferred: per-arch `libpdfium` build tooling still needed.

### 2.7 `gist-imageprep` ✅ Skeleton complete
`prepare_image()`: decode PNG/JPEG → luma8 → resize → re-encode PNG. M3 work: deskew, Otsu threshold, OCR post-processing.

### 2.8 `gist-web` ✅ Complete
HTTPS-only, robots.txt pre-fetch, ureq + rustls, max 5 redirects, 30s/60s timeouts, response size limit, readability extraction. ADR-005 + ADR-010.

### 2.9 `gist-rsvp` ✅ Complete
Pure state machine. WPM 100–1000. Punctuation-aware pauses. No I/O, no timers. `(state, elapsed) -> token` interface.

### 2.10 `gist-core` ✅ M1 complete
Import pipeline: magic-byte type sniff → dispatch → normalise → persist → index. ImportObserver + cancellation. OcrEngine trait (defined here, not in gist-ffi). ParseLimits shared across all parsers.

### 2.11 `gist-ffi` ✅ M1 complete
`catch_unwind` + `ffi_catch!` macro on all exports. `GistError { Core, InternalPanic }`. OcrEngine uniffi callback interface. CoreOcrAdapter bridging FFI → core without circular dep.

---

## 3. macOS UI Shell — Phased Build Plan

### 3.1 App skeleton + core bridge ⏳ M2
`GistApp` scene, `CoreClient` actor, error-presentation surface.

### 3.2 Library view ⏳ M2
`NavigationSplitView`, `LazyVGrid` + `Table`, sort/filter, FTS search, cover thumbnails.

### 3.3 Import flows ⏳ M2
`.fileImporter` + drag-and-drop, URL paste sheet, OCR review screen, DRM error presentation.

### 3.4 Theme engine ⏳ M2
`Theme` (light/dark/sepia/OLED), OS-follow, semantic colour tokens, contrast validation.

### 3.5 Reader — flow view ⏳ M2
Virtualised `LazyVStack`, typography controls, TOC, in-document search, progress persistence. Built on `ReadingLayout` protocol. Decision: SwiftUI Text vs TextKit 2 to be prototyped in M2.

### 3.6 RSVP view + speed dial ⏳ M3
`CVDisplayLink`-driven. Rotary dial + accessible numeric stepper. Scrub/seek. Session stats.

### 3.7 Annotation UI ⏳ M3
Highlights (N colours), margin notes, bookmarks, sidebar, Markdown/text export.

### 3.8 Settings ⏳ M3

### 3.9 TTS + accessibility pass ⏳ M3
`AVSpeechSynthesizer`. Full VoiceOver + Dynamic Type audit.

### 3.10 App Sandbox Entitlements Review `[A3]` ⏳ M3
Produce explicit `.entitlements` file. Review against least-privilege. Required before M4.

### 3.11 Localisation scaffolding ⏳ M3
`.xcstrings` from day one. English (UK) only in v1.0.

---

## 4. Milestone Plan

### M0 ✅ Complete — Foundations & vertical slice (3 weeks)

### M1 ✅ Complete — Security Hardening + Import Breadth (7 weeks)

All security findings from review v1 addressed. Parser corpus + fuzz CI in place.

### M2 ⏳ Next — Library & Reading (5 weeks)

Goal: usable daily reading app.
- App skeleton + CoreClient
- Library grid/list, collections/tags/smart views
- All import flows + OCR review + DRM error
- Theme engine
- Flow reading view

Exit criterion: team member uses it as daily reader.

### M3 — RSVP, Annotations, Accessibility (4 weeks)

RSVP view + dial + scrub + stats. Annotations. Settings. TTS. VoiceOver audit. Entitlements review.

### M4 — Hardening & Release Engineering (4 weeks)

Benchmarks. Fuzz review. Notarised DMG pipeline. PRIVACY.md. `CONTRIBUTING.md`. Public beta.

### M5 — v1.0 Public Release (2 weeks)

**Total: ~25 weeks / ~6 months** to public macOS v1.0.

---

## 5. Open Questions

| Q | Question | Decide by |
|---|---|---|
| Q3 | Paginated view v1.0 or v1.1? (plan: v1.1) | M2 start |
| Q4 | Windows OCR: Windows.Media.Ocr vs Tesseract? | Windows kickoff |
| Q8 | SwiftUI Text vs TextKit 2 for flow view? | M2 end |
| Q10 | Schema/IR versioning + forward compatibility | M4 start |
| Q11 | At-rest integrity: BLAKE3 checksums on stored blobs? `[A4]` | M4 start |

---

## 6. Security Register

| ID | Severity | Status |
|---|---|---|
| F1–F9, F11, F13 | High/Med/Low | ✅ All closed M0–M1 |
| F10 | Low | Open — M4 (release pipeline) |
| F12 | Info | Accepted — document in PRIVACY.md at M4 |
| A1, A2 | Architecture | ✅ Closed |
| A3 | Architecture | Open — M3 (entitlements review) |
| A4 | Architecture | Open — M4 (integrity decision) |

**Open items blocking release:** F10, A3, A4.
