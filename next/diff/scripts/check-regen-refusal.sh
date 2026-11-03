#!/bin/bash
# Checks that `regen-reference.sh` refuses an output directory in the corpus before it builds or
# writes anything, in `--check` and in default mode: the corpus's `reference/` directory, a
# symlink to it, a directory below it, the corpus itself, and a directory whose
# `reference-outputs.tar.zst` is a hard link or a symlink to a corpus archive. Writing there would
# replace the archive `--check` compares with (and `gate.sh full` reads), and `--check` would then
# compare the new outputs with themselves. Also checks that a directory outside the corpus gets
# as far as the build.
#
# Runs on a scratch corpus of small archives, with a `cargo` on PATH that only records that it was
# started; nothing is built. Prints `REGEN refusal ok`.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=stamp.sh
source "$here/scripts/stamp.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "check-regen-refusal.sh: $*" >&2; exit 1; }

# A scratch corpus whose archives hold the members `--check` extracts.
corpus=$tmp/corpus
mkdir -p "$corpus/real-blocks" "$corpus/reference" "$tmp/members/oracles" "$tmp/members/raw/satin" \
  "$tmp/members/raw/satin-block" "$tmp/bin"
for f in satin-oracle.json oracles/fq.json oracles/fqbig.json oracles/fqh.json oracles/fqhbig.json \
  oracles/big.json oracles/harsh.json; do
  echo '{}' > "$tmp/members/$f"
done
for name in reference-outputs q11-oracles; do
  echo "$name" > "$tmp/members/raw/satin/marker"
  tar -cf - -C "$tmp/members" satin-oracle.json oracles raw | zstd -q -o "$corpus/reference/$name.tar.zst"
done
cat > "$tmp/bin/cargo" << 'EOF'
#!/bin/sh
echo "cargo $*" >> "$CARGO_STARTED"
exit 1
EOF
chmod +x "$tmp/bin/cargo"
export CARGO_STARTED=$tmp/cargo-started

ln -s "$corpus/reference" "$tmp/alias"
mkdir -p "$tmp/hard" "$tmp/soft"
ln "$corpus/reference/reference-outputs.tar.zst" "$tmp/hard/reference-outputs.tar.zst"
ln -s "$corpus/reference/q11-oracles.tar.zst" "$tmp/soft/reference-outputs.tar.zst"

# Every path and the content of every file under the corpus.
corpus_state() { (cd "$corpus" && find . | LC_ALL=C sort && find . -type f -exec shasum -a 256 {} + | LC_ALL=C sort); }
before=$(corpus_state)
data=$(stamp_dir "$here/data")

# regen <args...>: runs regen-reference.sh on the scratch corpus; sets `status` and `log`.
regen() {
  status=0
  log=$(PATH="$tmp/bin:$PATH" MEGA_EVM2_CORPUS_DIR=$corpus CARGO_TARGET_DIR=$tmp/target \
    bash "$here/scripts/regen-reference.sh" "$@" 2>&1) || status=$?
}

for out in "$corpus/reference" "$tmp/alias" "$corpus/reference/new" "$corpus" "$tmp/hard" "$tmp/soft"; do
  for mode in --check default; do
    if [ "$mode" = --check ]; then regen --check "$out"; else regen "$out"; fi
    what="$mode $out"
    [ "$status" = 2 ] || fail "$what: exit status $status, not the refusal's 2: $log"
    grep -q 'corpus' <<< "$log" || fail "$what: the refusal does not name the corpus: $log"
    [ ! -e "$CARGO_STARTED" ] || fail "$what: cargo started before the refusal"
    [ ! -e "$tmp/target" ] || fail "$what: the target directory was created before the refusal"
    [ "$(corpus_state)" = "$before" ] || fail "$what: the corpus changed"
  done
done
[ ! -e "$corpus/reference/new" ] || fail "a refused run created its output directory"

# A directory outside the corpus is not refused: the run reaches the build (which the stand-in
# cargo fails), still without touching the corpus.
for mode in --check default; do
  rm -f "$CARGO_STARTED"
  if [ "$mode" = --check ]; then regen --check "$tmp/elsewhere"; else regen "$tmp/elsewhere"; fi
  [ -s "$CARGO_STARTED" ] || fail "$mode $tmp/elsewhere: refused or failed before the build: $log"
  [ "$(corpus_state)" = "$before" ] || fail "$mode $tmp/elsewhere: the corpus changed"
done
[ "$(stamp_dir "$here/data")" = "$data" ] || fail "next/diff/data changed"
echo "REGEN refusal ok"
