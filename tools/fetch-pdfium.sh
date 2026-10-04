#!/usr/bin/env bash
# Fetch the pinned prebuilt pdfium (ADR-002 addendum, M6 R1).
#
# - Downloads one specific bblanchon/pdfium-binaries release asset (never
#   "latest"), verifies its pinned SHA-256, and FAILS CLOSED on any mismatch.
# - Extracts into artifacts/pdfium/ (gitignored): lib/libpdfium.dylib,
#   licenses/, LICENSE, VERSION. No binary is ever committed.
# - Idempotent: if artifacts/pdfium/.pinned-sha256 already matches, does nothing.
#
# To bump: change PDFIUM_TAG + PDFIUM_SHA256 together in a reviewed PR, using a
# hash you computed yourself from the downloaded asset (shasum -a 256), and
# re-run the pdfium-render compatibility check described in ADR-002.
set -euo pipefail

PDFIUM_TAG="chromium/8076"
PDFIUM_ASSET="pdfium-mac-univ.tgz"   # universal2 (arm64 + x86_64) dylib
PDFIUM_SHA256="3bdb93e229298dfdf083dc8ccc7d1a8cf87790b6917e5073335504fe2ff0bdc1"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="$REPO_ROOT/artifacts/pdfium"
STAMP="$OUT_DIR/.pinned-sha256"

if [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$PDFIUM_SHA256" ] && [ -f "$OUT_DIR/lib/libpdfium.dylib" ]; then
    echo "✓ pdfium $PDFIUM_TAG already present and verified ($OUT_DIR)"
    exit 0
fi

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
    else sha256sum "$1" | awk '{print $1}'; fi
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG//\//%2F}/$PDFIUM_ASSET"
echo "→ Downloading $PDFIUM_ASSET ($PDFIUM_TAG)..."
curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 -o "$TMP/$PDFIUM_ASSET" "$URL"

ACTUAL="$(sha256_of "$TMP/$PDFIUM_ASSET")"
if [ "$ACTUAL" != "$PDFIUM_SHA256" ]; then
    echo "error: SHA-256 mismatch for $PDFIUM_ASSET" >&2
    echo "  expected: $PDFIUM_SHA256" >&2
    echo "  actual:   $ACTUAL" >&2
    echo "Refusing to use an unverified pdfium binary." >&2
    exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
tar -xzf "$TMP/$PDFIUM_ASSET" -C "$OUT_DIR"
[ -f "$OUT_DIR/lib/libpdfium.dylib" ] || { echo "error: archive has no lib/libpdfium.dylib" >&2; rm -rf "$OUT_DIR"; exit 1; }
echo "$PDFIUM_SHA256" > "$STAMP"
echo "✓ pdfium $PDFIUM_TAG verified and extracted to $OUT_DIR"
