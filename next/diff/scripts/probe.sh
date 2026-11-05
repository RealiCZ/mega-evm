#!/bin/bash
# Runs probe inputs on the reference and on the Satin engine, and compares each as the synthetic
# gate does.
#
# Usage: probe.sh <file.jsonl>
#
# Each non-blank line of <file.jsonl> that does not start with `#` is one input: a scenario by name
# with overrides, or a whole scenario (the keys are in `next/diff/shared/src/probe.rs`;
# `next/diff/probe-examples.jsonl` has examples). The reference (`next/reference`, binary `probe`)
# writes its record of every input, then the engine side (`mega-evm2-diff`, binary `probe`) runs
# the same inputs and prints, per input, both records and the gate's verdict: `MATCH`, `KNOWN` (an
# accepted difference of `known-differences.toml`, by name) or `DIFFER [<fields>]` with each
# differing field's two values; last, `PROBE total=<n> match=<n> known=<n> differ=<n>`.
#
# Exit status: 0 when no input differs, 1 when one does, 2 on unreadable input or a failed build.
# The reference's build fetches its pinned revisions from GitHub, as `check-reference.sh` does.
# `MEGA_EVM2_DISABLE=<switch>,...` turns engine switches off, as in the gates. Builds and the
# reference's records are under `<target>/mega-evm2-diff/probe`.
set -euo pipefail

[ $# = 1 ] && [ -f "$1" ] || { echo "usage: probe.sh <file.jsonl>" >&2; exit 2; }

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
input=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
work=$(fresh_dir "$target/mega-evm2-diff/probe")
export RUST_MIN_STACK=8388608
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}

# build <log> <dir> <cargo target> <cargo args...>: builds a probe binary, the log's tail on failure.
build() {
  local log=$1 dir=$2 dest=$3
  shift 3
  (cd "$dir" && CARGO_TARGET_DIR=$dest cargo build --release --locked "$@") > "$log" 2>&1 ||
    { echo "probe.sh: build failed; tail of $log:" >&2; tail -n 25 "$log" >&2; exit 2; }
}

build "$work/reference-build.log" "$next/reference" "$target/mega-evm2-reference" --bin probe
build "$work/engine-build.log" "$next" "$target" -p mega-evm2-diff --bin probe
"$target/mega-evm2-reference/release/probe" "$input" "$work/reference.jsonl" ||
  { echo "probe.sh: the reference failed on $input" >&2; exit 2; }
status=0
"$target/release/probe" "$input" "$work/reference.jsonl" || status=$?
exit "$status"
