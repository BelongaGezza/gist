#!/usr/bin/env bash
# Unit test for the pre-extraction archive guard in tools/fetch-pdfium.sh
# (security review finding F34c). Temp fixtures only: no network, no macOS, and
# nothing under artifacts/ is touched.
#
# Symlink, hardlink and device fixtures are built by writing ustar headers byte
# by byte rather than by creating real symlinks, because a Windows dev machine
# without Developer Mode cannot create one (`ln -s` fails with EPERM) and the
# guard must still be testable there.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"; trap 'chmod -R u+rwx "$tmp" 2>/dev/null || true; rm -rf "$tmp"' EXIT

guard() { bash "$root/tools/fetch-pdfium.sh" --check-archive "$1" >/dev/null 2>&1; }

fail=0
ok()   { echo "  ok   - $1"; }
bad()  { echo "  FAIL - $1"; fail=1; }

# expect_pass <tarball> <description>
expect_pass() { if guard "$1"; then ok "$2"; else bad "$2 (guard rejected a clean archive)"; fi; }
# expect_fail <tarball> <description>
expect_fail() { if guard "$1"; then bad "$2 (guard ACCEPTED it)"; else ok "$2"; fi; }

# ── ustar writer ────────────────────────────────────────────────────────────
# write_entry <outfile> <name> <typeflag> <linkname> <content>
#   typeflag: 0 regular, 2 symlink, 1 hardlink, 3 char device, 5 directory
# Field layout per POSIX ustar; the checksum is the unsigned sum of all 512
# header bytes with the checksum field itself filled with spaces.
nuls() { head -c "$1" /dev/zero; }

write_entry() {
    local out="$1" name="$2" typeflag="$3" linkname="$4" content="$5"
    local size hdr sum
    size=${#content}
    hdr="$tmp/.hdr.$$"

    # $1 = "-" for the spaces pass, otherwise the numeric checksum. The checksum
    # field must be emitted here, not interpolated from a command substitution:
    # it contains a NUL byte, which $( ) silently drops.
    emit_header() {
        {
            printf '%s' "$name";                     nuls $((100 - ${#name}))
            # mode: 0755 for a directory (typeflag 5), 0644 otherwise. A
            # directory header carrying 0644 extracts on POSIX hosts into a
            # directory with no search bit, so the sanity extraction below
            # fails with "Permission denied" and the temp tree cannot be
            # removed (R-final review, 2026-10-05).
            if [ "$typeflag" = "5" ]; then printf '%07o\0' 493
            else printf '%07o\0' 420; fi
            printf '%07o\0' 0                        # uid
            printf '%07o\0' 0                        # gid
            printf '%011o\0' "$size"                 # size
            printf '%011o\0' 0                       # mtime
            if [ "$1" = "-" ]; then printf '        '    # 8 spaces for the summing pass
            else printf '%06o\0 ' "$1"; fi            # chksum: 6 octal, NUL, space
            printf '%s' "$typeflag"                  # typeflag
            printf '%s' "$linkname";                 nuls $((100 - ${#linkname}))
            printf 'ustar\000'                       # magic
            printf '00'                              # version
            nuls 32                                  # uname
            nuls 32                                  # gname
            printf '%07o\0' 0                        # devmajor
            printf '%07o\0' 0                        # devminor
            nuls 155                                 # prefix
            nuls 12                                  # pad to 512
        }
    }

    emit_header - > "$hdr"
    sum=$(LC_ALL=C od -An -tu1 -v < "$hdr" | awk '{for(i=1;i<=NF;i++) s+=$i} END{print s}')
    emit_header "$sum" >> "$out"

    if [ "$size" -gt 0 ]; then
        printf '%s' "$content" >> "$out"
        nuls $(( (512 - size % 512) % 512 )) >> "$out"
    fi
}

# build_tgz <outfile.tgz> then entries passed as name:type:linkname:content tuples
build_tgz() {
    local outtgz="$1"; shift
    local raw="$tmp/.raw.$$"
    : > "$raw"
    local spec name type link content
    for spec in "$@"; do
        IFS='|' read -r name type link content <<< "$spec"
        write_entry "$raw" "$name" "$type" "$link" "$content"
    done
    nuls 1024 >> "$raw"          # two zero blocks terminate a tar
    gzip -c < "$raw" > "$outtgz"
    rm -f "$raw"
}

echo "fetch-pdfium archive guard:"

# ── 1. clean archive, shaped like the real asset ────────────────────────────
build_tgz "$tmp/clean.tgz" \
    'lib/|5||' \
    'lib/libpdfium.dylib|0||fake-dylib-bytes' \
    'licenses/|5||' \
    'LICENSE|0||MIT-ish' \
    'VERSION|0||8076'
expect_pass "$tmp/clean.tgz" "clean archive accepted"

# Sanity: the hand-built tar is real and extracts to what we expect, so a later
# "accepted" result cannot be an artefact of tar silently failing to read it.
mkdir -p "$tmp/x"
tar -xzf "$tmp/clean.tgz" -C "$tmp/x"
if [ "$(cat "$tmp/x/lib/libpdfium.dylib")" = "fake-dylib-bytes" ]; then
    ok "fixture writer produces a genuinely readable tar"
else
    bad "fixture writer produced a tar that does not extract correctly"
fi

# ── 2. symlink entry ────────────────────────────────────────────────────────
build_tgz "$tmp/symlink.tgz" \
    'lib/libpdfium.dylib|0||ok' \
    'lib/evil|2|/etc/passwd|'
expect_fail "$tmp/symlink.tgz" "symlink entry rejected"

# ── 3. hardlink entry ───────────────────────────────────────────────────────
build_tgz "$tmp/hardlink.tgz" \
    'lib/libpdfium.dylib|0||ok' \
    'lib/hard|1|lib/libpdfium.dylib|'
expect_fail "$tmp/hardlink.tgz" "hardlink entry rejected"

# ── 4. path traversal ───────────────────────────────────────────────────────
build_tgz "$tmp/dotdot.tgz" \
    'lib/libpdfium.dylib|0||ok' \
    '../../../../tmp/pwned|0||x'
expect_fail "$tmp/dotdot.tgz" "leading ../ traversal rejected"

build_tgz "$tmp/dotdot-mid.tgz" \
    'lib/libpdfium.dylib|0||ok' \
    'lib/../../../pwned|0||x'
expect_fail "$tmp/dotdot-mid.tgz" "embedded /../ traversal rejected"

# ── 5. absolute path ────────────────────────────────────────────────────────
build_tgz "$tmp/abs.tgz" \
    'lib/libpdfium.dylib|0||ok' \
    '/etc/cron.d/pwned|0||x'
expect_fail "$tmp/abs.tgz" "absolute path entry rejected"

# ── 6. device node ──────────────────────────────────────────────────────────
build_tgz "$tmp/dev.tgz" \
    'lib/libpdfium.dylib|0||ok' \
    'lib/zero|3||'
expect_fail "$tmp/dev.tgz" "character device entry rejected"

# ── 7. a name that merely CONTAINS dots must still be accepted ──────────────
build_tgz "$tmp/dots.tgz" \
    'lib/libpdfium.dylib|0||ok' \
    'lib/libpdfium..so.1|0||x' \
    'include/fpdf_doc.h|0||x'
expect_pass "$tmp/dots.tgz" "dots inside a component still accepted (no false positive)"

# ── 8. entry-count cap ──────────────────────────────────────────────────────
# Exercised with a lowered cap rather than by building 500+ entries: the bash
# ustar writer is slow, and the comparison under test is the same either way.
build_tgz "$tmp/many.tgz" 'a|0||x' 'b|0||x' 'c|0||x' 'd|0||x' 'e|0||x'
if PDFIUM_MAX_ENTRIES=3 guard "$tmp/many.tgz"; then
    bad "over-the-cap entry count rejected (guard ACCEPTED it)"
else
    ok "over-the-cap entry count rejected"
fi
if PDFIUM_MAX_ENTRIES=10 guard "$tmp/many.tgz"; then
    ok "same archive accepted under a cap that allows it"
else
    bad "same archive accepted under a cap that allows it (guard rejected it)"
fi

# ── 9. not an archive at all ────────────────────────────────────────────────
printf 'this is not a tarball' > "$tmp/garbage.tgz"
expect_fail "$tmp/garbage.tgz" "non-archive input rejected"

# ── 10. the guard names the offending entry ─────────────────────────────────
msg="$(bash "$root/tools/fetch-pdfium.sh" --check-archive "$tmp/symlink.tgz" 2>&1 || true)"
case "$msg" in
    *"lib/evil"*) ok "rejection message names the offending entry" ;;
    *)            bad "rejection message does not name the offending entry: $msg" ;;
esac

if [ "$fail" -ne 0 ]; then
    echo "fetch-pdfium archive guard: FAILURES"
    exit 1
fi
echo "ok: fetch-pdfium archive guard tests passed"
