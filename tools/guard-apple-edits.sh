#!/usr/bin/env bash
# PreToolUse hook (Write|Edit|NotebookEdit): a safety net that blocks edits to
# Apple-only paths on non-macOS machines. It is NOT the enforcement mechanism
# (CODEOWNERS + PR review are; edits via the Bash tool are not covered).
# Override for a deliberate, user-approved edit with GIST_ALLOW_APPLE_EDITS=1.
# Exit 2 = block; stderr is shown to Claude.
input="$(cat)"
os="${GIST_FAKE_OS:-$(uname -s 2>/dev/null)}"
[ "$os" = Darwin ] && exit 0
[ "${GIST_ALLOW_APPLE_EDITS:-}" = 1 ] && exit 0

path="$(printf '%s' "$input" | sed -n 's/.*"\(file_path\|notebook_path\)" *: *"\([^"]*\)".*/\2/p' | head -1)"
# Normalise: JSON-escaped and plain backslashes -> '/', lower-case, and always
# lead with '/' so relative paths (apps/apple/x) match the */dir/* patterns.
norm="$(printf '%s' "$path" | sed 's#\\\\#/#g; s#\\#/#g' | tr '[:upper:]' '[:lower:]')"
norm="/${norm#/}"

case "$norm" in
  */apps/apple/*|*/ios/*|*/macos/*|*.xcodeproj/*|*.xcworkspace/*|*.entitlements|*/info.plist)
    echo "BLOCKED: '$path' is Apple-platform code and this is not a macOS session. Record the needed change in PENDING_APPLE_CHANGES.md instead (CLAUDE.md sec. 7). If the user explicitly instructed this edit, re-run with GIST_ALLOW_APPLE_EDITS=1." >&2
    exit 2 ;;
esac
exit 0
