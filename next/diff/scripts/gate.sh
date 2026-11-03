#!/bin/bash
# Runs the strict differential gates of `next/diff` and prints one summary line per gate:
#
#   GATE main total=707 match=706 known=1 differ=0
#   GATE fuzz total=<n> match=<n> differ=0
#   GATE replay blocks=<b> match=<m> pre=<p> refused=<r> differ=0 applied=0
#   GATE block blocks=<b> included=<i> refused=<r> counters=<b> post=<b> differ=0 applied=0
#
# Usage: gate.sh sample|full
#   sample  the in-repo sample under `next/diff/data`
#   full    the full corpus under $MEGA_EVM2_CORPUS_DIR (layout in `data/README.md`)
#
# The run fails (exit status 1) when a gate fails or does not print its summary line, or when a
# summary differs from the expected counts, and it refuses to start when a `MEGA_EVM2_*` variable
# other than `MEGA_EVM2_CORPUS_DIR` is set (see `common.sh`). Inputs, expected counts and the gate
# runs are in `gate-lib.sh`. Test output goes to logs under the cargo target directory; the tail
# of a failing gate's log is printed.
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
gate_inputs "$mode" "$work"

fail=0
summaries=()
for gate in main fuzz replay block; do
  line=$(gate_run "$work" "$work/logs" "$gate") || fail=1
  summaries+=("${line:-GATE $gate FAILED}")
done

printf '%s\n' "${summaries[@]}"
if [ "$fail" != 0 ]; then
  echo "gate.sh $mode: FAILED (a summary above differs from the expected line, or a gate failed)" >&2
  exit 1
fi
