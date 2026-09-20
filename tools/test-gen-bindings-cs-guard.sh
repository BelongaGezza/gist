#!/usr/bin/env bash
# Unit test for the review-F5 `exclude` guard in tools/gen-bindings-cs.sh (temp fixtures only).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
g() { bash "$root/tools/gen-bindings-cs.sh" --check-exclude-guard "$1" >/dev/null 2>&1; }
mkdir -p "$tmp/clean" "$tmp/bad1" "$tmp/bad2" "$tmp/ok"
printf '[bindings.csharp]\nnamespace = "x"\n' > "$tmp/ok/uniffi.toml"
g "$tmp/clean" || { echo "FAIL: empty dir rejected"; exit 1; }
g "$tmp/ok"    || { echo "FAIL: benign uniffi.toml rejected"; exit 1; }
printf '[bindings.csharp]\nexclude = ["Foo.bar"]\n' > "$tmp/bad1/uniffi.toml"
if g "$tmp/bad1"; then echo "FAIL: exclude in uniffi.toml accepted"; exit 1; fi
printf '[bindings.csharp]\nexclude = []\n' > "$tmp/bad2/other.toml"
if g "$tmp/bad2"; then echo "FAIL: exclude in csharp-section toml accepted"; exit 1; fi
g "$root" || { echo "FAIL: repo itself violates guard"; exit 1; }
echo "ok: exclude guard tests passed"
