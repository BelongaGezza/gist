#!/usr/bin/env bash
# Build gist-ffi for Windows and stage the DLL for the WinUI shell.
#
# Usage: tools/build-core-windows.sh <x64|arm64> <debug|release>
#   x64   -> x86_64-pc-windows-msvc   -> apps/windows/native/x64/gist_ffi.dll
#   arm64 -> aarch64-pc-windows-msvc  -> apps/windows/native/arm64/gist_ffi.dll
# Uses the toolchain pinned in rust-toolchain.toml. Release builds are checked with
# tools/check-dll-imports.sh (no dynamic VC++ runtime; see .cargo/config.toml, review Q2).
# Idempotent: safe to re-run; only overwrites the staged DLL.
set -euo pipefail

usage() { echo "usage: $0 <x64|arm64> <debug|release>" >&2; exit 2; }
[ $# -eq 2 ] || usage
arch="$1"; profile="$2"
case "$arch" in
  x64)   triple=x86_64-pc-windows-msvc ;;
  arm64) triple=aarch64-pc-windows-msvc ;;
  *) usage ;;
esac
case "$profile" in
  debug)   cargo_profile_args=() ;;
  release) cargo_profile_args=(--release) ;;
  *) usage ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

command -v cargo >/dev/null 2>&1 || { echo "build-core-windows: cargo not found on PATH (install rustup)" >&2; exit 1; }
command -v rustup >/dev/null 2>&1 || { echo "build-core-windows: rustup not found on PATH" >&2; exit 1; }

# Rust target for the pinned toolchain (rust-toolchain.toml). Never auto-install: report instead.
if ! rustup target list --installed 2>/dev/null | grep -qx "$triple"; then
  echo "build-core-windows: Rust target $triple is not installed for the pinned toolchain." >&2
  echo "  fix: rustup target add $triple   (run from the repo root so rust-toolchain.toml applies)" >&2
  exit 1
fi

arm_hint() {
  echo "  fix: Visual Studio Installer > Modify > Individual components > add" >&2
  echo "       'MSVC v143 - VS 2022 C++ ARM64/ARM64EC build tools (Latest)'" >&2
  echo "       (component id Microsoft.VisualStudio.Component.VC.Tools.ARM64; needs elevation)." >&2
}

# ARM64 additionally needs the MSVC ARM64 C++ tools (linker + CRT libs).
if [ "$arch" = arm64 ]; then
  vswhere=""
  if command -v vswhere >/dev/null 2>&1; then
    vswhere="vswhere"
  else
    pf86="$(cmd.exe //c "echo %ProgramFiles(x86)%" 2>/dev/null | tr -d '\r' || true)"
    if [ -n "$pf86" ] && [ -f "$pf86/Microsoft Visual Studio/Installer/vswhere.exe" ]; then
      vswhere="$pf86/Microsoft Visual Studio/Installer/vswhere.exe"
    fi
  fi
  if [ -n "$vswhere" ]; then
    found="$("$vswhere" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath 2>/dev/null | tr -d '\r' || true)"
    if [ -z "$found" ]; then
      echo "build-core-windows: the MSVC ARM64 C++ build tools are not installed." >&2
      arm_hint
      exit 1
    fi
  else
    echo "build-core-windows: vswhere not found; cannot pre-check ARM64 tools, attempting the build anyway." >&2
  fi
fi

echo "==> cargo build -p gist-ffi --target $triple ($profile)"
if ! cargo build -p gist-ffi --target "$triple" ${cargo_profile_args[@]+"${cargo_profile_args[@]}"}; then
  echo "build-core-windows: cargo build failed for $triple ($profile)." >&2
  if [ "$arch" = arm64 ]; then echo "  (if the error is a missing linker/libs, the ARM64 MSVC tools are the usual cause)" >&2; arm_hint; fi
  exit 1
fi

src="target/$triple/$profile/gist_ffi.dll"
[ -f "$src" ] || { echo "build-core-windows: expected $src was not produced" >&2; exit 1; }
dest_dir="apps/windows/native/$arch"
mkdir -p "$dest_dir"
cp -f "$src" "$dest_dir/gist_ffi.dll"
echo "staged $dest_dir/gist_ffi.dll"

if [ "$profile" = release ]; then
  bash tools/check-dll-imports.sh "$dest_dir/gist_ffi.dll"
fi
