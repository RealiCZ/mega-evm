#!/usr/bin/env bash
# Coverage of the engine crates of `next/` (mega-evm2, op-evm2), with branch coverage.
#
# Usage: coverage.sh
#   COV_DIR           output directory (default: <target>/coverage/next); receives
#                     lcov.info, summary.txt (llvm-cov report with a TOTAL line, plus
#                     per-crate line and branch totals) and html/.
#   CARGO_TARGET_DIR  cargo target directory (default: next/target).
#
# What runs: the unit and integration tests of the engine crates, then the sample gate's
# test targets (synthetic, replay, block) on the in-repo sample, which execute every
# scenario, fuzz seed and recorded block through the engine. The diff harness is only the
# driver: its files, tests, benches and the evm2 fork are left out of the report.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
next=$(cd "$here/.." && pwd)
# shellcheck source=../diff/scripts/common.sh
source "$next/diff/scripts/common.sh"
# shellcheck source=../diff/scripts/gate-lib.sh
source "$next/diff/scripts/gate-lib.sh"
refuse_overrides

cd "$next"
target=$(abs_dir "${CARGO_TARGET_DIR:-$next/target}")
export CARGO_TARGET_DIR=$target
COV_DIR=$(abs_dir "${COV_DIR:-$target/coverage/next}")
IGNORE_FILENAME_REGEX="${IGNORE_FILENAME_REGEX:-(/tests/|/benches/|/examples/|/diff/|/\\.cargo/registry/|/\\.cargo/git/|/rustc/|/evm2/crates/)}"

if ! cargo llvm-cov --version > /dev/null 2>&1; then
  echo "cargo-llvm-cov is required. Install it with 'cargo install cargo-llvm-cov --locked'." >&2
  exit 1
fi
rustup component add llvm-tools-preview > /dev/null 2>&1 || true

rm -rf "$COV_DIR"
mkdir -p "$COV_DIR"
cargo llvm-cov clean --workspace

run() {
  printf '+ %s\n' "$*"
  "$@"
}

# The tests of the engine crates and of the scenario library.
run cargo llvm-cov --locked --branch --no-report --no-fail-fast \
  -p mega-evm2 -p op-evm2 -p mega-evm2-scenarios
run cargo llvm-cov --locked --branch --no-report --no-fail-fast -p mega-evm2-diff --lib

# The sample gate: the same inputs, environments and test targets as `gate.sh sample`.
inputs=$target/mega-evm2-diff/coverage/gate-inputs
gate_inputs sample "$inputs"
# shellcheck disable=SC1091
source "$inputs/expect.env"

gate_cov() {
  local test=$1
  shift
  run env RUST_MIN_STACK=8388608 "$@" \
    cargo llvm-cov --locked --branch --no-report -p mega-evm2-diff --test "$test" -- --nocapture
}

count=${EXPECT_MAIN#total=}
gate_env "$inputs" main
gate_cov synthetic "${GATE_ENV[@]}" MEGA_EVM2_EXPECT="${count%% *}"
for spec in $FUZZ_BATCHES; do
  gate_env "$inputs" fuzz "$spec"
  gate_cov synthetic "${GATE_ENV[@]}" MEGA_EVM2_EXPECT="${spec##*:}"
done
gate_env "$inputs" replay
gate_cov replay "${GATE_ENV[@]}"
gate_cov block "${GATE_ENV[@]}"

report() {
  cargo llvm-cov report --branch --ignore-filename-regex "$IGNORE_FILENAME_REGEX" "$@"
}
report --lcov --output-path "$COV_DIR/lcov.info"
report --html --output-dir "$COV_DIR"
report > "$COV_DIR/summary.txt"

# Per-crate line and branch totals from the lcov file.
python3 -I - "$COV_DIR/lcov.info" >> "$COV_DIR/summary.txt" << 'PY'
import re, sys
tot = {}
crate = None
for line in open(sys.argv[1]):
    line = line.strip()
    if line.startswith("SF:"):
        m = re.search(r"/next/([^/]+)/", line) or re.search(r"/(op-evm2|mega-evm2)/", line)
        crate = m.group(1) if m else "other"
        tot.setdefault(crate, [0, 0, 0, 0])
    elif crate and line.split(":")[0] in ("LF", "LH", "BRF", "BRH"):
        k, v = line.split(":")
        tot[crate][("LF", "LH", "BRF", "BRH").index(k)] += int(v)
print()
print("PER-CRATE (lcov)")
for c, (lf, lh, bf, bh) in sorted(tot.items()):
    lp = 100 * lh / lf if lf else 0
    bp = 100 * bh / bf if bf else 0
    print(f"{c}: lines {lh}/{lf} ({lp:.2f}%) branches {bh}/{bf} ({bp:.2f}%)")
PY

echo
echo "Coverage artifacts written to $COV_DIR"
grep -E 'TOTAL' "$COV_DIR/summary.txt"
