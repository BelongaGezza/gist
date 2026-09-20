#!/usr/bin/env bash
# PreToolUse hook (Write|Edit|NotebookEdit): WARN-ONLY guard for Windows-only
# paths (apps/windows/**) on macOS sessions. Text edits are fine, but C#/XAML
# cannot be built or tested on macOS, so the edit must be logged in
# PENDING_WINDOWS_CHANGES.md for verification on Windows. Always exits 0;
# stderr is shown to Claude. Silent on Windows/Linux.
# Test seam: GIST_FAKE_OS=macos|windows|linux overrides uname (inert if unset).
input="$(cat)"
case "${GIST_FAKE_OS:-$(uname -s 2>/dev/null)}" in
  Darwin|macos) ;;
  *) exit 0 ;;
esac

path="$(printf '%s' "$input" | sed -n 's/.*"\(file_path\|notebook_path\)" *: *"\([^"]*\)".*/\2/p' | head -1)"
path="$(printf '%s' "$path" | sed 's#\\\\#/#g; s#\\#/#g')"

case "$path" in
  */apps/windows/*)
    echo "WARNING: '$path' is Windows-only code (WinUI 3/C#/XAML); it cannot be built or tested in a macOS session. The edit is allowed, but record what needs building/verifying on Windows in PENDING_WINDOWS_CHANGES.md (## Pending Windows Change)." >&2 ;;
esac
exit 0
