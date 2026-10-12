# GIST for Linux — Development Plan

**Status:** Draft, 2026-10-06; proposed, not adopted; scope amended 2026-10-09 (see the notice below).  
**Purpose:** Add a Linux desktop shell while continuing the already-planned Apple M7 and Windows W6 work. This plan is deliberately incremental: it starts Linux compatibility and risk-reduction work in parallel, but does not make Linux a blocker for the v1.0 Apple release or the Windows work.  
**Current product boundary:** spec v1.6 (2026-10-09) lists Linux as post-v1.0 and not committed; Linux work remains exploratory until this plan is adopted.
> **Scope decision, 2026-10-09 (owner).** OCR, annotations and export are **potential future capability, not planned** on every platform that does not already have them (spec v1.6 §11). Linux therefore has no OCR adapter, no annotation UI and no export in any planned phase; references below are design history. Scanned or image-only input must fail with the typed `PdfNoTextLayer` / unsupported-format limitation.


---

## 1. Recommendation and goals

Treat Linux as a **post-v1.0 desktop target**. Start the reversible engineering work now, interwoven with M7 and W6, and make a Linux preview release only after the Linux app has its own packaging, security and human-verification gates. Linux must not delay M7's v1.0 sort-key gate, Apple's credential/manual-QA gates, or Windows W6.

The first Linux release should provide the same core local-reading workflow as the other desktop shells:

- Open/import supported files and HTTPS URLs; browse, search, sort, filter, tag and organise the library.
- Read in Flow and RSVP modes, with progress, themes and keyboard/accessibility support.
- Keep data local; preserve copy-on-import, checksum and per-item encryption behaviour; never silently downgrade encrypted data to plaintext.
- Use Linux desktop conventions for file dialogs, settings, application identity, key storage and packaging.

Feature parity is a target for the Linux release, not a precondition for beginning. Missing native dependencies (especially PDF and keyring support) must be exposed as typed, explicit limitations, not hidden behind generic errors or unverified claims.

### Recommended technical direction (subject to L0 spikes)

Build a **Rust GTK4 shell** (evaluate libadwaita as an optional GNOME presentation layer) that calls `gist-core` directly in-process. Keep GTK types out of the core. Apple and Windows continue to use their existing uniffi surfaces; Linux does not need an extra language binding merely to call Rust from Rust. This avoids a third FFI generator/runtime and lets the Linux host implement the existing Rust `KeyProvider`, `OcrEngine` and RSVP/core interfaces without crossing an ABI.

Use XDG Base Directory locations for data/config/cache, GTK file choosers through desktop portals where available, and Flatpak as the first distribution candidate. Confirm all three choices with small spikes before committing to them in an ADR. Do not assume that GNOME-specific APIs, an installed Secret Service, or a system PDFium/Tesseract package exists on every supported desktop.

---

## 2. Decisions to resolve at adoption

| ID | Decision | Recommendation / evidence gate |
|---|---|---|
| D1 | Does Linux join the supported product platforms, and when? | Add it as a post-v1.0 desktop target. Linux is not a v1.0 release blocker. A Linux preview can follow after a usable core workflow and security gate. |
| D2 | UI technology and architectural boundary | Spike GTK4 + Rust direct `gist-core` access against Avalonia/.NET 10 + the existing C# binding path. Prefer GTK4 if accessibility, text rendering, packaging and team velocity are acceptable; keep app state/logic out of GTK so the shell can change without rewriting the Rust core. Record the choice in a new ADR. |
| D3 | Minimum supported Linux environment | Start with x86_64 Linux and a documented Flatpak runtime/base. Decide whether arm64 is in the first Linux release after the PDFium and packaging probes. Name the tested GNOME/KDE environments and minimum GTK/runtime versions; do not claim generic Linux support from Ubuntu CI alone. |
| D4 | Key custody and encryption availability | Use a maintained Secret Service integration (or a supported keyring abstraction whose selected backend is verified on GNOME and KDE). Never write the AES key as plaintext or silently fall back to a new key. If no usable key service exists, keep unencrypted items usable where possible, clearly report that encrypted items cannot be opened, and disable new encryption with an actionable message. Decide whether a user-approved plaintext-only mode is acceptable. |
| D5 | Distribution format | Prototype Flatpak first, including portals and Secret Service access in the sandbox. Decide later whether to publish a Flathub build and/or provide a signed tarball/AppImage/deb; avoid maintaining several channels before one works reliably. |
| D6 | PDF support | The current pinned PDFium fetcher produces a macOS dylib only. Linux PDF support therefore needs its own pinned, hash-verified Linux binary and packaging/signing review, or an explicitly chosen system-library policy. Do not use an unpinned system library or claim PDF support until the Linux load/import path is tested. |
| D7 | Read-aloud engine (OCR removed from scope 2026-10-09) | Spike a speech-dispatcher/desktop TTS path. Keep speech on-device. Record required packages, licences, offline behaviour and fallback UX before including it in a release claim. |
| D8 | Icon policy | Replace the current Linux icon note's light/dark monochrome launcher-art rule with freedesktop-compatible full-colour app icons plus a separate symbolic monochrome in-app icon set. Confirm the existing source art and licensing before generating assets. |

---

## 3. Integration rules — run alongside M7 and W6

1. **No platform schedule is allowed to block another platform's release gate.** M7's R1/R-final remain the v1.0 gates stated in `docs/m7-agent-roles.md`; Linux is additive and post-v1.0. Windows W6 continues under `docs/windows-development-plan.md`.
2. **Start only low-risk Linux work before M7's Rust API stabilises.** L0 may test Linux builds and prototype toolkit, keyring and packaging choices. The production shell must consume the integrated M7 APIs (not a parallel schema or API designed from an old `main` snapshot).
3. **Consume shared Rust behaviour; do not fork persistence or RSVP pacing.** Use `gist-core`, `gist-store`, the current schema/migrations, and the Rust RSVP session/pacing APIs. Do not duplicate timing arithmetic from Apple or Windows. Shared model/store changes remain Rust-first and require the existing cross-platform test matrix.
4. **Coordinate contracts, not UI code.** M7's `last_opened_at` and sort/progress semantics, typed resource-limit errors and any model changes are dependencies for the Linux shell. W6's exposed RSVP session is the reference pacing contract. Linux may contribute fixes to shared Rust crates, but must not edit Apple or Windows platform files from a Linux session; record cross-platform follow-ups in `PENDING_APPLE_CHANGES.md` or `PENDING_WINDOWS_CHANGES.md`.
5. **Avoid three implementations of the same view-model logic.** Reuse golden fixtures and behaviour tests for sorting, search, text flattening, annotation anchors, table reading order and progress semantics. Consider extracting logic into Rust only where all platform shells benefit; do not move platform presentation state into Rust solely to make GTK easier.
6. **Keep Linux CI scoped and honest.** Add a SHA-pinned, read-only-permission `linux-build.yml` for `apps/linux/**`, shared Rust crates and relevant tooling. Existing `core-test.yml` already runs Rust tests on `ubuntu-latest`; Linux UI, keyring, PDFium and packaging checks need separate jobs. No Linux check becomes a required branch-protection gate until the workflow is green on real GitHub Actions and its scope is understood.
7. **Record evidence by platform.** Update `PLATFORM_VERIFICATION.md` only after commands actually run on Linux. Distinguish core-only Linux CI from a GTK app build, desktop-session UI tests, Flatpak install/run, and a person’s manual verification.

### Dependencies on current work

| Existing work | Linux dependency / integration |
|---|---|
| M7 R1 reading-state/schema v6 | Linux library must use shared last-opened and progress semantics; wait for its migration/API contract before implementing library sorting and progress UI. |
| M7 R3 PDF resource limits | Reuse typed limits and budget policy. Linux parser/binary work is separate from the cross-platform parser logic. |
| M7 R6 localisation | Adopt the shared message catalogue and localisation conventions; investigate gettext/GTK extraction compatibility during L0. Do not introduce untranslated Linux-only user-facing strings. |
| M7 R7/R8 tables and pagination | Consume the landed IR and semantics. Linux table rendering must preserve Rust/Swift separator rules; paginated view is optional for Linux's first preview unless adopted as release scope. |
| M7 R-final and release gates | Linux work is not substituted for M7's independent review or human/credential-gated v1.0 work. Run a separate Linux review before Linux preview. |
| Windows W6 hardening / shared Rust fixes | Re-test shared core on Linux after Rust changes; do not infer Linux desktop correctness from Windows CI. Any Linux-discovered shared-core fix goes through the normal Rust test/CI gates. |

### Parallel sequencing

| Window | Existing work | Linux work that can safely overlap |
|---|---|---|
| M7 Batch 1 / Windows W6 | M7 R5 audit, R1/R3 shared-core work; Windows hardening and packaging | L0 Linux CI/core portability, toolkit spike, and keyring/Flatpak/PDFium feasibility probes only. Do not start production Linux sort/progress UI against a moving schema. |
| M7 feature batches / W6 completion | M7 table/pagination/localisation work; Windows reader/accessibility/release work | Finish L0, accept the architecture ADR, create shell and storage-path scaffolding; consume shared changes through the integrated `main`/PR base. Keep Linux-specific packaging changes isolated. |
| After M7 R1/R-final gate | Apple v1.0 may proceed through its separate human/credential gates; Windows continues its own exit work | Begin L2 library against the stable reading-state API, then L3. Linux remains post-v1.0 and does not gate either platform. |
| Linux preview hardening | Existing Apple/Windows maintenance continues | L4/L5 only after the chosen keyring, package and native dependency paths have working prototypes; release only the features whose packaged tests and manual checks pass. |

---

## 4. Phases and exit criteria

Effort estimates are rough single-engineer effort, not commitments. Parallel work can shorten elapsed time only where dependencies permit.

### L0 — Linux feasibility and architecture gate (1–2 weeks; start in parallel)

**Purpose:** prove the core builds and choose a viable shell, key provider and packaging path before building UI.

- On a clean Linux machine, run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources`, the wasm32 model check, and a release-mode FFI panic-containment probe. Investigate Linux-only linker/runtime failures rather than treating existing Ubuntu core CI as a desktop verification.
- Add a minimal `gist-ffi`/`gist-core` Linux build smoke and verify the direct `gist-core` path works with real SQLite and temporary storage.
- Build tiny GTK4 and Avalonia spikes: import a fixture, show a native window, and read a document. Compare accessibility tree, text selection/rendering, theme integration, test automation, runtime size, contributor prerequisites and long-term dependency risk. Decide D2 from evidence.
- Prototype XDG paths, GTK file-picker portal access, app-data migration, Secret Service access from an ordinary desktop session and from a Flatpak sandbox. Exercise key create/read/concurrent access and missing/corrupt/unavailable service cases. No real user key or library is involved.
- Verify Linux PDFium availability, supported architectures, dynamic loading and redistributable licence notices; separately measure TTS package requirements. No PDF feature is promised by this gate.
- Define Linux security boundaries (file access, URL import, logs, encryption, native parser crash risk) and write the architecture ADR plus the D1–D8 outcomes.
- Add the Linux CI job in a non-required state, pinned by immutable action SHAs with `permissions: contents: read`.

**Exit:** ADR accepted; clean Linux core gates pass; toolkit, minimum runtime, key-service failure policy and first package format are evidenced; explicit yes/no for PDF in the first preview. If GTK or keyring/Flatpak cannot meet the gates, stop and revise the design before L1.

### L1 — App shell, storage and CoreClient (2–3 weeks)

- Create `apps/linux/` with a UI-free Linux client/application-services layer and GTK shell. Keep filesystem, settings, keyring, OCR and speech adapters behind small interfaces.
- Implement XDG data/config/cache path resolution and migration-safe app identity. Store SQLite and IR blobs under one stable data root; store the encryption key in the chosen OS key service, not in the library directory.
- Open the Rust core with a real Linux key provider from first launch, matching ADR-014's read-after-encrypt requirement. Handle missing/unavailable/corrupt keys distinctly and fail closed; do not silently regenerate a lost key.
- Build a small window with library loading, typed error presentation, async import, cancellation-safe teardown and clean shutdown. No synchronous parsing/SQLite work on the GTK main loop.
- Add unit/integration tests using a fresh temp data root and a deterministic fake key provider. Real key-service tests are isolated integration tests and must not touch the developer's account keyring.
- Add dependency lockfiles, notices, secure defaults, `desktop` entry and icon staging for the chosen package.

**Exit:** app starts on the named reference distro, creates isolated temporary test storage, imports and lists a TXT item through real `gist-core`, and survives missing-key-service and corrupt-key cases without data loss.

### L2 — Library workflow (2–4 weeks)

- Implement list/search, paging, sort/filter, multi-selection, collections, tags, import URL/file, remove, encrypt, integrity verification and clear empty/error states.
- Follow the adopted M7 API and product decisions for source type, last-read, progress, removal and encryption. Never delete the user's original source file; use the core's copy-on-import semantics.
- Use GTK-native selection, menus, file dialogs and keyboard shortcuts; use portals where supported and document any fallback. Keep blocking core/file work off the GTK main context.
- Port behavioural tests from Apple/Windows with shared fixtures: FTS prefix search, sort ties, tag/collection round trips, complete removal, shared-copy handling, encryption read-back, checksum mismatch vs unverified, long/Unicode filenames and URL rejection.
- Add UI automation for library workflows under a reproducible virtual display; add at least one interactive desktop smoke-test environment to CI or explicitly keep that part manual.

**Exit:** core library flows pass automated tests; integration tests prove no original-source deletion, no plaintext key storage and correct encrypted-item read-back; a person completes the library checklist on GNOME and one non-GNOME desktop.

### L3 — Flow and RSVP reading (2–4 weeks)

- Port the Flow reader over the canonical document JSON/model, including headings, lists, images where supported, tables, TOC, search/highlighting, typography, themes, position restore and progress.
- Make long-document rendering bounded and responsive. Reuse W5's pathological-document fixtures and add Linux-specific measurements; test chunk boundaries, combining sequences, emoji, RTL text and very long unbroken content.
- Drive RSVP from the Rust pacing/session API with a monotonic clock; do not hand-port token-duration or punctuation rules. Verify pause/resume, seek, WPM changes and persisted position across restart.
- Add keyboard navigation, screen-reader labels and system text scaling. Verify GTK accessibility metadata by inspection and automation, then run a real Orca pass; automated assertions alone do not close the accessibility gate.
- Add integration tests that open fixture-derived documents in both reading modes and validate section ordering and progress semantics against Rust's golden results.

**Exit:** both reading modes function end-to-end; normal and pathological performance is measured on Linux; no high-severity accessibility/security issues remain; a person completes the reader checklist with Orca and keyboard-only use.

### L4 — Import parity: PDF, web and speech (OCR removed from scope 2026-10-09; 2–4 weeks; feature gate)

- Reuse the existing TXT/ePub/DOCX/URL parsers and SSRF-safe Rust fetcher. Test file-picker and portal behaviour without adding shell-specific fetch code.
- For PDF, add an independently pinned Linux PDFium asset/build path (SHA-256 checked, fail closed, safe archive extraction, included licence texts); verify dynamic-library loading inside the selected package and run the existing parser corpus/fuzzer. If D6 cannot be closed, PDF stays explicitly unavailable on Linux rather than loading an arbitrary system library.
- ~~Scans/images via a local `OcrEngine` adapter~~ — unplanned future capability. Scanned PDFs and images report the typed no-text-layer / unsupported limitation.
- For TTS, use the selected local Linux speech service and report missing voices/services clearly; do not add network speech.
- Add tests for unavailable backends, encrypted PDFs, image-only-PDF routing, limits, cancellation, non-English text and offline operation.

**Exit:** every feature included in the Linux release has an end-to-end test in the packaged environment; unsupported formats/backends have typed explanations; binary provenance/licensing and no-network TTS claims are independently reviewed.

### L5 — Accessibility, packaging, release and independent review (2–3 weeks)

- Produce a reproducible Flatpak candidate with restricted filesystem permissions, portal-only user file access, network permission only for URL import, bundled native dependencies pinned and licensed, and an explicit Secret Service access policy.
- Test install, upgrade, launch, data retention/uninstall behaviour and migration on clean GNOME and KDE/Plasma environments; do not infer this from an unpackaged GTK run.
- Validate app ID, `.desktop` metadata, MIME types, scalable full-colour launcher icons, symbolic in-app icons, keyboard shortcuts and localization catalogue extraction.
- Complete Orca, keyboard-only, high-contrast, fractional-scaling and theme checks, including dark/sepia/OLED. Record exactly which distributions/desktops/architectures were run.
- Add Linux package and desktop-session CI, dependency/security scans, SBOM or dependency inventory, and a Linux release checklist. Update Dependabot for any new package manager manifests in the same PR.
- Run a fresh independent security/quality review of the integrated Linux diff, checking keyring failure paths, filesystem permissions/portals, FFI/core panic boundaries, PDFium loader and extraction, URL-import trust boundary, native dependencies, licensing, logs and data migration.

**Exit:** reproducible package installs/runs on the declared support matrix; release artifact integrity is verified; security review has no open High findings; manual checklist is complete; `PLATFORM_VERIFICATION.md` reports only checks genuinely run.

---

## 5. Test and CI matrix

| Layer | Required evidence |
|---|---|
| Shared Rust core | Existing `core-test` Linux leg plus the current workspace test/clippy/fmt/deny/wasm checks; add Linux PDFium execution only after a pinned Linux binary is available. |
| GTK shell | `cargo check/build` for `apps/linux`; UI logic tests independent of display; desktop-session UI smoke under Xvfb or a controlled Wayland/X11 runner. A virtual display is not a substitute for manual accessibility/visual checks. |
| Data/keyring | Fake-provider unit/integration tests on every PR; real Secret Service integration in an isolated session bus, temporary service name/account and disposable keyring. No tests against a developer's default keyring. |
| Packaging | Build Flatpak from pinned manifests; inspect permissions and bundled notices; install/run/update smoke in a clean VM or container capable of desktop services. |
| Security | `cargo deny` and dependency audit; action SHA verification; archive/path validation for fetched binaries; no source paths/titles in normal logs; explicit network permission only for URL import. |
| Manual | GNOME + KDE/Plasma launch/import/read, Orca, keyboard-only, theme/scale, portals, keyring unlock/missing service, upgrade and uninstall/data retention. Mark each platform/environment independently. |

The Linux workflow should trigger on `apps/linux/**`, `crates/**`, shared manifests and Linux tooling. A Rust-only change still runs the existing cross-platform Rust matrix. Linux UI and Flatpak checks should be a separate job with pinned actions and least-privilege permissions. Avoid making a flaky desktop-session job a required check until it has repeatable real-CI evidence.

---

## 6. Out of scope for the first Linux release

- Android, Web, account/sync, cloud services, remote OCR/TTS and online document conversion.
- A Linux-specific parser or a second database/storage implementation.
- An unpinned system PDFium dependency, silent plaintext-key fallback or automatic key reset.
- Supporting every distribution, display server, CPU architecture and desktop environment at launch. Support must be a named and tested matrix.
- Rewriting Apple or Windows shells, holding v1.0 for Linux, or treating feature checklists as proof of manual accessibility verification.

---

## 7. Adoption and roadmap updates

This draft becomes binding only after the user approves D1–D8 (or records alternatives). On adoption:

1. Add a Linux row to the milestone register with L0 as parallel exploratory work and L1–L5 as post-v1.0 milestones; do not change M7's R1/R-final gate.
2. Update `docs/product-spec-reader-app-v3.md` platform scope/non-goals, `docs/development-plan-v2.md`'s platform roadmap, `docs/ARCHITECTURE.md`, and `docs/iconspecification.md` to reflect the chosen direction. Update `SETUP_NOTES.md` only with verified Linux prerequisites.
3. Add an ADR for the shell/core boundary, key custody and packaging; split it into separate ADRs if the evidence shows these are independent decisions.
4. Add `apps/linux/`, CI, Dependabot package ecosystems, notices and Linux manual QA checklist only after L0 decides the toolkit/package direction.
5. Create a short-lived topic branch/PR from current `main`; do not push directly to `main`. Rebase against the current integration tip before each role, verify the full integrated tree and have an independent reviewer inspect it before updating closure claims.

**Rough effort:** L0 1–2, L1 2–3, L2 2–4, L3 2–4, L4 2–4, L5 2–3 engineer-weeks (about 11–20 engineer-weeks total; L4 can be narrowed if PDF/TTS are deferred). Calendar time depends on team capacity and the decision gates. The first useful GTK library/reader preview can precede full Linux import parity; a supported Linux release should not be declared until L5 is complete.
