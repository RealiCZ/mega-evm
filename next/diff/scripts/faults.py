"""The fixed list of faults `faults.sh` injects into a copy of the sample's gate inputs.

Each fault changes one thing in the reference data (or the run's environment) that a gate must
reject, and names the check that must catch it: a regular expression that must match the gate's
log or comparison output (and not the unmutated run's), or `summary` when the gate's tests pass
and only the comparison of its summary line with the expected counts can reject it.

Usage:
  faults.py list               prints `<id> <gate> <evidence regex>` per fault (tab-separated)
  faults.py apply <id> <work>  applies the fault to the inputs under <work>, then prints the extra
                               environment of the gate run, one `VAR=value` per line

<work> holds links to the shared, unmutated inputs (the layout of `gate-lib.sh`); a fault
replaces the links on the path to every file it changes with copies, so the shared inputs are
never written. Targets are picked by rule (for example "the first scenario with logs"), in file
order, so they follow a regenerated sample; a rule that matches nothing is an error.
"""
import json, os, re, shutil, sys

HERE = os.path.dirname(os.path.abspath(__file__))
KNOWN = os.path.join(HERE, "..", "known-differences.toml")
sys.path.insert(0, HERE)
from known_toml import tables as known_tables  # noqa: E402

# --- copies on write ------------------------------------------------------------------------------

def own(work, rel):
    """Replaces every link on the path work/rel with a real copy (a directory becomes links to
    its children) and returns the path, now safe to write."""
    p = work
    for part in rel.split("/"):
        p = os.path.join(p, part)
        if os.path.islink(p):
            target = os.path.realpath(p)
            os.unlink(p)
            if os.path.isdir(target):
                os.mkdir(p)
                for child in sorted(os.listdir(target)):
                    os.symlink(os.path.join(target, child), os.path.join(p, child))
            else:
                shutil.copyfile(target, p)
    return p

def edit_json(work, rel, f):
    p = own(work, rel)
    with open(p) as h:
        v = json.load(h)
    v = f(v) or v
    with open(p, "w") as h:
        json.dump(v, h)

def edit_lines(work, rel, f):
    p = own(work, rel)
    with open(p) as h:
        rows = [json.loads(l) for l in h if l.strip()]
    rows = f(rows) or rows
    with open(p, "w") as h:
        for r in rows:
            h.write(json.dumps(r) + "\n")

def edit_text(work, rel, f):
    p = own(work, rel)
    with open(p) as h:
        t = h.read()
    t2 = f(t)
    if t2 == t:
        raise SystemExit("faults.py: the edit of %s changed nothing" % rel)
    with open(p, "w") as h:
        h.write(t2)

def first(items, pred, what):
    for x in items:
        if pred(x):
            return x
    raise SystemExit("faults.py: no %s in the sample" % what)

def blocks(work, kind):
    """(rel path, rows) of every reference file of `kind` (replay or block), in name order."""
    out = []
    for s in sorted(os.listdir(os.path.join(work, kind))):
        for n in sorted(os.listdir(os.path.join(work, kind, s))):
            rel = "%s/%s/%s" % (kind, s, n)
            with open(os.path.join(work, rel)) as h:
                out.append((rel, [json.loads(l) for l in h if l.strip()]))
    return out

def bump(v):
    """A different value of the same shape (the fields faulted with it are integers)."""
    if isinstance(v, int) and not isinstance(v, bool):
        return v + 1
    raise SystemExit("faults.py: cannot change %r" % (v,))

# --- the synthetic gates (main and fuzz) ---------------------------------------------------------

def scen_field(rel, field, pred=lambda o: True, change=bump, sub=None):
    """Changes `field` (or `field[sub]`) of the first scenario matching `pred`."""
    def apply(work):
        def f(entries):
            o = first(entries, lambda o: field in o and pred(o), "scenario with %s" % field)
            if sub is None:
                o[field] = change(o[field])
            else:
                o[field][sub] = change(o[field][sub])
            NOTE.append(o["name"])
        edit_json(work, rel, f)
    return apply

def frame_field(rel, key):
    """Changes `key` of the first frame of the first successful multi-frame scenario."""
    def apply(work):
        def f(entries):
            o = first(entries, lambda o: o.get("status") == "success" and len(o["frames"]) > 1 and
                      o["frames"][0]["status"] == "success", "successful multi-frame scenario")
            o["frames"][0][key] = bump(o["frames"][0][key])
            NOTE.append(o["name"])
        edit_json(work, rel, f)
    return apply

def scen_edit(rel, f_entries):
    def apply(work):
        edit_json(work, rel, f_entries)
    return apply

def drop_entry(entries):
    o = entries.pop(len(entries) // 2)
    NOTE.append(o["name"])

def dup_entry(entries):
    o = entries[len(entries) // 2]
    entries.append(dict(o))
    NOTE.append(o["name"])

def extra_entry(entries):
    o = dict(entries[0])
    o["name"] = "zz_fault_extra"
    entries.append(o)
    NOTE.append(o["name"])

def named(name, f):
    """Changes the scenario `name` with `f`."""
    def g(entries):
        o = first(entries, lambda o: o["name"] == name, "scenario %s" % name)
        f(o)
        NOTE.append(name)
    return g

def known_toml(f):
    """Runs the gate with an edited copy of `known-differences.toml`."""
    def apply(work):
        dst = os.path.join(work, "known-differences.toml")
        shutil.copyfile(KNOWN, dst)
        edit_text(work, "known-differences.toml", f)
        return ["MEGA_EVM2_KNOWN_DIFFERENCES=" + dst]
    return apply

def expect(var, f):
    def apply(work):
        edit_text(work, "expect.env", lambda t: re.sub(r'^(%s=")([^"]*)"' % var, lambda m: m.group(1) + f(m.group(2)) + '"', t, flags=re.M))
    return apply

def count(word, delta):
    return lambda v: re.sub(r"\b%s=(\d+)" % word, lambda m: "%s=%d" % (word, int(m.group(1)) + delta), v, count=1)

# --- the block gates (replay and block) -----------------------------------------------------------

def row_edit(kind, pred, f, what):
    """Applies `f(row)` to the first row (in file order) of a `kind` reference file matching
    `pred(row)`."""
    def apply(work):
        for rel, rows in blocks(work, kind):
            for r in rows:
                if pred(r):
                    def g(rows2, idx=rows.index(r)):
                        f(rows2[idx])
                    edit_lines(work, rel, g)
                    NOTE.append("%s#%s" % (rel, r.get("index", r.get("kind"))))
                    return
        raise SystemExit("faults.py: no %s in the sample" % what)
    return apply

def file_edit(kind, pred_rows, f, what):
    """Applies `f(rows)` to the first `kind` reference file whose rows match `pred_rows`."""
    def apply(work):
        for rel, rows in blocks(work, kind):
            if pred_rows(rows):
                edit_lines(work, rel, f)
                NOTE.append(rel)
                return
        raise SystemExit("faults.py: no %s in the sample" % what)
    return apply

def is_tx(r):
    return r.get("kind") == "tx"

def satin_field(field, pred=lambda s: True, change=bump):
    def f(r):
        r["satin"][field] = change(r["satin"][field])
    return row_edit("replay", lambda r: is_tx(r) and field in r["satin"] and pred(r["satin"]), f, "replay transaction with %s" % field)

def first_value(change):
    """Changes the first value of a non-empty object."""
    def g(d):
        k = next(iter(d))
        return dict(d, **{k: change(d[k])})
    return g

def pre_call(pred, f, what):
    def g(r):
        c = first(r["calls"], pred, what)
        f(c)
    return row_edit("replay", lambda r: r.get("kind") == "pre_block" and any(pred(c) for c in r["calls"]), g, what)

def block_row(kind, field, pred=lambda r: True, change=bump):
    def f(r):
        r[field] = change(r[field])
    return row_edit("block", lambda r: r.get("kind") == kind and field in r and pred(r), f, "block %s row with %s" % (kind, field))

def sample_replay_known(work):
    """The first `[[replay_difference]]` entry whose block is in the sample, and its file there."""
    for e in known_tables(KNOWN, "replay_difference"):
        rel = "replay/%s/%d.jsonl" % (e["set"], e["block"])
        if os.path.exists(os.path.join(work, rel)):
            return e, rel
    raise SystemExit("faults.py: no listed replay transaction in the sample")

def stale_replay_known(work):
    """Gives the first listed replay transaction of the sample the frames the engine reports: the
    difference its `[[replay_difference]]` entry accepts no longer occurs."""
    e, rel = sample_replay_known(work)
    def f(rows):
        r = first(rows, lambda r: is_tx(r) and r["index"] == e["index"], "listed transaction")
        r["frames"] = json.loads(e["candidate"])
    edit_lines(work, rel, f)
    NOTE.append("%s#%d" % (rel, e["index"]))

def replay_known_entry(edit):
    """Runs the replay gate with a copy of `known-differences.toml` whose first entry of a sample
    block is changed by `edit(entry, entry_text)`, which returns the new text of that table."""
    def apply(work):
        e, rel = sample_replay_known(work)
        NOTE.append("%s#%d" % (rel, e["index"]))
        def f(t):
            parts = t.split("[[replay_difference]]")
            for i, part in enumerate(parts[1:], 1):
                if ("\nblock = %d\n" % e["block"]) in part and ("\nindex = %d\n" % e["index"]) in part:
                    parts[i] = edit(e, part)
                    return "[[replay_difference]]".join(parts)
            raise SystemExit("faults.py: entry %s:%d:%d not found" % (e["set"], e["block"], e["index"]))
        return known_toml(f)(work)
    return apply

def unlist_replay_known(work):
    """Runs the replay gate with a copy of `known-differences.toml` without the first entry of a
    sample block: its transaction, of the class `halted_first_frame`, is then the class's."""
    e, rel = sample_replay_known(work)
    NOTE.append("%s#%d" % (rel, e["index"]))
    def f(t):
        head = "[[replay_difference]]\n"
        starts = [i for i in range(len(t)) if t.startswith(head, i)]
        for a, b in zip(starts, starts[1:] + [len(t)]):
            table = t[a:b]
            if ("\nblock = %d\n" % e["block"]) in table and ("\nindex = %d\n" % e["index"]) in table:
                return t[:a] + t[b:]
        raise SystemExit("faults.py: entry %s:%d:%d not found" % (e["set"], e["block"], e["index"]))
    return known_toml(f)(work)

def other_hash(e, part):
    h = e["hash"]
    return part.replace(h, h[:-1] + ("0" if h[-1] != "0" else "1"))

def other_frame_gas(e, part):
    frames = json.loads(e["candidate"])
    gas = frames[0]["gas_limit"]
    return part.replace('"gas_limit": %d' % gas, '"gas_limit": %d' % (gas + 1))

def drop_tx(rows):
    i = next(i for i, r in enumerate(rows) if is_tx(r))
    rows.pop(i)

def has_txs(n):
    return lambda rows: sum(1 for r in rows if is_tx(r)) >= n

def unknown_row(rows):
    """Appends a row of a kind the replay gate does not read, every other row kept."""
    rows.append({"kind": "unexpected_result", "block": rows[0].get("block"), "status": "halt", "gas_used": 999999})

# --- the list -------------------------------------------------------------------------------------

M, F = "main.json", "fuzz/mode4.json"
ok = lambda o: o.get("status") == "success"

def frameless_halt(o):
    """A scenario of the class `halted_first_frame` on the reference's side: it ran out of gas
    without opening a frame."""
    return o.get("frames") == [] and "error" not in o and o.get("halt") == "Base(OutOfGas(Basic))"

def drop_class(t, name="halted_first_frame"):
    """`known-differences.toml` without its `[[class]]` table of `name`."""
    a = t.index('[[class]]\nname = "%s"\n' % name)
    b = t.index("\n\n", a) + 1
    return t[:a] + t[b:]

DUPN = "edge_dupn_deeper_than_stack"


def diff(field):
    """Evidence: a synthetic DIFFER line that names `field`."""
    return r'^DIFFER [^ ]+ \[.*"%s"' % field

def cmp(field):
    """Evidence: a replay or block comparison row whose differing fields include `field`."""
    return r'"fields": ?\[[^]]*"%s"' % field


# The gate's tests pass on the fault: only the comparison of its summary with the expected
# counts can reject it.
SUMMARY = "summary"

FAULTS = [
    # main: a compared field, an inspected frame, the reference's plain and inspected runs, entries, the expected count
    ("main-gas-used", "main", diff("gas_used"), scen_field(M, "gas_used", ok)),
    ("main-frame-gas-left", "main", r"^DIFFER .*frame 0 gas_left", frame_field(M, "gas_left")),
    ("main-frame-count", "main", r"^DIFFER .*frame count", scen_field(M, "frames", lambda o: ok(o) and len(o["frames"]) > 1, lambda v: v[:-1])),
    ("main-reference-inspected", "main", diff("reference_plain_vs_inspected"), scen_field(M, "inspected_equal", ok, lambda v: False)),
    ("main-missing-entry", "main", r"^MISSING ", scen_edit(M, drop_entry)),
    ("main-extra-entry", "main", r"^EXTRA zz_fault_extra$", scen_edit(M, extra_entry)),
    ("main-expected-total", "main", r"MEGA_EVM2_EXPECT", expect("EXPECT_MAIN", count("total", 1))),
    # main: the accepted difference hides exactly itself
    ("known-stale", "main", r"^DIFFER p6_dupn .*stale known difference", scen_edit(M, named("p6_dupn", lambda o: o.update(halt="Base(StackUnderflow)")))),
    ("known-toml-wrong-field", "main", r"^DIFFER p6_dupn .*did not occur as listed", known_toml(lambda t: t.replace('field = "halt"', 'field = "gas_used"'))),
    ("known-toml-wrong-case", "main", r'^DIFFER p6_add .*stale known difference', known_toml(lambda t: t.replace('scenario = "p6_dupn"', 'scenario = "p6_add"'))),
    # main: the class `halted_first_frame` takes exactly its shape, only while the file enables it
    ("class-other-field", "main", r'^DIFFER [^ ]+ \["gas_used", "frame count 0 vs 1"\]', scen_field(M, "gas_used", frameless_halt)),
    ("class-reference-frame", "main", r'^DIFFER [^ ]+ \["frame 0 ', scen_field(M, "frames", frameless_halt, lambda v: [{"kind": "Call", "level": 0, "gas_limit": 1, "reservoir": 0, "value": "0", "status": "halt", "output": "0x", "gas_left": 0, "reservoir_left": 0}])),
    ("class-not-enabled", "main", r'^DIFFER [^ ]+ \["frame count 0 vs 1"\]$', known_toml(drop_class)),
    ("main-expected-class", "main", SUMMARY, expect("EXPECT_MAIN", count("class", 1))),
    # main: the class `dup_stack_label` takes exactly its label, only while the file enables it
    ("dup-class-other-field", "main", r'^DIFFER %s \[.*"gas_used"' % DUPN, scen_field(M, "gas_used", lambda o: o["name"] == DUPN)),
    ("dup-class-other-label", "main", r'^DIFFER %s \["halt"\]$' % DUPN, scen_edit(M, named(DUPN, lambda o: o.update(halt="Base(OutOfGas(Basic))")))),
    ("dup-class-not-enabled", "main", r'^DIFFER %s \["halt"\]$' % DUPN, known_toml(lambda t: drop_class(t, "dup_stack_label"))),
    # fuzz
    ("fuzz-gas-used", "fuzz", diff("gas_used"), scen_field(F, "gas_used", ok)),
    ("fuzz-duplicate-entry", "fuzz", r"duplicate oracle names", scen_edit(F, dup_entry)),
    ("fuzz-expected-count", "fuzz", r"MEGA_EVM2_EXPECT", expect("FUZZ_BATCHES", lambda v: v.replace(":2000", ":2001"))),
    ("fuzz-expected-class", "fuzz", SUMMARY, expect("FUZZ_CLASS", lambda v: str(int(v) + 1))),
    # replay
    ("replay-gas-used", "replay", cmp("gas_used"), satin_field("gas_used")),
    ("replay-reference-inspected", "replay", cmp("satin_plain_vs_inspected"), row_edit("replay", is_tx, lambda r: r.update(inspected_equal=False), "transaction")),
    ("replay-pre-status", "replay", r'"fields": ?\["status"', pre_call(lambda c: True, lambda c: c.update(status="revert"), "pre-block call")),
    ("replay-pre-reads", "replay", r'"reads_differ": ?true', pre_call(lambda c: c["reads"]["storage"], lambda c: c["reads"].update(storage=c["reads"]["storage"][:-1]), "pre-block call with reads")),
    ("replay-pre-reference-inspected", "replay", r'"pre_block:differ": 1', pre_call(lambda c: True, lambda c: c.update(inspected_equal=False), "pre-block call")),
    ("replay-pre-registry-due", "replay", r'"registry_due:differ": 1', row_edit("replay", lambda r: r.get("kind") == "pre_block", lambda r: r.update(registry_due=not r["registry_due"]), "pre-block entry")),
    ("replay-missing-tx", "replay", r"oracle transaction count", file_edit("replay", has_txs(2), drop_tx, "block with two transactions")),
    ("replay-unknown-row", "replay", r"unknown row kind", file_edit("replay", has_txs(1), unknown_row, "block with a transaction")),
    ("replay-expected-counts", "replay", SUMMARY, expect("EXPECT_REPLAY", count("match", 1))),
    # replay: an accepted difference that no longer occurs, occurs otherwise, or names another transaction
    ("replay-known-stale", "replay", r"stale known difference: [a-z0-9]+:[0-9]+:[0-9]+ no longer differs", stale_replay_known),
    ("replay-known-other-frame", "replay", r"known difference [a-z0-9]+:[0-9]+:[0-9]+\.frames did not occur as listed", replay_known_entry(other_frame_gas)),
    ("replay-known-other-hash", "replay", r"known replay difference [a-z0-9]+:[0-9]+:[0-9]+: hash", replay_known_entry(other_hash)),
    # replay: a listed transaction of the class, unlisted, is counted as `class`, not `known`
    ("replay-class-unlisted", "replay", SUMMARY, unlist_replay_known),
    # block
    ("block-gas-used", "block", cmp("gas_used"), block_row("tx", "gas_used")),
    ("block-reference-inspected", "block", cmp("satin_plain_vs_inspected"), block_row("tx", "inspected_equal", change=lambda v: False)),
    ("block-counters-gas", "block", cmp("gas_used"), block_row("block_counters", "gas_used")),
    ("block-post-storage", "block", cmp("storage"), block_row("post_state", "storage", lambda r: r["storage"], first_value(lambda s: s + "1"))),
    ("block-missing-row", "block", r"oracle count", file_edit("block", has_txs(2), drop_tx, "block with two transactions")),
]

NOTE = []

def main():
    if sys.argv[1:] == ["list"]:
        ids = [f[0] for f in FAULTS]
        assert len(ids) == len(set(ids)), "duplicate fault id"
        for fid, gate, evidence, _ in FAULTS:
            print("%s\t%s\t%s" % (fid, gate, evidence))
    elif len(sys.argv) == 4 and sys.argv[1] == "apply":
        fault = [f for f in FAULTS if f[0] == sys.argv[2]]
        if not fault:
            raise SystemExit("faults.py: unknown fault %s" % sys.argv[2])
        extra = fault[0][3](sys.argv[3]) or []
        for a in extra:
            print(a)
        if NOTE:
            sys.stderr.write("target: %s\n" % ", ".join(NOTE))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
