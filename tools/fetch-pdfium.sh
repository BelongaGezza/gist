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
# - The archive listing is validated before extraction (validate_archive): only
#   regular files and directories; no absolute, ".." or control-character paths,
#   no symlinks/hardlinks/devices, bounded entry count. Self-test:
#   tools/test-fetch-pdfium-guard.sh (drives --check-archive; no network/macOS).
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

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
    else sha256sum "$1" | awk '{print $1}'; fi
}

# Upper bound on archive members. The pinned asset has 45; this is a sanity
# ceiling against a pathological archive, not a tight fit. Overridable only so
# tools/test-fetch-pdfium-guard.sh can exercise the cap with a handful of
# fixture entries instead of building 500; the real fetch never sets it.
PDFIUM_MAX_ENTRIES="${PDFIUM_MAX_ENTRIES:-500}"

# Validate a .tar.gz BEFORE extracting it. Fails closed, naming the bad entry.
#
# Deliberately uses two listings rather than parsing one: `tar -tzf` gives clean
# one-name-per-line output for the path checks, and only the FIRST CHARACTER of
# each `tar -tvzf` line is read for the entry type. Parsing names out of the
# verbose listing is not portable (GNU and BSD tar differ, and a symlink line
# reads "name -> target"), and names may contain spaces.
validate_archive() {
    local archive="$1" names types count line name type

    if ! names="$(tar -tzf "$archive" 2>/dev/null)"; then
        echo "error: cannot list $archive (not a readable gzip tar archive)" >&2
        return 1
    fi
    if ! types="$(tar -tvzf "$archive" 2>/dev/null)"; then
        echo "error: cannot list $archive verbosely" >&2
        return 1
    fi

    count="$(printf '%s\n' "$names" | grep -c . || true)"
    if [ "$count" -eq 0 ]; then
        echo "error: $archive contains no entries" >&2
        return 1
    fi
    if [ "$count" -gt "$PDFIUM_MAX_ENTRIES" ]; then
        echo "error: $archive has $count entries, over the $PDFIUM_MAX_ENTRIES cap" >&2
        return 1
    fi

    while IFS= read -r name; do
        [ -n "$name" ] || continue
        case "$name" in
            /*)        echo "error: $archive: absolute path entry: $name" >&2; return 1 ;;
            ..|../*)   echo "error: $archive: path traversal entry: $name" >&2; return 1 ;;
            *..?*)     ;;   # a name may legitimately contain ".." inside a component (e.g. "a..b")
        esac
        case "$name" in
            */../*|*/..) echo "error: $archive: path traversal entry: $name" >&2; return 1 ;;
        esac
        # Reject a Windows drive-letter or UNC style absolute path, and backslashes.
        case "$name" in
            ?:[/\\]*|'\\\\'*|*\\*)
                echo "error: $archive: non-portable or absolute path entry: $name" >&2; return 1 ;;
        esac
        # Control characters (including an embedded newline, which would also
        # split this line-oriented listing) are never legitimate here.
        if printf '%s' "$name" | LC_ALL=C grep -q '[[:cntrl:]]'; then
            echo "error: $archive: control character in entry name" >&2
            return 1
        fi
    done <<EOF
$names
EOF

    while IFS= read -r line; do
        [ -n "$line" ] || continue
        type="${line:0:1}"
        case "$type" in
            -|d) ;;   # regular file, directory
            l)  echo "error: $archive: symlink entry rejected: $line" >&2; return 1 ;;
            h)  echo "error: $archive: hardlink entry rejected: $line" >&2; return 1 ;;
            b|c) echo "error: $archive: device node entry rejected: $line" >&2; return 1 ;;
            p)  echo "error: $archive: fifo entry rejected: $line" >&2; return 1 ;;
            s)  echo "error: $archive: socket entry rejected: $line" >&2; return 1 ;;
            *)  echo "error: $archive: unexpected entry type '$type': $line" >&2; return 1 ;;
        esac
        # GNU tar renders a hardlink as a regular-file mode line with " link to "
        # in it rather than an 'h' type char, so catch that spelling too.
        case "$line" in
            *" link to "*) echo "error: $archive: hardlink entry rejected: $line" >&2; return 1 ;;
            *" -> "*)      echo "error: $archive: symlink entry rejected: $line" >&2; return 1 ;;
        esac
    done <<EOF
$types
EOF

    echo "  archive validated: $count entries, no symlinks/hardlinks/devices, no absolute or traversing paths"
    return 0
}

# Test hook (mirrors --check-exclude-guard in tools/gen-bindings-cs.sh): validate
# an archive and exit, without downloading or extracting anything.
if [ "${1:-}" = "--check-archive" ]; then
    validate_archive "${2:?archive path required}"
    exit $?
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="$REPO_ROOT/artifacts/pdfium"
STAMP="$OUT_DIR/.pinned-sha256"

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

# Inspect the archive before writing a single file to disk.
echo "→ Validating archive contents before extraction..."
validate_archive "$TMP/$PDFIUM_ASSET"

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
