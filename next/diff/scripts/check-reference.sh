#!/bin/bash
# Regenerates the reference outputs of the in-repo sample with `next/reference` (the Satin
# implementation under review) and compares them with `next/diff/data`.
#
# Usage: check-reference.sh sample
#
# Prints `REFERENCE sample identical` when every regenerated output equals the stored one, and
# exits non-zero otherwise. Only timing fields (`micros`) are ignored. The build fetches the
# pinned revisions of mega-evm and its forks from GitHub.
set -euo pipefail

[ "${1:-}" = sample ] || { echo "usage: check-reference.sh sample" >&2; exit 2; }

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
target=${CARGO_TARGET_DIR:-$next/target}
work=$target/mega-evm2-diff/reference-sample
export CARGO_TARGET_DIR=$target/mega-evm2-reference
export RUST_MIN_STACK=8388608
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}

rm -rf "$work"
mkdir -p "$work/blocks" "$work/expected" "$work/out"
unz() { mkdir -p "$(dirname "$2")"; zstd -q -d -f -o "$2" "$1"; }

# Inputs: the recorded blocks. Expected outputs: the stored reference.
unz "$here/data/synthetic/main.json.zst" "$work/expected/main.json"
unz "$here/data/synthetic/fuzz-mode4.json.zst" "$work/expected/fuzz-mode4.json"
for src in real-blocks replay block; do
  (cd "$here/data/$src" && find . -type f) | while read -r f; do
    f=${f#./}
    case "$src" in
      real-blocks) dst=$work/blocks/$f ;;
      *) dst=$work/expected/$src/$f ;;
    esac
    case "$f" in
      *.zst) unz "$here/data/$src/$f" "${dst%.zst}" ;;
      *) mkdir -p "$(dirname "$dst")"; cp "$here/data/$src/$f" "$dst" ;;
    esac
  done
done

gen() {
  local test=$1
  shift
  (cd "$next/reference" && env "$@" cargo test --release --locked --test "$test" -- --nocapture) \
    > "$work/$test.log" 2>&1 || { echo "reference $test failed; tail:" >&2; tail -n 25 "$work/$test.log" >&2; exit 1; }
}

gen synthetic MEGA_EVM2_ORACLE_OUT="$work/out/main.json" MEGA_EVM2_EXPECT=698
gen synthetic MEGA_EVM2_ORACLE_OUT="$work/out/fuzz-mode4.json" MEGA_EVM2_FUZZ_ONLY=1 \
  MEGA_EVM2_FUZZ_MODE=4 MEGA_EVM2_FUZZ=1..=2000 MEGA_EVM2_EXPECT=2000
gen replay MEGA_EVM2_BLOCK_DIR="$work/blocks" MEGA_EVM2_REPLAY_OUT="$work/out/replay"
gen block MEGA_EVM2_BLOCK_DIR="$work/blocks" MEGA_EVM2_BLOCK_OUT="$work/out/block"

python3 -I "$here/scripts/compare_outputs.py" "$work/expected" "$work/out"
echo "REFERENCE sample identical"
