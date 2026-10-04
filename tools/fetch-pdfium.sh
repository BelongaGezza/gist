#!/usr/bin/env bash
# Fetch the pinned prebuilt pdfium (ADR-002 addendum, M6 R1).
#
# - Downloads one specific bblanchon/pdfium-binaries release asset (never
#   "latest"), verifies its pinned SHA-256, and FAILS CLOSED on any mismatch.
# - Extracts into artifacts/pdfium/ (gitignored): lib/libpdfium.dylib,
#   licenses/, LICENSE, VERSION. No binary is ever committed.
# - Idempotent: if the extracted libpdfium.dylib still hashes to the pinned
#   PDFIUM_DYLIB_SHA256 (re-computed on every run, not just trusted from a stamp
#   file), does nothing; otherwise it re-fetches and re-verifies.
# - The archive listing is checked before extraction: only regular files and
#   directories, no absolute or ".." paths (no symlinks/hardlinks/devices).
#
# To bump: change PDFIUM_TAG, PDFIUM_SHA256 and PDFIUM_DYLIB_SHA256 together in
# a reviewed PR, using hashes you computed yourself (shasum -a 256) from the
# downloaded asset and from the lib/libpdfium.dylib extracted out of it, and
# re-run the pdfium-render compatibility check described in ADR-002.
set -euo pipefail

PDFIUM_TAG="chromium/8076"
PDFIUM_ASSET="pdfium-mac-univ.tgz"   # universal2 (arm64 + x86_64) dylib
PDFIUM_SHA256="3bdb93e229298dfdf083dc8ccc7d1a8cf87790b6917e5073335504fe2ff0bdc1"
# SHA-256 of lib/libpdfium.dylib inside that (already verified) archive. Pinned
# here, in git, so a tampered on-disk copy cannot vouch for itself via a stamp.
PDFIUM_DYLIB_SHA256="3ed692213ade3cdab960198466adf1534d68632a4bfc1756f288b056939af1f4"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="$REPO_ROOT/artifacts/pdfium"
STAMP="$OUT_DIR/.pinned-sha256"

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
    else sha256sum "$1" | awk '{print $1}'; fi
}

# Re-hash the real on-disk dylib every time (cheap: ~15 MB) rather than trusting
# the stamp alone, so a modified or truncated artifacts/pdfium is never reused.
if [ -f "$OUT_DIR/lib/libpdfium.dylib" ]; then
    if [ "$(sha256_of "$OUT_DIR/lib/libpdfium.dylib")" = "$PDFIUM_DYLIB_SHA256" ]; then
        echo "$PDFIUM_SHA256" > "$STAMP"
        echo "✓ pdfium $PDFIUM_TAG already present and verified ($OUT_DIR)"
        exit 0
    fi
    echo "! existing $OUT_DIR/lib/libpdfium.dylib does not match the pinned hash; re-fetching" >&2
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG//\//%2F}/$PDFIUM_ASSET"
echo "→ Downloading $PDFIUM_ASSET ($PDFIUM_TAG)..."
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 -o "$TMP/$PDFIUM_ASSET" "$URL"

ACTUAL="$(sha256_of "$TMP/$PDFIUM_ASSET")"
if [ "$ACTUAL" != "$PDFIUM_SHA256" ]; then
    echo "error: SHA-256 mismatch for $PDFIUM_ASSET" >&2
    echo "  expected: $PDFIUM_SHA256" >&2
    echo "  actual:   $ACTUAL" >&2
    echo "Refusing to use an unverified pdfium binary." >&2
    exit 1
fi

# Inspect the archive before extracting anything: refuse absolute or ".."
# paths and any entry that is not a regular file or directory (symlinks,
# hardlinks, devices), so extraction can only ever write under $OUT_DIR.
if tar -tzf "$TMP/$PDFIUM_ASSET" | grep -q -E '(^/|(^|/)\.\.(/|$))'; then
    echo "error: archive contains an absolute or parent-relative path; refusing to extract" >&2
    exit 1
fi
if tar -tvzf "$TMP/$PDFIUM_ASSET" | cut -c1 | grep -q -v -E '^[-d]$'; then
    echo "error: archive contains a link or special file entry; refusing to extract" >&2
    exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
tar -xzf "$TMP/$PDFIUM_ASSET" -C "$OUT_DIR"
[ -f "$OUT_DIR/lib/libpdfium.dylib" ] || { echo "error: archive has no lib/libpdfium.dylib" >&2; rm -rf "$OUT_DIR"; exit 1; }
if [ "$(sha256_of "$OUT_DIR/lib/libpdfium.dylib")" != "$PDFIUM_DYLIB_SHA256" ]; then
    echo "error: extracted libpdfium.dylib does not match the pinned PDFIUM_DYLIB_SHA256" >&2
    rm -rf "$OUT_DIR"
    exit 1
fi
echo "$PDFIUM_SHA256" > "$STAMP"
echo "✓ pdfium $PDFIUM_TAG verified and extracted to $OUT_DIR"
