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

## Pending Apple Change — 2026-10-11 (SC-03 / SC-06: IR `Metadata` gains fields; new FFI `getItemMetadata`)
**File:** `apps/apple/Shared/CoreClient.swift`, `LibraryView.swift` (and wherever import results are shown); `apps/apple/Generated/gist_ffi.swift` is gitignored — regenerate with `./tools/gen-bindings.sh`.
**Change required:** (1) Regenerate bindings; new `GistCore.getItemMetadata(itemId:) -> FfiDocumentMetadata` (title, author, sourceType, `trackedChanges`, importDate, language, publishedDate, retrievedDate, canonicalUrl). (2) SC-03 UI: after a DOCX import (or when opening one) call it and, if `trackedChanges`, show the "unresolved tracked changes" notice (spec §3.4). (3) SC-06 UI (optional): show byline/published/retrieved/canonical for web items in an info pane. `source_type` for DOCX is now always `docx` (the `docx:tracked-changes` value is no longer written; `FfiDocumentMetadata` maps legacy rows to `docx` + `trackedChanges=true`). `get_document_json`'s `metadata` object also carries the new keys; Swift decoders ignore unknown keys.
**Reason:** Rust-side fields exist and are tested; nothing reads them yet, so SC-03 and SC-06 stay open in spec §12.
**Related commit/PR:** branch worktree-agent-a9db5a49d682ea802 (SC-03/SC-06).
**Action:** apply on macOS, add a `CoreClient` test importing a DOCX with `<w:ins>` and asserting `trackedChanges`; delete this entry once done.

