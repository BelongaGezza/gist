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
