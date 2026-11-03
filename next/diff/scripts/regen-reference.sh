#!/bin/bash
# Regenerates the reference outputs of the full corpus with `next/reference` (the Satin
# implementation under review) and, unless `--check` is given, the in-repo sample.
#
# Usage: MEGA_EVM2_CORPUS_DIR=<corpus> regen-reference.sh [--check] <out-dir>
#
# It runs `next/reference` over the main scenarios, the six fuzz batches of `gate.sh full` and
# every block of `$MEGA_EVM2_CORPUS_DIR/real-blocks` (engine replay and block level), and packs
# the outputs as `<out-dir>/reference-outputs.tar.zst` in the layout `gate.sh full` reads:
#   satin-oracle.json, oracles/<batch>.json, raw/satin/<set>/<n>.jsonl, raw/satin-block/<set>/<n>.jsonl
#
#   --check   Writes nothing into the repository. Compares the regenerated outputs, taken back out
#             of the packed archive, with the same paths of
#             `$MEGA_EVM2_CORPUS_DIR/reference/q11-oracles.tar.zst` (only `micros` is ignored), and
#             checks the full counts against `data/expected.env`. Prints `REFERENCE full identical`.
#   default   Also re-cuts the sample under `next/diff/data` with `make-sample.sh` from the new
#             archive and rewrites `data/expected.env` from the regenerated files. Review the
#             resulting `git diff` of `data/`: it is the effect of the reference change.
#
# The build fetches the pinned revisions of mega-evm and its forks from GitHub and the run takes
# tens of minutes.
set -euo pipefail

check=0
if [ "${1:-}" = --check ]; then
  check=1
  shift
fi
[ $# = 1 ] || { echo "usage: regen-reference.sh [--check] <out-dir>" >&2; exit 2; }
: "${MEGA_EVM2_CORPUS_DIR:?set MEGA_EVM2_CORPUS_DIR to the full corpus}"

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
refuse_overrides
out=$(abs_dir "$1")
corpus=$(cd "$MEGA_EVM2_CORPUS_DIR" && pwd)
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
stage=$out/stage
export CARGO_TARGET_DIR=$target/mega-evm2-reference
export RUST_MIN_STACK=8388608
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}

rm -rf "$stage"
mkdir -p "$stage/new/oracles" "$stage/new/raw"

gen() {
  local name=$1 test=$2
  shift 2
  echo "regen: $name" >&2
  (cd "$next/reference" && env "$@" cargo test --release --locked --test "$test" -- --nocapture) \
    > "$stage/$name.log" 2>&1 || { echo "reference $name failed; tail:" >&2; tail -n 25 "$stage/$name.log" >&2; exit 1; }
}

gen main synthetic MEGA_EVM2_ORACLE_OUT="$stage/new/satin-oracle.json" MEGA_EVM2_EXPECT=707
for spec in "${FULL_FUZZ_BATCHES[@]}"; do
  IFS=: read -r name fmode range count <<< "$spec"
  gen "fuzz-$name" synthetic MEGA_EVM2_ORACLE_OUT="$stage/new/oracles/$name.json" MEGA_EVM2_FUZZ_ONLY=1 \
    MEGA_EVM2_FUZZ_MODE="$fmode" MEGA_EVM2_FUZZ="$range" MEGA_EVM2_EXPECT="$count"
done
gen replay replay MEGA_EVM2_BLOCK_DIR="$corpus/real-blocks" MEGA_EVM2_REPLAY_OUT="$stage/new/raw/satin"
gen block block MEGA_EVM2_BLOCK_DIR="$corpus/real-blocks" MEGA_EVM2_BLOCK_OUT="$stage/new/raw/satin-block"

archive=$out/reference-outputs.tar.zst
tar -cf - -C "$stage/new" satin-oracle.json oracles raw | zstd -q -19 --long=27 -f -o "$archive"
echo "regen: wrote $archive" >&2

# The packed archive, read back the way `gate.sh full` reads it.
mkdir -p "$stage/packed"
tar --zstd -xf "$archive" -C "$stage/packed"
python3 -I "$here/scripts/count_expected.py" "$stage/packed/raw/satin" "$stage/packed/raw/satin-block" \
  | sed -E 's/^EXPECT_/EXPECT_FULL_/' > "$stage/full-counts.env"

if [ "$check" = 1 ]; then
  mkdir -p "$stage/expected"
  tar --zstd -xf "$corpus/reference/q11-oracles.tar.zst" -C "$stage/expected" \
    satin-oracle.json oracles/fq.json oracles/fqbig.json oracles/fqh.json oracles/fqhbig.json \
    oracles/big.json oracles/harsh.json raw/satin raw/satin-block
  python3 -I "$here/scripts/compare_outputs.py" "$stage/expected" "$stage/packed"
  stored=$(grep -E '^EXPECT_FULL_(REPLAY|BLOCK)=' "$here/data/expected.env")
  [ "$stored" = "$(cat "$stage/full-counts.env")" ] || {
    echo "full counts differ from data/expected.env:" >&2
    cat "$stage/full-counts.env" >&2
    exit 1
  }
  rm -rf "$stage"
  echo "REFERENCE full identical"
  exit 0
fi

# The sample is cut from the new archive: a view of the corpus whose reference is the new one.
mkdir -p "$stage/view/reference"
ln -s "$corpus/real-blocks" "$stage/view/real-blocks"
ln -s "$archive" "$stage/view/reference/reference-outputs.tar.zst"
MEGA_EVM2_CORPUS_DIR=$stage/view bash "$here/scripts/make-sample.sh"
{
  grep '^#' "$here/data/expected.env"
  python3 -I "$here/scripts/count_expected.py" "$here/data/replay" "$here/data/block"
  cat "$stage/full-counts.env"
} > "$stage/expected.env"
mv "$stage/expected.env" "$here/data/expected.env"
rm -rf "$stage"
echo "REFERENCE regenerated: archive $archive, sample and data/expected.env under $here/data"
echo "Review: git diff --stat -- next/diff/data"
