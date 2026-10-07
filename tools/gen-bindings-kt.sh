#!/usr/bin/env bash
# Generate the uniffi Kotlin bindings for gist-ffi (Android shell, see docs/android-development-plan.md
# and docs/adr/025-android-ffi-binding.md).
#
# Usage: tools/gen-bindings-kt.sh [--no-format] [out-dir]
#   out-dir default: apps/android/core/src/main/java
#   GIST_FFI_SO=<path>  generate from this .so instead of target/debug/libgist_ffi.so
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

OUT_DIR="$REPO_ROOT/apps/android/core/src/main/java"
OUT_DIR_SET=0
NO_FORMAT=0
while (($# > 0)); do
    case "$1" in
        --no-format)
            NO_FORMAT=1
            ;;
        -h|--help)
            sed -n '2,5p' "$0"
            exit 0
            ;;
        -*)
            echo "Unknown option: $1" >&2
            exit 2
            ;;
        *)
            if [ "$OUT_DIR_SET" -eq 1 ]; then
                echo "Only one output directory may be specified." >&2
                exit 2
            fi
            OUT_DIR="$1"
            OUT_DIR_SET=1
            ;;
    esac
    shift
done

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

FORMAT_ARGS=()
if [ "$NO_FORMAT" -eq 1 ]; then
    FORMAT_ARGS+=(--no-format)
fi

"$REPO_ROOT/target/debug/uniffi-bindgen" generate "$SO_PATH" \
    --language kotlin \
    --out-dir "$OUT_DIR" \
    "${FORMAT_ARGS[@]}"

echo "✓ Kotlin bindings written to $OUT_DIR/"
