"""Refuses a run of `regen-reference.sh` that could delete or write anything in the corpus.

Usage: refuse_corpus_writes.py <corpus> <out-dir> <cargo dir> [<data dir>]

The arguments are the paths the run uses, resolved: <out-dir>, which the run creates when missing
and in which it creates a new directory of its own and renames the archive over
`<out-dir>/reference-outputs.tar.zst`; <cargo dir>, the directory cargo builds in; and, in default
mode only, <data dir> (`next/diff/data`), which the run rewrites. Exits 2 with the reason on stderr
when one of them could reach the corpus, and 0 otherwise. Creates nothing.

A path is in a directory when, resolved through every symlink, it or one of its ancestors is that
directory, compared by device and inode: a second name of the corpus (a symlink, a bind mount,
other letter case on a case-insensitive file system) counts. A path that does not exist yet is
judged by its deepest existing ancestor, since the run creates everything below that.
"""
import os, sys

def ident(path):
    st = os.stat(path)
    return st.st_dev, st.st_ino

def inside(path, directory):
    """Whether `path`, resolved, is `directory` or lies below it."""
    if not os.path.isdir(directory):
        return False
    want = ident(directory)
    p = os.path.realpath(path)
    while not os.path.exists(p):
        p = os.path.dirname(p)
    while True:
        if ident(p) == want:
            return True
        parent = os.path.dirname(p)
        if parent == p:
            return False
        p = parent

def refuse(reason):
    print(f"regen-reference.sh: {reason}", file=sys.stderr)
    sys.exit(2)

corpus, out, cargo = sys.argv[1:4]
data = sys.argv[4] if len(sys.argv) > 4 else None
archive = os.path.join(out, "reference-outputs.tar.zst")
corpus_archives = [os.path.join(corpus, "reference", n) for n in ("reference-outputs.tar.zst", "q11-oracles.tar.zst")]

if inside(out, corpus):
    refuse(f"the output directory {out} is the corpus ({corpus}) or lies in it; the new archive would replace the corpus's. Write it elsewhere.")
if inside(archive, corpus):
    refuse(f"{archive} resolves into the corpus ({os.path.realpath(archive)}); the archive must not replace or write a corpus file. Write it elsewhere.")
if os.path.isdir(archive):
    refuse(f"{archive} is a directory or a link to one; the new archive is renamed over that path, which must be a file or missing. Write it elsewhere.")
for a in corpus_archives:
    if os.path.exists(a) and os.path.exists(archive) and os.path.samefile(archive, a):
        refuse(f"{archive} is the corpus archive {a} (a hard link to it); the archive must not replace or write a corpus file. Write it elsewhere.")
if inside(cargo, corpus) or inside(corpus, cargo):
    refuse(f"the cargo directory {cargo} and the corpus ({corpus}) overlap: cargo writes there. Set CARGO_TARGET_DIR outside the corpus.")
if data is not None and (inside(data, corpus) or inside(corpus, data)):
    refuse(f"{data}, which the default mode rewrites, and the corpus ({corpus}) overlap. Use another corpus copy or checkout.")
