#!/usr/bin/env bash
# Mutation testing of the engine crates of `next/` (mega-evm2, op-evm2), with the sample
# gate as the oracle: the test command is `cargo test` over the engine crates and the diff
# harness, run with the sample gate's inputs (synthetic scenarios, replay, block execution).
#
# Usage: mutants.sh <mode>
#        mutants.sh list <mode>   print the mutants <mode> selects, one per line; nothing is built
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
# Every run starts afresh: the results of an earlier run in OUT are removed first.
#
# Environment:
#   OUT               output directory (default: <target>/mutants-out); the results are OUT/mutants.out/
#                     (outcomes.json, caught.txt, missed.txt, ..., and inventory.txt and status.json)
#   CARGO_TARGET_DIR  cargo target directory (default: next/target)
#   JOBS              parallel mutants (default: half the CPUs)
#   SUPPRESS          suppressions file (default: next/diff/mutants/suppressions.toml)
#   MUTANTS_PROFILE   cargo profile of the warm build, the mutant builds and the oracle (default:
#                     `mutants`, next/Cargo.toml). CARGO_INCREMENTAL is unset: the profile decides.
#   MUTANTS_TEST_MEMORY_MB  memory cap of each test binary of the oracle (default 4096; 0 for none),
#                     applied by scripts/mutants-test-limit.sh as cargo's runner for the host target
#   MUTANTS_BUDGET=<s> the run's deadline, <s> seconds after this script started. It supervises every
#                     expensive phase: listing the mutants, extracting the oracle's inputs, the oracle
#                     on the unmutated tree (which warms the build) and cargo-mutants. At the deadline
#                     the phase in flight gets SIGINT (its whole process group), and SIGKILL
#                     MUTANTS_GRACE seconds later (default 300); a line starting `MUTANTS-BUDGET
#                     reached` is printed, the outcomes so far are kept and the exit status is 0.
#
# Before anything is built, the run lists the mutants it selects: its inventory, kept as
# OUT/mutants.out/inventory.txt. The last two lines of every run are
#   MUTANTS-STATUS status=<complete|partial|not-started|baseline-failed> expected=<n|unknown> outcomes=<n>
#   MUTANTS-RESULT caught=<n> missed=<n> timeout=<n> unviable=<n> tested=<n> skipped=0
# complete: every mutant of the inventory has an outcome, also when the inventory is empty (nothing
# to mutate); partial: some have; not-started: none has, because the deadline or a signal stopped
# the run first; baseline-failed: the oracle fails on the unmutated tree. expected is the size of
# the inventory (unknown when the run stopped before listing it), outcomes the number of mutants
# with an outcome. OUT/mutants.out/status.json records the same with the run's identity (commit,
# selection, cargo-mutants version), which scripts/mutants-results.py merge checks.
#
# The warm build goes to next/target (ignored by git); the oracle inputs and OUT use CARGO_TARGET_DIR.
# The scratch directory is a new one under TMPDIR, removed at exit.
#
# Exit status: 0 when the run completed or reached its deadline, whatever the outcomes
# (cargo-mutants exits 2 for missed and 3 for timed-out mutants); scoring them is
# mutation_gate.py's job. 1 when the oracle fails on the unmutated tree, 4 when cargo-mutants'
# own baseline fails, any other status of cargo-mutants as it is (a bad diff, an internal error),
# 130 after SIGINT and 143 after SIGTERM.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
next=$(cd "$here/.." && pwd)
root=$(cd "$next/.." && pwd)
# shellcheck source=../diff/scripts/common.sh
source "$next/diff/scripts/common.sh"
# shellcheck source=../diff/scripts/gate-lib.sh
source "$next/diff/scripts/gate-lib.sh"
refuse_overrides

usage="usage: mutants.sh [list] smoke | pr <base-ref> | push <before-sha> | files <glob>... | shard <k>/<n> | full"
list_only=0
if [ "${1:-}" = list ]; then
  list_only=1
  shift
fi
mode=${1:-}
[ $# -eq 0 ] || shift
case "$mode" in
  smoke | pr | push | files | shard | full) ;;
  *) echo "$usage" >&2; exit 2 ;;
esac
selection_desc="$mode${*:+ $*}"
# A list prints the mutants on stdout and everything else on stderr.
[ "$list_only" = 0 ] || exec 3>&1 1>&2
cd "$next"
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export CARGO_TARGET_DIR=$target
OUT=$(abs_dir "${OUT:-$target/mutants-out}")
SUPPRESS=${SUPPRESS:-$next/diff/mutants/suppressions.toml}
profile=${MUTANTS_PROFILE:-mutants}
results=$OUT/mutants.out
inventory=$OUT/inventory.txt
budget=${MUTANTS_BUDGET:-}
case "$budget" in
  "" | *[!0-9]*) [ -z "$budget" ] || { echo "MUTANTS_BUDGET must be a number of seconds" >&2; exit 2; } ;;
esac
grace=${MUTANTS_GRACE:-300}
case "$grace" in
  "" | *[!0-9]*) echo "MUTANTS_GRACE must be a number of seconds" >&2; exit 2 ;;
esac
if [ -z "${JOBS:-}" ]; then
  JOBS=$(( $(getconf _NPROCESSORS_ONLN) / 2 ))
  [ "$JOBS" -ge 1 ] || JOBS=1
fi

if ! tool=$(cargo mutants --version 2> /dev/null); then
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

# engine_diff <git diff revision args...>: the changes to the engine sources, with paths relative
# to next/ (the way cargo-mutants names its files) and fixed a/ b/ prefixes.
engine_diff() {
  git -C "$next" -c diff.noprefix=false -c diff.mnemonicPrefix=false diff --no-color --no-ext-diff \
    --no-renames --relative --src-prefix=a/ --dst-prefix=b/ "$@" -- mega-evm2/src op-evm2/src
}

# What the status line at exit is made of: the phase in flight, whether the inventory was listed,
# whether the oracle failed on the unmutated tree, and what stopped the run early.
phase="the selection"
listed=0
baseline_failed=0
stopped=""
scratch=""
child=""

# stop_child <grace>: SIGINT to the process group of the phase in flight, SIGKILL after <grace>
# seconds (and to whatever of the group is left once its leader has exited).
stop_child() {
  [ -n "$child" ] || return 0
  kill -INT -- "-$child" 2> /dev/null || true
  local waited=0
  while kill -0 "$child" 2> /dev/null && [ "$waited" -lt "$1" ]; do
    sleep 1
    waited=$((waited + 1))
  done
  kill -KILL -- "-$child" 2> /dev/null || true
}

# deadline_passed: true (after printing the MUTANTS-BUDGET line) once the deadline has passed.
deadline_passed() {
  if [ -n "$budget" ] && [ "$SECONDS" -ge "$budget" ]; then
    stopped=deadline
    echo "MUTANTS-BUDGET reached after ${SECONDS}s (budget ${budget}s) $1 $phase."
    return 0
  fi
  return 1
}

# supervise <command...>: runs one expensive phase in its own process group under the deadline
# and returns its exit status. When the deadline passes before or while it runs, `stopped` is set
# to `deadline`; a phase in flight is stopped with stop_child.
supervise() {
  local rc=0
  if deadline_passed before; then return 1; fi
  set -m
  "$@" < /dev/null &
  child=$!
  set +m
  while kill -0 "$child" 2> /dev/null; do
    if deadline_passed during; then
      stop_child "$grace"
      break
    fi
    sleep 1
  done
  wait "$child" || rc=$?
  child=""
  return "$rc"
}

finish() {
  local rc=$?
  trap - EXIT INT TERM
  stop_child 10
  [ -z "$scratch" ] || rm -rf "$scratch"
  [ "$list_only" = 0 ] || exit "$rc"
  local args=(--results "$results" --selection "$selection_desc" --tool "$tool" --phase "$phase")
  [ "$listed" = 0 ] || args+=(--inventory "$inventory")
  [ "$baseline_failed" = 0 ] || args+=(--baseline-failed)
  [ -z "$stopped" ] || args+=(--stopped "$stopped")
  if git -C "$next" rev-parse --verify -q HEAD > /dev/null; then
    args+=(--commit "$(git -C "$next" rev-parse HEAD)")
    [ -z "$(git -C "$next" status --porcelain -- .)" ] || args+=(--dirty)
  fi
  "$py" -I "$here/mutants-results.py" status "${args[@]}" || [ "$rc" != 0 ] || rc=1
  exit "$rc"
}
# A signal (a cancelled CI job) stops the phase in flight within a few seconds, then exits
# through finish with the signal's status.
on_signal() {
  stopped=signal
  echo "mutants.sh: $1 during $phase; stopping it" >&2
  stop_child 10
  exit "$2"
}
trap finish EXIT
trap 'on_signal SIGINT 130' INT
trap 'on_signal SIGTERM 143' TERM

# A run replaces the previous results, also when it ends before cargo-mutants writes anything,
# so they are never read as this run's.
if [ "$list_only" = 0 ]; then
  rm -rf "$results" "$results.old" "$inventory" "$OUT.diff"
fi
scratch=$(mktemp -d "${TMPDIR:-/tmp}/mega-evm2-mutants.XXXXXX")

# nothing_to_mutate <why>: ends a run whose selection is explicitly empty, as complete.
nothing_to_mutate() {
  echo "$1; nothing to mutate."
  if [ "$list_only" = 0 ]; then
    : > "$inventory"
    listed=1
  fi
  exit 0
}

selection=()
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
          nothing_to_mutate "push base: none ($why, and the fallback $fallback does not resolve)"
        fi
        echo "push base: $base (the stated fallback $fallback, because $why)"
      fi
      range=("$base" HEAD)
    fi
    diff_file="$OUT.diff"
    [ "$list_only" = 0 ] || diff_file=$scratch/engine.diff
    engine_diff "${range[@]}" > "$diff_file"
    [ -s "$diff_file" ] || nothing_to_mutate "No engine changes against $base"
    selection=(--in-diff "$diff_file")
    ;;
  files)
    [ $# -ge 1 ] || { echo "$usage" >&2; exit 2; }
    for g in "$@"; do selection+=(-f "$g"); done
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
# list_mutants <file>: the mutants of this run's selection, as cargo-mutants names them, into <file>.
list_mutants() {
  env -u CARGO_TARGET_DIR cargo mutants --list --colors never --package mega-evm2 --package op-evm2 \
    ${exclude_args[@]+"${exclude_args[@]}"} ${selection[@]+"${selection[@]}"} > "$1"
}

if [ "$list_only" = 1 ]; then
  list_mutants /dev/fd/3
  exit 0
fi

# The inventory: what this run sets out to test, listed before anything is built.
phase="listing the mutants"
rc=0
supervise list_mutants "$inventory" || rc=$?
if [ "$stopped" = deadline ]; then exit 0; fi
if [ "$rc" != 0 ]; then
  echo "listing the mutants failed with status $rc" >&2
  exit "$rc"
fi
listed=1
echo "inventory: $(wc -l < "$inventory" | tr -d ' ') mutants ($selection_desc)"
[ -s "$inventory" ] || nothing_to_mutate "No mutants in this selection"

# cargo-mutants builds in a copy of next/ under $TMPDIR, where the relative path dependency
# on the evm2 fork (`../../evm2`) would resolve beside $TMPDIR. A scratch $TMPDIR with a link
# to the fork one level up makes it resolve to the same checkout. The scratch directory is
# made under the caller's TMPDIR, outside next/: the copy includes next/target (`copy_target`),
# so a scratch directory under it (CARGO_TARGET_DIR unset) would be copied into itself.
mkdir "$scratch/tmp"
ln -s "$(cd "$next/../../evm2" && pwd -P)" "$scratch/evm2"

# The oracle's environment: the sample gate's main scenarios, replay and block execution.
# (One synthetic run takes one reference file, so the fuzz batches are not part of it.)
phase="extracting the oracle's inputs"
inputs=$target/mega-evm2-diff/mutants/gate-inputs
rc=0
supervise gate_inputs sample "$inputs" || rc=$?
if [ "$stopped" = deadline ]; then exit 0; fi
[ "$rc" = 0 ] || { echo "extracting the oracle's inputs failed with status $rc" >&2; exit "$rc"; }
gate_env "$inputs" main
env_main=("${GATE_ENV[@]}")
gate_env "$inputs" replay
export RUST_MIN_STACK=8388608
export "${env_main[@]}" "${GATE_ENV[@]}"
export TMPDIR=$scratch/tmp
# CI toolchain setups export CARGO_INCREMENTAL=0, which would override the profile's choice.
unset CARGO_INCREMENTAL
# Every test binary of the oracle runs under a memory cap (enforced by Linux, as RLIMIT_DATA). A
# mutant that loops while allocating, such as a parser stepping back onto the value it just read
# and pushing it again, otherwise grows until the CI runner is shut down and every result of its
# shard is lost; under the cap its test aborts on a failed allocation and it counts as caught. The
# unmutated oracle peaks near 1 GiB in one test binary, and two run at a time on a runner with
# 16 gigabytes of memory.
export MUTANTS_TEST_MEMORY_MB=${MUTANTS_TEST_MEMORY_MB:-4096}
host=$(rustc -vV | sed -n 's/^host: //p')
export "CARGO_TARGET_$(printf '%s' "$host" | tr 'a-z-' 'A-Z_')_RUNNER=$here/mutants-test-limit.sh"

# Each mutant builds in its own copy of next/ with its own `target/`. A shared
# CARGO_TARGET_DIR would make the parallel builds overwrite one another's test binaries and
# the outcomes would depend on timing, so cargo-mutants runs without it. `copy_target` seeds
# every copy from next/target, which is warmed here with the unmutated test binaries.
#
# The warm build also runs the whole oracle once on the unmutated tree. cargo-mutants' own
# baseline tests only the mutated packages, not the gate harness, so an oracle that failed
# without any mutation would mark every viable mutant caught.
phase="the oracle on the unmutated tree"
echo "+ cargo test --profile $profile (the oracle on the unmutated tree; warms next/target)"
rc=0
supervise env -u CARGO_TARGET_DIR cargo test --profile "$profile" --locked \
  -p mega-evm2 -p op-evm2 -p mega-evm2-scenarios -p mega-evm2-diff || rc=$?
if [ "$stopped" = deadline ]; then exit 0; fi
if [ "$rc" != 0 ]; then
  echo "the oracle fails on the unmutated tree; no mutant would mean anything" >&2
  baseline_failed=1
  exit 1
fi

# On SIGINT cargo-mutants terminates the builds and tests in flight and exits 1, leaving every
# finished outcome in its output directory.
phase=cargo-mutants
rc=0
supervise env -u CARGO_TARGET_DIR cargo mutants \
  --package mega-evm2 --package op-evm2 \
  --profile "$profile" \
  --jobs "$JOBS" \
  --output "$OUT" \
  --no-shuffle \
  --colors never \
  -vV \
  ${exclude_args[@]+"${exclude_args[@]}"} \
  ${selection[@]+"${selection[@]}"} || rc=$?
# Stopped by the deadline, cargo-mutants exits 1 (130 when the signal came before its handler,
# 137 when it had to be killed); every other status keeps its meaning, also when the deadline
# came at the same time.
if [ "$stopped" = deadline ]; then
  case "$rc" in
    1 | 130 | 137)
      echo "MUTANTS-BUDGET reached: stopped cargo-mutants; the outcomes so far are kept in $results."
      exit 0
      ;;
  esac
fi
case "$rc" in
  0 | 2 | 3) ;;
  4)
    echo "cargo-mutants' baseline fails on the unmutated tree" >&2
    baseline_failed=1
    exit 4
    ;;
  *) echo "cargo mutants failed with status $rc" >&2; exit "$rc" ;;
esac
phase=done
echo "Mutation results written to $results"
