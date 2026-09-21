# PENDING_APPLE_CHANGES.md

Apple-platform changes identified during a non-macOS session that must be applied in the
next macOS session. On macOS, the SessionStart hook (`tools/detect-platform.sh`) flags
this file when it contains entries.

**Delete each entry's block after the change is applied and committed.**

<!-- Template — copy to add an entry (real headings start at column 0 with "## Pending Apple Change"; this example is indented so the detector ignores it):

    ## Pending Apple Change — [YYYY-MM-DD]
**File:** [path]
**Change required:** [what]
**Reason:** [why]
**Related commit/PR:** [hash or PR]
**Action:** [steps to apply]

-->

<!-- No pending items. -->

## Pending Apple Change — 2026-09-20
**File:** `Cargo.toml` (workspace `[profile.release]`), consumed by `tools/build-core-xcframework.sh`
**Change required:** none in Swift. The release profile changed from `panic = "abort"` to `panic = "unwind"` so `ffi_catch!` actually contains panics in shipped builds (review Q1). Verify on macOS.
**Reason:** under `panic = "abort"` the shipped xcframework killed the app on any core/parser panic instead of returning `GistError.InternalPanic`. Fixed and proven on Windows; Apple release build not run here.
**Related commit/PR:** fix/w1-gates
**Action:** run `./tools/build-core-xcframework.sh` and `xcodebuild build/test` (scheme GISTmacOS); note the xcframework size delta; optionally add a Swift test that a forced panic yields `.InternalPanic` (needs the `test-panic` feature build).

## Pending Apple Change — 2026-09-20
**File:** none edited; verification only (`Cargo.lock`, `tools/gen-bindings.sh`, `apps/apple/Generated/`)
**Change required:** none expected. Verify that the dependency merges of 2026-09-20 (uniffi 0.32.0 -> 0.32.1 #5, zip 2 -> 8.6.0 #10, sha2 0.10 -> 0.11 #24) did not disturb the Apple build.
**Reason:** these landed while verified only on Windows and Linux/macOS CI for the Rust core. The Windows C# generator and 20-check .NET spike pass against uniffi 0.32.1, but the Swift bindings were not regenerated and `apple-build` did not run on those PRs (path-filtered).
**Related commit/PR:** #5 (f7f77b2), #10, #24
**Action:** on macOS run `./tools/gen-bindings.sh` and confirm the regenerated `apps/apple/Generated/gist_ffi.swift` compiles; run `./tools/build-core-xcframework.sh`, then `xcodegen generate` and `xcodebuild build`/`test` (scheme GISTmacOS). Confirm the `originals/` content-addressed filenames are unchanged (sha2 0.11 output must equal 0.10's; the Rust tests cover this, the Swift `removeItems` test re-derives it independently). Delete this block once verified.

## Pending Apple Change — 2026-09-21
**File:** `apps/apple/Shared/CoreClient.swift` (no edit made from Windows)
**Change required:** review `importFile`/`removeItems`/`encryptItems` error handling. A failed operation publishes an error string, then the trailing `refresh()` on the success path clears it. The Windows port found the same shape made its DRM dialog impossible to trigger (caught by two tests) and fixed it by splitting a public `RefreshAsync` (clears the error on success) from an internal reload that does not clear it.
**Reason:** Apple's DRM alert survives because `drmProtectedFile` is a separate property, but the generic `error` string is still wiped by the refresh, so a real import failure can silently lose its message.
**Related commit/PR:** #45 (Windows CoreClient)
**Action:** on macOS, add a test that a failed import still shows its error after the follow-up refresh; if it fails, mirror the Windows split.

## Pending Apple Change — 2026-09-21
**File:** `Cargo.lock` + `crates/gist-store/Cargo.toml` (rusqlite 0.31.0 -> 0.40.2, libsqlite3-sys 0.28.0 -> 0.38.2, bundled SQLite 3.45.0 -> 3.53.2). No Swift or Xcode file edited.
**Change required:** none in Swift; verify only. `gist-store` needed zero source changes, but the bundled SQLite C amalgamation is recompiled for every Apple slice, and existing users' libraries on macOS have never been opened under the new SQLite here.
**Reason:** this bumps the C library that holds every user's library database and its FTS5 index. Windows/Linux/CI cover the Rust side; the Apple slices (arm64/x86_64 macOS, arm64 iOS + simulator) and the real `~/Library/Application Support/GIST` store do not.
**Related commit/PR:** chore/rusqlite-0.40-compat (supersedes Dependabot #48)
**Action:** on macOS run `./tools/build-core-xcframework.sh` (confirm every slice's bundled SQLite compiles and the xcframework size delta is sane), then `xcodegen generate` + `xcodebuild build`/`test` (scheme `GISTmacOS`) — 49 tests, including the `CoreClient` ones that drive a real `GistCore`. Then, most importantly, **launch the debug build against the real existing `~/Library/Application Support/GIST` store** and confirm the library still renders, search still returns results (type a partial word — the FTS5 prefix path), and an item opens in RSVP/Flow View. Note the result in `PLATFORM_VERIFICATION.md` and delete this block.

## Pending Apple Change — 2026-09-21
**File:** `Cargo.lock` (aes-gcm 0.10.3 -> 0.11.1, transitive aes pinned at 0.9.2), `crates/gist-store/src/lib.rs`
**Change required:** none in Swift; verify only. Item encryption at rest now runs on aes-gcm 0.11 (aead 0.6). A known-answer test proves a blob written by 0.10.3 still decrypts, but the Swift app has never been run against it.
**Reason:** existing users' encrypted items must stay readable after this ships.
**Related commit/PR:** #39
**Action:** on macOS run `./tools/build-core-xcframework.sh`, `xcodebuild test` (scheme GISTmacOS) including the encrypt round-trip tests, and ideally decrypt an item encrypted by the previous release build. Delete this block once verified.

## Pending Apple Change — 2026-09-21
**File:** `crates/gist-core/src/lib.rs`, `crates/gist-ffi/src/lib.rs`, `crates/gist-store/src/lib.rs` (no Apple file edited)
**Change required:** none required — **additive FFI only, adoption optional.** Two new exports (`GistCore.removeItemsDetailed(ids:deleteSourceFiles:)` -> `FfiRemoveOutcome`, `GistCore.sweepOrphanedFiles()` -> `FfiSweepOutcome`, plus the `FfiFileDeleteFailureKind` enum) appear in `apps/apple/Generated/gist_ffi.swift` automatically the next time `./tools/gen-bindings.sh` runs. Existing `removeItems` keeps its exact signature and behaviour (it is now `remove_items_detailed` with the outcome discarded), so `CoreClient` compiles and behaves unchanged if nothing is adopted.
**Reason:** review finding Q10 (Windows W2). Removal deleted stored files best-effort and logged failures at `debug!`, so the UI could report "removed" while a stored copy was still on disk. `removeItemsDetailed` reports removed ids plus file counts and a coarse per-file failure kind (locked/permission/other — no paths or titles cross the boundary, per the source-path logging policy); `sweepOrphanedFiles` reclaims anything a failed delete left behind on a later launch. It matters most on Windows (a handle without `FILE_SHARE_DELETE` makes the delete fail outright), but macOS is not immune — an SMB/AFP share or a file with `uchg`/`schg` set fails the same way, and the sweep is the only thing that ever cleans those up.

**If you adopt this, read the counters correctly.** Both records carry `filesDeleted`, `filesMissing`, `filesFailed` and `failureKinds`. **Only `filesFailed` is a warning.** `filesMissing` counts files that were already gone, which is the *normal* result for any item imported before ADR-013 added checksum sidecars — it has no `.blake3` files, so a perfectly clean removal of it reports `filesMissing: 2`. Surfacing that as a problem would make every legacy item in a user's library warn on removal for no reason. There is deliberately no `notFound` case in `FfiFileDeleteFailureKind`: every variant it has is a genuine failure.
**Also in this change (Apple-relevant, already active without any Swift edit):** `gist-store` now normalises a database path close to Windows' 260-character limit to the `\\?\` verbatim form before handing it to SQLite. The helper is `#[cfg(windows)]`; the non-Windows build gets a no-op passthrough, so Apple behaviour is byte-for-byte unchanged. Worth knowing it exists if a future path question comes up.
**Verified where:** locally on Windows 11 — `cargo test --workspace` (27 `gist-core`, 46 `gist-store`), `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, plus regenerated C# bindings building and `dotnet test` passing 118 tests.

**More was verified on Apple than usual for a Windows-session change, because `apple-build` triggers on `crates/gist-ffi/**` and therefore ran on this PR.** On PR #55 it regenerated the Swift bindings via `./tools/build-core-xcframework.sh`, ran `xcodegen generate`, and `xcodebuild build` + `test` (scheme `GISTmacOS`) came back **TEST SUCCEEDED, 48/48**. So the new FFI surface genuinely compiles into the Swift bindings and the macOS app still builds and passes its suite against it — that is not an assumption. `test (macos-latest)` also ran the new Rust tests on macOS, including the `#[cfg(unix)]` symlink-sweep test.

**Still not verified on Apple — and the gap is bigger than "it compiles" suggests.** Nobody has *called* `removeItemsDetailed`/`sweepOrphanedFiles` from Swift; there is no call site, so their runtime behaviour on macOS/iOS is untested. **Nothing here has been run against a real macOS library** — every Apple-side test uses a fresh temp directory, so the sweep has never looked at a real `~/Library/Application Support/GIST` store with real items, real `originals/` copies and whatever else has accumulated there. That is the one test worth doing by hand before adopting the sweep, since it is the only code in this change that *deletes* files it decides are unreferenced. No macOS-specific failure mode (SMB/AFP share, `uchg`/`schg`) has been exercised either. The `#[cfg(windows)]` tests (delete-sharing, reserved names, junctions, case-insensitive keep-list) do not compile there by design. iOS was not built at all.
**Related commit/PR:** PR #55, `feat/w2-windows-paths` (W2 Q10)
**Action:** adoption only — the build/regeneration half is already CI-proven above. Optionally have `CoreClient.removeItems` call `removeItemsDetailed` and surface a non-zero **`filesFailed`** (never `filesMissing`) in the existing result-summary alert, and call `sweepOrphanedFiles()` once at launch. Before wiring the sweep, run it once against the real `~/Library/Application Support/GIST` store and check the counts look sane — ideally after backing that directory up, since no Apple-side test has ever pointed it at a real library. Delete this block once adopted, or keep it as the adoption ticket if deferred.

## Pending Apple Change — 2026-09-21
**File:** `crates/gist-store/src/lib.rs` (no Apple file edited)
**Change required:** none — **behaviour change in `list_all_tags()` only (read query, no schema change, no migration).** It now returns only tags that currently have at least one item link (distinct, alphabetical, same casing as before). Previously `remove_tag` (and cascade removal of the last tagged item) left the row in `tags`, so an unused tag stayed in the Filter menu and selecting it showed an empty result. `tags` rows are not deleted; `add_tag` of an orphaned name reuses the existing row. Swift needs no code change; `CoreClient.listAllTags()` simply returns the corrected list.
**Action:** verify on macOS that the Library Filter menu drops a tag after its last use is removed (Tag editor) or its last tagged item is deleted. Delete this block once verified. CI `apple-build` runs on this PR (touches `crates/`).
**Related PR:** `fix/orphaned-tags`
