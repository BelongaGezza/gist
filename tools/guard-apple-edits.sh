#!/usr/bin/env bash
# PreToolUse hook (Write|Edit|NotebookEdit): blocks edits to Apple-only paths
# on non-macOS machines. Override for a deliberate, user-approved edit with
# GIST_ALLOW_APPLE_EDITS=1. Exit 2 = block; stderr is shown to Claude.
input="$(cat)"
[ "$(uname -s 2>/dev/null)" = Darwin ] && exit 0
[ "${GIST_ALLOW_APPLE_EDITS:-}" = 1 ] && exit 0

path="$(printf '%s' "$input" | sed -n 's/.*"\(file_path\|notebook_path\)" *: *"\([^"]*\)".*/\2/p' | head -1)"
path="$(printf '%s' "$path" | sed 's#\\\\#/#g; s#\\#/#g')"

case "$path" in
  */apps/apple/*|*/ios/*|*/macos/*|*.xcodeproj/*|*.xcworkspace/*|*.entitlements|*/Info.plist)
    echo "BLOCKED: '$path' is Apple-platform code and this is not a macOS session. Record the needed change in PENDING_APPLE_CHANGES.md instead (CLAUDE.md sec. 7). If the user explicitly instructed this edit, re-run with GIST_ALLOW_APPLE_EDITS=1." >&2
    exit 2 ;;
esac
exit 0
