# PLATFORM_VERIFICATION.md

What has actually been verified, on which machine, when. Update the row whenever you build,
test or change something. "Not verified" is a valid and honest state.

| Area | macOS (M1 Pro) | Windows (ProArt13) | Linux | Notes |
|---|---|---|---|---|
| Rust core: `cargo test/clippy/fmt` | verified 2026-09-12 (per CLAUDE.md history) | test/clippy/fmt not verified | CI only | Pinned Rust 1.88.0 |
| Rust core: `cargo build -p gist-ffi` | not re-run | **verified 2026-09-20** (debug, x64 MSVC, 14.6 s incremental; produced `gist_ffi.dll` 11 MB) | CI only | cdylib output is what a Windows shell would load |
| Dependency audit: `cargo audit` (root + `fuzz/Cargo.lock`) | n/a | **verified 2026-09-20** (both clean; fuzz `cargo check` passes) | CI (`core-quality`) | RustSec DB, 1251 advisories |
| Fuzz targets (`cargo-fuzz`, nightly) | ran locally 2026-09-18 (~100 s per target, no crashes) | not run | **verified 2026-09-20** on GitHub Actions ubuntu (manual run, 20 s per target, all 4 pass, no crashes; nightly 120 s run not yet observed green) | Needs `RUSTUP_TOOLCHAIN: nightly` (fixed in #26) |
| SwiftUI app: `xcodebuild build/test` | verified 2026-09-19 (49 tests, plus GitHub CI) | n/a | n/a | Apple only |
| WinUI 3 shell (`apps/windows`) | n/a | not started; **no .NET SDK installed** (8.0.31 runtimes only), VS 2026 Community + Build Tools 2022 present, WinUI workload unverified | n/a | Plan: `docs/windows-development-plan.md` |
| FFI bindings (uniffi) | Swift generated | C# generator compatibility with uniffi 0.32 unverified (W0 spike) | n/a | ADR-001; plan ADR-015 |
| Manual UI click-through | outstanding (`docs/qa-manual-clickthrough-m2.md`) | n/a | n/a | Needs a person |

Convention: `verified <date>` means the check was executed on that platform. Anything a session
could not run gets a `// TODO(test): verify on [PLATFORM: X]` in code and `not verified` here.
