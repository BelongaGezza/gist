#!/usr/bin/env bash
# Proves a Rust panic crossing the uniffi FFI boundary surfaces in Swift as
# `GistError.InternalPanic`, not a crash -- the Swift-side half of the
# `ffi_catch!`/panic-containment guarantee (F22, and the 2026-09-20
# panic="abort" -> "unwind" release-profile fix). The Rust-only half is
# already covered on every platform by:
#   cargo run --release -p gist-ffi --features test-panic --example panic_containment
#
# This script goes one step further: it builds a debug AND a release
# gist-ffi with the test-only `test-panic` feature (never enabled in a
# shipped build -- see `crates/gist-ffi/src/lib.rs`'s `test_support` module),
# regenerates Swift bindings from each into a scratch directory, compiles a
# tiny standalone Swift program against them with `swiftc`, and runs it.
#
# Not wired into CI or the normal Xcode project on purpose: the probe symbol
# must never appear in the xcframework `apple-build`/dev builds actually
# link, so this has to build its own throwaway dylib+bindings rather than
# reuse the checked-in build. Run by hand after touching anything in
# `ffi_catch!`'s path (the panic hook, the release panic profile, uniffi
# error mapping).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PROFILE="${1:-release}" # release (default) or debug
case "$PROFILE" in
release) CARGO_FLAG=(--release); LIB_DIR="target/release" ;;
debug) CARGO_FLAG=(); LIB_DIR="target/debug" ;;
*)
    echo "usage: $0 [release|debug]" >&2
    exit 1
    ;;
esac

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

echo "→ Building gist-ffi ($PROFILE, features=test-panic)..."
cargo build "${CARGO_FLAG[@]}" -p gist-ffi --features test-panic

echo "→ Generating Swift bindings into $SCRATCH..."
cargo run -p gist-ffi --features uniffi-bindgen-bin --bin uniffi-bindgen -- \
    generate "$LIB_DIR/libgist_ffi.dylib" --language swift --out-dir "$SCRATCH"

cat >"$SCRATCH/main.swift" <<'EOF'
import Foundation

do {
    try ffiPanicProbeUniffi()
    print("FAIL: expected ffiPanicProbeUniffi() to throw, but it returned normally")
    exit(1)
} catch GistError.InternalPanic {
    print("ok: Swift caught GistError.InternalPanic across the uniffi boundary")
    exit(0)
} catch {
    print("FAIL: expected GistError.InternalPanic, got \(error)")
    exit(1)
}
EOF

echo "→ Compiling and running the Swift probe..."
swiftc -I "$SCRATCH" \
    -Xcc -fmodule-map-file="$SCRATCH/gist_ffiFFI.modulemap" \
    -L "$LIB_DIR" -lgist_ffi \
    "$SCRATCH/gist_ffi.swift" "$SCRATCH/main.swift" \
    -o "$SCRATCH/panic_probe_test"

DYLD_LIBRARY_PATH="$REPO_ROOT/$LIB_DIR" "$SCRATCH/panic_probe_test"

echo "→ Rebuilding gist-ffi ($PROFILE) without test-panic to restore the normal build..."
cargo build "${CARGO_FLAG[@]}" -p gist-ffi
