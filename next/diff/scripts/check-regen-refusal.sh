#!/bin/bash
# Checks that a run of `regen-reference.sh`, in `--check` and in default mode, deletes nothing in
# the corpus and writes nothing into it, whatever paths it is given. Refused with exit status 2,
# before anything is built or written:
#   - an output directory in the corpus: its `reference/` directory, a symlink to it, a directory
#     below it (also spelled in other letter case, where the file system ignores case, and below
#     a corpus whose name ends in a line break), the corpus itself;
#   - an archive path (`<out-dir>/reference-outputs.tar.zst`) that is a hard link or a symlink to a
#     corpus archive, a symlink to another corpus file or to a missing path in the corpus, the
#     corpus itself, a directory, or a symlink to a directory (the corpus's `reference/`, or one
#     outside the corpus);
#   - a cargo target directory in the corpus, one whose `mega-evm2-reference` (where cargo
#     builds) is the corpus, or one holding a link into the corpus (a symlink to the corpus or to a
#     missing path in it, a hard link to a corpus file);
#   - in default mode, a corpus below `next/diff/data` or holding it (the default mode rewrites it);
#   - the same through a corpus made of symlinks to another one (an output directory in the linked
#     corpus; a corpus that links into `next/diff/data`), and a corpus with a symlink that leads
#     nowhere (the run could create its target).
# Writing there would delete or replace corpus files, or write into a linked directory; for the
# corpus archive, `--check` would then compare the new outputs with themselves.
# Not refused, and left unchanged: a corpus elsewhere inside the output directory, at
# `<out-dir>/stage`, deeper below it, or as another child. The run writes there only a new
# directory of its own and the archive path, so no path it writes reaches such a corpus. Nor a
# corpus with a symlink cycle, which the check walks once.
# The archive is renamed into place: a symlink to the corpus's `reference/` that appears at the
# archive path during the build, after the checks, is replaced and not written through.
#
# Runs on scratch corpora of small archives, with a `cargo` on PATH that records that it was
# started and fails (for the rename case, writes the file the run packs and succeeds); nothing is
# built. Every case runs; the failures are listed. Prints `REGEN refusal ok`.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=stamp.sh
source "$here/scripts/stamp.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failed=0
fail() {
  echo "check-regen-refusal.sh: $*" >&2
  failed=1
}

# The members `--check` extracts, for the scratch corpora's archives.
mkdir -p "$tmp/members/oracles" "$tmp/members/raw/satin" "$tmp/members/raw/satin-block" "$tmp/bin"
for f in satin-oracle.json oracles/fq.json oracles/fqbig.json oracles/fqh.json oracles/fqhbig.json \
  oracles/big.json oracles/harsh.json; do
  echo '{}' > "$tmp/members/$f"
done

# make_corpus <dir>: a scratch corpus at <dir>: both archives, a block file, and a sentinel
# `reference/archive.tar.zst` (the name a new archive moved into `reference/` would take).
make_corpus() {
  mkdir -p "$1/real-blocks" "$1/reference"
  echo keep > "$1/real-blocks/keep.json"
  echo sentinel > "$1/reference/archive.tar.zst"
  for name in reference-outputs q11-oracles; do
    echo "$name" > "$tmp/members/raw/satin/marker"
    tar -cf - -C "$tmp/members" satin-oracle.json oracles raw | zstd -q -o "$1/reference/$name.tar.zst"
  done
}

# state <path>: every path under <path>, where each symlink points, and the content of every file.
state() {
  if [ ! -e "$1" ] && [ ! -L "$1" ]; then
    echo missing
    return
  fi
  (
    cd "$1"
    find . | LC_ALL=C sort
    find . -type l | LC_ALL=C sort | while IFS= read -r l; do echo "$l -> $(readlink "$l")"; done
    find . -type f -exec shasum -a 256 {} + | LC_ALL=C sort
  )
}

cat > "$tmp/bin/cargo" << 'EOF'
#!/bin/sh
# Records that it was started and fails. With STANDIN_LINK set it writes the oracle file the run
# packs, puts the symlink STANDIN_LINK -> STANDIN_LINK_TO during the last generator (block), and
# succeeds.
echo "cargo $*" >> "$CARGO_STARTED"
[ -n "${STANDIN_LINK:-}" ] || exit 1
[ -z "${MEGA_EVM2_ORACLE_OUT:-}" ] || echo '{}' > "$MEGA_EVM2_ORACLE_OUT"
if [ -n "${MEGA_EVM2_BLOCK_OUT:-}" ]; then
  ln -s "$STANDIN_LINK_TO" "$STANDIN_LINK"
  echo planted >> "$CARGO_STARTED"
fi
exit 0
EOF
chmod +x "$tmp/bin/cargo"
export CARGO_STARTED=$tmp/cargo-started

# The corpus and cargo target directory of the next runs, and their modes.
corpus=$tmp/corpus
target=$tmp/target
modes="--check default"

# regen <mode> <out-dir>: runs regen-reference.sh on $corpus with the stand-in cargo; sets `status`
# and `log`.
regen() {
  status=0
  rm -f "$CARGO_STARTED"
  local args=("$2")
  [ "$1" = default ] || args=(--check "$2")
  log=$(PATH="$tmp/bin:$PATH" MEGA_EVM2_CORPUS_DIR=$corpus CARGO_TARGET_DIR=$target \
    bash "$here/scripts/regen-reference.sh" "${args[@]}" 2>&1) || status=$?
}

# refused <pattern> <out-dir> [<path>...]: in each of $modes the run exits 2 with a message
# matching <pattern>, before cargo starts, and leaves the corpus, the target directory, <out-dir>
# and every <path> as they were (a missing one is not created).
refused() {
  local pattern=$1 out=$2 mode p what before
  shift 2
  for mode in $modes; do
    what="$mode $out (corpus $corpus)"
    before=$(for p in "$corpus" "$target" "$out" "$@"; do state "$p"; done)
    regen "$mode" "$out"
    [ "$status" = 2 ] || fail "$what: exit status $status, not the refusal's 2: $log"
    grep -q -- "$pattern" <<< "$log" || fail "$what: the refusal does not say '$pattern': $log"
    [ ! -e "$CARGO_STARTED" ] || fail "$what: cargo started before the refusal"
    [ "$(for p in "$corpus" "$target" "$out" "$@"; do state "$p"; done)" = "$before" ] \
      || fail "$what: the corpus, the target, the output directory or one of: $* changed"
  done
}

# reached <out-dir>: in each of $modes the run is not refused, gets as far as the build (the
# stand-in cargo started), and leaves the corpus as it was.
reached() {
  local out=$1 mode what before
  for mode in $modes; do
    what="$mode $out (corpus $corpus)"
    before=$(state "$corpus")
    regen "$mode" "$out"
    [ -s "$CARGO_STARTED" ] || fail "$what: refused or failed before the build: $log"
    [ "$(state "$corpus")" = "$before" ] || fail "$what: the corpus changed: $log"
  done
}

make_corpus "$corpus"
data=$(stamp_dir "$here/data")

# The output directory in the corpus.
ln -s "$corpus/reference" "$tmp/alias"
for out in "$corpus/reference" "$tmp/alias" "$corpus/reference/new" "$corpus"; do
  refused 'the corpus (' "$out"
done
# A directory to be created below the corpus spelled in other letter case, where the file system
# ignores case (macOS by default).
upper=$tmp/CORPUS
if [ -d "$upper" ]; then
  refused 'the corpus (' "$upper/reference/new"
fi

# The archive path a corpus file or path, under another name.
mkdir -p "$tmp/hard" "$tmp/soft" "$tmp/file" "$tmp/dangling"
ln "$corpus/reference/reference-outputs.tar.zst" "$tmp/hard/reference-outputs.tar.zst"
ln -s "$corpus/reference/q11-oracles.tar.zst" "$tmp/soft/reference-outputs.tar.zst"
ln -s "$corpus/real-blocks/keep.json" "$tmp/file/reference-outputs.tar.zst"
ln -s "$corpus/reference/new.tar.zst" "$tmp/dangling/reference-outputs.tar.zst"
for out in "$tmp/hard" "$tmp/soft" "$tmp/file" "$tmp/dangling"; do
  refused 'the corpus (' "$out"
done

# The archive path a directory, or a symlink to one: the corpus's `reference/` (with its sentinel),
# a directory outside the corpus, a directory in place.
mkdir -p "$tmp/dirlink" "$tmp/outside" "$tmp/outlink" "$tmp/dir/reference-outputs.tar.zst"
echo sentinel > "$tmp/outside/archive.tar.zst"
echo sentinel > "$tmp/dir/reference-outputs.tar.zst/archive.tar.zst"
ln -s "$corpus/reference" "$tmp/dirlink/reference-outputs.tar.zst"
ln -s "$tmp/outside" "$tmp/outlink/reference-outputs.tar.zst"
refused 'the corpus (' "$tmp/dirlink"
refused 'a directory' "$tmp/outlink" "$tmp/outside"
refused 'a directory' "$tmp/dir"

# The corpus itself at the archive path.
corpus=$tmp/at-archive/reference-outputs.tar.zst
make_corpus "$corpus"
refused 'the corpus (' "$tmp/at-archive"

# The cargo target directory in the corpus, and the corpus as cargo's directory.
corpus=$tmp/target-corpus
make_corpus "$corpus"
target=$corpus/target
refused 'the corpus (' "$tmp/elsewhere"
corpus=$tmp/target-holder/mega-evm2-reference
make_corpus "$corpus"
target=$tmp/target-holder
refused 'the corpus (' "$tmp/elsewhere"

# A link in cargo's directory into the corpus, where cargo (`release/`) or anything else writes: a
# symlink to the corpus, one to a missing path in it, a hard link to a corpus file.
corpus=$tmp/corpus
target=$tmp/target-links
mkdir -p "$target/mega-evm2-reference/tmp"
ln -s "$corpus" "$target/mega-evm2-reference/release"
refused 'the corpus (' "$tmp/elsewhere"
rm "$target/mega-evm2-reference/release"
ln -s "$corpus/reference/new" "$target/mega-evm2-reference/release"
refused 'the corpus (' "$tmp/elsewhere"
rm "$target/mega-evm2-reference/release"
ln "$corpus/real-blocks/keep.json" "$target/mega-evm2-reference/tmp/keep.json"
refused 'the corpus (' "$tmp/elsewhere"
target=$tmp/target

# A corpus directory whose name ends in a line break, which `$(...)` would drop, next to a corpus of
# the name without it: the output directory below the first is still in the corpus.
make_corpus "$tmp/newline"
corpus=$tmp/newline$'\n'
make_corpus "$corpus"
refused 'the corpus (' "$corpus/nested-out"

# Default mode: a corpus below `next/diff/data`, and one holding it (both only read here).
modes=default
corpus=$here/data/real-blocks
refused 'the corpus (' "$tmp/elsewhere"
corpus=$here
refused 'the corpus (' "$tmp/elsewhere"
modes="--check default"

# A corpus made of links to another one covers that one: an output directory in the linked
# corpus, and (default mode) a corpus that links into `next/diff/data`. A link that leads nowhere
# is refused (the run could create its target).
corpus=$tmp/farm
mkdir -p "$corpus"
ln -s "$tmp/corpus/real-blocks" "$corpus/real-blocks"
ln -s "$tmp/corpus/reference" "$corpus/reference"
refused 'the corpus (' "$tmp/corpus/reference/new"
refused 'the corpus (' "$tmp/corpus/real-blocks/new"
corpus=$tmp/farm-data
mkdir -p "$corpus"
ln -s "$here/data/real-blocks" "$corpus/real-blocks"
ln -s "$tmp/corpus/reference" "$corpus/reference"
modes=default refused 'the corpus (' "$tmp/elsewhere"
corpus=$tmp/broken
make_corpus "$corpus"
ln -s "$tmp/elsewhere/reference-outputs.tar.zst" "$corpus/reference/next.tar.zst"
refused 'leads nowhere' "$tmp/elsewhere"

# A corpus elsewhere inside the output directory: not refused, not touched. A fresh corpus for each
# run, so one run's damage cannot hide another's.
for mode in --check default; do
  for sub in stage stage/deep/c corpus; do
    out=$tmp/inside-$mode-${sub//\//-}
    corpus=$out/$sub
    make_corpus "$corpus"
    modes=$mode reached "$out"
  done
done

# A directory outside the corpus: not refused, the run gets as far as the build. Also with a link
# cycle in the corpus, which the check walks once.
corpus=$tmp/corpus
reached "$tmp/elsewhere"
corpus=$tmp/cycle
make_corpus "$corpus"
ln -s .. "$corpus/real-blocks/up"
reached "$tmp/elsewhere"

# The archive is renamed over its path: a symlink to the corpus's `reference/` that the stand-in
# cargo puts there during the build is replaced, not written through. `--check` only: a default
# run that got this far would go on to rewrite `next/diff/data`.
corpus=$tmp/rename-corpus
make_corpus "$corpus"
out=$tmp/rename-out
before=$(state "$corpus")
export STANDIN_LINK=$out/reference-outputs.tar.zst STANDIN_LINK_TO=$corpus/reference
regen --check "$out"
unset STANDIN_LINK STANDIN_LINK_TO
grep -q '^planted$' "$CARGO_STARTED" 2> /dev/null || fail "rename: the stand-in did not put the link: $log"
[ "$(state "$corpus")" = "$before" ] || fail "rename: the corpus changed (the archive was written through the link): $log"
{ [ -f "$out/reference-outputs.tar.zst" ] && [ ! -L "$out/reference-outputs.tar.zst" ]; } \
  || fail "rename: the archive path is not the new archive: $(ls -l "$out" 2>&1)"

[ "$(stamp_dir "$here/data")" = "$data" ] || fail "next/diff/data changed"
[ "$failed" = 0 ] || exit 1
echo "REGEN refusal ok"
