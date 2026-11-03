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
# A run deletes nothing in the corpus and writes nothing into it. It writes only: a new directory of
# its own, `<out-dir>/stage.XXXXXX` (removed when the run succeeds; a failed run keeps it, with its
# logs); the archive path, over which the new archive is renamed (rename(2)), so a link there is
# replaced and never written through; cargo's directory `<target>/mega-evm2-reference`; and, in
# default mode, `next/diff/data`. It creates <out-dir> and the target directory when missing. It
# removes its own directory, the previous entry at the archive path, and, in default mode, what
# `make-sample.sh` rewrites; nothing else.
# Before anything is built or written (`refuse_corpus_writes.py`), it exits with status 2 when one
# of these paths could reach the corpus: <out-dir> in the corpus (its `reference/`, a link to it, a
# directory below it, the corpus itself); an archive path that resolves into the corpus (a symlink
# to a corpus file, directory or missing path, or a hard link to a corpus archive) or is a
# directory or a link to one; a target directory that overlaps the corpus; in default mode, a corpus
# that overlaps `next/diff/data`. A corpus elsewhere inside <out-dir> is not touched. Replace the
# corpus archive by hand once the regenerated one is reviewed: a `--check` against an archive the
# run itself had replaced would compare the new outputs with themselves. `--check` takes the
# expected outputs out of the corpus archive before it generates anything.
#
# The build fetches the pinned revisions of mega-evm and its forks from GitHub and the run takes
# tens of minutes.
set -euo pipefail

check=0
if [ "${1:-}" = --check ]; then
  check=1
  shift
fi
[ $# = 1 ] && [ -n "$1" ] || { echo "usage: regen-reference.sh [--check] <out-dir>" >&2; exit 2; }
: "${MEGA_EVM2_CORPUS_DIR:?set MEGA_EVM2_CORPUS_DIR to the full corpus}"

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
refuse_overrides
corpus=$(cd -P "$MEGA_EVM2_CORPUS_DIR" && pwd -P)

# resolved <path>: <path> with every symlink resolved; components that do not exist yet are kept.
resolved() { python3 -I -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$1"; }

# Decided before anything is built or written, on the paths the run then uses (resolved, so a
# directory it creates is exactly the one checked): none of them can reach the corpus, and the
# archive `--check` compares with is the one `gate.sh full` reads (a regenerated archive wins over
# the original one).
out=$(resolved "$1")
target=$(resolved "${CARGO_TARGET_DIR:-$next/target}")
archive=$out/reference-outputs.tar.zst
data=
[ "$check" = 1 ] || data=$here/data
python3 -I "$here/scripts/refuse_corpus_writes.py" "$corpus" "$out" "$target/mega-evm2-reference" ${data:+"$data"}
expected_archive=$corpus/reference/reference-outputs.tar.zst
[ -f "$expected_archive" ] || expected_archive=$corpus/reference/q11-oracles.tar.zst
if [ "$check" = 1 ] && [ ! -f "$expected_archive" ]; then
  echo "regen-reference.sh: no expected archive: $corpus/reference has neither reference-outputs.tar.zst nor q11-oracles.tar.zst" >&2
  exit 2
fi
mkdir -p "$out" "$target"
# This run's own directory: new, so removing it removes nothing else, and in <out-dir>, so the
# archive is renamed from it within one file system. A failed run keeps it, with the logs.
run=$(mktemp -d "$out/stage.XXXXXX")
kept() { [ "$1" = 0 ] || echo "regen-reference.sh: failed; this run's files are kept in $run" >&2; }
trap 'kept $?' EXIT
export CARGO_TARGET_DIR=$target/mega-evm2-reference
export RUST_MIN_STACK=8388608
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}

mkdir -p "$run/new/oracles" "$run/new/raw"
if [ "$check" = 1 ]; then
  # The expected outputs, taken out before anything is generated.
  mkdir -p "$run/expected"
  tar --zstd -xf "$expected_archive" -C "$run/expected" \
    satin-oracle.json oracles/fq.json oracles/fqbig.json oracles/fqh.json oracles/fqhbig.json \
    oracles/big.json oracles/harsh.json raw/satin raw/satin-block
fi

gen() {
  local name=$1 test=$2
  shift 2
  echo "regen: $name" >&2
  (cd "$next/reference" && env "$@" cargo test --release --locked --test "$test" -- --nocapture) \
    > "$run/$name.log" 2>&1 || { echo "reference $name failed; tail:" >&2; tail -n 25 "$run/$name.log" >&2; exit 1; }
}

gen main synthetic MEGA_EVM2_ORACLE_OUT="$run/new/satin-oracle.json" MEGA_EVM2_EXPECT="$MAIN_TOTAL"
for spec in "${FULL_FUZZ_BATCHES[@]}"; do
  IFS=: read -r name fmode range count <<< "$spec"
  gen "fuzz-$name" synthetic MEGA_EVM2_ORACLE_OUT="$run/new/oracles/$name.json" MEGA_EVM2_FUZZ_ONLY=1 \
    MEGA_EVM2_FUZZ_MODE="$fmode" MEGA_EVM2_FUZZ="$range" MEGA_EVM2_EXPECT="$count"
done
gen replay replay MEGA_EVM2_BLOCK_DIR="$corpus/real-blocks" MEGA_EVM2_REPLAY_OUT="$run/new/raw/satin"
gen block block MEGA_EVM2_BLOCK_DIR="$corpus/real-blocks" MEGA_EVM2_BLOCK_OUT="$run/new/raw/satin-block"

# Packed under a temporary name in this run's directory, then renamed over the archive path: a link
# there is replaced, never written through (`mv` would move the file into a linked directory).
tar -cf - -C "$run/new" satin-oracle.json oracles raw | zstd -q -19 --long=27 -f -o "$run/archive.tar.zst"
rename_exact "$run/archive.tar.zst" "$archive"
echo "regen: wrote $archive" >&2

# The packed archive, read back the way `gate.sh full` reads it.
mkdir -p "$run/packed"
tar --zstd -xf "$archive" -C "$run/packed"
python3 -I "$here/scripts/count_expected.py" "$run/packed/raw/satin" "$run/packed/raw/satin-block" \
  | sed -E 's/^EXPECT_/EXPECT_FULL_/' > "$run/full-counts.env"

if [ "$check" = 1 ]; then
  python3 -I "$here/scripts/compare_outputs.py" "$run/expected" "$run/packed"
  stored=$(grep -E '^EXPECT_FULL_(REPLAY|BLOCK)=' "$here/data/expected.env")
  [ "$stored" = "$(cat "$run/full-counts.env")" ] || {
    echo "full counts differ from data/expected.env:" >&2
    cat "$run/full-counts.env" >&2
    exit 1
  }
  rm -rf "$run"
  echo "REFERENCE full identical"
  exit 0
fi

# The sample is cut from the new archive: a view of the corpus whose reference is the new one.
mkdir -p "$run/view/reference"
ln -s "$corpus/real-blocks" "$run/view/real-blocks"
ln -s "$archive" "$run/view/reference/reference-outputs.tar.zst"
MEGA_EVM2_CORPUS_DIR=$run/view bash "$here/scripts/make-sample.sh"
{
  grep '^#' "$here/data/expected.env"
  python3 -I "$here/scripts/count_expected.py" "$here/data/replay" "$here/data/block"
  cat "$run/full-counts.env"
} > "$run/expected.env"
rename_exact "$run/expected.env" "$here/data/expected.env"
rm -rf "$run"
echo "REFERENCE regenerated: archive $archive, sample and data/expected.env under $here/data"
echo "Review: git diff --stat -- next/diff/data"
