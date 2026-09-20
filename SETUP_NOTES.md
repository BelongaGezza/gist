# SETUP_NOTES.md

One-time setup steps per machine. Mark done by appending `[DONE — machine, date]`.

## All machines
- Install the Rust toolchain via rustup; `rust-toolchain.toml` pins the version (the first `cargo` run downloads it).
- Run `git config` identity setup.
- Tools under `tools/*.sh` are bash scripts.

## macOS (only machine that can build/sign Apple targets)
- Full Xcode (not just Command Line Tools); `xcode-select -p` must point at an Xcode.app.
- `brew install xcodegen`, then `xcodegen generate` in `apps/apple/` (never commit `*.xcodeproj`).
- `./tools/build-core-xcframework.sh` and `./tools/gen-bindings.sh` to produce the FFI artifacts.
- Apply anything listed in `PENDING_APPLE_CHANGES.md` before new work.

## Windows
- Install Git for Windows (provides Git Bash, which runs the `tools/*.sh` scripts and Claude Code hooks).
- Rust: install rustup with the `x86_64-pc-windows-msvc` target plus Visual Studio Build Tools (C++ workload).
- WinUI 3 shell (`apps/windows/`, not yet started): .NET SDK and Visual Studio with the Windows App SDK workload.
- Cannot run here: `build-core-xcframework.sh`, `gen-bindings.sh` (Apple slices), `notarize.sh`, XcodeGen, xcodebuild.
- Do not use `python3` in scripts: the Windows Store stub hangs.

## Linux
- Rust toolchain plus system build essentials. No Apple or WinUI targets.
