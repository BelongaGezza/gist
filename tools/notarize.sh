#!/usr/bin/env bash
# Notarisation helper — called by the release-macos CI workflow.
# Requires: APPLE_ID, APPLE_TEAM_ID, APPLE_APP_SPECIFIC_PASSWORD env vars (set as repo secrets).
set -euo pipefail

DMG_PATH="${1:?Usage: notarize.sh <path-to.dmg>}"
BUNDLE_ID="com.gist.macos"

echo "→ Submitting $DMG_PATH for notarisation..."
xcrun notarytool submit "$DMG_PATH" \
    --apple-id "$APPLE_ID" \
    --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --wait \
    --timeout 30m

echo "→ Stapling..."
xcrun stapler staple "$DMG_PATH"
echo "✓ Notarised and stapled: $DMG_PATH"
