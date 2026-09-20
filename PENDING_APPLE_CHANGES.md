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
