#!/usr/bin/env bash
# Fetch the pinned prebuilt pdfium (ADR-002 addendum, M6 R1).
#
# - Downloads one specific bblanchon/pdfium-binaries release asset (never
#   "latest"), verifies its pinned SHA-256, and FAILS CLOSED on any mismatch.
# - Extracts into artifacts/pdfium/ (gitignored): lib/libpdfium.dylib,
#   licenses/, LICENSE, VERSION. No binary is ever committed.
# - Idempotent: a stamp hit RE-HASHES the extracted dylib before trusting it.
#
# To bump: change PDFIUM_TAG + PDFIUM_SHA256 together in a reviewed PR, using a
# hash you computed yourself from the downloaded asset (shasum -a 256), and
# re-run the pdfium-render compatibility check described in ADR-002.
#
# Hardened 2026-10-04 for security review finding F34
# (docs/security-quality-review-2026-10-04.md). Three sub-findings, all fixed:
#
#   (a) The idempotence stamp used to short-circuit on "stamp matches the pinned
#       archive hash AND the dylib exists", without ever re-hashing the dylib, so
#       a locally replaced artifacts/pdfium/lib/libpdfium.dylib was embedded in a
#       build unchecked. The stamp now also records the extracted dylib's own
#       SHA-256, and a stamp hit re-hashes the file on disk and re-downloads on
#       any mismatch. NOTE the scope: that second hash is self-recorded at
#       extraction time, not independently pinned, so it detects tampering or
#       corruption AFTER a verified extraction. The pinned PDFIUM_SHA256 on the
#       archive is what establishes the binary's provenance in the first place;
#       this does not weaken or replace it.
#   (b) curl's --proto restricts only the INITIAL scheme; redirect hops were
#       unrestricted (the GitHub URL does 302 to release-assets.githubusercontent.com).
#       --proto-redir '=https' now constrains every hop, so a redirect cannot
#       downgrade to http/file/ftp mid-fetch.
#   (c) The archive was extracted with a plain `tar -xzf` and no pre-listing.
#       It is now listed and validated BEFORE anything is written to disk:
#       absolute paths, `..` components, symlinks, hardlinks, device/fifo/socket
#       entries, control characters in names, and an unreasonable entry count are
#       all rejected with a message naming the offending entry.
#
# Self-test: tools/test-fetch-pdfium-guard.sh (drives --check-archive against
# hand-built fixture tarballs; needs no network and no macOS).
set -euo pipefail

PDFIUM_TAG="chromium/8076"
PDFIUM_ASSET="pdfium-mac-univ.tgz"   # universal2 (arm64 + x86_64) dylib
PDFIUM_SHA256="3bdb93e229298dfdf083dc8ccc7d1a8cf87790b6917e5073335504fe2ff0bdc1"

# Upper bound on archive members. The pinned asset has 45; this is a sanity
# ceiling against a pathological archive, not a tight fit. Overridable only so
# tools/test-fetch-pdfium-guard.sh can exercise the cap with a handful of
# fixture entries instead of building 500; the real fetch never sets it.
PDFIUM_MAX_ENTRIES="${PDFIUM_MAX_ENTRIES:-500}"

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
    else sha256sum "$1" | awk '{print $1}'; fi
}

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
DYLIB="$OUT_DIR/lib/libpdfium.dylib"

# Stamp format (two lines): the pinned archive hash, then the extracted dylib's
# hash recorded at extraction time. A one-line stamp is a pre-F34 stamp and is
# treated as a miss, so an existing checkout re-verifies once rather than
# trusting a stamp written before the dylib was ever hashed.
if [ -f "$STAMP" ] && [ -f "$DYLIB" ]; then
    STAMPED_ARCHIVE="$(sed -n '1p' "$STAMP")"
    STAMPED_DYLIB="$(sed -n '2p' "$STAMP")"
    if [ "$STAMPED_ARCHIVE" = "$PDFIUM_SHA256" ] && [ -n "$STAMPED_DYLIB" ]; then
        CURRENT_DYLIB="$(sha256_of "$DYLIB")"
        if [ "$CURRENT_DYLIB" = "$STAMPED_DYLIB" ]; then
            echo "✓ pdfium $PDFIUM_TAG already present, dylib re-hashed and verified ($OUT_DIR)"
            exit 0
        fi
        echo "warning: $DYLIB does not match the hash recorded at extraction time." >&2
        echo "  recorded: $STAMPED_DYLIB" >&2
        echo "  on disk:  $CURRENT_DYLIB" >&2
        echo "Discarding it and re-fetching the pinned archive." >&2
    fi
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG//\//%2F}/$PDFIUM_ASSET"
echo "→ Downloading $PDFIUM_ASSET ($PDFIUM_TAG)..."
# --proto gates the initial request; --proto-redir gates every redirect hop.
curl --fail --silent --show-error --location \
     --proto '=https' --proto-redir '=https' --tlsv1.2 \
     -o "$TMP/$PDFIUM_ASSET" "$URL"

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
[ -f "$DYLIB" ] || { echo "error: archive has no lib/libpdfium.dylib" >&2; rm -rf "$OUT_DIR"; exit 1; }
{ echo "$PDFIUM_SHA256"; sha256_of "$DYLIB"; } > "$STAMP"
echo "✓ pdfium $PDFIUM_TAG verified and extracted to $OUT_DIR"
