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

refuse_corpus_writes "$corpus" "$work" "$work/.cargo" "$data"

BLOCKS="
d4:2045117 d4:3224158 d4:7487457 d4:8064067 d4:10108963 d4:12747297 d4:14924732
d4:18138664 d4:19010529 d4:22016777 d4:27670576 d4:27985540 d4:28102900 d4:28107033
p9:26400000 p9:26400004 p9:26400020 p9:26400051 p9:26400077 p9:26400193
"
# Seeds 1..=FUZZ_SEEDS of the mode-4 (harsh, with the MegaETH mechanisms) fuzz batch.
FUZZ_SEEDS=2000

rm -rf "$work" "$data/synthetic" "$data/real-blocks" "$data/replay" "$data/block"
mkdir -p "$work" "$data/synthetic" "$data/real-blocks" "$data/replay" "$data/block"
archive=$corpus/reference/reference-outputs.tar.zst
[ -f "$archive" ] || archive=$corpus/reference/q11-oracles.tar.zst
tar --zstd -xf "$archive" -C "$work" \
  satin-oracle.json oracles/fqh.json raw/satin raw/satin-block

zst() { zstd -q -19 --long=27 -f -o "$2" "$1"; }

zst "$work/satin-oracle.json" "$data/synthetic/main.json.zst"
jq -c "map(select(.name | test(\"^fqh0*([1-9][0-9]{0,3})\$\")) | select((.name[3:] | tonumber) <= $FUZZ_SEEDS))" \
  "$work/oracles/fqh.json" > "$work/fuzz.json"
zst "$work/fuzz.json" "$data/synthetic/fuzz-mode4.json.zst"

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
