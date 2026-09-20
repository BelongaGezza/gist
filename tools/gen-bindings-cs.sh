#!/usr/bin/env bash
# Generate the uniffi C# bindings for gist-ffi (Windows shell, see docs/windows-development-plan.md
# and docs/adr/015-windows-ffi-binding.md).
#
# Usage: tools/gen-bindings-cs.sh [out-dir]   (default: apps/windows/spikes/bindings/Generated)
# Needs: MSVC Rust toolchain, and uniffi-bindgen-cs installed at the pinned revision below.
#   cargo +stable-x86_64-pc-windows-msvc install --locked \
#     --git https://github.com/BelongaGezza/uniffi-bindgen-cs --rev "$BINDGEN_REV" uniffi-bindgen-cs
# (--locked is required: without it cargo pulls a second `toml` major and the generator fails to compile.)
set -euo pipefail
# Source is the project-controlled mirror BelongaGezza/uniffi-bindgen-cs (fork of dennisameling/…; ref gist-pin, tag gist-pin-0fc022a).
# A git rev is content-addressed, so the mirror yields byte-identical source to the original fork; it only removes the risk of the outside fork vanishing.
BINDGEN_REV=0fc022aa1d73fb1dda91a778b63f2824d7dca58b   # PR #176, uniffi 0.32 support (unmerged upstream)
root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/apps/windows/spikes/bindings/Generated}"
cd "$root"
cargo build -p gist-ffi
mkdir -p "$out"
# --no-format: do not run csharpier from PATH (docs/security-review-windows-bindgen.md F3)
uniffi-bindgen-cs --no-format --library target/debug/gist_ffi.dll --out-dir "$out"
echo "Bindings written to $out (pinned generator rev $BINDGEN_REV)"
