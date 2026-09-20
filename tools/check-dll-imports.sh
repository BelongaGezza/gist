#!/usr/bin/env bash
# Fails if a Windows DLL imports the dynamic Visual C++ runtime (VCRUNTIME140 /
# MSVCP140 / concrt140 ...), i.e. it would not load on a machine without the
# VC++ Redistributable. UCRT (api-ms-win-crt-*) is part of Windows 10+ and allowed.
# Usage: tools/check-dll-imports.sh [path/to/gist_ffi.dll]   (default: target/release/gist_ffi.dll)
set -euo pipefail
dll="${1:-target/release/gist_ffi.dll}"
[ -f "$dll" ] || { echo "check-dll-imports: $dll not found" >&2; exit 2; }
bad="$(grep -a -o -i -E '(vcruntime140[_0-9a-z]*|msvcp140[_0-9a-z]*|vcomp140|concrt140|vccorlib140)\.dll' "$dll" | sort -u || true)"
if [ -n "$bad" ]; then
  echo "FAIL: $dll imports the dynamic VC++ runtime:" >&2
  echo "$bad" >&2
  exit 1
fi
echo "ok: $dll has no dynamic VC++ runtime imports"
