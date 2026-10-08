#!/bin/bash
# Usage: bench.sh <repo-root> <large.pdf> <crash-dir> [runs=5]
# Prints one TSV line per run: <label> <file> <host output line>. Progress on stderr.
# Synthetic/hostile fixtures only (decision D6). Unsigned. Run from anywhere.
ROOT=$1; LARGE=$2; CRASH=$3; N=${4:-5}
B=$ROOT/spikes/pdf-isolation/target/release
H=$B/spike-host; HELPER=$B/spike-helper
LIB=$ROOT/artifacts/pdfium/lib/libpdfium.dylib
FILES="$ROOT/fixtures/pdf/plain_text.pdf $ROOT/fixtures/pdf/two_column.pdf $ROOT/fixtures/pdf/image_only.pdf $ROOT/fixtures/pdf/adversarial/*.pdf $LARGE"
for f in $FILES; do
  for mode in inproc pipe fd; do
    for i in $(seq 1 "$N"); do
      echo "run $mode $(basename "$f") $i/$N" >&2
      case $mode in
        inproc) out=$("$H" inproc "$f" "$LIB") ;;
        pipe) out=$("$H" isolated "$f" "$LIB" "$HELPER") ;;
        fd) out=$("$H" isolated "$f" "$LIB" "$HELPER" --fd) ;;
      esac
      printf '%s\t%s\t%s\n' "$mode" "$(basename "$f")" "$out"
    done
  done
done
echo "BENCH DONE" >&2
