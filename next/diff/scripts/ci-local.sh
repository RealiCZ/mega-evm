#!/bin/bash
# Runs the check job of `.github/workflows/next.yml` from `next/`.
#
# Usage: ci-local.sh
#   The workflow calls this script, so the commands below are the job.
#   CARGO_TARGET_DIR, when set, is the cargo target directory; otherwise a new
#   directory under the system temp is used. The first failing command stops the run.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
next=$(cd "$here/../.." && pwd)

if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  CARGO_TARGET_DIR=$(mktemp -d "${TMPDIR:-/tmp}/mega-evm2-ci.XXXXXX")
fi
mkdir -p "$CARGO_TARGET_DIR"
case "$CARGO_TARGET_DIR" in
  /*) ;;
  *) CARGO_TARGET_DIR=$(cd "$CARGO_TARGET_DIR" && pwd) ;;
esac
export CARGO_TARGET_DIR

cd "$next"
echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
rustc --version
rustup target add riscv64imac-unknown-none-elf

run() {
  printf '+ %s\n' "$*"
  "$@"
}

run cargo fmt --all --check
run cargo clippy -p mega-evm2 --all-targets --locked -- -D warnings
run cargo clippy -p mega-evm2 --all-targets --features compare --locked -- -D warnings
run cargo clippy -p op-evm2 --all-targets --locked -- -D warnings
run cargo clippy -p mega-evm2-diff --all-targets --locked -- -D warnings
run cargo clippy -p mega-evm2-scenarios --all-targets --locked -- -D warnings
run cargo test --locked -p mega-evm2 -p op-evm2 -p mega-evm2-scenarios
run cargo test --locked -p mega-evm2 --features compare --lib
run cargo test --locked -p mega-evm2-diff --lib
run cargo test --locked -p mega-evm2-diff --test known_differences
run cargo test --locked -p mega-evm2-diff --test give_back
run cargo test --locked -p mega-evm2-diff --test create2_salt_order
run cargo bench --locked -p mega-evm2-diff --bench engine --no-run
run cargo check --locked -p op-evm2 --target riscv64imac-unknown-none-elf --no-default-features
run bash diff/scripts/check-regen-refusal.sh
run bash diff/scripts/gate.sh sample
run bash diff/scripts/inspector-blocks.sh
run bash diff/scripts/controls.sh sample
run bash diff/scripts/faults.sh
