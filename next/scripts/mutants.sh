#!/usr/bin/env bash
# Mutation testing of the engine crates of `next/` (mega-evm2, op-evm2), with the sample
# gate as the oracle: the test command is `cargo test` over the engine crates and the diff
# harness, run with the sample gate's inputs (synthetic scenarios, replay, block execution).
#
# Usage: mutants.sh <mode>
#   smoke           a small fixed set (one engine file, satin_ext.rs); runs locally in about ten minutes
#   pr <base-ref>   only the lines changed against <base-ref> under next/mega-evm2 and next/op-evm2
#   shard <k>/<n>   shard k of n (0-based) of the full set
#   full            the full set
#   OUT               output directory (default: <target>/mutants); cargo-mutants writes
#                     OUT/mutants.out/ (outcomes.json, caught.txt, missed.txt, ...)
#   CARGO_TARGET_DIR  cargo target directory (default: next/target)
#   JOBS              parallel mutants (default: half the CPUs)
#   SUPPRESS          suppressions file (default: next/diff/mutants/suppressions.toml)
#
# The warm build goes to next/target (ignored by git); the oracle inputs, scratch and OUT use CARGO_TARGET_DIR.
#
# Exit status: 0 when the run completed, whatever the outcomes (cargo-mutants exits 2 for
# missed and 3 for timed-out mutants); scoring them is mutation_gate.py's job. Any other
# status of cargo-mutants (a broken baseline, a bad diff) fails the run.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
next=$(cd "$here/.." && pwd)
root=$(cd "$next/.." && pwd)
# shellcheck source=../diff/scripts/common.sh
source "$next/diff/scripts/common.sh"
# shellcheck source=../diff/scripts/gate-lib.sh
source "$next/diff/scripts/gate-lib.sh"
refuse_overrides

mode=${1:-}
cd "$next"
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export CARGO_TARGET_DIR=$target
OUT=$(abs_dir "${OUT:-$target/mutants}")
SUPPRESS=${SUPPRESS:-$next/diff/mutants/suppressions.toml}
if [ -z "${JOBS:-}" ]; then
  JOBS=$(( $(getconf _NPROCESSORS_ONLN) / 2 ))
  [ "$JOBS" -ge 1 ] || JOBS=1
fi

if ! cargo mutants --version > /dev/null 2>&1; then
  echo "cargo-mutants is required. Install it with 'cargo install cargo-mutants --locked'." >&2
  exit 1
fi

# The scorer reads TOML with tomllib (Python 3.11+): the first interpreter that has it.
py=""
for cand in "${PYTHON:-}" python3 python3.13 python3.12 python3.11; do
  [ -n "$cand" ] || continue
  if "$cand" -I -c 'import tomllib' > /dev/null 2>&1; then py=$cand; break; fi
done
if [ -z "$py" ]; then
  echo "Python 3.11 or newer is required (set PYTHON)." >&2
  exit 1
fi

# Function-scoped suppressions are never generated. Captured first, so a malformed file
# fails the run instead of silently dropping the excludes.
if ! exclude_output=$("$py" -I "$root/scripts/mutation_gate.py" exclude-re --suppressions "$SUPPRESS"); then
  echo "failed to read the function suppressions of $SUPPRESS" >&2
  exit 1
fi
exclude_args=()
if [ -n "$exclude_output" ]; then
  while IFS= read -r line; do exclude_args+=("$line"); done <<< "$exclude_output"
fi

# The oracle's environment: the sample gate's main scenarios, replay and block execution.
# (One synthetic run takes one reference file, so the fuzz batches are not part of it.)
inputs=$target/mega-evm2-diff/mutants/gate-inputs
gate_inputs sample "$inputs"
gate_env "$inputs" main
env_main=("${GATE_ENV[@]}")
gate_env "$inputs" replay
export RUST_MIN_STACK=8388608
export "${env_main[@]}" "${GATE_ENV[@]}"

selection=()
case "$mode" in
  smoke)
    selection=(-f 'mega-evm2/src/satin_ext.rs')
    ;;
  pr)
    base=${2:?usage: mutants.sh pr <base-ref>}
    diff_file="$OUT.diff"
    git -C "$root" diff --no-color "$base"...HEAD -- 'next/mega-evm2/src/**' 'next/op-evm2/src/**' > "$diff_file"
    if [ ! -s "$diff_file" ]; then
      echo "No engine changes against $base; nothing to mutate." >&2
      exit 0
    fi
    selection=(--in-diff "$diff_file")
    ;;
  shard)
    selection=(--shard "${2:?usage: mutants.sh shard <k>/<n>}")
    ;;
  full) ;;
  *)
    echo "usage: mutants.sh smoke | pr <base-ref> | shard <k>/<n> | full" >&2
    exit 2
    ;;
esac

# cargo-mutants builds in a copy of next/ under $TMPDIR, where the relative path dependency
# on the evm2 fork (`../../evm2`) would resolve beside $TMPDIR. A scratch $TMPDIR with a link
# to the fork one level up makes it resolve to the same checkout.
scratch=$target/mutants-scratch
rm -rf "$scratch"
mkdir -p "$scratch/tmp"
ln -s "$(cd "$next/../../evm2" && pwd -P)" "$scratch/evm2"
export TMPDIR=$scratch/tmp

# Each mutant builds in its own copy of next/ with its own `target/`. A shared
# CARGO_TARGET_DIR would make the parallel builds overwrite one another's test binaries and
# the outcomes would depend on timing, so cargo-mutants runs without it. `copy_target` seeds
# every copy from next/target, which is warmed here with the unmutated test binaries.
echo "+ cargo test --no-run (warming next/target)"
env -u CARGO_TARGET_DIR cargo test --no-run --release --locked \
  -p mega-evm2 -p op-evm2 -p mega-evm2-scenarios -p mega-evm2-diff

rm -rf "$OUT/mutants.out"
rc=0
env -u CARGO_TARGET_DIR cargo mutants \
  --package mega-evm2 --package op-evm2 \
  --jobs "$JOBS" \
  --output "$OUT" \
  --no-shuffle \
  --colors never \
  -vV \
  ${exclude_args[@]+"${exclude_args[@]}"} \
  ${selection[@]+"${selection[@]}"} || rc=$?
case "$rc" in
  0 | 2 | 3) ;;
  *) echo "cargo mutants failed with status $rc" >&2; exit "$rc" ;;
esac
echo "Mutation results written to $OUT/mutants.out"
