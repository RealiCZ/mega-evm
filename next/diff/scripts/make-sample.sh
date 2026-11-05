#!/bin/bash
# Rebuilds the in-repo sample (`next/diff/data`) from the full corpus.
#
# Usage: MEGA_EVM2_CORPUS_DIR=<corpus> make-sample.sh
#
# The selection of blocks is the list below; why each block is there is in `data/README.md`.
# The full corpus layout is documented there as well.
set -euo pipefail

: "${MEGA_EVM2_CORPUS_DIR:?set MEGA_EVM2_CORPUS_DIR to the full corpus}"
here=$(cd "$(dirname "$0")/.." && pwd)
corpus=$(cd "$MEGA_EVM2_CORPUS_DIR" && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
target=$(abs_dir "${CARGO_TARGET_DIR:-$here/../target}")
work=$target/mega-evm2-diff/make-sample
data=$here/data

BLOCKS="
d4:2045117 d4:3224158 d4:7487457 d4:8064067 d4:10108963 d4:12747297 d4:14924732
d4:18138664 d4:19010529 d4:22016777 d4:27670576 d4:27985540 d4:28102900 d4:28107033
p9:26400000 p9:26400004 p9:26400020 p9:26400051 p9:26400077 p9:26400193
"

rm -rf "$work" "$data/synthetic" "$data/real-blocks" "$data/replay" "$data/block"
mkdir -p "$work" "$data/synthetic" "$data/real-blocks" "$data/replay" "$data/block"
archive=$corpus/reference/reference-outputs.tar.zst
[ -f "$archive" ] || archive=$corpus/reference/q11-oracles.tar.zst
tar --zstd -xf "$archive" -C "$work" satin-oracle.json oracles raw/satin raw/satin-block

zst() { zstd -q -19 --long=27 -f -o "$2" "$1"; }

zst "$work/satin-oracle.json" "$data/synthetic/main.json.zst"
# Each sample batch is the first seeds of the full batch of its mode that starts at seed 1, whose
# outputs are in seed order: a slice of it.
for spec in "${SAMPLE_FUZZ_BATCHES[@]}"; do
  IFS=: read -r name fmode range count <<< "$spec"
  source_batch=""
  for full in "${FULL_FUZZ_BATCHES[@]}"; do
    IFS=: read -r fname ffmode frange _ <<< "$full"
    [ "$ffmode" = "$fmode" ] && [ "${frange%%..=*}" = 1 ] && source_batch=$fname
  done
  [ -n "$source_batch" ] && [ "${range%%..=*}" = 1 ] ||
    { echo "make-sample.sh: no full batch of mode $fmode starts at seed 1 for $name" >&2; exit 1; }
  jq -c ".[0:$count]" "$work/oracles/$source_batch.json" > "$work/fuzz-$name.json"
  [ "$(jq length "$work/fuzz-$name.json")" = "$count" ] ||
    { echo "make-sample.sh: $source_batch holds fewer than $count seeds for $name" >&2; exit 1; }
  zst "$work/fuzz-$name.json" "$data/synthetic/fuzz-$name.json.zst"
done

for item in $BLOCKS; do
  set=${item%%:*}
  n=${item##*:}
  mkdir -p "$data/real-blocks/$set" "$data/replay/$set" "$data/block/$set"
  zst "$corpus/real-blocks/$set/$n.json" "$data/real-blocks/$set/$n.json.zst"
  zst "$work/raw/satin/$set/$n.jsonl" "$data/replay/$set/$n.jsonl.zst"
  zst "$work/raw/satin-block/$set/$n.jsonl" "$data/block/$set/$n.jsonl.zst"
done
for set in $(echo "$BLOCKS" | tr ' ' '\n' | grep : | cut -d: -f1 | sort -u); do
  numbers=$(echo "$BLOCKS" | tr ' ' '\n' | grep "^$set:" | cut -d: -f2 | paste -sd, -)
  jq -c --argjson keep "[$numbers]" '[.[] | select(.number as $n | $keep | index($n))]' \
    "$corpus/real-blocks/$set/manifest.json" > "$data/real-blocks/$set/manifest.json"
done
echo "sample written to $data"
du -sk "$data"
