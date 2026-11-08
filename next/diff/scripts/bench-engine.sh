#!/bin/bash
# Run the shared fixed workloads, comparing their outcomes before engine timing.
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
next=$(cd "$here/.." && pwd)
source "$here/scripts/common.sh"
refuse_overrides
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export RUST_MIN_STACK=8388608
export CARGO_NET_GIT_FETCH_WITH_CLI=${CARGO_NET_GIT_FETCH_WITH_CLI:-true}
export CARGO_NET_RETRY=0
export BENCH_WORKLOAD_RECORDS=$target/mega-evm2-diff/bench-workloads
mkdir -p "$BENCH_WORKLOAD_RECORDS"
(cd "$next" && CARGO_TARGET_DIR="$target" cargo bench --locked -p mega-evm2-diff --bench engine --no-run)
(cd "$next/reference" && CARGO_TARGET_DIR="$target/mega-evm2-reference" cargo bench --locked --bench engine --no-run)
load() { python3 -c 'import datetime,os; print(datetime.datetime.now().isoformat(), "load_average=", os.getloadavg())'; }
echo "reference before: $(load)"
(cd "$next/reference" && CARGO_TARGET_DIR="$target/mega-evm2-reference" cargo bench --locked --bench engine -- "$@")
echo "reference after: $(load)"
echo "engine before: $(load)"
(cd "$next" && CARGO_TARGET_DIR="$target" cargo bench --locked -p mega-evm2-diff --bench engine -- "$@")
echo "engine after: $(load)"
