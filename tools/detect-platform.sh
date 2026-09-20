#!/usr/bin/env bash
# SessionStart hook: prints the active dev machine's platform and buildable
# targets. Claude Code injects stdout into session context. Detection is by
# toolchain signals, never assumed. Portable: macOS, Linux, Windows (Git Bash).
set -u

have() { command -v "$1" >/dev/null 2>&1; }

case "$(uname -s 2>/dev/null)" in
  Darwin) os=macos ;;
  Linux) os=linux ;;
  MINGW*|MSYS*|CYGWIN*) os=windows ;;
  *) os=unknown ;;
esac
arch="$(uname -m 2>/dev/null || echo unknown)"
host="$(hostname 2>/dev/null || echo unknown)"
root="${CLAUDE_PROJECT_DIR:-.}"

present=""; missing=""
for t in cargo rustup xcodebuild xcodegen dotnet msbuild flutter dart; do
  if have "$t"; then present="$present $t"; else missing="$missing $t"; fi
done

yn() { [ "$1" = 1 ] && echo yes || echo NO; }
core=0; apple=0; win=0
have cargo && core=1
[ "$os" = macos ] && have xcodebuild && apple=1
[ "$os" = windows ] && { have dotnet || have msbuild; } && win=1

# Pinned Rust toolchain check (read-only; never triggers an install).
pin_note=""
if [ -f "$root/rust-toolchain.toml" ]; then
  pin="$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' "$root/rust-toolchain.toml" | head -1)"
  if have rustup && ! rustup toolchain list 2>/dev/null | grep -q "^$pin"; then
    pin_note="pinned Rust $pin NOT installed (first cargo run will download it)"
  else
    pin_note="pinned Rust $pin"
  fi
fi

echo "[GIST ENV] os=$os arch=$arch host=$host"
echo "[GIST ENV] buildable this session: rust-core=$(yn $core) apple(macOS/iOS)=$(yn $apple) windows-shell=$(yn $win)"
echo "[GIST ENV] tools present:${present:- none} | missing:${missing:- none}"
[ -n "$pin_note" ] && echo "[GIST ENV] $pin_note"
echo "[GIST ENV] Machine-specific claims in CLAUDE.md (Xcode version, permissions, 'verified locally') are historical; THIS block is current truth."
if [ "$os" != macos ]; then
  echo "[GIST ENV] NON-macOS: do not edit apps/apple/**, ios/, macos/, Xcode/entitlements/Info.plist (enforced by hook). Log needed Apple changes in PENDING_APPLE_CHANGES.md."
fi
if [ "$os" = windows ]; then
  echo "[GIST ENV] tools/*.sh are bash (Git Bash); the xcframework/bindings/notarize scripts need macOS."
fi
if [ "$os" = macos ] && [ -f "$root/PENDING_APPLE_CHANGES.md" ] \
   && grep -q '^## Pending Apple Change' "$root/PENDING_APPLE_CHANGES.md"; then
  echo "[GIST ENV] ACTION: PENDING_APPLE_CHANGES.md has unapplied entries - review before new work."
fi
[ -f "$root/PLATFORM_VERIFICATION.md" ] && echo "[GIST ENV] See PLATFORM_VERIFICATION.md for per-platform verification status; update it when you verify or change something."
exit 0
