#!/bin/bash
# Runs the recorded blocks of the sample's block gate plain and under a heavy combination of the
# inspector crate's tracers (`tests/inspector_blocks.rs`) and prints its summary line:
#
#   INSPECTED blocks=<b> txs=<t> traced=<n> equal=<b>
#
# Usage: inspector-blocks.sh
#   The inputs are the sample's (`next/diff/data`), extracted afresh into
#   `<target>/mega-evm2-diff/inspector-blocks/`; the test's output is in `inspector-blocks.log`
#   there. It refuses to start when a `MEGA_EVM2_*` variable other than `MEGA_EVM2_CORPUS_DIR` is
#   set (see `common.sh`).
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
# shellcheck source=gate-lib.sh
source "$here/scripts/gate-lib.sh"
refuse_overrides
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export CARGO_TARGET_DIR=$target
work=$target/mega-evm2-diff/inspector-blocks
inputs=$work/inputs
gate_inputs sample "$inputs"
gate_env "$inputs" replay
log=$work/inspector-blocks.log
if ! (cd "$next" && env RUST_MIN_STACK=8388608 "${GATE_ENV[@]}" \
  cargo test --release --locked -p mega-evm2-diff --test inspector_blocks -- --nocapture) > "$log" 2>&1; then
  echo "inspector-blocks.sh: the test failed; tail of $log:" >&2
  tail -n 30 "$log" >&2
  exit 1
fi
line=$(grep '^INSPECTED ' "$log" || true)
if [ "$(grep -c '^INSPECTED ' <<< "$line")" != 1 ]; then
  echo "inspector-blocks.sh: no single INSPECTED line (see $log)" >&2
  exit 1
fi
echo "$line"
