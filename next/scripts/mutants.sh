#!/usr/bin/env bash
# Mutation testing of the engine crates of `next/` (mega-evm2, op-evm2), with the sample
# gate as the oracle: the test command is `cargo test` over the engine crates and the diff
# harness, run with the sample gate's inputs (synthetic scenarios, replay, block execution).
#
# Usage: mutants.sh <mode>
#   smoke             a small fixed set (one engine file, satin_ext.rs); runs locally in about ten minutes
#   pr <base-ref>     only the engine lines changed against <base-ref> (from their merge base)
#   push <before>     only the engine lines changed since the previous push <before>; when <before> is
#                     empty or all zeros, missing from the clone, or shares no history with HEAD, the
#                     base is the stated fallback MUTANTS_PUSH_FALLBACK (default HEAD^, the head's
#                     parent); after a force push it is the merge base of <before> and HEAD
#   files <glob>...   the engine files matching the globs (relative to next/, as cargo-mutants names
#                     them: a glob with a slash matches the path, one without the file name)
#   shard <k>/<n>     shard k of n (0-based) of the full set
#   full              the full set
#
# Environment:
#   OUT               output directory (default: <target>/mutants-out); the results are OUT/mutants.out/
#                     (outcomes.json, caught.txt, missed.txt, ...), the previous ones OUT/mutants.out.old/
#   CARGO_TARGET_DIR  cargo target directory (default: next/target)
#   JOBS              parallel mutants (default: half the CPUs)
#   SUPPRESS          suppressions file (default: next/diff/mutants/suppressions.toml)
#   MUTANTS_PROFILE   cargo profile of the warm build, the mutant builds and the oracle (default:
#                     `mutants`, next/Cargo.toml). CARGO_INCREMENTAL is unset: the profile decides.
#   MUTANTS_ITERATE=1 carry over the results already in OUT/mutants.out instead of starting afresh:
#                     a mutant caught or unviable there is not tested again, unless its code changed
#                     since (the diff from the commit recorded in carry-base.txt) or a refresh drops
#                     it; missed and timed-out mutants are tested again. Without it, a run starts
#                     afresh and its results replace OUT/mutants.out.
#   MUTANTS_BUDGET=<s> end the run cleanly <s> seconds after this script started: cargo-mutants is
#                     interrupted, the outcomes so far are kept as OUT/mutants.out, a line starting
#                     `MUTANTS-BUDGET reached` is printed and the exit status is 0. A run with
#                     MUTANTS_ITERATE=1 continues from there.
#   MUTANTS_REFRESH   with MUTANTS_ITERATE=1: space-separated globs of engine files (or `all`) whose
#                     carried-over results are dropped, so they are tested again without carry-over
#   MUTANTS_REFRESH_FILE  refresh requests applied once to a carried-over set (default:
#                     next/diff/mutants/refresh.txt)
#
# The last line of every run is
#   MUTANTS-RESULT caught=<n> missed=<n> timeout=<n> unviable=<n> tested=<n> skipped=<n>
# where caught and unviable include the carried-over mutants, tested counts the mutants this run
# built or tested, and skipped the carried-over ones.
#
# The warm build goes to next/target (ignored by git); the oracle inputs and OUT use CARGO_TARGET_DIR.
# The scratch directory is a new one under TMPDIR, removed at exit.
#
# Exit status: 0 when the run completed or reached its budget, whatever the outcomes
# (cargo-mutants exits 2 for missed and 3 for timed-out mutants); scoring them is
# mutation_gate.py's job. Any other status of cargo-mutants (a broken baseline, a bad diff) fails
# the run.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
next=$(cd "$here/.." && pwd)
root=$(cd "$next/.." && pwd)
# shellcheck source=../diff/scripts/common.sh
source "$next/diff/scripts/common.sh"
# shellcheck source=../diff/scripts/gate-lib.sh
source "$next/diff/scripts/gate-lib.sh"
refuse_overrides

usage="usage: mutants.sh smoke | pr <base-ref> | push <before-sha> | files <glob>... | shard <k>/<n> | full"
mode=${1:-}
[ $# -eq 0 ] || shift
cd "$next"
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export CARGO_TARGET_DIR=$target
OUT=$(abs_dir "${OUT:-$target/mutants-out}")
SUPPRESS=${SUPPRESS:-$next/diff/mutants/suppressions.toml}
REFRESH_FILE=${MUTANTS_REFRESH_FILE:-$next/diff/mutants/refresh.txt}
profile=${MUTANTS_PROFILE:-mutants}
results=$OUT/mutants.out
stage=$OUT/run
iterate=0
[ "${MUTANTS_ITERATE:-0}" = 1 ] && iterate=1
budget=${MUTANTS_BUDGET:-}
case "$budget" in
  "" | *[!0-9]*) [ -z "$budget" ] || { echo "MUTANTS_BUDGET must be a number of seconds" >&2; exit 2; } ;;
esac
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
ledger() { "$py" -I "$here/mutants-ledger.py" "$@"; }

# The commit the results will refer to: HEAD, unless the engine sources have uncommitted changes
# (then the next carried-over run cannot tell what changed since, and carries over by name).
base_commit=""
if git -C "$next" rev-parse --verify -q HEAD > /dev/null &&
  [ -z "$(git -C "$next" status --porcelain -- mega-evm2/src op-evm2/src)" ]; then
  base_commit=$(git -C "$next" rev-parse HEAD)
fi

# What happened, for the result line printed at exit: `fresh` when this run's results are
# OUT/mutants.out, `carried` when it tested nothing and OUT/mutants.out is the carried-over set.
outcome=none
scratch=""
cm_pid=""
finish() {
  local rc=$?
  trap - EXIT INT TERM
  if [ -d "$stage/mutants.out" ]; then
    ledger finalize --stage "$stage" --out "$OUT" ${base_commit:+--base "$base_commit"} \
      --refresh-file "$REFRESH_FILE" || rc=1
    outcome=fresh
  fi
  rm -rf "$stage"
  [ -z "$scratch" ] || rm -rf "$scratch"
  case "$outcome" in
    fresh) ledger tally --results "$results" ;;
    carried) ledger tally --results "$results" --carried-only ;;
    *) ledger tally ;;
  esac
  exit "$rc"
}
# A signal (a cancelled CI job) stops cargo-mutants like the budget does, then exits through finish.
on_signal() {
  if [ -n "$cm_pid" ]; then
    kill -INT "$cm_pid" 2> /dev/null || true
    wait "$cm_pid" 2> /dev/null || true
  fi
  exit 130
}
trap finish EXIT
trap on_signal INT TERM

# engine_diff <git diff revision args...>: the changes to the engine sources, with paths relative
# to next/ (the way cargo-mutants names its files) and fixed a/ b/ prefixes.
engine_diff() {
  git -C "$next" -c diff.noprefix=false -c diff.mnemonicPrefix=false diff --no-color --no-ext-diff \
    --no-renames --relative --src-prefix=a/ --dst-prefix=b/ "$@" -- mega-evm2/src op-evm2/src
}

case "$mode" in
  smoke | pr | push | files | shard | full) ;;
  *) echo "$usage" >&2; exit 2 ;;
esac
# A fresh run replaces the previous results, also when it ends before cargo-mutants writes
# anything (a diff with no engine change), so they are never read as this run's.
[ "$iterate" = 1 ] || rm -rf "$results" "$results.old"

selection=()
refresh=()
case "$mode" in
  smoke)
    selection=(-f 'mega-evm2/src/satin_ext.rs')
    ;;
  pr | push)
    if [ "$mode" = pr ]; then
      base=${1:?$usage}
      range=("$base...HEAD")
      echo "pr base: $base (diff from its merge base with HEAD)"
    else
      before=${1-}
      base="" why=""
      if [ -z "$before" ] || [ -z "${before//0/}" ]; then
        why="there is no previous push"
      elif ! git -C "$root" cat-file -e "$before^{commit}" 2> /dev/null; then
        why="the previous push $before is not in this clone"
      elif git -C "$root" merge-base --is-ancestor "$before" HEAD; then
        base=$(git -C "$root" rev-parse "$before^{commit}")
        echo "push base: $base (the previous push)"
      elif base=$(git -C "$root" merge-base "$before" HEAD); then
        echo "push base: $base (the merge base with the previous push $before, which HEAD does not contain: a force push)"
      else
        base="" why="the previous push $before shares no history with HEAD"
      fi
      if [ -z "$base" ]; then
        fallback=${MUTANTS_PUSH_FALLBACK:-HEAD^}
        if ! base=$(git -C "$root" rev-parse --verify -q "$fallback^{commit}"); then
          echo "push base: none ($why, and the fallback $fallback does not resolve); nothing to mutate." >&2
          exit 0
        fi
        echo "push base: $base (the stated fallback $fallback, because $why)"
      fi
      range=("$base" HEAD)
    fi
    diff_file="$OUT.diff"
    engine_diff "${range[@]}" > "$diff_file"
    if [ ! -s "$diff_file" ]; then
      echo "No engine changes against $base; nothing to mutate." >&2
      exit 0
    fi
    selection=(--in-diff "$diff_file")
    ;;
  files)
    [ $# -ge 1 ] || { echo "$usage" >&2; exit 2; }
    for g in "$@"; do
      selection+=(-f "$g")
      refresh+=(--refresh "$g")
    done
    ;;
  shard)
    selection=(--shard "${1:?$usage}")
    ;;
  full) ;;
esac

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
list_mutants() {
  env -u CARGO_TARGET_DIR cargo mutants --list --colors never --package mega-evm2 --package op-evm2 \
    ${exclude_args[@]+"${exclude_args[@]}"} "$@"
}

# cargo-mutants builds in a copy of next/ under $TMPDIR, where the relative path dependency
# on the evm2 fork (`../../evm2`) would resolve beside $TMPDIR. A scratch $TMPDIR with a link
# to the fork one level up makes it resolve to the same checkout. The scratch directory is
# made under the caller's TMPDIR, outside next/: the copy includes next/target (`copy_target`),
# so a scratch directory under it (CARGO_TARGET_DIR unset) would be copied into itself.
scratch=$(mktemp -d "${TMPDIR:-/tmp}/mega-evm2-mutants.XXXXXX")
mkdir "$scratch/tmp"
ln -s "$(cd "$next/../../evm2" && pwd -P)" "$scratch/evm2"

# The starting point. cargo-mutants writes into $stage/mutants.out; finish() makes it the new
# OUT/mutants.out. A carried-over run seeds $stage with the names it skips (mutants-ledger.py).
rm -rf "$stage"
mkdir -p "$stage"
if [ "$iterate" = 1 ]; then
  outcome=carried
  prepare=(--results "$results" --stage "$stage" --refresh-file "$REFRESH_FILE")
  refresh_env=()
  read -r -a refresh_env <<< "${MUTANTS_REFRESH:-}" || true
  for g in ${refresh_env[@]+"${refresh_env[@]}"}; do prepare+=(--refresh "$g"); done
  prepare+=(${refresh[@]+"${refresh[@]}"})
  list_mutants > "$scratch/universe.txt"
  prepare+=(--universe "$scratch/universe.txt")
  carried_base=""
  [ ! -f "$results/carry-base.txt" ] || carried_base=$(tr -d '[:space:]' < "$results/carry-base.txt")
  if [ ! -d "$results" ]; then
    echo "carry-over: no earlier results in $results; starting afresh"
  elif [ -n "$carried_base" ] && git -C "$root" cat-file -e "$carried_base^{commit}" 2> /dev/null; then
    # The carried names refer to $carried_base: move them to their lines in this tree and drop
    # the mutants of changed code (any line of the mutant's span changed).
    engine_diff "$carried_base" > "$scratch/carry.diff"
    prepare+=(--diff "$scratch/carry.diff")
    if [ -s "$scratch/carry.diff" ]; then
      list_mutants --in-diff "$scratch/carry.diff" > "$scratch/changed.txt"
      prepare+=(--changed "$scratch/changed.txt")
    fi
    echo "carry-over: the results refer to $carried_base"
  else
    echo "WARNING: the commit the results in $results refer to is ${carried_base:-not recorded}${carried_base:+, which is not in this clone}; carrying them over by name, so a caught mutant whose function body changed without moving its first line is not tested again" >&2
  fi
  ledger prepare "${prepare[@]}"
fi

# budget_reached <when>: true (after printing the MUTANTS-BUDGET line) once the budget is spent.
# The previous results stay as they were.
budget_reached() {
  if [ -n "$budget" ] && [ "$SECONDS" -ge "$budget" ]; then
    echo "MUTANTS-BUDGET reached after ${SECONDS}s (budget ${budget}s) $1; $results is unchanged."
    rm -rf "$stage"
    return 0
  fi
  return 1
}

# The oracle's environment: the sample gate's main scenarios, replay and block execution.
# (One synthetic run takes one reference file, so the fuzz batches are not part of it.)
inputs=$target/mega-evm2-diff/mutants/gate-inputs
gate_inputs sample "$inputs"
gate_env "$inputs" main
env_main=("${GATE_ENV[@]}")
gate_env "$inputs" replay
export RUST_MIN_STACK=8388608
export "${env_main[@]}" "${GATE_ENV[@]}"
export TMPDIR=$scratch/tmp
if budget_reached "before the warm build"; then exit 0; fi
# CI toolchain setups export CARGO_INCREMENTAL=0, which would override the profile's choice.
unset CARGO_INCREMENTAL

# Each mutant builds in its own copy of next/ with its own `target/`. A shared
# CARGO_TARGET_DIR would make the parallel builds overwrite one another's test binaries and
# the outcomes would depend on timing, so cargo-mutants runs without it. `copy_target` seeds
# every copy from next/target, which is warmed here with the unmutated test binaries.
#
# The warm build also runs the whole oracle once on the unmutated tree. cargo-mutants' own
# baseline tests only the mutated packages, not the gate harness, so an oracle that failed
# without any mutation would mark every viable mutant caught, and a carried-over run would
# keep those results.
echo "+ cargo test --profile $profile (the oracle on the unmutated tree; warms next/target)"
if ! env -u CARGO_TARGET_DIR cargo test --profile "$profile" --locked \
  -p mega-evm2 -p op-evm2 -p mega-evm2-scenarios -p mega-evm2-diff; then
  echo "the oracle fails on the unmutated tree; no mutant would mean anything" >&2
  exit 1
fi

if budget_reached "before cargo-mutants started"; then exit 0; fi

iterate_args=()
[ "$iterate" = 0 ] || iterate_args=(--iterate)
# cargo-mutants runs in the background so that the budget can stop it: on SIGINT it terminates
# the builds and tests in flight (their mutants are tested again by the next run) and exits 1,
# leaving every finished outcome in its output directory.
env -u CARGO_TARGET_DIR cargo mutants \
  --package mega-evm2 --package op-evm2 \
  --profile "$profile" \
  --jobs "$JOBS" \
  --output "$stage" \
  --no-shuffle \
  --colors never \
  -vV \
  ${iterate_args[@]+"${iterate_args[@]}"} \
  ${exclude_args[@]+"${exclude_args[@]}"} \
  ${selection[@]+"${selection[@]}"} &
cm_pid=$!
stopped=0
if [ -n "$budget" ]; then
  while kill -0 "$cm_pid" 2> /dev/null; do
    if [ "$SECONDS" -ge "$budget" ]; then
      if kill -INT "$cm_pid" 2> /dev/null; then stopped=1; fi
      break
    fi
    sleep 2
  done
  if [ "$stopped" = 1 ]; then
    waited=0
    while kill -0 "$cm_pid" 2> /dev/null && [ "$waited" -lt 300 ]; do
      sleep 2
      waited=$((waited + 2))
    done
    kill -KILL "$cm_pid" 2> /dev/null || true
  fi
fi
rc=0
wait "$cm_pid" || rc=$?
cm_pid=""
# Stopped by the budget, cargo-mutants exits 1 (or 137 when it had to be killed); a run that ended
# by itself as the budget was reached keeps its own status.
if [ "$stopped" = 1 ] && [ "$rc" != 0 ] && [ "$rc" != 2 ] && [ "$rc" != 3 ]; then
  echo "MUTANTS-BUDGET reached after ${SECONDS}s (budget ${budget}s): stopped cargo-mutants; the outcomes so far are kept in $results, and a run with MUTANTS_ITERATE=1 continues from them."
  rc=0
fi
case "$rc" in
  0 | 2 | 3) ;;
  *) echo "cargo mutants failed with status $rc" >&2; exit "$rc" ;;
esac
echo "Mutation results written to $results"
