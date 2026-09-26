# M3 Execution Plan — Agent Roles & Team Structure

**Status:** execution complete as of 2026-09-26 — every role in §2 is done and merged (see that section's status update). Authoritative for *how M3 gets built* — scope/exit-criteria authority remains `docs/development-plan-v2.md` §3.6–3.11/§5 M3 and `CLAUDE.md`'s milestone register; this document does not restate or override those, it refines them into assignable work. If "proceed with development" is invoked again before the manual click-through/device-verification exit item (still outstanding, same as M2's) is done, the Team Leader should confirm with the user whether there's new M3 backlog to add here, or whether this pointer and this file should be rewritten for M4 per §4's note below — don't assume either without asking, since re-reading this file's §2 will otherwise correctly report "nothing left to assign" for the current plan.

**Last updated:** 2026-09-26.

**Trigger:** the phrase **"proceed with development"** (or `/proceed-with-development`) invokes this plan — see `.claude/commands/proceed-with-development.md` and the pointer in `CLAUDE.md`. Whoever executes it (this session or a future one) acts as **Team Leader** and follows §0 below.

**Platform scope:** this plan covers the macOS shell only, matching `development-plan-v2.md`'s own scope. It requires a session where `apple(macOS/iOS)=yes` per the `[GIST ENV]` banner (Xcode + xcodegen present). If run from a session where that's not true, the Team Leader executes only the Rust-only roles (R1, R2) and defers the Swift roles, logging what's pending the way `PENDING_APPLE_CHANGES.md` already does. Windows has its own plan (`docs/windows-development-plan.md`) and is out of scope here.

---

## 0. Team Leader role

The Team Leader is **not a spawned subagent** — it's whichever session read this file in response to the trigger. It does not write M3 feature code itself; its job is decomposition, delegation, integration, verification, and reporting.

**Responsibilities, in order:**

1. **Preflight.** Check `PENDING_APPLE_CHANGES.md` / `PENDING_WINDOWS_CHANGES.md` for actionable (not merely informational) entries and resolve or explicitly carry them forward before spawning anything. Confirm `git status` is clean on `main` (stash/ask if not — see the repo's standing git-safety rules).
2. **Compute the ready batch.** From §2's dependency table, find every role whose `depends_on` are all merged and verified on `main`/the integration branch. Batch 1 has no dependencies.
3. **Spawn the batch in parallel.** One `Agent` tool call per ready role, all in a single message (true parallelism, not sequential calls), each with `isolation: "worktree"` and a self-contained prompt built from that role's §3 entry — do not assume the subagent has read this file or `CLAUDE.md`; quote the relevant parts into the prompt. Use `subagent_type: "general-purpose"` unless a role's own entry says otherwise.
4. **Integrate one at a time.** For each completed agent, note the worktree path/branch the tool result reports. Merge onto a single integration branch (`integration/m3-<YYYY-MM-DD>`, created fresh off `main`) one branch at a time, resolving conflicts by hand — expect overlap in `CoreClient.swift`, `ContentView.swift`, `GISTApp.swift`, `LibraryView.swift`, and `crates/gist-core/src/lib.rs`, the same files M2's parallel workstreams collided on. Do not trust a role's own isolated test run as sufficient; the merged tree gets re-verified as a whole in step 6.
5. **Advance to the next batch** once its dependencies are satisfied on the integration branch, repeating steps 2–4.
6. **Full verification on the fully-integrated tree**, genuinely run, not claimed: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources` (and `cargo deny check advisories` if the local `cargo-deny` binary supports it — see `CLAUDE.md` §"Security policies"), then `xcodegen generate` + `xcodebuild build`/`xcodebuild test` (scheme `GISTmacOS`, `CODE_SIGNING_ALLOWED=NO`). Fix or bounce back to the relevant role's line of work if anything fails — don't paper over a failure in the integration step.
7. **Update the record.** Dated update notes (not rewrites) in: `CLAUDE.md`'s M3 narrative and milestone register, `docs/development-plan-v2.md` §3.6–3.11/§5 M3/§8, the relevant ADRs (§3's roles say which), `PLATFORM_VERIFICATION.md`, and this file's §2 status column. Follow the house style already used throughout `CLAUDE.md` — precise, dated, says what was actually run and what wasn't, corrects rather than deletes prior claims that turned out stale.
8. **Report to the user**, and **stop before publishing.** Summarize what shipped, what's verified and how, and what's still outstanding (VoiceOver click-through by a person is expected to remain outstanding here, same as M2's manual click-through gap — this environment cannot automate it). **Do not push the integration branch or open a PR without a separate explicit go-ahead** — "proceed with development" authorizes building and verifying the work locally, not publishing it; pushing/PRs are visible, harder-to-reverse actions per the standing git-safety rules and get their own confirmation, same as any other session.

---

## 1. What's already done (do not re-assign)

Verified against `git log` and `CLAUDE.md` as of 2026-09-25 — re-confirm before trusting if this file is read much later:

- **RSVP wall-clock timing** (`a0c3b3d`) — `RsvpWallClockEngine` in `apps/apple/macOS/RsvpView.swift` replaced the drifting `Task.sleep`-per-token loop; covered by `apps/apple/Tests/RsvpPacingTests.swift` (14 tests). Closes the fidelity gap `development-plan-v2.md` §5 M2 flagged.
- **Annotations backend scaffold** (`39c0942`) — `gist-model::Annotation`/`AnnotationKind`, `gist-store` schema v6 + CRUD, `gist-core` wrappers, `gist-ffi` `FfiAnnotation` CRUD exports. **Explicitly excludes** the anchoring/re-anchoring/orphaning logic ADR-003 specifies (see the commit message) and all UI — that's R1 and R4 below, not done.
- **`[A3]` entitlements review** — closed 2026-09-18, unchanged since.

## 2. Remaining M3 backlog → roles

**Status update, 2026-09-26: every role below is done, merged onto `integration/m3-2026-09-26` (18 commits over `main` @ `5324f7d`, HEAD `a13f059`), and re-verified for real on the fully-integrated tree** (`cargo test --workspace`/`clippy -D warnings`/`fmt --check`/`cargo deny check bans licenses sources` all green; `xcodegen generate` + `xcodebuild build`/`test` — BUILD SUCCEEDED, 163/163 tests). Not yet pushed/PR'd — see `CLAUDE.md`'s M3 milestone-register row for the full rundown of what each role built and the cross-role bugs integration caught (block-join-separator mismatch between R1/R4a; a missing `xcodegen generate` step of the Team Leader's own, twice). This table's `Depends on` column and the batching below are kept as a historical record of how it was actually executed, not a live TODO list.

| # | Role | Scope | Primary files/crates | Depends on | Status |
|---|---|---|---|---|---|
| R1 | Rust — Annotation anchoring | ADR-003's re-anchoring logic | `crates/gist-core/`, `crates/gist-model/` | — | ✅ Done 2026-09-26 |
| R2a | Rust — OCR ADR addendum | `[A7]` ADR-009 addendum | `docs/adr/009-ocr-callback-interface.md` | — | ✅ Done 2026-09-26 |
| R2b | Rust — OCR pipeline wiring | `import_image_with_ocr` + FFI export | `crates/gist-core/src/lib.rs:1259`, `crates/gist-ffi/` | R2a reviewed (R7) | ✅ Done 2026-09-26 |
| R3 | Swift — RSVP view polish | §3.6 | `apps/apple/macOS/RsvpView.swift` | — | ✅ Done 2026-09-26 |
| R4a | Swift — Annotation UI (CRUD) | §3.7, minus anchor-status | new Swift files + `CoreClient.swift` | — | ✅ Done 2026-09-26 |
| R4b | Swift — Annotation UI (anchor status) | orphaned-annotation surfacing | same as R4a | R1 | ✅ Done 2026-09-26 |
| R5a | Swift — Settings scene | §3.8 | new `SettingsView.swift` etc. | — | ✅ Done 2026-09-26 |
| R5b | Swift — Localisation retrofit | §3.11 | all `apps/apple/{Shared,macOS}/*.swift` | R3, R4a, R5a, R8 | ✅ Done 2026-09-26 |
| R6 | Swift — Accessibility + TTS | §3.9 | audit across all Apple views | R3, R4a, R5a, R8 | ✅ Done 2026-09-26 |
| R8 | Swift — OCR review screen | §3.3's missing review UI | new Swift files | R2b | ✅ Done 2026-09-26 |
| R7 | Security/Architecture reviewer | gate R2a before R2b; final gate | n/a — reviews, doesn't build | continuous | ✅ Done 2026-09-26 — no new findings |

**Batching** (as actually executed 2026-09-26):
- **Batch 1** (parallel): R1, R2a, R3, R4a, R5a
- **Batch 2** (parallel, after Batch 1 integrated and R7 had reviewed R2a): R2b, R4b
- **Batch 2.5**: R8 (needed R2b's FFI export — started once R2b merged)
- **Batch 3** (parallel, after everything above was integrated): R5b, R6 (both hit a mid-run session rate limit and were resumed via `SendMessage` rather than restarted, per their own worktrees' partial context)
- **Continuous**: R7 did a light-touch review of R2a mid-stream (gated R2b) and a full gate pass at the end (FFI panic-safety, mutex discipline, `ParseLimits` enforcement on the new OCR path, source-path logging discipline — all clean, no new findings)

---

## 3. Role briefs

Each brief below is written to be pasted into a fresh subagent's prompt more or less verbatim — the subagent has no context beyond what the Team Leader gives it.

### R1 — Rust Core Engineer: Annotation anchoring

**Mission:** implement ADR-003's re-anchoring logic, the piece the 2026-09-24 annotations backend scaffold (`gist-store`/`gist-core`/`gist-ffi`, commit `39c0942`) deliberately left out.

**Read first:** `docs/adr/003-annotation-anchoring.md`, the annotation CRUD already in `crates/gist-core/src/lib.rs` and `crates/gist-ffi/src/lib.rs`, and `Core::get_document`/`GistCore::get_document_json` (added for the M2 flow view — the source of the "current" document text to re-anchor against).

**Scope:** on load, verify `prefix_hash` and `quote_hash` for each stored `Annotation` against the current document's block text. If `prefix_hash` mismatches, search the block for a `quote_hash` match (text shifted) and update the anchor. If `quote_hash` isn't found either, mark the annotation orphaned rather than erroring. Land this in `gist-core` (it's the layer with access to both the assembled `Document` and stored annotations — `gist-model` is I/O-free by design and shouldn't gain a store dependency, `gist-store` only persists rows). Expose whatever Swift needs to render orphan status through `gist-ffi`, `ffi_catch!`-wrapped per the FFI safety policy.

**Explicitly out of scope:** any UI (that's R4b). Don't touch `apps/apple/**`.

**Acceptance:** new tests in `crates/gist-core` perturbing a document's text (shift, delete-around, delete-through) and asserting correct re-anchor vs. orphan behavior, per ADR-003's "property tests should perturb documents" note. `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` all green.

---

### R2a — Rust Core Engineer: ADR-009 addendum (OCR image-byte caps)

**Mission:** close `[A7]` — write the ADR-009 addendum defining a `ParseLimits`-style size/dimension cap for raw image bytes crossing the `OcrEngine` FFI callback boundary, before any OCR pipeline code is written (this is a hard sequencing requirement from `development-plan-v2.md` §3.3/§5 M3, not a preference).

**Read first:** `docs/adr/009-ocr-callback-interface.md`, and `crates/gist-imageprep/src/lib.rs`'s `prepare_image` — it already fixed the concrete decode-before-check gap (`N4`, peeks declared dimensions via `ImageReader::into_dimensions()` before full decode) but that's a different boundary (post-fetch, inside `gist-imageprep`) from the one this addendum covers (bytes arriving from the OCR callback itself, before they even reach `prepare_image`).

**Scope:** documentation only — an addendum section in `docs/adr/009-ocr-callback-interface.md` stating the cap(s) and where they're enforced (at the FFI entry point, mirroring how every other importer's `ParseLimits.max_bytes` is checked before allocation). No code in this step.

**Acceptance:** the addendum exists, is internally consistent with `ParseLimits`'s existing conventions, and is reviewed by R7 before R2b starts.

---

### R2b — Rust Core Engineer: OCR pipeline wiring

**Depends on:** R2a's addendum, reviewed by R7. Do not start until told the addendum has landed and been reviewed.

**Mission:** implement `Core::import_image_with_ocr` (`crates/gist-core/src/lib.rs:1259`, currently `todo!("OCR import pipeline — Phase M3")`) for real.

**Read first:** the reviewed ADR-009 addendum (R2a), `crates/gist-imageprep/src/lib.rs::prepare_image`, the existing `OcrEngine` trait (defined in `gist-core` per `[A2]`/ADR-009), and `Core::import_file`'s shape (type sniff → dispatch → normalise → persist → index) as the pattern to match.

**Scope:** wire multi-page image input → `prepare_image` (per-page) → `OcrEngine` callback → assemble ordered blocks with `ocrConfidence[]` per page (per `gist-imageprep`'s M3 remaining-work note) → persist via the existing `gist-core` import path (copy-on-import per ADR-006, same as other importers). Enforce R2a's addendum caps at the FFI entry point, not only inside `gist-imageprep`. Add the `GistCore` FFI export, `ffi_catch!`-wrapped.

**Acceptance:** tests including an oversized-image-bytes rejection test exercised at the `Core`/FFI layer (not only `gist-imageprep`'s existing unit tests), and an end-to-end happy-path test using a mock `OcrEngine`. `cargo test --workspace`/`clippy -D warnings`/`fmt --check` green.

---

### R3 — Apple Engineer: RSVP view polish

**Read first:** `apps/apple/macOS/RsvpView.swift` (already has `RsvpWallClockEngine` — the wall-clock drift fix landed 2026-09-24, don't re-do it), `apps/apple/Tests/RsvpPacingTests.swift`.

**Scope (per `development-plan-v2.md` §3.6):** ORP-highlighted word display, rotary speed dial, numeric WPM readout, an accessible stepper alternative (`accessibilityAdjustableAction` is mandatory, not optional, per spec §5.5), scrub/seek, back-5-words, punctuation-pause toggle, exit-to-flow, session stats. Investigate whether `RsvpWallClockEngine`'s wall-clock anchoring is sufficient for smooth on-screen redraw at 600–1000 WPM before reaching for `CVDisplayLink`/AppKit — the plan calls for `CVDisplayLink` because `Timer` jitter is visible at high WPM, but the model-level fix already addresses the underlying drift; document whichever choice you make and why, don't silently skip the investigation.

**Acceptance:** new pure-logic tests in `RsvpPacingTests.swift` (scrub math, back-5-words index math, punctuation-toggle behavior) following the file's existing non-`@State` pure-function testing convention. Full VoiceOver click-through is **not** this role's job — that's R6, once this lands.

---

### R4a — Apple Engineer: Annotation UI (CRUD)

**Read first:** the annotation CRUD exports in `crates/gist-ffi/src/lib.rs` (`FfiAnnotation`/`FfiAnnotationKind` + create/list/update-note/delete), and how `CoreClient.swift` wraps existing FFI calls (e.g. `listCollections`, `addTag`) as the pattern to match.

**Scope (per `development-plan-v2.md` §3.7):** selection → highlight in N colours, margin notes, bookmarks, an annotations sidebar, jump-to-annotation, export via `.fileExporter`. Wrap the CRUD FFI in `CoreClient`. Do **not** build orphan/anchor-status UI yet — R1 hasn't landed when this role starts in Batch 1; that's R4b, a follow-up pass.

**Acceptance:** new tests in `GISTTests.swift` covering the CoreClient wrapper round trip, matching the file's existing real-`GistCore`-in-a-temp-dir convention (no mocking).

---

### R4b — Apple Engineer: Annotation UI (anchor status)

**Depends on:** R1 merged and verified.

**Scope:** surface R1's orphan/re-anchor result in the sidebar built by R4a (e.g. a visible "orphaned" badge, per ADR-003's "mark annotation orphaned — surface in UI"). Small, focused follow-up — not a rebuild of R4a's surface.

---

### R5a — Apple Engineer: Settings scene

**Scope (per `development-plan-v2.md` §3.8):** a `Settings` scene with tabs Reading / Typography / RSVP / Import / Storage / About. The Storage tab's "Delete source files on removal" default preference should back the same default the removal-confirmation checkbox in `LibraryView.swift` already uses (per-action, not yet a persisted app-level default) — read that existing checkbox's default-`true` behavior first and promote it to a `UserDefaults`-backed setting the checkbox then reads its default from, rather than inventing a second, disconnected preference.

---

### R5b — Apple Engineer: Localisation retrofit

**Depends on:** R3, R4a, R5a, R8 merged (don't migrate strings in views that are still about to change).

**Scope (per `development-plan-v2.md` §3.11):** introduce String Catalogs (`.xcstrings`); migrate string literals out of existing `apps/apple/{Shared,macOS}/*.swift` views, old and new. English (UK) only ships in v1.0 — this is scaffolding, not translation work.

---

### R6 — Apple Engineer: Accessibility + TTS

**Depends on:** R3, R4a, R5a, R8 merged (audits real UI, not stubs).

**Scope (per `development-plan-v2.md` §3.9):** `AVSpeechSynthesizer` read-aloud integration; a full VoiceOver audit across every screen — Library, Collections, Flow View, RSVP, Annotations, Settings, OCR review; Dynamic Type verification; contrast checks against `Theme.swift`'s palettes including OLED and Sepia. Note up front in your report: this environment has no Accessibility permission for `osascript`/System Events, so an automated click-through isn't possible here either — do what's verifiable (VoiceOver-label/trait correctness by code inspection, Dynamic Type via preview snapshots at multiple size categories, contrast ratios computed against the token values in `Theme.swift`) and say plainly what still needs a person, the same honest framing `CLAUDE.md` uses throughout for this exact limitation.

---

### R8 — Apple Engineer: OCR review screen

**Depends on:** R2b merged (needs its FFI export).

**Scope (per `development-plan-v2.md` §3.3):** the OCR review screen that doesn't exist yet — multi-page review with low-confidence highlighting (using the `ocrConfidence[]` per page R2b's pipeline produces), wired to R2b's new FFI export. Don't wire any button to `import_image_with_ocr` until R2b has actually landed and been merged — calling a still-`todo!()` export panics cleanly but is indistinguishable from a real bug to anyone testing the UI, per the existing standing caution in `development-plan-v2.md` §5 M2.

---

### R7 — Security/Architecture Reviewer

**Not a build role** — reviews others' output, doesn't produce feature code. Use the existing `security-review` skill as the mechanism.

**Scope:**
1. **Gates R2b:** review R2a's ADR-009 addendum for adequacy (does it actually define an enforceable cap, is it consistent with `ParseLimits` conventions) before the Team Leader lets R2b start.
2. **Reviews R1's** re-anchoring logic against ADR-003's stated intent (correct hash comparison order, orphaning-not-erroring on quote_hash miss).
3. **Spot-checks the already-shipped annotation CRUD** (`39c0942`) for `ffi_catch!` coverage on every new `#[uniffi::export]` — it's new surface since the last full security pass and hasn't had a dedicated review yet.
4. **Final gate:** on the fully-integrated tree, run the full policy checklist from `development-plan-v2.md` §0 and `CLAUDE.md`'s "Security policies — must not be relaxed" section — FFI panic safety, mutex discipline in any new `gist-store` code, `ParseLimits` enforcement on the new OCR path, `source_ref`/path logging discipline. Report findings the way prior audit passes in this repo have (dated, `[Fxx]`-numbered if new, cross-referenced into the security register) — don't invent a new format.

---

## 4. Notes for whoever refines this plan later

- This file is deliberately the single source of truth the Team Leader re-reads at trigger time — update it in place (dated notes, matching `CLAUDE.md`/`development-plan-v2.md`'s own convention) rather than letting `.claude/commands/proceed-with-development.md` accumulate plan details of its own. The command file should stay a thin pointer.
- If a role's dependency graph changes (e.g. R2b turns out not to need R2a's review gate, or a new role is needed), edit §2's table and §3's briefs directly — the Team Leader computes batches from the table, it doesn't hardcode them.
- If M3 fully lands and M4 becomes "next," this file's role should either be rewritten in place for M4's backlog or a sibling `docs/m4-agent-roles.md` created and this file's header updated to point at it — don't leave both active with unclear precedence.
