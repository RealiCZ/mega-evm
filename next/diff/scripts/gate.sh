#!/bin/bash
# Runs the strict differential gates of `next/diff` and prints one summary line per gate:
#
#   GATE main total=<n> match=<n> known=<n> class=<n> differ=0
#   GATE fuzz total=<n> match=<n> class=<n> differ=0
#   GATE replay blocks=<b> match=<m> pre=<p> refused=<r> known=<k> class=<c> differ=0
#
# `known` counts the cases a `known-differences.toml` entry accepts, `class` those an accepted
# class accepts by its shape (`gate::halted_first_frame`).
#   GATE block blocks=<b> included=<i> refused=<r> counters=<b> post=<b> differ=0
#
# and last the regular-gas invariant of Satin's give-back rule (`src/invariant.rs`), checked on
# every case of the four gates, plain and inspected: the gates' counts summed,
#
#   INVARIANT regular-gas scope=<sample|full> checked=<cases> violations=0
#
# Usage: gate.sh sample|full
#   sample  the in-repo sample under `next/diff/data`
#   full    the full corpus under $MEGA_EVM2_CORPUS_DIR (layout in `data/README.md`)
#
# The run fails (exit status 1) when a gate fails or does not print its summary line, or when a
# summary differs from the expected counts, and it refuses to start when a `MEGA_EVM2_*` variable
# other than `MEGA_EVM2_CORPUS_DIR` is set (see `common.sh`). Inputs and the gate
# runs are in `gate-lib.sh`. Test output goes to logs in a directory of this script,
# `<target>/mega-evm2-diff/<mode>/gate-logs`, cleared at the start of a run; the tail of a failing
# gate's log is printed.
set -euo pipefail

mode=${1:-}
case "$mode" in sample | full) ;; *) echo "usage: gate.sh sample|full" >&2; exit 2 ;; esac

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
# shellcheck source=gate-lib.sh
source "$here/scripts/gate-lib.sh"
refuse_overrides
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export CARGO_TARGET_DIR=$target
work=$target/mega-evm2-diff/$mode
inputs=$work/gate-inputs
gate_inputs "$mode" "$inputs"
logs=$(fresh_dir "$work/gate-logs")

fail=0
summaries=()
for gate in main fuzz replay block; do
  line=$(gate_run "$inputs" "$logs" "$gate") || fail=1
  summaries+=("${line:-GATE $gate FAILED}")
done

printf '%s\n' "${summaries[@]}"
# Every gate's test prints one `INVARIANT regular-gas gate=...` line (a fuzz batch, one each); a
# log without it is a gate that did not complete, and fails the run as its summary does.
checked=0 violations=0 unchecked=0
for log in "$logs"/*.log; do
  inv=$(grep -E '^INVARIANT regular-gas gate=[a-z]+ checked=[0-9]+ violations=[0-9]+$' "$log" || true)
  if [ -z "$inv" ] || [ "$(wc -l <<< "$inv" | tr -d ' ')" != 1 ]; then
    unchecked=$((unchecked + 1))
    continue
  fi
  checked=$((checked + $(sed -E 's/.* checked=([0-9]+) .*/\1/' <<< "$inv")))
  violations=$((violations + $(sed -E 's/.* violations=([0-9]+)$/\1/' <<< "$inv")))
done
echo "INVARIANT regular-gas scope=$mode checked=$checked violations=$violations"
if [ "$violations" != 0 ] || [ "$unchecked" != 0 ]; then
  echo "gate.sh $mode: the regular-gas invariant: $violations violating cases, $unchecked gate logs without a count" >&2
  fail=1
fi
if [ "$fail" != 0 ]; then
  echo "gate.sh $mode: FAILED (a summary above differs from the expected line, or a gate failed; logs in $logs)" >&2
  exit 1
fi
