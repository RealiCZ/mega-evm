#!/bin/bash
# Negative controls: shows that the differential gates catch each engine fix, and each fixture
# choice of the gate harness, being switched off.
#
# Every fix the engine makes over stock evm2 can be switched off with `MEGA_EVM2_DISABLE=<name>`
# (the `off("<name>")` calls in `next/mega-evm2/src`, feature `compare` only), and so can the
# harness's fixture choices (the `off("<name>")` calls in `next/diff`). For every control of
# `next/diff/negative-controls.toml` in scope (`[[control]]` and `[[harness]]` alike), the
# control's gate runs on the control's one case twice:
#   on   as built: must pass;
#   off  with `MEGA_EVM2_DISABLE=<switch>`: must fail because the gate reported a difference for
#        that case. A run that panics before the gate's summary, fails on anything other than the
#        gate's difference assertion, or passes, is a miss.
# It also checks the manifest against the sources both ways: a switch without a table is
# `unlisted`, a table whose switch is not in its source is `stale`.
#
# Usage: controls.sh sample|full
#   sample  the controls with `scope = "sample"`, on the in-repo sample (`next/diff/data`)
#   full    every control, on the full corpus under $MEGA_EVM2_CORPUS_DIR
#
# Prints one line per control, and last:
#   CONTROLS scope=<s> total=<n> caught=<n> missed=<m> baseline_failed=<b> unlisted=<u> stale=<t>
# Exit status 0 only when missed, baseline_failed, unlisted and stale are all 0 and every gate ran
# at least one control (a gate that ran none is reported as `NO_CONTROL <gate>`). Like `gate.sh`,
# it refuses to start when a `MEGA_EVM2_*` variable other than `MEGA_EVM2_CORPUS_DIR` is set.
set -euo pipefail

mode=${1:-}
case "$mode" in sample | full) ;; *) echo "usage: controls.sh sample|full" >&2; exit 2 ;; esac

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
inputs=$work/controls-inputs
gate_inputs "$mode" "$inputs"
logs=$(fresh_dir "$work/controls-logs")
# shellcheck disable=SC1091
source "$inputs/expect.env"

# Build once: the gate tests and the manifest lister.
cd "$next"
if ! cargo test --release --locked -p mega-evm2-diff --no-run > "$logs/build.log" 2>&1; then
  echo "controls.sh: build failed; tail of $logs/build.log:" >&2
  tail -n 25 "$logs/build.log" >&2
  exit 1
fi
if ! list=$(cargo run --release --locked -q -p mega-evm2-diff --bin negative-controls -- \
  "$here/negative-controls.toml" "$next/mega-evm2/src" "$here" "$mode"); then
  echo "controls.sh: the manifest or the switch sources cannot be read (see above)" >&2
  exit 2
fi

# fuzz_batch <mode> <seed>: prints the batch of this run (`name:mode:first..=last:count`) that
# holds the seed, or nothing.
fuzz_batch() {
  local spec name fmode range first last
  for spec in $FUZZ_BATCHES; do
    IFS=: read -r name fmode range _ <<< "$spec"
    first=${range%%..=*}
    last=${range##*..=}
    if [ "$fmode" = "$1" ] && [ "$2" -ge "$first" ] && [ "$2" -le "$last" ]; then
      echo "$spec"
      return
    fi
  done
}

# The verdict of one run of a control, from its log and exit status:
#   pass     exit 0 with the case's clean partial summary
#   differ   a failure on the gate's difference assertion, after a summary that counts the
#            difference (and, for a scenario, its DIFFER line)
#   other <reason>   anything else
verdict() {
  local log=$1 status=$2 gate=$3 scenario=$4 line panics
  line=$(grep -E '^(PARTIAL )?GATE ' "$log" || true)
  panics=$(grep -c 'panicked at' "$log" || true)
  case "$gate" in
    main | fuzz)
      if [ "$status" = 0 ] && [ "$line" = "PARTIAL GATE $gate total=1 match=1 known=0 differ=0" ] &&
        grep -q "^MATCH $scenario\$" "$log"; then
        echo pass
      elif [ "$status" != 0 ] && [ "$line" = "PARTIAL GATE $gate total=1 match=0 known=0 differ=1" ] &&
        grep -q "^DIFFER $scenario " "$log" && [ "$panics" = 1 ] &&
        grep -q 'differential mismatches' "$log"; then
        echo differ
      else
        echo "other exit $status, summary '${line:-none}', $panics panic(s)$(grep -m1 -A1 'panicked at' "$log" | tail -n 1 | sed 's/^/: /')"
      fi
      ;;
    replay | block)
      if [ "$status" = 0 ] && echo "$line" | grep -Eq "^PARTIAL GATE $gate blocks=1 .* differ=0\$"; then
        echo pass
      elif [ "$status" != 0 ] && echo "$line" | grep -Eq "^PARTIAL GATE $gate blocks=1 .* differ=[1-9][0-9]*\$" &&
        [ "$panics" = 1 ] && grep -q 'differential gate:' "$log"; then
        echo differ
      else
        echo "other exit $status, summary '${line:-none}', $panics panic(s)$(grep -m1 -A1 'panicked at' "$log" | tail -n 1 | sed 's/^/: /')"
      fi
      ;;
  esac
}

# source_dir <table>: the source a manifest table's switches are in.
source_dir() { if [ "$1" = harness ]; then echo next/diff; else echo next/mega-evm2/src; fi; }

total=0 caught=0 missed=0 baseline_failed=0 unlisted=0 stale=0 gates_run=" "
while read -r kind switch gate _ case; do
  case "$kind" in
    UNLISTED) unlisted=$((unlisted + 1)); echo "UNLISTED $switch: no [[$gate]] in negative-controls.toml"; continue ;;
    STALE) stale=$((stale + 1)); echo "STALE $switch: [[$gate]] names no off(\"$switch\") in $(source_dir "$gate")"; continue ;;
    CONTROL) ;;
    *) continue ;;
  esac
  total=$((total + 1))
  gates_run="$gates_run$gate "
  scenario="" test=synthetic
  case "$gate" in
    main)
      scenario=$case
      gate_env "$inputs" main
      select=(MEGA_EVM2_ONLY="$scenario" MEGA_EVM2_EXPECT=1)
      ;;
    fuzz)
      IFS=: read -r fmode seed scenario <<< "$case"
      batch=$(fuzz_batch "$fmode" "$seed")
      if [ -z "$batch" ]; then
        baseline_failed=$((baseline_failed + 1))
        echo "BASELINE_FAILED $switch $gate $case: no fuzz batch of the $mode run holds mode $fmode seed $seed"
        continue
      fi
      gate_env "$inputs" fuzz "$batch"
      select=(MEGA_EVM2_FUZZ="$seed..=$seed" MEGA_EVM2_ONLY="$scenario" MEGA_EVM2_EXPECT=1)
      ;;
    replay | block)
      test=$gate
      gate_env "$inputs" "$gate"
      select=(MEGA_EVM2_BLOCKS="$case")
      ;;
  esac
  status=0
  gate_test "$logs/$switch-on.log" "$test" "${GATE_ENV[@]}" "${select[@]}" || status=$?
  on=$(verdict "$logs/$switch-on.log" "$status" "$gate" "$scenario")
  if [ "$on" != pass ]; then
    baseline_failed=$((baseline_failed + 1))
    echo "BASELINE_FAILED $switch $gate $case: ${on#other } (see $logs/$switch-on.log)"
    continue
  fi
  status=0
  gate_test "$logs/$switch-off.log" "$test" "${GATE_ENV[@]}" "${select[@]}" MEGA_EVM2_DISABLE="$switch" || status=$?
  off=$(verdict "$logs/$switch-off.log" "$status" "$gate" "$scenario")
  case "$off" in
    differ)
      caught=$((caught + 1))
      detail=$(grep -m1 -E "^DIFFER $scenario |^(PARTIAL )?GATE " "$logs/$switch-off.log")
      echo "CAUGHT $switch $gate $case: $detail"
      ;;
    pass)
      missed=$((missed + 1))
      echo "MISSED $switch $gate $case: the gate passed with the switch off"
      ;;
    *)
      missed=$((missed + 1))
      echo "MISSED $switch $gate $case: not a gate difference: ${off#other } (see $logs/$switch-off.log)"
      ;;
  esac
done <<< "$list"

no_control=0
for gate in main fuzz replay block; do
  case "$gates_run" in
    *" $gate "*) ;;
    *) no_control=$((no_control + 1)); echo "NO_CONTROL $gate: no control of the $gate gate ran in the $mode run" ;;
  esac
done

echo "CONTROLS scope=$mode total=$total caught=$caught missed=$missed baseline_failed=$baseline_failed unlisted=$unlisted stale=$stale"
[ "$total" -gt 0 ] && [ "$no_control" = 0 ] &&
  [ "$missed" = 0 ] && [ "$baseline_failed" = 0 ] && [ "$unlisted" = 0 ] && [ "$stale" = 0 ]
