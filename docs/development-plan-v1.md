# GIST — Development Plan

**Scope:** empty repo → notarised public v1.0 on macOS, with the Rust core built so iOS is a shell-only follow-on and Windows is a C-ABI follow-on.

See: docs/product-spec-reader-app-v3.md

---

## Key Decisions (Resolved)

| Decision | Resolution |
|---|---|
| Cross-platform strategy | Rust core + native UI shells per platform |
| FFI approach | uniffi proc-macro mode (not hand-rolled) |
| PDF backend | pdfium-render with a swappable trait |
| HTTP client | ureq + rustls (not reqwest — avoids tokio) |
| MVP platform | macOS first |
| Sync | None in v1 — all state is local SQLite |
| Monetisation | Free, MIT open source |

---

## 1. Repository & Project Structure

### Mono-repo layout

    gist/
    ├── Cargo.toml                  # workspace root
    ├── rust-toolchain.toml         # pinned stable
    ├── deny.toml                   # cargo-deny: licence allowlist
    ├── crates/
    │   ├── gist-model/          # document model, IR types, serde, errors
    │   ├── gist-parse-txt/
    │   ├── gist-parse-epub/
    │   ├── gist-parse-docx/
    │   ├── gist-parse-pdf/
    │   ├── gist-imageprep/      # deskew/contrast, pre-OCR
    │   ├── gist-web/            # fetch + readability extraction
    │   ├── gist-rsvp/           # pacing engine, pure, no I/O
    │   ├── gist-store/          # SQLite schema, migrations, FTS5
    │   ├── gist-core/           # facade: orchestration, import pipeline
    │   └── gist-ffi/            # uniffi scaffolding, staticlib + cdylib
    ├── apps/
    │   ├── apple/
    │   │   ├── project.yml         # XcodeGen — do NOT commit .pbxproj
    │   │   ├── Shared/             # SwiftUI views, view models, theme engine
    │   │   ├── macOS/              # AppKit bridges, menus, window mgmt
    │   │   ├── iOS/                # share ext, camera, BackgroundTasks
    │   │   ├── Generated/          # uniffi Swift bindings — gitignored
    │   │   └── Tests/
    │   └── windows/                # placeholder until post-v1.0
    ├── assets/                     # fonts (verify OFL), icons
    ├── fixtures/                   # test corpus (public-domain only)
    ├── tools/                      # build-core-xcframework.sh, notarize.sh
    ├── docs/                       # ARCHITECTURE.md, BUILDING-macos.md, FFI.md, ADRs
    └── .github/workflows/

Crate boundary rule: gist-model has zero I/O dependencies and must
compile to wasm32-unknown-unknown cleanly.

### CI Pipelines

| Pipeline | Trigger | Does |
|---|---|---|
| core-test | every PR | cargo test, clippy -D warnings, fmt --check |
| core-quality | PR + nightly | cargo-deny (blocking), cargo-audit, cargo-udeps |
| parser-corpus | nightly | runs fixture corpus, records quality metrics |
| fuzz | nightly | cargo-fuzz targets per parser |
| apple-build | PR on apps/apple/** or FFI | XcodeGen → xcodebuild macOS Debug + tests |
| release-macos | tag v* | universal xcframework, sign, notarise, DMG |

release-macos is tag-triggered on main repo only — fork PRs must not access signing secrets.

---

## 2. Rust Core Build Order

    Phase A:  model ──┬──> store ──┐
                      │            │
    Phase B:          ├──> txt     ├──> core ──> ffi ──> [Swift shell]
                      ├──> epub    │
                      ├──> docx    │
                      ├──> rsvp ───┘
    Phase C:          ├──> web
                      ├──> pdf
                      └──> imageprep

Vertical slice (proves architecture): model + txt + rsvp + store + ffi
→ SwiftUI window displaying RSVP over a .txt file. Build before any second parser.

Complexity: S ≤ 3 days · M 1–2 weeks · L 3–5 weeks · XL 6+ weeks

### 2.1 gist-model — M
Document, Section, Block (Paragraph/Image/List/Table), TextRun, Metadata,
OcrConfidence. Stable block IDs. Flat token stream (compute once at import,
persist — shared by RSVP/TTS/FTS/reading-time). Annotation anchors:
(block_id, start, len, prefix_hash, quote_hash) with re-anchoring on mismatch.

Crates: serde, serde_json, thiserror, uuid (v7), unicode-segmentation

### 2.2 gist-store — L
SQLite schema + migrations, library CRUD, collections/tags/smart views,
progress, annotations, preferences, FTS5 index, source-file management.
WAL mode; one writer behind Mutex; small read pool. Never expose a connection
across FFI. FTS5: external-content table, porter unicode61 tokenizer.
Smart views are stored predicates evaluated as SQL.

Crates: rusqlite (bundled, fts5, backup), refinery or hand-rolled PRAGMA user_version

### 2.3 gist-parse-txt — S
Encoding detection + decode, paragraph inference. Ship first.

Crates: encoding_rs, chardetng, encoding_rs_io

### 2.4 gist-parse-epub — M
ZIP → OPF/spine → NCX/nav TOC → XHTML → blocks. DRM detection with clear error
(do not false-positive on IDPF font obfuscation in encryption.xml).

Crates: zip, quick-xml, roxmltree, url, percent-encoding

### 2.5 gist-parse-docx — M–L
Heading detection is style-resolution, not tag-matching (w:pStyle → styles.xml
→ w:basedOn chain). Lists via w:numPr → numbering.xml. Tables: parse+persist
in v1.0, flatten at render. Tracked changes: accept ins, drop del, flag.

Crates: zip, quick-xml (evaluate docx-rs, expect to go direct)

### 2.6 gist-parse-pdf — XL ⚠ Highest risk
Reading-order extraction: positioned text runs → line clusters by baseline →
column detection via whitespace-gap projection → order L→R, T→B.
Header/footer stripping via repeated-text detection across pages.

Backend: pdfium-render (BSD-3, battle-tested, ~8–10MB libpdfium per arch).
Avoid mupdf-rs / Poppler — AGPL/GPL, incompatible with MIT.
Keep parser behind a trait so a pure-Rust backend remains swappable.

### 2.7 gist-imageprep — M
Pre: greyscale, contrast normalisation, deskew (Hough), denoise/crop.
Post: assemble Vision OCR output into ordered blocks with ocrConfidence[].
Recognition itself is native (Vision) — not in this crate.

FFI design: OcrEngine as a uniffi callback interface (trait implemented in Swift).
Keeps multi-page orchestration in Rust; Windows is a second trait implementation.

Crates: image, imageproc, rayon, fast_image_resize

### 2.8 gist-web — M
ureq + rustls (not reqwest — avoids tokio for the app's only network call).
Readability-style extraction via scraper/html5ever. Respect robots.txt.
Hard-stop on paywall/auth. See Q5 re: whether to move URL fetch to Swift on iOS.

### 2.9 gist-rsvp — M
Token stream → timed presentation schedule. Pure std, no timers.
Expose (state, elapsed) -> current_token. SwiftUI shell drives from CVDisplayLink.
Pacing factors: word length, sentence-end (~1.8×), comma (~1.3×), para (~2.2×).
WPM changes apply mid-playback (lazy per-token schedule, not precomputed).

### 2.10 gist-core — L
Import pipeline: infer type → dispatch parser → normalise → thumbnail → persist → index.
Error taxonomy: one exhaustive enum crossing FFI, stable variants, localisation keys.
Long-running work: ImportObserver callback interface with progress + cancellation.

Crates: infer, thiserror, tracing (feature-gated, no network sink), rayon

### 2.11 gist-ffi — L
uniffi proc-macro mode over gist-core for Swift.
Coarse API surface — avoid per-item accessors; prefer paginated bulk calls.
All FFI objects must be Send + Sync (Mutex/RwLock for interior mutability).
Windows C ABI: #[repr(C)] surface + versioned header. Designed-for now, built later.

---

## 3. macOS UI Shell Build Order

3.1  App skeleton + CoreClient actor bridge                          M
3.2  Library view — LazyVGrid + Table, sidebar, sort/filter         L
3.3  Import flows — fileImporter, drag-drop, URL, OCR review        L
3.4  Theme engine (build before the reader)                         S–M
     Token set: bg, surface, textPrimary, textSecondary,
     accent, highlight[1..5]. No literal colours in review.
3.5  Reader — flow view                                              XL
     SwiftUI Text per block in LazyVStack (v1.0).
     Built on ReadingLayout protocol so paginated view is
     a second implementation in v1.1, not a rewrite.
     Key fork: SwiftUI Text vs TextKit 2 — prototype M2, decide M2-end.
3.6  RSVP view + speed dial                                         L
     Drive from CVDisplayLink (not Timer — jitter visible at 600+ WPM).
     Dial: Canvas + DragGesture with accumulated rotation.
     accessibilityAdjustableAction + visible numeric stepper required.
3.7  Annotation UI                                                  L
     Sequence after flow view settles — depends on rendering decision.
3.8  Settings / preferences                                         M
3.9  TTS + accessibility audit                                      M
     Schedule audit as a budgeted work item, not a polish task.
3.10 Localisation scaffolding (.xcstrings from day one)             S

---

## 4. iOS Extension

Core: zero changes required. Same uniffi bindings, xcframework adds
ios + ios-simulator slices.

Shared Swift (~60–70%): CoreClient, view models, model-mapping, theme,
typography, preferences, RSVP engine wiring, dial geometry/rendering,
annotation logic, export, TTS, library data flow.

Platform-specific variants needed: navigation chrome, library layout,
input handling, toolbars/menus, settings scene, file import, window/scene,
Dynamic Type reflow.

iOS-only work:
  Share extension  M  — inbox-directory approach (not direct SQLite write)
  Camera capture   M  — VNDocumentCameraViewController
  BackgroundTasks  M  — BGProcessingTask; pipeline must be checkpointable (design in M1)
  App Store        S–M — privacy manifest, nutrition labels, export compliance

Sizing: ~35–45% of macOS shell effort (not 10%).
Can start in parallel with M3 if a third engineer is available.

---

## 5. Milestone Plan

M0  Foundations & vertical slice       3 weeks
    Workspace + crate skeleton, gist-model v1,
    gist-parse-txt, gist-rsvp, minimal gist-store,
    uniffi bindings + xcframework build script,
    macOS app: import .txt → library → RSVP.
    CI: core-test + apple-build green.
    docs/ARCHITECTURE.md + ADRs 001–003.
    DO A THROWAWAY NOTARISATION RUN during M0, not M4.
    Exit: clone → one script → running macOS app.

M1  Import breadth                     5 weeks
    ePub (DRM detection), DOCX, PDF reading-order,
    URL fetch + readability, OCR round-trip via Vision.
    Full gist-store schema incl. FTS5.
    Import pipeline: progress/cancel/error taxonomy.
    Fixture corpus + parser-corpus CI + fuzz targets.
    Exit: 50-doc mixed corpus, zero panics, ≥90% acceptable extraction.
    *** Most likely milestone to overrun. PDF is the reason. ***
    If PDF quality misses bar: ship v1.0 with documented limitation,
    not a slipped release.

M2  Library & reading                  5 weeks
    Library grid/list, collections/tags/smart views, sort/filter/search.
    All import flows in UI incl. OCR review screen.
    Theme engine (all four themes + OS-follow).
    Flow reading view: full typography controls, TOC, in-doc search,
    progress persistence.
    Prototype SwiftUI Text vs TextKit 2; decide by M2-end.
    Exit: team member uses it as daily reader.

M3  RSVP, annotations, accessibility  4 weeks
    RSVP view + dial + accessible alternative + scrub + session stats
    + exit-to-flow-at-position.
    Annotations: highlights/notes/bookmarks + sidebar + Markdown export.
    Settings; TTS; full VoiceOver + Dynamic Type audit.
    Localisation scaffolding.
    Exit: RSVP stable at 1000 WPM; VoiceOver navigable end-to-end.

M4  Hardening & release engineering   4 weeks
    Performance benchmarks (20-page PDF < 5s; 1k-item library).
    Crash/error-path sweep. Storage management.
    Notarised signed DMG from a tag.
    CONTRIBUTING.md, BUILDING-macos.md, cargo-deny clean.
    Public beta (~20–50 users) + triage.
    Exit: tag → DMG a stranger can open without Gatekeeper warning.

M5  v1.0 public release               2 weeks
    Beta feedback triaged; release notes; README; GitHub issue templates.

Total: ~23 weeks / ~5.5 months to macOS v1.0.
iOS (M6): ~8–10 weeks additional; can start in parallel at M3.

---

## 6. Key Technical Risks

R1  PDF reading-order quality          L/H
    Heuristic-only; quality varies enormously across producers.
    Mitigation: pdfium-render; graded fixture corpus from M1 start;
    parser behind a trait; explicit acceptable-bar; documented-limitation
    fallback rather than schedule slip.

R2  Parser crashes on malformed input  H/H
    A panic across FFI is UB or abort — app dies.
    Mitigation: catch_unwind at FFI boundary; cargo-fuzz per parser;
    resource limits (zip bombs, max pages, max nesting); no unwrap on parsed data.

R3  Licence contamination              M/H
    GPL/AGPL crates or non-OFL fonts break MIT distribution.
    Mitigation: cargo-deny allowlist as blocking CI gate from M0;
    manual review of all fonts and fixtures; THIRD-PARTY.md.

R4  RSVP timing jitter                 M/H
    At 1000 WPM a word is 60ms (~4 frames). Timer drift is visible.
    Mitigation: pure (state, elapsed) -> token; CVDisplayLink; pre-fetch
    token window across FFI; p99 frame-time benchmark in CI.

R5  FFI build tooling friction         M/M
    Universal xcframework, binding regeneration, cross-language debug.
    Mitigation: uniffi; one `make bootstrap`; bindings as Xcode build phase;
    apple-build in CI from M0; FFI.md covering debugging story.

R6  Notarisation problems              M/M
    libpdfium embedding reliably surfaces hardened-runtime issues.
    Discovered late, this blocks release.
    Mitigation: throwaway notarisation run during M0; automate on every tag.

R7  Annotation anchor breakage         M/H
    Parser improvements shift text, silently orphaning highlights.
    Mitigation: content-fingerprinted anchors; re-anchoring on hash mismatch;
    orphaned-annotations UI; property-test against perturbed documents.

R8  Scope / small team                 M/M
    Three UI codebases; Rust core reduces logic duplication, not UI work.
    Mitigation: macOS-only for v1.0; no Windows until iOS validates core API.

---

## 7. Open Questions (decide before milestone noted)

Q1  Full-text search: v1.0 or v1.1?          M1 start
    Recommend v1.0 — retrofitting FTS5 requires schema migration + re-index.

Q2  Tables: parse+persist v1.0, flatten render until v1.1?  M1 start
    Recommend yes — data destroyed at parse cannot be recovered without re-import.

Q3  Paginated view: v1.0 or v1.1?            M2 start
    This plan: v1.1; flow view built on layout abstraction regardless.

Q4  Windows OCR: Windows.Media.Ocr vs Tesseract?  Windows kickoff
    Design OcrEngine trait to support both; decide at Windows kickoff.

Q5  URL fetching on iOS: ureq or URLSession?  M1
    URLSession gets system proxy, ATS compliance, cellular-awareness.

Q6  Copy-on-import vs reference-in-place?    M1 start
    Recommend copy — simplifies sandboxing and iOS enormously.

Q7  Document IR storage: JSON vs postcard/bincode vs SQLite rows?  M1 start
    Interacts with FTS5 and lazy loading of large documents.

Q8  SwiftUI Text vs TextKit 2 for flow view?  M2 end
    Drives selection precision and therefore annotation implementation.

Q9  Minimum macOS version?                   M0
    Recommend macOS 14 (Observable, modern NavigationSplitView, string catalogs).

Q10 Schema/IR versioning + forward compatibility?  M4 start
    No sync means no server-side migration safety net.
    Policy needed before first public beta.
