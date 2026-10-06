#!/bin/bash
# Usage: sandbox.sh <repo-root> <profile.sb (already @ROOT@-substituted)>
# OPTIONAL experiment: run the helper under a deny-default sandbox-exec profile.
ROOT=$1; SB=$2
B=$ROOT/spikes/pdf-isolation/target/release
LIB=$ROOT/artifacts/pdfium/lib/libpdfium.dylib
for f in "$ROOT/fixtures/pdf/plain_text.pdf" "$ROOT/fixtures/pdf/two_column.pdf"; do
  echo "sandboxed $(basename "$f")" >&2
  "$B/spike-host" isolated "$f" "$LIB" "$B/spike-helper" --sandbox "$SB"
done
echo "SANDBOX DONE" >&2
