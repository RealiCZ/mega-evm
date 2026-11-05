#!/bin/bash
# Checks that `regen-reference.sh` refuses, before building or writing anything, the realistic
# mistake of pointing its output at the corpus: `--check` against `<corpus>/reference` would replace
# the expected archive and compare the new outputs with themselves. Refused with exit status 2, in
# `--check` and default mode, with the corpus unchanged:
#   - the output directory is `<corpus>/reference`;
#   - the output directory is a symlink to it;
#   - the archive path (`<out-dir>/reference-outputs.tar.zst`) is a hard link to a corpus archive.
# A positive control: an output directory outside the corpus gets as far as the build.
#
# Runs on a scratch corpus of small archives, with a `cargo` on PATH that records that it was
# started and fails; nothing is built. Prints `REGEN refusal ok`.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=common.sh
source "$here/scripts/common.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failed=0
fail() {
  echo "check-regen-refusal.sh: $*" >&2
  failed=1
}

# A scratch corpus: both archives (with the members `--check` extracts) and a block file.
corpus=$tmp/corpus
mkdir -p "$tmp/members/oracles" "$tmp/members/raw/satin" "$tmp/members/raw/satin-block" "$tmp/bin" \
  "$corpus/real-blocks" "$corpus/reference"
members=(satin-oracle.json)
for spec in "${FULL_FUZZ_BATCHES[@]}"; do members+=("oracles/${spec%%:*}.json"); done
for f in "${members[@]}"; do
  echo '{}' > "$tmp/members/$f"
done
echo keep > "$corpus/real-blocks/keep.json"
for name in reference-outputs q11-oracles; do
  echo "$name" > "$tmp/members/raw/satin/marker"
  tar -cf - -C "$tmp/members" satin-oracle.json oracles raw | zstd -q -o "$corpus/reference/$name.tar.zst"
done
printf '#!/bin/sh\necho started >> "$CARGO_STARTED"\nexit 1\n' > "$tmp/bin/cargo"
chmod +x "$tmp/bin/cargo"
export CARGO_STARTED=$tmp/cargo-started

# state: every path of the corpus, and the content of every file.
state() { (cd "$corpus" && find . | LC_ALL=C sort && find . -type f -exec shasum -a 256 {} + | LC_ALL=C sort); }

# regen <mode> <out-dir>: runs regen-reference.sh with the stand-in cargo; sets `status` and `log`.
regen() {
  local args=("$2")
  [ "$1" = default ] || args=(--check "$2")
  status=0
  rm -f "$CARGO_STARTED"
  log=$(PATH="$tmp/bin:$PATH" MEGA_EVM2_CORPUS_DIR=$corpus CARGO_TARGET_DIR=$tmp/target \
    bash "$here/scripts/regen-reference.sh" "${args[@]}" 2>&1) || status=$?
}

# refused <out-dir>: in both modes the run exits 2 before cargo starts and the corpus is unchanged.
refused() {
  local mode before
  for mode in --check default; do
    before=$(state)
    regen "$mode" "$1"
    [ "$status" = 2 ] || fail "$mode $1: exit status $status, not the refusal's 2: $log"
    [ ! -e "$CARGO_STARTED" ] || fail "$mode $1: cargo started before the refusal"
    [ "$(state)" = "$before" ] || fail "$mode $1: the corpus changed"
  done
}

refused "$corpus/reference"
ln -s "$corpus/reference" "$tmp/alias"
refused "$tmp/alias"
mkdir "$tmp/hard"
ln "$corpus/reference/reference-outputs.tar.zst" "$tmp/hard/reference-outputs.tar.zst"
refused "$tmp/hard"

# Positive control: an output directory outside the corpus reaches the build.
for mode in --check default; do
  before=$(state)
  regen "$mode" "$tmp/outside"
  [ -s "$CARGO_STARTED" ] || fail "$mode $tmp/outside: refused or failed before the build: $log"
  [ "$(state)" = "$before" ] || fail "$mode $tmp/outside: the corpus changed"
done

[ "$failed" = 0 ] || exit 1
echo "REGEN refusal ok"
