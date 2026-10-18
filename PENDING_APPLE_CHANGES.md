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


## Pending Apple Change — 2026-10-12 (shared Rust: F39 orphan-sweep minimum age)
**File:** `apps/apple/Shared/AppSettings.swift` (doc comment on `deleteSourceFilesOnRemoval`), `apps/apple/Shared/SettingsView.swift` (Storage tab wording about the next launch), `apps/apple/Tests/**` only if a test expects a just-orphaned file to be swept immediately (none found by grep on 2026-10-12).
**Change required:** wording only, no code is required. `Core::sweep_orphaned_files` (Swift `sweepOrphanedFiles()`) now keeps any unreferenced file modified in the last ten minutes (`gist_core::ORPHAN_SWEEP_MIN_AGE`), so a kept-then-orphaned stored copy is deleted "at a later launch, once it is ten minutes old" rather than strictly "at the next launch". A stored copy's modification time is its import time (refreshed on a dedup re-import), so in practice this is still usually the next launch. Adjust the comment/Settings sentence if it promises "next launch" exactly. The FFI shape is unchanged (no binding change). Also: `layout_table` now keeps text typed into a DOCX `vMerge` continuation cell (F77) — Swift reads the stored grid as-is, so no Swift change; the Rust/Swift table goldens are unaffected (their continuation cells are empty).
**Reason:** F39 (Windows review): a second process's startup sweep could delete an import that had written its blobs but not committed its row.
**Related commit/PR:** branch `feat/core-hardening-2026-10-12`.
**Action:** on macOS, confirm `apple-build` CI was green on the PR (shared Rust), optionally adjust the wording, delete this entry.

## Pending Apple Change — 2026-10-18 (verification todo for issue #123: removing the open item)
**File:** none yet — check first. Likely places to read: `apps/apple/macOS/RsvpView.swift` (progress saves while playing), `apps/apple/Shared/CoreClient.swift` (`removeItems`, save-progress calls), `apps/apple/Shared/FlowDocumentModel.swift`.
**Change required:** none known. Linux found a bug where removing the library item that is open in the RSVP reader left the reader saving progress for the deleted item once a second; every failure opened a modal error and closing it just triggered the next, so the app had to be killed. Confirm whether the macOS app has the same problem.
**Reason:** GitHub issue #123. The Linux fix is in the PR that closes it; macOS has not been checked.
**Related commit/PR:** issue #123.
**Action:** on macOS: (1) import a document, open it in the RSVP reader and press Play; (2) remove that same document from the library; (3) see whether errors repeat or the reader keeps running on a removed item. Also try removing the item while it is open in the Flow reader and from a collection view. If it reproduces: clear/stop the reader session when its item is removed and make a failed progress save report once, not per tick; add a test. Tick the macOS box in #123, record the result in `PLATFORM_VERIFICATION.md`, then delete this entry.
