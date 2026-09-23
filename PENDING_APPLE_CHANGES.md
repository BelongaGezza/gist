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

## Pending Apple Change — 2026-09-23 — new finding, not a regression
**File:** none — environment/tooling observation only
**Change required:** none. `xcodebuild test` (scheme `GISTmacOS`) reliably hangs at `KeychainKeyProviderIntegrationTests` (specifically observed at `testConcurrentGetOrCreateKeyCallsConvergeOnOneKey`, 20 concurrent `SecItemAdd`/`SecItemCopyMatching` calls) when run from an unattended/headless session on this machine — reproduced twice in a row. Almost certainly a macOS Keychain access-authorization prompt that needs a human to dismiss; it isn't a code bug (`FlowViewTests` and `GISTTests` — 28 tests total, including both new tests above — pass cleanly *before* the hang every time).
**Reason:** blocks `LibraryFiltering`/`LibrarySelection`/sort and `ThemeManager` suites from ever running in this kind of session, since XCTest runs suites in sequence and never reaches them. Also leaves a few orphaned, uniquely-UUID-suffixed test Keychain items behind when a hung run is killed instead of completing tearDown (harmless — isolated from the real production key by the existing test seam — but `security dump-keychain | grep 'com.gist.macos.encryption-at-rest.TEST-'` found 4 after this session's two interrupted runs).
**Action:** when running the full suite unattended, expect to need a human present to click through a Keychain prompt the first time (or run interactively in Xcode once to pre-authorize). Periodically clean up stray `...TEST-<uuid>` Keychain items. No code action needed.

## Pending Apple Change — 2026-09-21 — ⚠️ not verified, no change needed unless a live gap surfaces
**File:** `crates/gist-store/src/lib.rs` (no Apple file edited)
**Change required:** none — **behaviour change in `list_all_tags()` only (read query, no schema change, no migration).** It now returns only tags that currently have at least one item link (distinct, alphabetical, same casing as before). Previously `remove_tag` (and cascade removal of the last tagged item) left the row in `tags`, so an unused tag stayed in the Filter menu and selecting it showed an empty result. `tags` rows are not deleted; `add_tag` of an orphaned name reuses the existing row. Swift needs no code change; `CoreClient.listAllTags()` simply returns the corrected list.
**2026-09-23:** not specifically re-verified this session — `testListAllTagsAndListItemsByTagRoundTrip` passed, but that test doesn't target the orphaned-tag-drop scenario specifically.
**Action:** verify on macOS that the Library Filter menu drops a tag after its last use is removed (Tag editor) or its last tagged item is deleted. Delete this block once verified. CI `apple-build` runs on this PR (touches `crates/`).
**Related PR:** `fix/orphaned-tags`

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

