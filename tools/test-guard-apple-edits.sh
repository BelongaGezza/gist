#!/usr/bin/env bash
# Tests for guard-apple-edits.sh. Run: bash tools/test-guard-apple-edits.sh
here="$(cd "$(dirname "$0")" && pwd)"
fail=0
t() { # expected_exit path
  local json out
  json="{\"tool_input\":{\"file_path\":\"$2\"}}"
  out="$(printf '%s' "$json" | GIST_FAKE_OS=Windows_NT GIST_ALLOW_APPLE_EDITS= bash "$here/guard-apple-edits.sh" 2>/dev/null; echo $?)"
  out="${out##*$'\n'}"; out="${out: -1}"
  if [ "$out" = "$1" ]; then echo "ok   [$1] $2"; else echo "FAIL [want $1 got $out] $2"; fail=1; fi
}
# Must block (2)
t 2 'C:\x\apps\APPLE\a.swift'
t 2 'C:\\x\\Apps\\Apple\\a.swift'   # JSON-escaped form as sent by the harness
t 2 'C:\x\Apps\Apple\a.swift'
t 2 'C:\x\APPS\apple\a.swift'
t 2 'C:\x\apps\apple\a.swift'
t 2 'C:/x/apps/apple/a.swift'
t 2 'apps/apple/a.swift'
t 2 'apps\apple\a.swift'
t 2 '/repo/ios/App.swift'
t 2 'macos/App.swift'
t 2 'C:\x\Foo.XCODEPROJ\project.pbxproj'
t 2 'x/Foo.xcworkspace/contents.xcworkspacedata'
t 2 'x/App.Entitlements'
t 2 'x/INFO.PLIST'
t 2 'Info.plist'
# Must allow (0)
t 0 'apps/windows/x.cs'
t 0 'C:\x\apps\windows\x.cs'
t 0 'crates/foo.rs'
t 0 'docs/apple-notes.md'
t 0 'crates/apple.rs'
t 0 'apps/applesauce/x.txt'
t 0 'docs/macos-notes.md'
# Override seam
r="$(printf '%s' '{"file_path":"apps/apple/a.swift"}' | GIST_FAKE_OS=Windows_NT GIST_ALLOW_APPLE_EDITS=1 bash "$here/guard-apple-edits.sh" 2>/dev/null; echo $?)"
[ "$r" = 0 ] && echo "ok   override seam" || { echo "FAIL override seam"; fail=1; }
r="$(printf '%s' '{"file_path":"apps/apple/a.swift"}' | GIST_FAKE_OS=Darwin bash "$here/guard-apple-edits.sh" 2>/dev/null; echo $?)"
[ "$r" = 0 ] && echo "ok   macOS allowed" || { echo "FAIL macOS"; fail=1; }
exit $fail
