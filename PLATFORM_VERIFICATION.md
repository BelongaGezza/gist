# PLATFORM_VERIFICATION.md

What has actually been verified, on which machine, when. Update the row whenever you build,
test or change something. "Not verified" is a valid and honest state.

| Area | macOS (M1 Pro) | Windows (ProArt13) | Linux | Notes |
|---|---|---|---|---|
| Rust core: `cargo test/clippy/fmt` | verified 2026-09-12 (per CLAUDE.md history) | not verified | CI only | Pinned Rust 1.88.0 |
| SwiftUI app: `xcodebuild build/test` | verified 2026-09-19 (49 tests, plus GitHub CI) | n/a | n/a | Apple only |
| WinUI 3 shell (`apps/windows`) | n/a | not started | n/a | Placeholder |
| FFI bindings (uniffi) | Swift generated | C# path not designed | n/a | ADR-001 |
| Manual UI click-through | outstanding (`docs/qa-manual-clickthrough-m2.md`) | n/a | n/a | Needs a person |

Convention: `verified <date>` means the check was executed on that platform. Anything a session
could not run gets a `// TODO(test): verify on [PLATFORM: X]` in code and `not verified` here.
