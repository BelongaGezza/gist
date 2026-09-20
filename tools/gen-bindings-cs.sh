#!/usr/bin/env bash
# Generate the uniffi C# bindings for gist-ffi (Windows shell, see docs/windows-development-plan.md
# and docs/adr/015-windows-ffi-binding.md).
#
# Usage: tools/gen-bindings-cs.sh [--no-format] [out-dir]
#   out-dir default: apps/windows/GIST.Core/Generated
#   GIST_FFI_DLL=<path>  generate from this DLL instead of the staged apps/windows/native/x64/gist_ffi.dll
#                        (if the default is absent it is built with tools/build-core-windows.sh x64 debug)
#   --no-format          accepted for compatibility; formatting is ALWAYS disabled (review F3: never runs csharpier from PATH)
#   --check-exclude-guard <dir>   only run the review-F5 guard against <dir> (used by tools/test-gen-bindings-cs-guard.sh)
# Needs: MSVC Rust toolchain, and uniffi-bindgen-cs installed at the pinned revision below:
#   cargo +stable-x86_64-pc-windows-msvc install --locked \
#     --git https://github.com/BelongaGezza/uniffi-bindgen-cs --rev "$BINDGEN_REV" uniffi-bindgen-cs
# (--locked is required: without it cargo pulls a second `toml` major and the generator fails to compile.)
set -euo pipefail
# Source is the project-controlled mirror BelongaGezza/uniffi-bindgen-cs (fork of dennisameling/...; ref gist-pin, tag gist-pin-0fc022a).
# A git rev is content-addressed, so the mirror yields byte-identical source to the original fork; it only removes the risk of the outside fork vanishing.
BINDGEN_REV=0fc022aa1d73fb1dda91a778b63f2824d7dca58b   # PR #176, uniffi 0.32 support (unmerged upstream)
BINDGEN_URL=https://github.com/BelongaGezza/uniffi-bindgen-cs

# Review F5: `[bindings.csharp] exclude` shifts callback-interface vtable slots silently. Never allow it.
# Returns 1 if any uniffi.toml sets `exclude`, or any toml with a csharp bindings section does.
check_exclude_guard() {
  local dir="$1" f bad=0
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    if [ "$(basename "$f")" = uniffi.toml ] || grep -q 'bindings\.csharp' "$f"; then
      if grep -Eq '^[[:space:]]*(bindings\.csharp\.)?exclude[[:space:]]*=' "$f"; then
        echo "gen-bindings-cs: FORBIDDEN 'exclude' in $f (docs/security-review-windows-bindgen.md F5)" >&2
        bad=1
      fi
    fi
  done < <(find "$dir" \( -name target -o -name .git -o -name node_modules -o -name worktrees -o -name bin -o -name obj \) -prune -o -name '*.toml' -type f -print)
  return $bad
}

if [ "${1:-}" = "--check-exclude-guard" ]; then
  check_exclude_guard "${2:?dir required}"
  exit $?
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

args=()
for a in "$@"; do
  case "$a" in
    --no-format) ;;   # always on
    *) args+=("$a") ;;
  esac
done
out="${args[0]:-$root/apps/windows/GIST.Core/Generated}"

check_exclude_guard "$root" || exit 1

# Verify the installed generator is the pinned rev.
gen="$(command -v uniffi-bindgen-cs || true)"
install_hint="install it: cargo +stable-x86_64-pc-windows-msvc install --locked --git $BINDGEN_URL --rev $BINDGEN_REV uniffi-bindgen-cs"
[ -n "$gen" ] || { echo "gen-bindings-cs: uniffi-bindgen-cs not on PATH; $install_hint" >&2; exit 1; }
# `cargo install` records the source rev in <root>/.crates.toml, where <root> is the parent of bin/.
croot="$(cd "$(dirname "$gen")/.." && pwd)"
recorded=""
[ -f "$croot/.crates.toml" ] && recorded="$(grep -F 'uniffi-bindgen-cs ' "$croot/.crates.toml" || true)"
[ -n "$recorded" ] || recorded="$(cargo install --list 2>/dev/null | grep -F 'uniffi-bindgen-cs v' || true)"
if ! printf '%s' "$recorded" | grep -qF "$BINDGEN_REV"; then
  echo "gen-bindings-cs: installed uniffi-bindgen-cs is not the pinned rev $BINDGEN_REV" >&2
  echo "  found: ${recorded:-<no install record next to $gen>}" >&2
  echo "  $install_hint" >&2
  exit 1
fi

dll="${GIST_FFI_DLL:-$root/apps/windows/native/x64/gist_ffi.dll}"
if [ ! -f "$dll" ]; then
  if [ -n "${GIST_FFI_DLL:-}" ]; then echo "gen-bindings-cs: GIST_FFI_DLL=$dll not found" >&2; exit 1; fi
  bash "$root/tools/build-core-windows.sh" x64 debug
fi

mkdir -p "$out"
# --no-format: do not run csharpier from PATH (docs/security-review-windows-bindgen.md F3)
"$gen" --no-format --library "$dll" --out-dir "$out"
echo "Bindings written to $out (pinned generator rev $BINDGEN_REV)"
