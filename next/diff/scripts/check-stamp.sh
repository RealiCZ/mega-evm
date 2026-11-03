#!/bin/bash
# Checks that the cache stamp of `gate.sh` follows the content of the sample data: an unchanged
# copy gets the same stamp, and changing one byte of one data file changes it.
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=stamp.sh
source "$here/scripts/stamp.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp -R "$here/data" "$tmp/data"
base=$(stamp_dir "$here/data")
[ "$base" = "$(stamp_dir "$tmp/data")" ] || { echo "stamp differs for an identical copy" >&2; exit 1; }
victim=$(cd "$tmp/data" && find . -name '*.jsonl.zst' | LC_ALL=C sort | head -n 1)
printf 'x' >> "$tmp/data/$victim"
[ "$base" != "$(stamp_dir "$tmp/data")" ] || { echo "stamp ignores a changed byte in $victim" >&2; exit 1; }
rm "$tmp/data/$victim"
[ "$base" != "$(stamp_dir "$tmp/data")" ] || { echo "stamp ignores a removed file" >&2; exit 1; }
echo "STAMP ok"
