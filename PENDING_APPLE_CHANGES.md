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

