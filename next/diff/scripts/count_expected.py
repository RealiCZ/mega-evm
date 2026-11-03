"""Counts the summary figures the gates expect from the reference files alone.

Usage: count_expected.py <replay dir> <block dir>

Each directory holds `<set>/<n>.jsonl` (or `<n>.jsonl.zst`). Prints the two lines
`gate.sh` compares the gates' summaries with:

  EXPECT_REPLAY="blocks=<b> match=<m> pre=<p> refused=<r>"
  EXPECT_BLOCK="blocks=<b> included=<i> refused=<r> counters=<b> post=<b>"

The counts do not use the gate's code: a transaction the reference applied is `match`, one the
reference refused is `refused` (replay: an `error` in its `satin` outcome; block: a status that
starts with `refused`), every pre-block call is one `pre`.
"""
import json, os, subprocess, sys

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

replay_dir, block_dir = sys.argv[1], sys.argv[2]
blocks = match = pre = refused = 0
for path in files(replay_dir):
    blocks += 1
    for r in rows(path):
        if r["kind"] == "pre_block":
            pre += len(r["calls"])
        elif r["kind"] == "tx":
            if "error" in r["satin"]:
                refused += 1
            else:
                match += 1
print(f'EXPECT_REPLAY="blocks={blocks} match={match} pre={pre} refused={refused}"')

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
