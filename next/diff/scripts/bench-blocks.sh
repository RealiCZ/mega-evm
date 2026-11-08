#!/bin/bash
# Build, compare and time both Satin block executors on exactly the gate's recorded inputs.
set -euo pipefail

case "${1:-}" in
  quick) mode=sample ;;
  full) mode=full ;;
  *) echo "usage: bench-blocks.sh <quick|full>" >&2; exit 2 ;;
esac
here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
source "$here/scripts/common.sh"
source "$here/scripts/gate-lib.sh"
refuse_overrides
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
work=$target/mega-evm2-diff/bench-$mode
export CARGO_TARGET_DIR=$target
export RUST_MIN_STACK=8388608
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}
export CARGO_NET_RETRY=0
gate_inputs "$mode" "$work"
gate_env "$work" block

(cd "$next" && cargo build --release --locked -p mega-evm2-diff --bin bench-blocks)
(cd "$next/reference" && CARGO_TARGET_DIR="$target/mega-evm2-reference" \
  cargo build --release --locked --bin bench-blocks)

# Generate current reference outcomes and compare all fields using the existing block gate.
(cd "$next/reference" && CARGO_TARGET_DIR="$target/mega-evm2-reference" \
  MEGA_EVM2_BLOCK_DIR="$work/blocks" MEGA_EVM2_BLOCK_OUT="$work/generated" \
  cargo test --release --locked --test block -- --nocapture) > "$work/reference-gate.log" 2>&1 \
  || { tail -30 "$work/reference-gate.log" >&2; exit 1; }
(cd "$next" && env "${GATE_ENV[@]}" MEGA_EVM2_BLOCK_REF="$work/generated" \
  cargo test --release --locked -p mega-evm2-diff --test block -- --nocapture) > "$work/engine-gate.log" 2>&1 \
  || { tail -30 "$work/engine-gate.log" >&2; exit 1; }
source "$work/expect.env"
grep -Fx "GATE block $EXPECT_BLOCK differ=0" "$work/engine-gate.log"

load() { python3 -c 'import datetime,os; print(datetime.datetime.now().isoformat(), "load_average=", os.getloadavg())'; }
case "${BENCH_ORDER:-engine-first}" in
  engine-first) sides=(engine reference) ;;
  reference-first) sides=(reference engine) ;;
  *) echo "BENCH_ORDER must be engine-first or reference-first" >&2; exit 2 ;;
esac
for side in "${sides[@]}"; do
  binary=$target/release/bench-blocks
  [ "$side" != reference ] || binary=$target/mega-evm2-reference/release/bench-blocks
  echo "$side before: $(load)"
  env "${GATE_ENV[@]}" "$binary" > "$work/$side.jsonl"
  echo "$side after: $(load)"
done
python3 -I "$here/scripts/bench-summary.py" "$work/engine.jsonl" "$work/reference.jsonl"
