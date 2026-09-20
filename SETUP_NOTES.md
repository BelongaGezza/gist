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
  `cargo +stable-x86_64-pc-windows-msvc install --locked --git https://github.com/BelongaGezza/uniffi-bindgen-cs --rev 0fc022aa1d73fb1dda91a778b63f2824d7dca58b uniffi-bindgen-cs`
  (`--locked` is mandatory). Then `tools/gen-bindings-cs.sh` from Git Bash; make sure the cargo bin dir is on PATH in POSIX form (`/c/Users/<you>/.cargo/bin`).
- Run the spike: `cd apps/windows/spikes/bindings && dotnet run` (needs `cargo build -p gist-ffi` and the generated bindings first).
- Still to do: confirm the VS "WinUI application development" workload (needed from W1).

## Windows — W0 additions (2026-09-20)
- Visual Studio's "WinUI application development" / ".NET desktop" workloads are **optional** (designer, Hot Reload, debugger only). A WinUI 3 app builds with plain `dotnet build` using the `Microsoft.WindowsAppSDK` NuGet package (2.5.1). Installed here: only the Native Desktop C++ workload.
- **Pending, needs elevation:** enable Developer Mode (Settings > System > For developers) to install/run MSIX packages locally. Until then, packaged-app behaviour (LocalState paths, `gist_ffi.dll` load from a package) is unverified.
- Run the key-custody tests: `cd apps/windows/spikes/keyprovider/DpapiKeyProvider.Tests && dotnet test` (13 tests, real DPAPI).
- Run the WinUI hello spike: `cargo build -p gist-ffi`, `cd apps/windows/spikes/winui-hello && dotnet build -p:Platform=x64`, copy `target/debug/gist_ffi.dll` next to the built exe, run it (unpackaged; needs Windows App Runtime 2.x installed, present here).

## Windows — ARM64 (required for v1.0, review Q7)
- [TODO — needs UAC] Visual Studio Installer > Modify > Individual components > "MSVC v143 - VS 2022 C++ ARM64/ARM64EC build tools (Latest)" (and the ARM64 Windows SDK libs if offered). Rust target `aarch64-pc-windows-msvc` is already installed on ProArt13.
- Verify: `cargo build -p gist-ffi --release --target aarch64-pc-windows-msvc` succeeds, then `tools/check-dll-imports.sh` on the ARM64 DLL. Runtime testing needs ARM64 hardware or a `windows-11-arm` runner.

## Windows developer inner loop (W1)
From the repo root in Git Bash (Windows 11, MSVC Rust toolchain per `rust-toolchain.toml`):
1. Build + stage the core: `tools/build-core-windows.sh x64 debug` (or `release`; release also runs `tools/check-dll-imports.sh`). Output: `apps/windows/native/x64/gist_ffi.dll` (gitignored). `arm64` needs the VS component "MSVC v143 - VS 2022 C++ ARM64/ARM64EC build tools (Latest)"; the script says so and fails if it is missing.
2. One-time: install the pinned generator (`cargo +stable-x86_64-pc-windows-msvc install --locked --git https://github.com/BelongaGezza/uniffi-bindgen-cs --rev 0fc022aa1d73fb1dda91a778b63f2824d7dca58b uniffi-bindgen-cs`, add its `bin` to PATH). The script refuses any other rev.
3. Generate bindings: `tools/gen-bindings-cs.sh` -> `apps/windows/GIST.Core/Generated/` (gitignored; builds the x64 debug DLL if none is staged; `GIST_FFI_DLL=<path>` overrides the DLL). It fails if any `uniffi.toml` / `[bindings.csharp]` sets `exclude` (review F5; guard test: `tools/test-gen-bindings-cs-guard.sh`).
4. Optional reproducibility check: `tools/check-bindings-reproducible.sh`.
5. `cd apps/windows && dotnet build && dotnet test`.
