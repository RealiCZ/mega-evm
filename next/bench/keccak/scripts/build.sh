#!/bin/bash
# Builds every binary the Keccak benchmark runs into <bin-dir>:
#
#   <bin-dir>/micro/<isa>/{kec-micro,prefix-bench,trace-cache,isa-probe}   one per ISA variant
#   <bin-dir>/micro/cache/validate-cache                                   generic build
#   <bin-dir>/block/<variant>/{kec-block,kec-trace}                        one per block variant
#
# Usage: build.sh <bin-dir> <isa-variants> <block-variants>
#   <isa-variants>    comma-separated: generic, x86-64-v3, x86-64-v4 (x86-64), sha3 (aarch64)
#   <block-variants>  comma-separated: baseline, asm, cache, asm-cache, asm-sha3 (aarch64)
#
# Each ISA flag set gets its own target directory, so no build reuses another's artifacts.
# `generic` is the target's default CPU (x86-64 or generic aarch64): never `target-cpu=native`.
set -euo pipefail

bin=$(mkdir -p "$1" && cd "$1" && pwd)
isas=$2
variants=$3
here=$(cd "$(dirname "$0")/.." && pwd)
target=${CARGO_TARGET_DIR:-$here/target}
unset CARGO_TARGET_DIR

isa_flags() {
  case "$1" in
    generic) echo "" ;;
    x86-64-v3) echo "-C target-cpu=x86-64-v3" ;;
    x86-64-v4) echo "-C target-cpu=x86-64-v4" ;;
    sha3) echo "-C target-feature=+sha3" ;;
    *) echo "unknown ISA variant $1" >&2; return 2 ;;
  esac
}

build() {
  local dir=$1 flags=$2
  shift 2
  (cd "$here" && RUSTFLAGS="$flags" CARGO_TARGET_DIR="$target/$dir" cargo build --release --locked "$@")
}

IFS=, read -ra isa_list <<< "$isas"
for isa in "${isa_list[@]}"; do
  flags=$(isa_flags "$isa")
  echo "== micro $isa (RUSTFLAGS='$flags')"
  build "micro-$isa" "$flags" -p kec-micro --bins
  mkdir -p "$bin/micro/$isa"
  for b in kec-micro prefix-bench trace-cache isa-probe; do
    cp "$target/micro-$isa/release/$b" "$bin/micro/$isa/$b"
  done
done

echo "== micro cache validation (generic)"
build micro-generic "" -p kec-micro --features cache --bin validate-cache
mkdir -p "$bin/micro/cache"
cp "$target/micro-generic/release/validate-cache" "$bin/micro/cache/validate-cache"

IFS=, read -ra variant_list <<< "$variants"
for v in "${variant_list[@]}"; do
  flags=""
  dir=block-generic
  case "$v" in
    baseline) features="" ;;
    asm) features=asm ;;
    cache) features=cache ;;
    asm-cache) features=asm,cache ;;
    asm-sha3) features=asm flags=$(isa_flags sha3) dir=block-sha3 ;;
    *) echo "unknown block variant $v" >&2; exit 2 ;;
  esac
  echo "== block $v (features='$features', RUSTFLAGS='$flags')"
  if [ -n "$features" ]; then
    build "$dir" "$flags" -p kec-block --bins --features "$features"
  else
    build "$dir" "$flags" -p kec-block --bins
  fi
  mkdir -p "$bin/block/$v"
  for b in kec-block kec-trace; do
    cp "$target/$dir/release/$b" "$bin/block/$v/$b"
  done
done
echo "built into $bin"
