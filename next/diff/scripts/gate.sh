#!/bin/bash
# Runs the strict differential gates of `next/diff` and prints one summary line per gate:
#
#   GATE main total=698 match=697 known=1 differ=0
#   GATE fuzz total=<n> match=<n> differ=0
#   GATE replay blocks=<b> match=<m> pre=<p> refused=<r> differ=0 applied=0
#   GATE block blocks=<b> included=<i> refused=<r> counters=<b> post=<b> differ=0 applied=0
#
# Usage: gate.sh sample|full
#   sample  the in-repo sample under `next/diff/data`
#   full    the full corpus under $MEGA_EVM2_CORPUS_DIR (layout in `data/README.md`)
#
# The run fails (exit status 1) when a gate fails or does not print its summary line, or when a
# summary differs from the expected counts. Test output goes to logs under the cargo target
# directory; the tail of a failing gate's log is printed.
set -euo pipefail

mode=${1:-}
case "$mode" in sample | full) ;; *) echo "usage: gate.sh sample|full" >&2; exit 2 ;; esac

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
target=${CARGO_TARGET_DIR:-$next/target}
work=$target/mega-evm2-diff/$mode
logs=$work/logs
mkdir -p "$work" "$logs"
export RUST_MIN_STACK=8388608
# shellcheck source=stamp.sh
source "$here/scripts/stamp.sh"

# --- inputs -------------------------------------------------------------------------------------
# Both modes end up with the same layout under $work:
#   blocks/<set>/{manifest.json,<n>.json}   recorded blocks
#   replay/<set>/<n>.jsonl                  reference, per transaction
#   block/<set>/<n>.jsonl                   reference, block level
#   main.json                               reference, hand-written and default fuzz scenarios
#   fuzz/<name>.json                        reference, fuzz batches
fuzz_batches=()  # name:mode:first..=last:count
if [ "$mode" = sample ]; then
  bash "$here/scripts/check-stamp.sh" > /dev/null
  # Every file of the sample, and this script (it defines the extracted layout).
  stamp=$(stamp_dir "$here/data")-$(stamp_file "$here/scripts/gate.sh")
  if [ "$(cat "$work/.stamp" 2>/dev/null)" != "$stamp" ]; then
    rm -rf "$work/blocks" "$work/replay" "$work/block" "$work/fuzz" "$work/main.json"
    mkdir -p "$work/fuzz"
    unz() { zstd -q -d -f -o "$2" "$1"; }
    unz "$here/data/synthetic/main.json.zst" "$work/main.json"
    unz "$here/data/synthetic/fuzz-mode4.json.zst" "$work/fuzz/mode4.json"
    for kind in real-blocks:blocks replay:replay block:block; do
      src=${kind%%:*}
      dst=${kind##*:}
      (cd "$here/data/$src" && find . -type f) | while read -r f; do
        f=${f#./}
        mkdir -p "$work/$dst/$(dirname "$f")"
        case "$f" in
          *.zst) unz "$here/data/$src/$f" "$work/$dst/${f%.zst}" ;;
          *) cp "$here/data/$src/$f" "$work/$dst/$f" ;;
        esac
      done
    done
    echo "$stamp" > "$work/.stamp"
  fi
  # shellcheck disable=SC1091
  source "$here/data/expected.env"
  fuzz_batches=("mode4:4:1..=2000:2000")
  blocks_dir=$work/blocks
else
  : "${MEGA_EVM2_CORPUS_DIR:?set MEGA_EVM2_CORPUS_DIR to the full corpus}"
  corpus=$(cd "$MEGA_EVM2_CORPUS_DIR" && pwd)
  archive=$corpus/reference/q11-oracles.tar.zst
  stamp=$(stamp_file "$archive")-$(stamp_file "$here/scripts/gate.sh")
  if [ "$(cat "$work/.stamp" 2>/dev/null)" != "$stamp" ]; then
    rm -rf "$work/extract" "$work/replay" "$work/block" "$work/fuzz" "$work/main.json"
    mkdir -p "$work/extract" "$work/fuzz"
    tar --zstd -xf "$archive" -C "$work/extract" satin-oracle.json oracles raw/satin raw/satin-block
    mv "$work/extract/satin-oracle.json" "$work/main.json"
    mv "$work/extract/raw/satin" "$work/replay"
    mv "$work/extract/raw/satin-block" "$work/block"
    for name in fq fqbig fqh fqhbig big harsh; do mv "$work/extract/oracles/$name.json" "$work/fuzz/$name.json"; done
    rm -rf "$work/extract"
    echo "$stamp" > "$work/.stamp"
  fi
  fuzz_batches=(
    fq:3:1..=3000:3000 fqbig:3:3001..=13000:10000 fqh:4:1..=2000:2000
    fqhbig:4:2001..=7000:5000 big:1:401..=3400:3000 harsh:2:1..=2000:2000
  )
  blocks_dir=$corpus/real-blocks
  EXPECT_REPLAY="blocks=263 match=28000 pre=526 refused=186"
  EXPECT_BLOCK="blocks=263 included=27879 refused=307 counters=263 post=263"
fi

# --- runs ---------------------------------------------------------------------------------------
fail=0
summaries=()

# run_test <log name> <test target> <env assignments...>: prints the test's GATE line.
run_test() {
  local name=$1 test=$2
  shift 2
  local log=$logs/$name.log
  if ! env "$@" cargo test --release --locked -p mega-evm2-diff --test "$test" -- --nocapture > "$log" 2>&1; then
    echo "gate $name failed; tail of $log:" >&2
    tail -n 25 "$log" >&2
    fail=1
    return 1
  fi
  local line
  line=$(grep '^GATE ' "$log" || true)
  if [ "$(echo "$line" | grep -c '^GATE ')" != 1 ]; then
    echo "gate $name printed no single GATE line (see $log)" >&2
    fail=1
    return 1
  fi
  echo "$line"
}

cd "$next"

# main: the hand-written scenarios plus the default fuzz seeds; one known difference.
line=$(run_test main synthetic MEGA_EVM2_ORACLE="$work/main.json" MEGA_EVM2_GATE_NAME=main MEGA_EVM2_EXPECT=698) || line=""
summaries+=("${line:-GATE main FAILED}")
[ "$line" = "GATE main total=698 match=697 known=1 differ=0" ] || fail=1

# fuzz: batches summed into one line.
total=0
matched=0
differ=0
for spec in "${fuzz_batches[@]}"; do
  IFS=: read -r name fmode range count <<< "$spec"
  if line=$(run_test "fuzz-$name" synthetic MEGA_EVM2_ORACLE="$work/fuzz/$name.json" MEGA_EVM2_GATE_NAME=fuzz \
    MEGA_EVM2_FUZZ_ONLY=1 MEGA_EVM2_FUZZ_MODE="$fmode" MEGA_EVM2_FUZZ="$range" MEGA_EVM2_EXPECT="$count"); then
    t=$(sed -E 's/.* total=([0-9]+) .*/\1/' <<< "$line")
    m=$(sed -E 's/.* match=([0-9]+) .*/\1/' <<< "$line")
    k=$(sed -E 's/.* known=([0-9]+) .*/\1/' <<< "$line")
    d=$(sed -E 's/.* differ=([0-9]+).*/\1/' <<< "$line")
    total=$((total + t))
    matched=$((matched + m))
    differ=$((differ + d + k))
  else
    differ=$((differ + 1))
  fi
done
expected_total=0
for spec in "${fuzz_batches[@]}"; do expected_total=$((expected_total + ${spec##*:})); done
summaries+=("GATE fuzz total=$total match=$matched differ=$differ")
{ [ "$total" = "$expected_total" ] && [ "$matched" = "$total" ] && [ "$differ" = 0 ]; } || fail=1

# replay and block: recorded blocks.
common=(MEGA_EVM2_BLOCK_DIR="$blocks_dir" MEGA_EVM2_REPLAY_REF="$work/replay" MEGA_EVM2_BLOCK_REF="$work/block" MEGA_EVM2_EXPECT_APPLIED=0)
line=$(run_test replay replay "${common[@]}") || line=""
summaries+=("${line:-GATE replay FAILED}")
[ "$line" = "GATE replay $EXPECT_REPLAY differ=0 applied=0" ] || fail=1

line=$(run_test block block "${common[@]}") || line=""
summaries+=("${line:-GATE block FAILED}")
[ "$line" = "GATE block $EXPECT_BLOCK differ=0 applied=0" ] || fail=1

printf '%s\n' "${summaries[@]}"
if [ "$fail" != 0 ]; then
  echo "gate.sh $mode: FAILED (a summary above differs from the expected line, or a gate failed)" >&2
  exit 1
fi
