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

## Pending Apple Change — 2026-10-11 — SC-01 TXT encoding prompt
**File:** `apps/apple/Shared/CoreClient.swift`, `apps/apple/Shared/LibraryView.swift` (import flow), `apps/apple/Shared/Localizable.xcstrings`; regenerate `apps/apple/Generated/gist_ffi.swift` with `./tools/gen-bindings.sh` first.
**Change required:** Before importing a `.txt`/`.md`/`.text` file, call `GistCore.detectTextEncoding(path:encoding:nil)` (new, returns `FfiTextEncodingInfo { name, source: FfiEncodingSource, uncertain, lossy }`). If `uncertain || lossy`, show a confirmation sheet/alert: "GIST guessed <name> for this file. Choose the encoding if the text looks wrong" with a picker (UTF-8, UTF-16, Windows-1252, Windows-1251, ISO-8859-1/2/15, KOI8-R, Shift_JIS, GBK, Big5, EUC-KR; the picker value is a WHATWG label string) and a preview of the first few hundred characters if practical (re-run `detectTextEncoding(path:encoding:chosen)` to confirm `lossy == false`). Then import with `GistCore.importFileWithEncoding(path:encoding:)` (`nil` = auto-detect, otherwise the chosen label). If `uncertain == false && lossy == false`, import immediately as today (no prompt). Map the new `GistError.UnsupportedTextEncoding` to a generic "unknown encoding" message. All user-facing strings go in the string catalog (`tools/check-localisation.sh` guard).
**Reason:** SC-01 (spec §3.3): the core now detects BOM/UTF-16/legacy encodings and reports confidence, but nothing in the app asks the user when the guess is weak; invalid-UTF-8 files that used to fail now import (possibly with U+FFFD) silently.
**Related commit/PR:** worktree branch for SC-01 (core/FFI half).
**Action:** implement the above, add `CoreClient` tests against the fixtures in `fixtures/txt/encodings/` (copy into `apps/apple/Tests/Fixtures/`), then close the spec §12 `SC-01` row. Windows: equivalent prompt in `apps/windows/**` (log in `PENDING_WINDOWS_CHANGES.md` from a Windows session).

## Pending Apple Change — 2026-09-23 — new finding, not a regression
**File:** none — environment/tooling observation only
**Change required:** none. `xcodebuild test` (scheme `GISTmacOS`) reliably hangs at `KeychainKeyProviderIntegrationTests` (specifically observed at `testConcurrentGetOrCreateKeyCallsConvergeOnOneKey`, 20 concurrent `SecItemAdd`/`SecItemCopyMatching` calls) when run from an unattended/headless session on this machine — reproduced twice in a row. Almost certainly a macOS Keychain access-authorization prompt that needs a human to dismiss; it isn't a code bug (`FlowViewTests` and `GISTTests` — 28 tests total, including both new tests above — pass cleanly *before* the hang every time).
**Reason:** blocks `LibraryFiltering`/`LibrarySelection`/sort and `ThemeManager` suites from ever running in this kind of session, since XCTest runs suites in sequence and never reaches them. Also leaves a few orphaned, uniquely-UUID-suffixed test Keychain items behind when a hung run is killed instead of completing tearDown (harmless — isolated from the real production key by the existing test seam — but `security dump-keychain | grep 'com.gist.macos.encryption-at-rest.TEST-'` found 4 after this session's two interrupted runs).
**Action:** when running the full suite unattended, expect to need a human present to click through a Keychain prompt the first time (or run interactively in Xcode once to pre-authorize). Periodically clean up stray `...TEST-<uuid>` Keychain items. No code action needed.

