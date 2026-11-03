"""Compares two trees of reference outputs, ignoring timing fields.

Usage: compare_outputs.py <expected dir> <regenerated dir>
"""
import json, os, sys

expected, out = sys.argv[1], sys.argv[2]
TIMING = {"micros"}

def strip(v):
    return {k: x for k, x in v.items() if k not in TIMING} if isinstance(v, dict) else v

def load(path):
    with open(path) as f:
        if path.endswith(".jsonl"):
            return [strip(json.loads(line)) for line in f if line.strip()]
        return [strip(x) for x in json.load(f)]

def files(root):
    found = set()
    for base, _, names in os.walk(root):
        for n in names:
            found.add(os.path.relpath(os.path.join(base, n), root))
    return found

want, got = files(expected), files(out)
bad = [f"missing from regenerated: {f}" for f in sorted(want - got)]
bad += [f"unexpected regenerated file: {f}" for f in sorted(got - want)]
for f in sorted(want & got):
    if load(os.path.join(expected, f)) != load(os.path.join(out, f)):
        bad.append(f"differs: {f}")
if bad:
    print("\n".join(bad), file=sys.stderr)
    sys.exit(1)
print(f"compared {len(want)} files")
