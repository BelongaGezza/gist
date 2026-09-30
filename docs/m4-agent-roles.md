# M4 Execution Plan — Agent Roles & Team Structure

**Status:** in progress, started 2026-09-28. Authoritative for *how M4 gets built* — scope/exit-criteria authority remains `docs/development-plan-v2.md` §5 M4 and `CLAUDE.md`'s milestone register; this document does not restate or override those, it refines them into assignable work. Modeled directly on `docs/m3-agent-roles.md`'s structure, which is now historical (M3's own manual-click-through/device-verification exit gate is still outstanding and unrelated to M4 starting — see that file's header).

**Trigger:** the phrase **"proceed with development"** (or `/proceed-with-development`) invokes this plan now — `CLAUDE.md`'s Working Conventions pointer was updated to this file 2026-09-28, per `m3-agent-roles.md`'s own instruction to confirm with the user before repointing it, which happened in-session (user chose "Start M4" when asked).

**Platform scope:** macOS shell only, same as M3's plan. Requires `apple(macOS/iOS)=yes` per the `[GIST ENV]` banner.

**Hard environment constraint, confirmed 2026-09-28:** this dev environment has **no Apple code-signing identity** (`security find-identity -v -p codesigning` → 0 valid identities) and **no notarization credentials** (`xcrun notarytool` requires `--apple-id`/`--team-id`/`--password` or an App Store Connect API key, none configured). Any M4 work that requires a real Developer ID certificate or notarization — a genuinely signed Release build, live `notarytool submit`, `[N8]`'s runtime verification — **cannot be executed in this environment** and is explicitly out of scope for agent roles below. That work is written as code/CI-config where possible (so it's ready to run once credentials exist) but is flagged, not silently skipped, and stays open in the security register until a human with real credentials runs it.

---

## 0. Team Leader role

Same protocol as `docs/m3-agent-roles.md` §0, unchanged, restated briefly:

1. **Preflight.** Check `PENDING_APPLE_CHANGES.md`/`PENDING_WINDOWS_CHANGES.md` for actionable entries. Confirm `git status` is clean on `main` before spawning (stash/ask if not).
2. **Compute the ready batch** from §2's dependency table.
3. **Spawn the batch in parallel** — one `Agent` call per ready role, all in a single message, `isolation: "worktree"`, self-contained prompts quoting the relevant role spec (agents don't have this session's context).
4. **Integrate one at a time** onto a fresh `integration/m4-2026-09-28` branch off `main` @ `75700c6`. Expect overlap in `SettingsView.swift`/`AppSettings.swift` (R4) and `CLAUDE.md`/`docs/development-plan-v2.md` (docs roles) if any land in the same batch.
5. **Advance to the next batch** once dependencies are satisfied on the integration branch.
6. **Full verification on the fully-integrated tree**: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `cargo deny check bans licenses sources` (+ `cargo deny check advisories` if the local binary supports it), `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`, `CODE_SIGNING_ALLOWED=NO`).
7. **Update the record**: `CLAUDE.md` (M4 narrative + milestone register + security register), `docs/development-plan-v2.md` §5 M4/§8, relevant ADRs, `PLATFORM_VERIFICATION.md`, this file's §2 status column.
8. **Report to the user, stop before publishing.** Do not push the integration branch or open a PR without a separate explicit go-ahead.

---

## 1. What's already done (do not re-assign)

Confirmed against current source, 2026-09-28:

- **`[F22]`/`[F23]`** ✅ closed 2026-09-12 (panic-hook routing, `store_original_copy` ext sanitization) — no M4 work.
- **`[A6]` encryption-at-rest** — ADR-011/ADR-014, switched on for production 2026-09-18 (`CoreClient.shared` uses `newWithReadKey`). No M4 work.
- **`[A4]` at-rest integrity, Rust half** — ADR-013, `StoreError::ChecksumMismatch` implemented and tested at the `gist-store` layer (checked pre-decryption). **Swift/FFI/UI half is NOT done** — no distinct `GistError` surface, no verify action, no presentation. This is R3/R4 below.
- **`CONTRIBUTING.md`, `BUILDING-macos.md`** already exist at repo root/`docs/` — §9.5's open-source-hygiene requirement is otherwise met.
- **`docs/THIRD-PARTY.md`** already exists (fixture/font provenance) — R6 below only needs to verify it's current, not create it from scratch.
- **`gist-store::list_items(offset, limit)`** already supports pagination at the SQL layer (`LIMIT ?1 OFFSET ?2`) — §9.1's "1,000+ items responsive" requirement may already be partially met; R2 below verifies whether `gist-core`/`gist-ffi`/`CoreClient`/`LibraryView` actually use paging or fetch everything in one call, and benchmarks either way.
- **`docs/PRIVACY.md` does NOT exist** — confirmed via `find`. §9.2 of the product spec explicitly requires it. This is R5 below.
- **No `criterion` benchmarks exist anywhere in the workspace** — confirmed via `grep -rl criterion **/Cargo.toml`. This is R2 below.
- **`tools/notarize.sh`** exists and is correct in shape (calls `notarytool submit`/`stapler staple` with env-var credentials) but is never invoked — `.github/workflows/release-macos.yml` is still the `[F10]` guard-step stub that exits non-zero unconditionally. This is R7 below.

---

## 2. M4 backlog → roles

| # | Role | Scope | Primary files/crates | Depends on | Status |
|---|---|---|---|---|---|
| R1 | Rust — crash/error-path sweep + fuzz review | §5 M4 bullet 2 | all `crates/`, `fuzz/` | — | ✅ Done 2026-09-29 |
| R2 | Rust — performance benchmarks vs §9.1 | new `benches/`, `gist-parse-*`, `gist-store` | — | ✅ Done 2026-09-28 |
| R3 | Rust/FFI — `[A4]` checksum-mismatch surfacing | `crates/gist-ffi/`, `crates/gist-core/` | — | ✅ Done 2026-09-28 |
| R4 | Swift — storage management UI | `apps/apple/Shared/SettingsView.swift`, `CoreClient.swift` | R3 | ✅ Done 2026-09-29 |
| R5 | Docs — `docs/PRIVACY.md` (`[F12]`) | `docs/PRIVACY.md` | — | ✅ Done 2026-09-28 |
| R6 | Docs/Swift — license attribution screen | `docs/THIRD-PARTY.md`, new Swift view under Settings → About | — | ✅ Done 2026-09-28 |
| R7 | Release engineering — real `release-macos.yml` + local unsigned DMG script | `.github/workflows/release-macos.yml`, new `tools/build-dmg.sh` | — | ✅ Done 2026-09-28 (unsigned path only — signing/notarization blocked on credentials) |
| — | `[N8]` signed-Release runtime verification | n/a | n/a | **Blocked — needs a human with a real Developer ID cert + notarization credentials. Not assigned to an agent.** |
| — | Public beta (~20–50 users) + triage | n/a | n/a | **Out of scope for agent execution — a human/product task, not code.** |

### R1 — Rust: crash/error-path sweep + fuzz-corpus review

- Audit every crate for `unwrap()`/`expect()` on parsed or external data (should be none per the crate conventions in `CLAUDE.md`) — grep and manually confirm each hit is either test code or justified (e.g., a literal/const that can't fail).
- Confirm every parser (`gist-parse-txt`, `gist-parse-epub`, `gist-parse-docx`, `gist-web`, `gist-imageprep`) enforces every field of `ParseLimits` before allocation, not just the ones exercised by existing tests.
- Run each of the four fuzz targets (`fuzz_parse_txt`, `fuzz_parse_epub`, `fuzz_parse_docx`, `fuzz_web_extract`) for a longer real session than the N1 baseline (target ≥300s each, i.e. `cargo +nightly fuzz run <target> -- -max_total_time=300`) and report exec counts/crashes found. Nightly toolchain + cargo-fuzz should already be installed per `A4`'s prior session — confirm, install if missing.
- If any crash/panic is found, fix it and add a regression test/fixture; do not just note it.
- Report findings (even "nothing found") as a dated update to `CLAUDE.md`'s security register (new row or extending `N1`) — this is real evidence for §9.4's reliability requirement, not busywork.

### R2 — Rust: performance benchmarks vs §9.1

- §9.1 targets: (a) "20-page PDF/DOCX import+normalisation under 5s" (PDF is a stub — benchmark DOCX and, as a proxy, a large synthetic TXT/ePub); (b) "library with 1,000+ items remains responsive (virtualised lists, paged library API)"; (c) RSVP frame timing (already addressed by `RsvpWallClockEngine`, not this role's concern — don't re-litigate).
- Add `criterion` as a dev-dependency to the relevant crates (`gist-parse-docx`, `gist-parse-epub`, `gist-parse-txt`, `gist-store`) and write real benches under each crate's `benches/`: parse-time for realistic-size fixtures (reuse/extend `fixtures/`), and `gist-store::list_items`/`search_items`/`insert_item` at a synthetic 1,000+ row scale.
- Specifically verify whether `gist-core`/`gist-ffi`/`CoreClient`/`LibraryView` actually call `list_items` with pagination or fetch the whole library in one unbounded call — read the current call chain and report which it is. If it's unbounded, that's a real gap against §9.1(b); fix it (thread `offset`/`limit` through `gist-core`→`gist-ffi`→`CoreClient`, with `LibraryView` loading in pages) rather than just flagging it, since the store-layer primitive already exists.
- Add a `cargo bench --workspace` CI step (new or extending `core-quality.yml`) that runs but does not gate on a hard threshold (informational only, per how criterion's own harness works) — pinned toolchain/action shapes exactly like the existing workflows.
- Report actual numbers achieved against the 5s target in the same dated-update style as the rest of `CLAUDE.md`.

### R3 — Rust/FFI: `[A4]` checksum-mismatch surfacing

- `StoreError::ChecksumMismatch` exists in `gist-store` (see `crates/gist-store/src/lib.rs:47`) but has no distinct representation once it crosses into `gist-core`'s error mapping or `gist-ffi`'s `GistError` — confirm this by reading the current error-mapping code, then add a dedicated `GistError::ChecksumMismatch { path: String }`-shaped variant (matching how `GistError::DrmProtected` is a distinct, structured case, not a generic wrapped string) so Swift can pattern-match it instead of getting an opaque error.
- Add `Core::verify_item_integrity(id) -> IntegrityStatus` (or a small bulk `verify_library_integrity()` covering every item) that calls through to the existing `Store::verify_original_copy` plus re-reads each item's doc/tokens blob (which already checks the checksum before returning, per ADR-013) and reports per-item pass/fail/unverified (a missing sidecar is "unverified," never "corrupt" — same policy as the existing backfill note in `CLAUDE.md`'s `A4` register row). Export via `GistCore`, `ffi_catch!`-wrapped, mirroring `remove_items`'s per-id-outcome shape.
- Add tests: a corrupted blob is reported as failed, a clean library reports all-pass, a pre-`A4` item with no sidecar reports unverified-not-failed.
- `cargo test --workspace`/`clippy -D warnings`/`fmt --check` must stay green.
- Regenerate `apps/apple/Generated/gist_ffi.swift` via `./tools/gen-bindings.sh` so R4 can consume the new exports — do this as the last step of this role so R4 (which depends on this role) sees fresh bindings.

### R4 — Swift: storage management UI

*Depends on R3's FFI export existing on the integration branch.*

- `apps/apple/Shared/SettingsView.swift`'s existing `StorageSettingsTab` (see `CLAUDE.md`'s R5a note) currently only has a "delete source files on removal" toggle and a read-only library-location path — no actual storage usage or management. Extend it with:
  - A real disk-usage breakdown: size of `originals/`, size of the IR blob directory (`<id>.json`/`<id>.tokens.json` per ADR-007), and total. Compute via `FileManager` directory enumeration, not a new FFI export, unless walking the DB is meaningfully cheaper — your call, but keep it simple.
  - A "Verify Library Integrity" button that calls R3's new `verifyLibraryIntegrity`/`verifyItemIntegrity` wrapper (add the `CoreClient` wrapper first) and presents a results summary — how many passed, how many failed (with a distinct, non-alarming presentation for "unverified" vs a real "failed" — a `ChecksumMismatch` should read as "this file was corrupted; consider re-importing it," not a generic error).
  - Do **not** add an automatic delete/cleanup action for orphaned files in this pass — reporting only. Deleting unreferenced files is exactly the kind of destructive action that needs its own explicit design/confirmation, out of scope here.
- Add tests to `apps/apple/Tests/` following the existing `CoreClient`-round-trip pattern (real temp-dir `GistCore`, no mocking) — at minimum: integrity check on a clean library reports all-pass, and on a library with a manually corrupted blob reports the failure.
- `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`) must pass.

### R5 — Docs: `docs/PRIVACY.md`

- Required by product spec §9.2 (`docs/product-spec-reader-app-v3.md:313`) and referenced but never created. Write it covering, at minimum:
  - What's stored locally and where (library metadata + `source_ref`, IR blobs, original copies under `originals/`, checksums, optional per-item encryption).
  - What ever leaves the device: only the URL-import fetch itself (the URL the user pastes, per ADR-005/`gist-web`), nothing else. No telemetry, analytics, crash reporting, or data collection (§9.2).
  - `source_ref`'s logging policy: full paths only at `debug!`, never `info!` or above (§9.2, `[F12]`).
  - OCR runs on-device only (ADR-009).
  - Encryption-at-rest state as it actually ships today: opt-in per-item (ADR-014), not on-by-default for new imports — describe accurately, not aspirationally.
  - At-rest integrity checksums (ADR-013) and what a verification failure means for the user's data (nothing is silently discarded).
- Cross-reference from `README.md` if it has a docs index; don't restructure anything else.
- This closes `[F12]` in the security register — update `CLAUDE.md`'s register row for it (currently "Accepted... document before M4") to closed, with the date.

### R6 — Docs/Swift: license attribution screen

- Verify `docs/THIRD-PARTY.md` is current against the actual dependency tree (`cargo tree` / cross-check against `Cargo.lock` and `deny.toml`'s allow-list) — update it if stale, don't just trust its existing content.
- Add a "Licenses" or "Third-Party Notices" screen reachable from the existing Settings → About tab (see `apps/apple/Shared/SettingsView.swift`'s About section), sourcing its content from a bundled copy of the license text (bundle `THIRD-PARTY.md`'s content, or generate a structured list — your call on format, but it must be genuinely bundled and readable in-app, not a dead link).
- Add at least one test confirming the view loads/renders its content without crashing (a basic SwiftUI `PreviewProvider`-adjacent unit test, matching this codebase's existing light-touch UI test conventions).

### R7 — Release engineering: real `release-macos.yml` + local unsigned DMG script

- Replace `.github/workflows/release-macos.yml`'s unconditional guard-step stub with real steps: build the xcframework (`./tools/build-core-xcframework.sh`), `xcodegen generate`, `xcodebuild archive` for `GISTmacOS`, export/codesign (conditional on `APPLE_DEVELOPER_ID_CERT`-shaped secrets actually being present — if they're not configured, the job should fail with a **clear, specific** message naming which secret is missing, not a generic error), create a DMG, then call the existing `tools/notarize.sh`. Keep `permissions: contents: read` at the top; the comment already there about `contents: write` needing to be added deliberately for the actual GitHub-release-upload step still applies — add that scoped permission only on the specific step that needs it, when you add that step.
- Add `tools/build-dmg.sh <path-to.app> <output.dmg>` — a small, testable script that turns a built `.app` into a DMG (via `hdiutil create` or similar), usable **unsigned/ad-hoc locally** so this piece of the pipeline can be exercised in this environment even without a real signing identity. Actually run it end-to-end against a local unsigned `xcodebuild build` product as a smoke test and report the result.
- Do **not** attempt to actually sign or notarize anything in this environment — there is no identity or credentials to do so (confirmed 2026-09-28, see this file's header). Write the workflow so it's ready to run for real once secrets are configured; don't fake success.
- Update `CLAUDE.md`'s `[F10]` register row to reflect what's now real vs. still blocked on credentials.

---

## 3. Exit criteria for this plan (not the same as M4's full exit criteria)

This plan covers the roles above only. It explicitly does **not** close:
- `[N8]` (needs a human with real signing credentials to do a live signed-Release launch test).
- The "tag produces a DMG a stranger can download without a Gatekeeper warning" half of M4's exit criterion (needs the same credentials).
- Public beta.

Once R1–R7 land and are verified, report to the user what's genuinely done, what's credential-blocked, and what M4 exit still needs from a human.
