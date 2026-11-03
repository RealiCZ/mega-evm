"""Refuses a run of `regen-reference.sh` that could delete or write anything in the corpus.

Usage: refuse_corpus_writes.py <corpus> <out-dir> <cargo dir> [<data dir>]

The arguments are the paths the run uses, resolved: <out-dir>, which the run creates when missing
and in which it creates a new directory of its own and renames the archive over
`<out-dir>/reference-outputs.tar.zst`; <cargo dir>, the directory cargo builds in; and, in default
mode only, <data dir> (`next/diff/data`), which the run rewrites. Exits 2 with the reason on stderr
when one of them could reach the corpus, and 0 otherwise. Creates nothing.

The corpus is every directory and file reachable from <corpus>, through its symlinks too, as the
gates read it: a corpus made of links to another one covers that one as well. A symlink in the
corpus that leads nowhere is refused, since the run could create its target. Cargo writes anywhere
below <cargo dir>, so every path reachable there (through symlinks too, and a symlink that leads
nowhere by where it leads) must stay out of the corpus. Paths are compared by device and inode,
so a second name (a symlink, a hard link, other letter case on a case-insensitive file system)
counts. A path that does not exist yet is judged by its deepest existing ancestor, since the run
creates everything below that.
"""
import os, sys

# More paths than a corpus or the reference's cargo directory has: a link to a large tree, such as
# `/`, is refused rather than walked.
LIMIT = 100_000

def ident(path):
    st = os.stat(path)
    return st.st_dev, st.st_ino

def chain(path):
    """The identities of `path`, resolved (or of its deepest existing ancestor), and of every
    directory above it."""
    p = os.path.realpath(path)
    while not os.path.exists(p):
        p = os.path.dirname(p)
    ids = []
    while True:
        ids.append(ident(p))
        parent = os.path.dirname(p)
        if parent == p:
            return ids
        p = parent

def refuse(reason):
    print(f"regen-reference.sh: {reason}", file=sys.stderr)
    sys.exit(2)

def walk(root):
    """Every path below the directory `root`, through its symlinks too, each directory once."""
    seen, count, stack = set(), 0, [root]
    while stack:
        d = stack.pop()
        if ident(d) in seen:
            continue
        seen.add(ident(d))
        for e in os.scandir(d):
            count += 1
            if count > LIMIT:
                refuse(f"{root} reaches more than {LIMIT} paths through its links; regen-reference.sh does not walk that far.")
            yield e.path
            if e.is_dir():
                stack.append(e.path)

corpus, out, cargo = sys.argv[1:4]
data = sys.argv[4] if len(sys.argv) > 4 else None
archive = os.path.join(out, "reference-outputs.tar.zst")

# The corpus's directories and files, and every directory that holds one of them.
dirs, files, holders = {ident(corpus)}, set(), set(chain(corpus))
for p in walk(corpus):
    if not os.path.exists(p):
        refuse(f"the corpus has a symlink that leads nowhere, {p} -> {os.readlink(p)}; a run could create its target. Fix or remove the link.")
    (dirs if os.path.isdir(p) else files).add(ident(p))
    holders.update(chain(p))

def in_corpus(path):
    """Whether `path` is a corpus directory or file (or will be created in one)."""
    return not dirs.isdisjoint(chain(path)) or (os.path.exists(path) and ident(path) in files)

def holds_corpus(path):
    return os.path.isdir(path) and ident(path) in holders

if in_corpus(out):
    refuse(f"the output directory {out} is in the corpus ({corpus}); the new archive would replace the corpus's. Write it elsewhere.")
if in_corpus(archive):
    refuse(f"{archive} is a corpus file or resolves into the corpus ({corpus}); the archive must not replace or write a corpus file. Write it elsewhere.")
if os.path.isdir(archive):
    refuse(f"{archive} is a directory or a link to one; the new archive is renamed over that path, which must be a file or missing. Write it elsewhere.")
if in_corpus(cargo):
    refuse(f"the cargo directory {cargo} is in the corpus ({corpus}): cargo writes there. Set CARGO_TARGET_DIR outside the corpus.")
if os.path.isdir(cargo):
    for p in walk(cargo):
        if in_corpus(p):
            refuse(f"the cargo directory {cargo} reaches the corpus ({corpus}) through {p}: cargo writes there. Remove that link, or set CARGO_TARGET_DIR elsewhere.")
if data is not None and (in_corpus(data) or holds_corpus(data)):
    refuse(f"{data}, which the default mode rewrites, and the corpus ({corpus}) overlap. Use another corpus copy or checkout.")
