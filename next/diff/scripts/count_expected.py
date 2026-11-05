"""Counts the summary figures the gates expect from the reference files and the accepted list.

Usage: count_expected.py <replay dir> <block dir> <known-differences.toml>

Each directory holds `<set>/<n>.jsonl` (or `<n>.jsonl.zst`). Prints the two lines
`gate.sh` compares the gates' summaries with:

  EXPECT_REPLAY="blocks=<b> match=<m> pre=<p> refused=<r> known=<k>"
  EXPECT_BLOCK="blocks=<b> included=<i> refused=<r> counters=<b> post=<b>"

The counts do not use the gate's code: a transaction the reference applied is `match`, one the
reference refused is `refused` (replay: an `error` in its `satin` outcome; block: a status that
starts with `refused`), every pre-block call is one `pre`. An applied transaction that a
`[[replay_difference]]` entry lists (by set, block and index, with its hash) is `known` instead of
`match`. An entry of a listed block whose transaction is missing, has another hash or was refused
is an error.
"""
import json, os, subprocess, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import known_toml  # noqa: E402

def rows(path):
    if path.endswith(".zst"):
        text = subprocess.run(["zstd", "-q", "-dc", path], check=True, capture_output=True, text=True).stdout
    else:
        with open(path) as f:
            text = f.read()
    return [json.loads(line) for line in text.splitlines() if line.strip()]

def files(root):
    for base, _, names in os.walk(root):
        for n in sorted(names):
            if n.endswith((".jsonl", ".jsonl.zst")):
                yield os.path.join(base, n)

if len(sys.argv) != 4:
    raise SystemExit(__doc__)
replay_dir, block_dir, known_path = sys.argv[1], sys.argv[2], sys.argv[3]
listed = {}
for e in known_toml.tables(known_path, "replay_difference"):
    listed[(e["set"], str(e["block"]), e["index"])] = e["hash"]
blocks = match = pre = refused = known = 0
for path in files(replay_dir):
    blocks += 1
    block_set = os.path.basename(os.path.dirname(path))
    block = os.path.basename(path).split(".")[0]
    pending = {k for k in listed if k[:2] == (block_set, block)}
    for r in rows(path):
        if r["kind"] == "pre_block":
            pre += len(r["calls"])
        elif r["kind"] == "tx":
            key = (block_set, block, r["index"])
            if "error" in r["satin"]:
                if key in listed:
                    raise SystemExit("count_expected.py: listed transaction %s:%s:%d was refused" % key)
                refused += 1
            elif key in listed:
                if r["hash"] != listed[key]:
                    raise SystemExit("count_expected.py: listed transaction %s:%s:%d has hash %s" % (key + (r["hash"],)))
                pending.discard(key)
                known += 1
            else:
                match += 1
    if pending:
        raise SystemExit("count_expected.py: listed transactions not in their block: %s" % sorted(pending))
print(f'EXPECT_REPLAY="blocks={blocks} match={match} pre={pre} refused={refused} known={known}"')

blocks = included = refused = counters = post = 0
for path in files(block_dir):
    blocks += 1
    for r in rows(path):
        if r["kind"] == "tx":
            if str(r["status"]).startswith("refused"):
                refused += 1
            else:
                included += 1
        elif r["kind"] == "block_counters":
            counters += 1
        elif r["kind"] == "post_state":
            post += 1
print(f'EXPECT_BLOCK="blocks={blocks} included={included} refused={refused} counters={counters} post={post}"')
