#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GENERATED_DIR="$REPO_ROOT/apps/apple/Generated"

cd "$REPO_ROOT"

# Build the library for the host architecture (development builds)
echo "→ Building gist-ffi (debug)..."
cargo build -p gist-ffi 2>&1

# Detect host arch
ARCH=$(uname -m)
if [ "$ARCH" = "arm64" ]; then
    TARGET="aarch64-apple-darwin"
else
    TARGET="x86_64-apple-darwin"
fi

LIB_PATH="$REPO_ROOT/target/debug/libgist_ffi.dylib"
if [ ! -f "$LIB_PATH" ]; then
    # Try .a (staticlib)
    LIB_PATH="$REPO_ROOT/target/debug/libgist_ffi.a"
fi

echo "→ Generating Swift bindings from $LIB_PATH..."
mkdir -p "$GENERATED_DIR"

# uniffi 0.28+ no longer ships uniffi-bindgen on crates.io; gist-ffi builds its
# own via the "uniffi-bindgen-bin" feature (see crates/gist-ffi/uniffi-bindgen.rs).
# Install once with a toolchain that supports uniffi_bindgen's MSRV (newer than
# this repo's pinned rust-toolchain.toml), e.g.:
#   cargo +stable install --path crates/gist-ffi --bin uniffi-bindgen \
#       --features uniffi-bindgen-bin --root ~/.cargo-uniffi-bindgen
# then add ~/.cargo-uniffi-bindgen/bin to PATH.
if command -v uniffi-bindgen &>/dev/null; then
    uniffi-bindgen generate "$LIB_PATH" --language swift --out-dir "$GENERATED_DIR"
elif cargo uniffi-bindgen --help &>/dev/null 2>&1; then
    cargo uniffi-bindgen generate "$LIB_PATH" --language swift --out-dir "$GENERATED_DIR"
else
    echo "uniffi-bindgen not found. See comment above this line for install steps."
    echo "Skipping binding generation — using existing generated files if present."
    exit 0
fi

echo "✓ Swift bindings written to $GENERATED_DIR/"
ls "$GENERATED_DIR/"
