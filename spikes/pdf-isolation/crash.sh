#!/bin/bash
# Usage: crash.sh <repo-root> <crash-dir-with crash_ABORT/SEGV/HANG.pdf>
# Runs the isolated host against fault-injected inputs; the host must print a
# typed HelperCrashed outcome and exit 0 itself. Proves the MECHANISM only.
ROOT=$1; CR=$2
B=$ROOT/spikes/pdf-isolation/target/release
LIB=$ROOT/artifacts/pdfium/lib/libpdfium.dylib
for k in ABORT SEGV HANG; do
  echo "fault $k" >&2
  "$B/spike-host" isolated "$CR/crash_$k.pdf" "$LIB" "$B/spike-helper"
  echo "host_exit=$?"
done
echo "then a normal parse after the crashes:"
"$B/spike-host" isolated "$ROOT/fixtures/pdf/plain_text.pdf" "$LIB" "$B/spike-helper"
echo "CRASH DONE" >&2
