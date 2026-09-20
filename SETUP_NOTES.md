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

## Windows — installed/verified 2026-09-20 (W0)
- [DONE — ProArt13, 2026-09-20] .NET SDK 10.0.401: `winget install --id Microsoft.DotNet.SDK.10`
- [DONE — ProArt13, 2026-09-20] `rustup toolchain install stable-x86_64-pc-windows-msvc` (the machine's default was the GNU stable; use MSVC for anything linking Windows libs)
- [DONE — ProArt13, 2026-09-20] uniffi C# generator, pinned (ADR-015):
  `cargo +stable-x86_64-pc-windows-msvc install --locked --git https://github.com/dennisameling/uniffi-bindgen-cs --rev 0fc022aa1d73fb1dda91a778b63f2824d7dca58b uniffi-bindgen-cs`
  (`--locked` is mandatory). Then `tools/gen-bindings-cs.sh` from Git Bash; make sure the cargo bin dir is on PATH in POSIX form (`/c/Users/<you>/.cargo/bin`).
- Run the spike: `cd apps/windows/spikes/bindings && dotnet run` (needs `cargo build -p gist-ffi` and the generated bindings first).
- Still to do: confirm the VS "WinUI application development" workload (needed from W1).

## Windows — W0 additions (2026-09-20)
- Visual Studio's "WinUI application development" / ".NET desktop" workloads are **optional** (designer, Hot Reload, debugger only). A WinUI 3 app builds with plain `dotnet build` using the `Microsoft.WindowsAppSDK` NuGet package (2.5.1). Installed here: only the Native Desktop C++ workload.
- **Pending, needs elevation:** enable Developer Mode (Settings > System > For developers) to install/run MSIX packages locally. Until then, packaged-app behaviour (LocalState paths, `gist_ffi.dll` load from a package) is unverified.
- Run the key-custody tests: `cd apps/windows/spikes/keyprovider/DpapiKeyProvider.Tests && dotnet test` (13 tests, real DPAPI).
- Run the WinUI hello spike: `cargo build -p gist-ffi`, `cd apps/windows/spikes/winui-hello && dotnet build -p:Platform=x64`, copy `target/debug/gist_ffi.dll` next to the built exe, run it (unpackaged; needs Windows App Runtime 2.x installed, present here).
