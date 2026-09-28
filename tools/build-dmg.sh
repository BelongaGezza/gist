#!/usr/bin/env bash
# Packages a built GISTmacOS.app into a distributable .dmg.
#
# Deliberately does NOT sign or notarise anything — it only arranges files on
# disk and calls `hdiutil create`, so it works against an unsigned/ad-hoc
# build (CODE_SIGNING_ALLOWED=NO) with no Developer ID identity present, which
# is exactly what this dev environment has (see CLAUDE.md's F10/N8 security
# register rows). The release-macos.yml workflow runs this *after* codesigning
# the .app for a real release; this script itself never touches signing.
#
# Usage: tools/build-dmg.sh <path-to.app> <output.dmg>
set -euo pipefail

APP_PATH="${1:?Usage: build-dmg.sh <path-to.app> <output.dmg>}"
DMG_PATH="${2:?Usage: build-dmg.sh <path-to.app> <output.dmg>}"

if [ ! -d "$APP_PATH" ]; then
    echo "ERROR: '$APP_PATH' does not exist or is not a directory (expected a .app bundle)" >&2
    exit 1
fi

if [[ "$APP_PATH" != *.app ]]; then
    echo "ERROR: '$APP_PATH' does not look like a .app bundle (must end in .app)" >&2
    exit 1
fi

VOLUME_NAME="$(basename "$APP_PATH" .app)"
STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/gist-dmg-staging.XXXXXX")"
trap 'rm -rf "$STAGING_DIR"' EXIT

echo "→ Staging DMG contents in $STAGING_DIR..."
# ditto (not cp -R) preserves the .app bundle's extended attributes/resource
# forks/symlinks exactly, which matters for a Mach-O bundle.
ditto "$APP_PATH" "$STAGING_DIR/$(basename "$APP_PATH")"

# Standard "drag to Applications" affordance.
ln -s /Applications "$STAGING_DIR/Applications"

mkdir -p "$(dirname "$DMG_PATH")"
rm -f "$DMG_PATH"

echo "→ Creating $DMG_PATH..."
hdiutil create \
    -volname "$VOLUME_NAME" \
    -srcfolder "$STAGING_DIR" \
    -fs HFS+ \
    -format UDZO \
    -ov \
    "$DMG_PATH"

echo "✓ DMG written to $DMG_PATH"
