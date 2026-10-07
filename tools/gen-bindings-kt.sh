#!/usr/bin/env bash
# Generate the uniffi Kotlin bindings for gist-ffi (Android shell, see docs/android-development-plan.md
# and docs/adr/022-android-ffi-binding.md).
#
# Usage: tools/gen-bindings-kt.sh [--no-format] [out-dir]
#   out-dir default: apps/android/core/src/main/java
#   GIST_FFI_SO=<path>  generate from this .so instead of target/debug/libgist_ffi.so
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

OUT_DIR="${1:-$REPO_ROOT/apps/android/core/src/main/java}"
SO_PATH="${GIST_FFI_SO:-$REPO_ROOT/target/debug/libgist_ffi.so}"

# Ensure uniffi-bindgen is built
if [ ! -f "$REPO_ROOT/target/debug/uniffi-bindgen" ]; then
    echo "→ Building uniffi-bindgen CLI from gist-ffi..."
    cargo build -p gist-ffi --bin uniffi-bindgen --features uniffi-bindgen-bin
fi

# Ensure gist-ffi cdylib is built
if [ ! -f "$SO_PATH" ]; then
    echo "→ Building gist-ffi (debug)..."
    cargo build -p gist-ffi
fi

echo "→ Generating Kotlin bindings from $SO_PATH..."
mkdir -p "$OUT_DIR"

"$REPO_ROOT/target/debug/uniffi-bindgen" generate "$SO_PATH" \
    --language kotlin \
    --out-dir "$OUT_DIR" \
    --no-format

echo "✓ Kotlin bindings written to $OUT_DIR/"
