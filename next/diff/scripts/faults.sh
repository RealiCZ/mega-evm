#!/bin/bash
# Fault injection: shows that the differential gates reject wrong reference data.
#
# Runs on a copy of the sample's gate inputs (the files of `next/diff/data`, extracted as
# `gate.sh sample` extracts them); the repository and the extracted inputs are never
# written. Every fault of `faults.py` changes one thing (a value of a compared field, a missing,
# duplicate or extra entry, a wrong expected count, a reference whose plain and inspected runs
# disagree, a known difference that no longer matches, ...); the
# relevant gate then runs through `gate_run` of `gate-lib.sh`, the code `gate.sh` accepts and
# rejects with. A fault is rejected only when that gate fails and its output shows the check the
# fault names (`faults.py list`) while the unmutated run's output does not, and every test of the
# run completed (it printed its libtest result and exited with that result's status); a gate that
# fails some other way, or a run killed or crashed before its result, does not prove the check,
# and the fault counts as accepted (`UNPROVEN`). A fault of evidence `killed:<regex>` checks that
# rule itself: its run is killed after its difference, and it is rejected only when the run is
# taken for one that did not complete. Each gate (and the main gate with a copied
# `known-differences.toml`) first runs on the unmutated copy: these baselines must pass.
#
# Usage: faults.sh
#
# Prints one line per baseline and fault, and last:
#   FAULTS total=<n> rejected=<n> accepted=<a> baselines=<k> passed=<k>
# Exit status 0 only when accepted is 0 and every baseline passed. Like `gate.sh`, it refuses to
# start when a `MEGA_EVM2_*` variable other than `MEGA_EVM2_CORPUS_DIR` is set.
set -euo pipefail

[ $# = 0 ] || { echo "usage: faults.sh" >&2; exit 2; }

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
# shellcheck source=gate-lib.sh
source "$here/scripts/gate-lib.sh"
refuse_overrides
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export CARGO_TARGET_DIR=$target
root=$target/mega-evm2-diff/faults
# This script's inputs, fresh: `gate_inputs` empties the directory it fills.
base=$root/inputs
gate_inputs sample "$base"
# The run's directory, cleared at the start: a passing run removes it, a failing one keeps it for
# the lines' `see`.
work=$(fresh_dir "$root/run")

cd "$next"
if ! cargo test --release --locked -p mega-evm2-diff --no-run > "$work/build.log" 2>&1; then
  echo "faults.sh: build failed; tail of $work/build.log:" >&2
  tail -n 25 "$work/build.log" >&2
  exit 1
fi

# fresh <dir>: a work directory whose entries link to the unmutated inputs.
fresh() {
  local entry
  mkdir -p "$1"
  for entry in blocks replay block main.json fuzz expect.env; do
    ln -s "$base/$entry" "$1/$entry"
  done
}

# run <dir> <gate> [VAR=value...]: gate_run on <dir>, logs and comparison output under <dir>.
run() {
  local dir=$1 gate=$2
  shift 2
  case "$gate" in
    replay | block) set -- MEGA_EVM2_OUT="$dir/out" "$@" ;;
  esac
  gate_run "$dir" "$dir/logs" "$gate" "$@" > "$dir/summary" 2> "$dir/stderr"
}

# The baselines and faults run in parallel, at most $jobs at a time; each writes one result line
# into its directory, and the lines are counted in list order afterwards.
jobs=$(getconf _NPROCESSORS_ONLN 2> /dev/null || echo 2)
[ "$jobs" -le 8 ] || jobs=8
pool() {
  while [ "$(jobs -rp | wc -l)" -ge "$jobs" ]; do sleep 0.1; done
}

# baseline_job <spec> <dir>: the gate of <spec> (`main:known` is the main gate with a copied
# `known-differences.toml`) on unmutated inputs.
baseline_job() {
  local spec=$1 dir=$2 extra=()
  fresh "$dir"
  if [ "$spec" = main:known ]; then
    cp "$here/known-differences.toml" "$dir/known-differences.toml"
    extra=(MEGA_EVM2_KNOWN_DIFFERENCES="$dir/known-differences.toml")
  fi
  if run "$dir" "${spec%%:*}" ${extra[@]+"${extra[@]}"}; then
    echo "BASELINE $spec passed: $(cat "$dir/summary")"
  else
    echo "BASELINE $spec FAILED: $(cat "$dir/summary") (see $dir)"
  fi
}

# fault_job <id> <gate> <evidence> <dir>: applies the fault and runs its gate.
fault_job() {
  local id=$1 gate=$2 evidence=$3 dir=$4 extra=() line lines note label
  fresh "$dir"
  if ! lines=$(python3 -I "$here/scripts/faults.py" apply "$id" "$dir" 2> "$dir/apply.log"); then
    echo "NOT_APPLIED $id: $(cat "$dir/apply.log")"
    return
  fi
  while IFS= read -r line; do [ -n "$line" ] && extra+=("$line"); done <<< "$lines"
  note=$(sed -n 's/^target: //p' "$dir/apply.log")
  label="$id ($gate${note:+, $note})"
  if run "$dir" "$gate" ${extra[@]+"${extra[@]}"}; then
    echo "ACCEPTED $label: the gate passed: $(cat "$dir/summary")"
    return
  fi
  case "$evidence" in
    killed:*)
      # The run was killed after its difference: what the verdict of an ordinary fault would make
      # of it, which must be that the run did not complete.
      line=$(verdict "$label" "$gate" "${evidence#killed:}" "$dir")
      case "$line" in
        "UNPROVEN $label: a test did not complete"*)
          echo "REJECTED $label: killed after its difference, the run is not taken for a rejection" ;;
        *) echo "ACCEPTED $label: killed after its difference, the run was taken for: $line" ;;
      esac
      ;;
    *) verdict "$label" "$gate" "$evidence" "$dir" ;;
  esac
}

# verdict <label> <gate> <evidence> <dir>: the line for a fault whose gate failed.
verdict() {
  local label=$1 gate=$2 evidence=$3 dir=$4
  if [ "$evidence" = summary ]; then
    if completed "$dir/logs"; then
      echo "REJECTED $label: by the expected summary"
    else
      echo "UNPROVEN $label: a test failed or did not complete, so the summary check is not what rejected it (see $dir)"
    fi
    return
  fi
  if ! finished "$dir/logs"; then
    echo "UNPROVEN $label: a test did not complete (no libtest result, or an exit status its result does not give), so its output proves no check (see $dir)"
  elif ! shows "$dir" "$evidence"; then
    echo "UNPROVEN $label: the gate failed, but its output does not show /$evidence/ (see $dir)"
  elif shows "$work/baseline-$gate" "$evidence"; then
    echo "UNPROVEN $label: /$evidence/ also matches the unmutated run's output"
  else
    echo "REJECTED $label"
  fi
}

# completed <logs>: whether every test of a run (every batch of a fuzz run) exited with status 0
# and printed its passing libtest result. A test that failed, or one that died before its result
# (a signal, a crash of cargo), proves nothing about the summary comparison.
completed() {
  local status found=0
  for status in "$1"/*.log.status; do
    [ -e "$status" ] || return 1
    found=1
    [ "$(cat "$status")" = 0 ] || return 1
    grep -q '^test result: ok' "${status%.status}" || return 1
  done
  [ "$found" = 1 ]
}

# finished <logs>: whether every test of a run ran to its libtest result and exited with the
# status that result gives: 0 after `test result: ok`, 101 (cargo's failed tests) after
# `test result: FAILED`. A test killed by a signal, or one whose run crashed or failed to build
# before its result, did not finish: its output stops wherever it was cut, so a difference it
# printed is not shown to be the check that failed the gate.
finished() {
  local status code found=0
  for status in "$1"/*.log.status; do
    [ -e "$status" ] || return 1
    found=1
    code=$(cat "$status")
    case "$code" in
      0) grep -q '^test result: ok' "${status%.status}" || return 1 ;;
      101) grep -q '^test result: FAILED' "${status%.status}" || return 1 ;;
      *) return 1 ;;
    esac
  done
  [ "$found" = 1 ]
}

# shows <dir> <regex>: whether the test logs or the comparison output of a run match <regex>.
shows() {
  grep -rqE -- "$2" "$1/logs" && return 0
  [ -d "$1/out" ] && grep -rqE -- "$2" "$1/out"
}

specs=(main fuzz replay block main:known)
for spec in "${specs[@]}"; do
  pool
  dir=$work/baseline-${spec/:/-}
  baseline_job "$spec" "$dir" > "$work/baseline-${spec/:/-}.result" &
done
# A fault's evidence is checked against its gate's baseline output: wait for the baselines.
wait
ids=()
while IFS=$'\t' read -r id gate evidence; do
  ids+=("$id")
  pool
  fault_job "$id" "$gate" "$evidence" "$work/$id" > "$work/$id.result" &
done < <(python3 -I "$here/scripts/faults.py" list)
wait

baselines=0 passed=0
for spec in "${specs[@]}"; do
  line=$(cat "$work/baseline-${spec/:/-}.result")
  echo "$line"
  baselines=$((baselines + 1))
  case "$line" in "BASELINE $spec passed"*) passed=$((passed + 1)) ;; esac
done
total=0 rejected=0 accepted=0
for id in "${ids[@]}"; do
  line=$(cat "$work/$id.result")
  echo "$line"
  total=$((total + 1))
  case "$line" in REJECTED*) rejected=$((rejected + 1)) ;; *) accepted=$((accepted + 1)) ;; esac
done

echo "FAULTS total=$total rejected=$rejected accepted=$accepted baselines=$baselines passed=$passed"
if [ "$total" -gt 0 ] && [ "$accepted" = 0 ] && [ "$passed" = "$baselines" ]; then
  rm -rf "$work"
else
  echo "faults.sh: FAILED; the run's files are in $work" >&2
  exit 1
fi
