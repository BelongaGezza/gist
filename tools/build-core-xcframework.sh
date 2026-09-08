#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ARTIFACTS_DIR="$REPO_ROOT/artifacts"
XCFRAMEWORK_PATH="$ARTIFACTS_DIR/GistCore.xcframework"

cd "$REPO_ROOT"

echo "→ Ensuring Rust cross-compilation targets are installed..."
rustup target add aarch64-apple-darwin 2>/dev/null || true
rustup target add x86_64-apple-darwin 2>/dev/null || true

echo "→ Building gist-ffi for aarch64-apple-darwin..."
cargo build -p gist-ffi --release --target aarch64-apple-darwin

echo "→ Building gist-ffi for x86_64-apple-darwin..."
cargo build -p gist-ffi --release --target x86_64-apple-darwin

# Create universal static library via lipo
AARCH64_LIB="$REPO_ROOT/target/aarch64-apple-darwin/release/libgist_ffi.a"
X86_64_LIB="$REPO_ROOT/target/x86_64-apple-darwin/release/libgist_ffi.a"
UNIVERSAL_LIB="$ARTIFACTS_DIR/libgist_ffi_universal.a"

mkdir -p "$ARTIFACTS_DIR"

echo "→ Creating universal binary with lipo..."
lipo -create \
    "$AARCH64_LIB" \
    "$X86_64_LIB" \
    -output "$UNIVERSAL_LIB"

# Generate Swift bindings (from the release build for the host arch)
ARCH=$(uname -m)
if [ "$ARCH" = "arm64" ]; then
    BINDGEN_LIB="$AARCH64_LIB"
else
    BINDGEN_LIB="$X86_64_LIB"
fi

echo "→ Generating Swift bindings..."
mkdir -p "$REPO_ROOT/apps/apple/Generated"
if command -v uniffi-bindgen &>/dev/null; then
    uniffi-bindgen generate "$BINDGEN_LIB" --language swift \
        --out-dir "$REPO_ROOT/apps/apple/Generated"
fi

SWIFT_HEADERS_DIR="$ARTIFACTS_DIR/headers"
mkdir -p "$SWIFT_HEADERS_DIR"

# Copy generated header if present (uniffi generates a .modulemap + .h)
if ls "$REPO_ROOT/apps/apple/Generated/"*.h &>/dev/null 2>&1; then
    cp "$REPO_ROOT/apps/apple/Generated/"*.h "$SWIFT_HEADERS_DIR/" 2>/dev/null || true
    cp "$REPO_ROOT/apps/apple/Generated/"*.modulemap "$SWIFT_HEADERS_DIR/module.modulemap" 2>/dev/null || true
fi

# Package as xcframework
echo "→ Creating xcframework..."
rm -rf "$XCFRAMEWORK_PATH"

HEADERS_ARG=""
if ls "$SWIFT_HEADERS_DIR/"*.h &>/dev/null 2>&1; then
    HEADERS_ARG="-headers $SWIFT_HEADERS_DIR"
fi

xcodebuild -create-xcframework \
    -library "$UNIVERSAL_LIB" \
    $HEADERS_ARG \
    -output "$XCFRAMEWORK_PATH"

echo "✓ xcframework written to $XCFRAMEWORK_PATH"
echo ""
echo "Next: open apps/apple/ with XcodeGen and add GistCore.xcframework to the target."
