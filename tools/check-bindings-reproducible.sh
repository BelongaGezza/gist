#!/usr/bin/env bash
# Review Q6: the generated C# must be reproducible. Generates twice into scratch dirs,
# normalises CRLF->LF, compares SHA-256. Exits non-zero on any difference.
# Usage: tools/check-bindings-reproducible.sh     (honours GIST_FFI_DLL like gen-bindings-cs.sh)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT

hash_dir() { # one hash over every file (sorted by name), LF-normalised
  ( cd "$1" && find . -type f | LC_ALL=C sort | while IFS= read -r f; do
      printf '%s  ' "$f"; tr -d '\r' < "$f" | sha256sum | cut -d' ' -f1
    done ) | sha256sum | cut -d' ' -f1
}

bash "$root/tools/gen-bindings-cs.sh" "$tmp/a" >/dev/null
bash "$root/tools/gen-bindings-cs.sh" "$tmp/b" >/dev/null
[ -n "$(find "$tmp/a" -type f)" ] || { echo "check-bindings-reproducible: nothing generated" >&2; exit 1; }
ha="$(hash_dir "$tmp/a")"; hb="$(hash_dir "$tmp/b")"
if [ "$ha" != "$hb" ]; then
  echo "FAIL: generated bindings differ between runs ($ha vs $hb)" >&2
  exit 1
fi
echo "ok: bindings reproducible (sha256 $ha)"
