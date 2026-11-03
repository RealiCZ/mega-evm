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
#             the corpus's expected archive (`reference/reference-outputs.tar.zst` when present,
#             else `reference/q11-oracles.tar.zst`, as `gate.sh` picks it; only `micros` is
#             ignored), and
#             checks the full counts against `data/expected.env`. Prints `REFERENCE full identical`.
#   default   Also re-cuts the sample under `next/diff/data` with `make-sample.sh` from the new
#             archive and rewrites `data/expected.env` from the regenerated files. Review the
#             resulting `git diff` of `data/`: it is the effect of the reference change.
#
# <out-dir> must be outside the corpus: the corpus's `reference/` directory (or a link to it, or a
# directory below the corpus), or a directory whose `reference-outputs.tar.zst` already is a corpus
# archive (a hard link or a symlink to it), is refused with exit status 2 before anything is built
# or written. The new archive would replace the corpus's, so `--check` would compare the new
# outputs with themselves; replace the corpus archive by hand once the regenerated one is
# reviewed. `--check` takes the expected outputs out of the corpus archive before it generates
# anything, and the new archive is written under a temporary name and renamed into place.
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
corpus=$(cd "$MEGA_EVM2_CORPUS_DIR" && pwd)

# refuse_corpus_out <out-dir>: exits 2 when <out-dir> is the corpus or lies inside it, through any
# symlink (resolved paths), or is the same directory as the corpus or its `reference/` (device and
# inode), or when its `reference-outputs.tar.zst` is the same file as a corpus archive (device and
# inode: a hard link or a symlink). Creates nothing.
refuse_corpus_out() {
  local real corpus_real a
  real=$(python3 -I -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$1")
  corpus_real=$(cd -P "$corpus" && pwd -P)
  case "$real/" in
    "$corpus_real/"*)
      echo "regen-reference.sh: the output directory $1 is in the corpus ($corpus, resolved $corpus_real); the new archive would replace the corpus's. Write it elsewhere." >&2
      exit 2
      ;;
  esac
  if [ -d "$1" ] && { [ "$1" -ef "$corpus" ] || [ "$1" -ef "$corpus/reference" ]; }; then
    echo "regen-reference.sh: the output directory $1 is the corpus directory or its reference/ under another name. Write it elsewhere." >&2
    exit 2
  fi
  for a in "$corpus/reference/reference-outputs.tar.zst" "$corpus/reference/q11-oracles.tar.zst"; do
    if [ -e "$a" ] && [ "$1/reference-outputs.tar.zst" -ef "$a" ]; then
      echo "regen-reference.sh: $1/reference-outputs.tar.zst is the corpus archive $a (a link to it); writing it would replace the corpus's archive. Write it elsewhere." >&2
      exit 2
    fi
  done
}

# Decided before anything is built or written: the output stays out of the corpus, and the archive
# `--check` compares with is the one `gate.sh full` reads (a regenerated archive wins over the
# original one).
refuse_corpus_out "$1"
expected_archive=$corpus/reference/reference-outputs.tar.zst
[ -f "$expected_archive" ] || expected_archive=$corpus/reference/q11-oracles.tar.zst
if [ "$check" = 1 ] && [ ! -f "$expected_archive" ]; then
  echo "regen-reference.sh: no expected archive: $corpus/reference has neither reference-outputs.tar.zst nor q11-oracles.tar.zst" >&2
  exit 2
fi
out=$(abs_dir "$1")
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
stage=$out/stage
export CARGO_TARGET_DIR=$target/mega-evm2-reference
export RUST_MIN_STACK=8388608
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}

rm -rf "$stage"
mkdir -p "$stage/new/oracles" "$stage/new/raw"
if [ "$check" = 1 ]; then
  # The expected outputs, taken out before anything is generated.
  mkdir -p "$stage/expected"
  tar --zstd -xf "$expected_archive" -C "$stage/expected" \
    satin-oracle.json oracles/fq.json oracles/fqbig.json oracles/fqh.json oracles/fqhbig.json \
    oracles/big.json oracles/harsh.json raw/satin raw/satin-block
fi

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
# Under a temporary name, then renamed: a link at the archive's path is replaced, not written
# through.
tar -cf - -C "$stage/new" satin-oracle.json oracles raw | zstd -q -19 --long=27 -f -o "$stage/archive.tar.zst"
mv -f "$stage/archive.tar.zst" "$archive"
echo "regen: wrote $archive" >&2

# The packed archive, read back the way `gate.sh full` reads it.
mkdir -p "$stage/packed"
tar --zstd -xf "$archive" -C "$stage/packed"
python3 -I "$here/scripts/count_expected.py" "$stage/packed/raw/satin" "$stage/packed/raw/satin-block" \
  | sed -E 's/^EXPECT_/EXPECT_FULL_/' > "$stage/full-counts.env"

if [ "$check" = 1 ]; then
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
