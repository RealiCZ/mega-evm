"""Refuses a run of a gate or reference script that could delete or write anything in the corpus.

Usage: refuse_corpus_writes.py <corpus> <out-dir> <cargo dir> [<data dir>]

The arguments are the paths the script uses, resolved: <out-dir>, which the script creates when
missing and in which it writes; <cargo dir>, the directory the script's cargo builds in; and, when
the script rewrites `next/diff/data`, <data dir>. The Cargo home actually used (`CARGO_HOME`, or
`$HOME/.cargo` when unset) is checked as well. Exits 2 with the reason on stderr when one of them
could reach the corpus, and 0 otherwise. Creates nothing.

The corpus is every directory and file reachable from <corpus>, through its symlinks too, as the
gates read it: a corpus made of links to another one covers that one as well. A symlink in the
corpus that leads nowhere is refused, since the run could create its target. Cargo writes anywhere
below <cargo dir>, so every path reachable there (through symlinks too, and a symlink that leads
nowhere by where it leads) must stay out of the corpus. The Cargo home is checked by its resolved
path only: the corpus walk already covers the directories a corpus symlink reaches, and links
planted inside the Cargo home itself are out of scope. Beyond these paths the check promises
nothing: links planted inside them, a corpus placed inside one of them, other process environment,
and changes made while a run is in progress are not defended against.
Paths are compared by device and inode, so a second name (a symlink, a hard link, other letter case
on a case-insensitive file system) counts. A path that does not exist yet is judged by its deepest
existing ancestor, since the run creates everything below that. An unreadable directory is refused
with a one-line message naming it, not a traceback.
"""
import os, sys

# More paths than a corpus or the reference's cargo directory has: a link to a large tree, such as
# `/`, is refused rather than walked.
LIMIT = 100_000

def ident(path):
    st = os.stat(path)
    return st.st_dev, st.st_ino

def refuse(reason):
    print(f"corpus guard: {reason}", file=sys.stderr)
    sys.exit(2)

def chain(path):
    """The identities of `path`, resolved (or of its deepest existing ancestor), and of every
    directory above it."""
    try:
        p = os.path.realpath(path)
        while not os.path.exists(p):
            parent = os.path.dirname(p)
            if parent == p:
                break
            p = parent
        ids = []
        while True:
            ids.append(ident(p))
            parent = os.path.dirname(p)
            if parent == p:
                return ids
            p = parent
    except OSError as e:
        refuse(f"cannot read {path}: {e}")

def walk(root):
    """Every path below the directory `root`, through its symlinks too, each directory once."""
    seen, count, stack = set(), 0, [root]
    while stack:
        d = stack.pop()
        try:
            if ident(d) in seen:
                continue
            seen.add(ident(d))
            with os.scandir(d) as entries:
                for e in entries:
                    count += 1
                    if count > LIMIT:
                        refuse(f"{root} reaches more than {LIMIT} paths through its links; the corpus guard does not walk that far.")
                    yield e.path
                    if e.is_dir():
                        stack.append(e.path)
        except OSError as e:
            refuse(f"cannot read {d}: {e}")

corpus, out, cargo = sys.argv[1:4]
data = sys.argv[4] if len(sys.argv) > 4 else None
archive = os.path.join(out, "reference-outputs.tar.zst")
cargo_home_env = os.environ.get("CARGO_HOME")
cargo_home = os.path.realpath(cargo_home_env or os.path.join(os.path.expanduser("~"), ".cargo"))
cargo_home_label = "CARGO_HOME" if cargo_home_env else "CARGO_HOME (default ~/.cargo)"

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
    refuse(f"the output directory {out} is in the corpus ({corpus}); the run would write there. Write it elsewhere.")
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
if in_corpus(cargo_home):
    refuse(f"{cargo_home_label} ({cargo_home}) is in the corpus ({corpus}): cargo writes its cache there. Set CARGO_HOME outside the corpus.")
if data is not None and (in_corpus(data) or holds_corpus(data)):
    refuse(f"{data}, which the run rewrites, and the corpus ({corpus}) overlap. Use another corpus copy or checkout.")
