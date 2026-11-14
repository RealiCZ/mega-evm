#!/usr/bin/env python3
"""Runs the Keccak benchmark's validation and measurements, and summarizes them.

Subcommands (all write under --out; binaries come from build.sh's --bin layout):

  validate   micro backend checks per ISA variant; kec-trace per block variant (every KECCAK256
             result and receipt Bloom against an independent Keccak, outcomes equal across
             variants and to the expected totals); Alloy's cache against uncached hashing on the
             captured inputs. Exits non-zero on any failure.
  micro      one-shot sizes, 32+32 streaming, and the exact-trace caches, per ISA variant, in
             rounds of alternating variant order, pinned to one core.
  blocks     the 263-block corpus per block variant, rounds of alternating order, pinned to one
             core; first traversal and warm traversals kept apart.
  parallel   the corpus on several threads per block variant (the cache contention smoke test).
  profile    perf samples of the timed traversals per block variant (skipped without perf).
  summarize  summary.json and summary.md from all of the above.

The corpus comes from MEGA_EVM2_BLOCK_DIR and MEGA_EVM2_REPLAY_REF, as the gates take it.
"""

import argparse
import csv
import io
import json
import os
import pathlib
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time


def pinned(core, argv):
    if core is None or core < 0 or not shutil.which("taskset"):
        return argv
    return ["taskset", "-c", str(core)] + argv


def load():
    try:
        return list(os.getloadavg())
    except OSError:
        return None


def run(argv, env=None, stdout=None, stderr=None, check=True, timeout=None):
    e = dict(os.environ)
    e.setdefault("RUST_MIN_STACK", "8388608")
    if env:
        e.update(env)
    return subprocess.run(argv, env=e, stdout=stdout, stderr=stderr, check=check, text=True,
                          timeout=timeout)


def json_lines(text):
    out = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("{"):
            out.append(json.loads(line))
    return out


def lst(s):
    return [x for x in s.split(",") if x]


# ---------------------------------------------------------------------------------------------
# validate


def cmd_validate(a):
    out = pathlib.Path(a.out)
    traces = out / "traces"
    traces.mkdir(parents=True, exist_ok=True)
    report = {"micro": {}, "blocks": {}, "cache": None, "failures": []}
    for isa in lst(a.isas):
        p = run([f"{a.bin}/micro/{isa}/kec-micro"], env={"VALIDATE_ONLY": "1", "KEC_VARIANT": isa},
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        (out / f"validate-micro-{isa}.log").write_text(p.stderr)
        ok = p.returncode == 0 and "validation=ok" in p.stderr
        meta = json_lines(p.stderr)
        report["micro"][isa] = {"ok": ok, "meta": meta[0] if meta else None}
        if not ok:
            report["failures"].append(f"micro backend validation failed for {isa}")
    reference = None
    for v in lst(a.variants):
        env = {"KEC_TRACE_OUT": str(traces)} if v == "baseline" else {}
        env["KEC_VARIANT"] = v
        p = run([f"{a.bin}/block/{v}/kec-trace"], env=env, stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, check=False)
        (out / f"trace-{v}.jsonl").write_text(p.stdout)
        (out / f"trace-{v}.log").write_text(p.stderr)
        summary = json_lines(p.stderr)
        outcomes = json_lines(p.stdout)
        entry = {"ok": p.returncode == 0, "summary": summary[-1] if summary else None}
        if p.returncode != 0:
            report["failures"].append(f"kec-trace failed for {v}: {p.stderr[-500:]}")
        if reference is None:
            reference = outcomes
        entry["outcomes_equal_baseline"] = outcomes == reference
        if outcomes != reference:
            report["failures"].append(f"{v}: per-block outcomes differ from {lst(a.variants)[0]}")
        s = entry["summary"] or {}
        expected = {"blocks": a.expect_blocks, "included": a.expect_included,
                    "refused": a.expect_refused, "gas": a.expect_gas}
        entry["expected_totals"] = all(s.get(k) == x for k, x in expected.items())
        if not entry["expected_totals"]:
            report["failures"].append(f"{v}: totals {s} differ from the expected {expected}")
        report["blocks"][v] = entry
    inputs = [str(traces / "opcode-inputs.bin"), str(traces / "bloom-inputs.bin")]
    p = run([f"{a.bin}/micro/cache/validate-cache"] + inputs, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, check=False)
    report["cache"] = {"ok": p.returncode == 0 and "cache-validation=ok" in p.stdout,
                       "log": p.stdout.strip()[-400:]}
    if not report["cache"]["ok"]:
        report["failures"].append("Alloy cache validation failed")
    (out / "validation.json").write_text(json.dumps(report, indent=2))
    print(json.dumps({"failures": report["failures"]}, indent=2))
    sys.exit(1 if report["failures"] else 0)


# ---------------------------------------------------------------------------------------------
# micro


def csv_rows(text, extra):
    rows = list(csv.DictReader(io.StringIO(text)))
    for r in rows:
        r.update(extra)
    return rows


def write_csv(path, rows):
    if not rows:
        path.write_text("")
        return
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)


def cmd_micro(a):
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    traces = [t for t in [out / "traces" / "opcode-inputs.bin", out / "traces" / "bloom-inputs.bin"]
              if t.exists()]
    isas = lst(a.isas)
    micro, prefix, trace = [], [], []
    meta = {}
    for rnd in range(a.rounds):
        order = isas if rnd % 2 == 0 else list(reversed(isas))
        for isa in order:
            env = {"KEC_VARIANT": isa, "KEC_REPS": str(a.reps)}
            extra = {"round": str(rnd), "load": json.dumps(load())}
            p = run(pinned(a.core, [f"{a.bin}/micro/{isa}/kec-micro"]), env=env,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            meta[isa] = json_lines(p.stderr)[0]
            micro += csv_rows(p.stdout, extra)
            p = run(pinned(a.core, [f"{a.bin}/micro/{isa}/prefix-bench"]), env=env,
                    stdout=subprocess.PIPE)
            prefix += csv_rows(p.stdout, extra)
            for t in traces:
                p = run(pinned(a.core, [f"{a.bin}/micro/{isa}/trace-cache", str(t)]), env=env,
                        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                trace += csv_rows(p.stdout, extra)
            print(f"micro round {rnd} {isa} done", flush=True)
    write_csv(out / "micro.csv", micro)
    write_csv(out / "prefix.csv", prefix)
    write_csv(out / "trace-cache.csv", trace)
    (out / "micro-meta.json").write_text(json.dumps(meta, indent=2))


# ---------------------------------------------------------------------------------------------
# blocks


def cmd_blocks(a):
    out = pathlib.Path(a.out)
    d = out / "blocks"
    d.mkdir(parents=True, exist_ok=True)
    variants = lst(a.variants)
    results, expected, failures = {}, None, []
    for rnd in range(a.rounds):
        order = variants if rnd % 2 == 0 else list(reversed(variants))
        for v in order:
            before = load()
            t0 = time.time()
            stem = d / f"{v}-{rnd}"
            with open(f"{stem}.jsonl", "w") as so, open(f"{stem}.first.jsonl", "w") as se:
                run(pinned(a.core, [f"{a.bin}/block/{v}/kec-block"]),
                    env={"BENCH_RUNS": str(a.runs), "KEC_VARIANT": v}, stdout=so, stderr=se)
            rows = json_lines(pathlib.Path(f"{stem}.jsonl").read_text())
            first = [r for r in json_lines(pathlib.Path(f"{stem}.first.jsonl").read_text())
                     if "first_ms" in r]
            outcomes = [{k: x for k, x in r.items() if k != "ms"} for r in rows]
            if expected is None:
                expected = outcomes
            if outcomes != expected or len(first) != len(rows):
                failures.append(f"{v} round {rnd}: outcomes or first-pass rows differ")
            warm = [sum(r["ms"][i] for r in rows) for i in range(a.runs)]
            res = {"round": rnd, "position": order.index(v), "first_pass_ms": sum(r["first_ms"] for r in first),
                   "warm_ms": warm, "load_before": before, "load_after": load(),
                   "process_s": time.time() - t0}
            results.setdefault(v, []).append(res)
            print(v, json.dumps(res), flush=True)
            (out / "blocks.json").write_text(json.dumps({"results": results, "failures": failures}, indent=2))
    rows = []
    for v, rs in results.items():
        for r in rs:
            rows.append({"variant": v, "round": r["round"], "kind": "first", "pass": 0,
                         "total_ms": f"{r['first_pass_ms']:.4f}"})
            for i, w in enumerate(r["warm_ms"]):
                rows.append({"variant": v, "round": r["round"], "kind": "warm", "pass": i + 1,
                             "total_ms": f"{w:.4f}"})
    write_csv(out / "blocks.csv", rows)
    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)


# ---------------------------------------------------------------------------------------------
# parallel


def cmd_parallel(a):
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    variants = lst(a.variants)
    rows = []
    for rnd in range(a.rounds):
        order = variants if rnd % 2 == 0 else list(reversed(variants))
        for v in order:
            p = run([f"{a.bin}/block/{v}/kec-block"],
                    env={"BENCH_RUNS": str(a.runs), "KEC_PARALLEL": a.configs, "KEC_VARIANT": v},
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            for r in json_lines(p.stdout):
                r["round"] = rnd
                rows.append(r)
            print(f"parallel round {rnd} {v} done", flush=True)
    with open(out / "parallel.jsonl", "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")


# ---------------------------------------------------------------------------------------------
# profile

KECCAK = re.compile(r"keccak|sha3|p1600|f1600|tiny_keccak", re.I)
CACHE = re.compile(r"fixed_cache|rapidhash|keccak_cache", re.I)
# `perf report --sort sym` rows: "  12.34%  [.] symbol" ([.] user space, [k] kernel).
LINE = re.compile(r"^\s*([\d.]+)%.*?\[(.)\]\s+(.*)$")


def cmd_profile(a):
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    report = {"available": False}
    perf = shutil.which("perf")
    if perf is None or run([perf, "--version"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                           check=False).returncode != 0:
        report["reason"] = "perf is not installed or does not run on this kernel"
        (out / "profile.json").write_text(json.dumps(report, indent=2))
        print(report["reason"])
        return
    report = {"available": True, "variants": {}}
    for v in lst(a.variants):
        if not os.path.exists(f"{a.bin}/block/{v}/kec-block"):
            report["variants"][v] = {"record_status": "not built on this runner"}
            continue
        tmp = pathlib.Path(tempfile.mkdtemp())
        ctl, ack = tmp / "ctl", tmp / "ack"
        os.mkfifo(ctl)
        os.mkfifo(ack)
        data = out / f"perf-{v}.data"
        argv = [perf, "record", "-e", "cpu-clock", "-F", str(a.freq), "-D", "-1",
                f"--control=fifo:{ctl},{ack}", "-o", str(data), "--"]
        argv += pinned(a.core, [f"{a.bin}/block/{v}/kec-block"])
        try:
            # Bounded: a perf that never opens its control fifo would leave kec-block waiting.
            p = run(argv, env={"BENCH_RUNS": str(a.runs), "KEC_PARALLEL": "1:split", "KEC_VARIANT": v,
                               "KEC_PERF_CTL": str(ctl), "KEC_PERF_ACK": str(ack)},
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=600)
        except subprocess.TimeoutExpired:
            report["variants"][v] = {"record_status": "timeout"}
            continue
        entry = {"record_status": p.returncode, "record_stderr": p.stderr[-600:]}
        traversals = json_lines(p.stdout)
        if traversals:
            entry["busy_over_wall"] = sum(t["busy_ms"] for t in traversals) / sum(t["wall_ms"] for t in traversals)
        if p.returncode == 0:
            r = run([perf, "report", "-i", str(data), "--stdio", "--no-children", "--sort", "sym",
                     "--percent-limit", "0"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
            (out / f"perf-{v}.txt").write_text(r.stdout)
            syms = []
            for line in r.stdout.splitlines():
                m = LINE.match(line)
                if m:
                    syms.append((float(m.group(1)), m.group(3).strip()))
            samples = re.search(r"^# Samples: (\S+)", r.stdout, re.M)
            keccak = sum(pct for pct, s in syms if KECCAK.search(s) and not CACHE.search(s))
            cache = sum(pct for pct, s in syms if CACHE.search(s))
            entry.update({
                "samples": samples.group(1) if samples else None,
                "keccak_leaf_pct": round(keccak, 3),
                "cache_leaf_pct": round(cache, 3),
                "keccak_symbols": [(pct, s) for pct, s in syms if KECCAK.search(s) and not CACHE.search(s)][:15],
                "top": syms[:25],
            })
            if "busy_over_wall" in entry and entry["busy_over_wall"] > 0:
                entry["keccak_leaf_pct_of_block_interval"] = round(keccak / entry["busy_over_wall"], 3)
        report["variants"][v] = entry
        shutil.rmtree(tmp, ignore_errors=True)
        print(v, json.dumps({k: x for k, x in entry.items() if k not in ("top", "keccak_symbols", "record_stderr")}), flush=True)
    (out / "profile.json").write_text(json.dumps(report, indent=2))


# ---------------------------------------------------------------------------------------------
# summarize


def med(xs):
    return statistics.median(xs) if xs else None


def pct_less(base, x):
    return None if base is None or x is None else (1 - x / base) * 100


def fmt(x, d=1):
    return "–" if x is None else f"{x:.{d}f}"


def read_json(p, default=None):
    try:
        return json.loads(pathlib.Path(p).read_text())
    except (OSError, ValueError):
        return default


def read_csv(p):
    try:
        with open(p) as f:
            return list(csv.DictReader(f))
    except OSError:
        return []


def cmd_summarize(a):
    out = pathlib.Path(a.out)
    s = {"arch": a.arch, "runner": read_json(out / "env" / "runner.json", {})}
    md = [f"# Keccak benchmark: {a.arch}", ""]
    r = s["runner"]
    md += [f"- CPU: {r.get('cpu', '?')} ({r.get('nproc', '?')} CPUs); image {r.get('image', '?')}",
           f"- rustc: {r.get('rustc', '?')}", ""]

    v = read_json(out / "validation.json", {})
    s["validation"] = {"failures": v.get("failures"),
                       "micro": {k: x["ok"] for k, x in v.get("micro", {}).items()},
                       "blocks": {k: x.get("summary") for k, x in v.get("blocks", {}).items()},
                       "cache": (v.get("cache") or {}).get("log")}
    md += ["## Validation", ""]
    md += [f"- failures: {v.get('failures')}"]
    for k, x in v.get("micro", {}).items():
        meta = x.get("meta") or {}
        md.append(f"- micro `{k}`: ok={x['ok']}, keccak_asm::IMPL `{meta.get('keccak_asm_impl')}`")
    for k, x in v.get("blocks", {}).items():
        sm = x.get("summary") or {}
        md.append(f"- kec-trace `{k}`: KECCAK256 checked {sm.get('keccak256_checked')}, Blooms {sm.get('receipts_bloom_checked')}, "
                  f"included {sm.get('included')}, refused {sm.get('refused')}, gas {sm.get('gas')}, same outcomes {x.get('outcomes_equal_baseline')}")
    md += [f"- cache: {(v.get('cache') or {}).get('log')}", ""]

    rows = read_csv(out / "micro.csv")
    if rows:
        groups = {}
        for x in rows:
            groups.setdefault((x["variant"], x["implementation"], int(x["len"])), []).append(float(x["ns_per_hash"]))
        medians = {k: med(xs) for k, xs in groups.items()}
        s["micro_median_ns"] = {f"{k[0]}/{k[1]}/{k[2]}": round(x, 3) for k, x in medians.items()}
        isas = sorted({k[0] for k in medians}, key=lambda i: (i != "generic", i))
        sizes = [32, 64, 96, 135, 136, 137, 1024, 4096, 16384, 24576, 65536]
        md += ["## Micro (median ns per hash; ratio = generic RustCrypto / this)", ""]
        cols = [(i, impl) for i in isas for impl in ("rustcrypto", "tiny", "asm")]
        md.append("| bytes | " + " | ".join(f"{i} {impl}" for i, impl in cols) + " |")
        md.append("|---:|" + "---:|" * len(cols))
        for n in sizes:
            base = medians.get(("generic", "rustcrypto", n))
            cells = []
            for c in cols:
                x = medians.get((c[0], c[1], n))
                cells.append("–" if x is None else f"{x:.1f} ({base / x:.2f}×)")
            md.append(f"| {n} | " + " | ".join(cells) + " |")
        md.append("")
    prefix = read_csv(out / "prefix.csv")
    if prefix:
        g = {}
        for x in prefix:
            g.setdefault((x["variant"], x["mode"]), []).append(float(x["ns"]))
        s["prefix_median_ns"] = {f"{k[0]}/{k[1]}": round(med(xs), 3) for k, xs in g.items()}
        md += ["## 64-byte prefix hash: one shot vs 32+32 streaming (median ns)", "",
               "| variant | " + " | ".join(m for m in ("rustcrypto_one", "rustcrypto_stream", "tiny_one", "tiny_stream", "asm_one", "asm_stream")) + " |",
               "|---|" + "---:|" * 6]
        for isa in sorted({k[0] for k in g}, key=lambda i: (i != "generic", i)):
            md.append(f"| {isa} | " + " | ".join(fmt(med(g.get((isa, m), [])), 1) for m in ("rustcrypto_one", "rustcrypto_stream", "tiny_one", "tiny_stream", "asm_one", "asm_stream")) + " |")
        md.append("")
    trace = read_csv(out / "trace-cache.csv")
    if trace:
        g, hits = {}, {}
        for x in trace:
            k = (x["variant"], x["trace"], x["mode"])
            g.setdefault(k, []).append(float(x["ms"]))
            hits[k] = int(x["hits"])
        s["trace_cache_median_ms"] = {"/".join(k): round(med(xs), 3) for k, xs in g.items()}
        md += ["## Exact-trace caches (median ms per whole trace; hits)", "",
               "| variant | trace | baseline | asm | tx1024 | block1024 | block4096 | asm_block4096 |", "|---|---|" + "---:|" * 6]
        for isa in sorted({k[0] for k in g}, key=lambda i: (i != "generic", i)):
            for t in ("opcode", "bloom"):
                cells = []
                for m in ("baseline", "asm", "tx1024", "block1024", "block4096", "asm_block4096"):
                    k = (isa, t, m)
                    cells.append(f"{fmt(med(g.get(k, [])), 2)}" + (f" ({hits[k]})" if hits.get(k) else ""))
                md.append(f"| {isa} | {t} | " + " | ".join(cells) + " |")
        md.append("")

    b = read_json(out / "blocks.json", {})
    if b.get("results"):
        res = b["results"]
        stats = {}
        for var, rs in res.items():
            warm = [w for r in rs for w in r["warm_ms"]]
            first = [r["first_pass_ms"] for r in rs]
            stats[var] = {"warm_median": med(warm), "warm_min": min(warm), "warm_max": max(warm),
                          "first": first, "first_median": med(first), "n_warm": len(warm)}
        base = stats.get("baseline", {})
        for var, st in stats.items():
            st["warm_less_pct"] = pct_less(base.get("warm_median"), st["warm_median"])
            st["first_less_pct"] = pct_less(base.get("first_median"), st["first_median"])
        s["blocks"] = stats
        s["blocks_failures"] = b.get("failures")
        md += ["## Blocks (263 blocks, whole-corpus ms per traversal)", "",
               "| variant | warm median [min–max] (n) | less than baseline | first passes | first median | less than baseline |",
               "|---|---:|---:|---:|---:|---:|"]
        for var, st in stats.items():
            md.append(f"| {var} | {st['warm_median']:.1f} [{st['warm_min']:.1f}–{st['warm_max']:.1f}] ({st['n_warm']}) | {fmt(st['warm_less_pct'], 2)}% | "
                      f"{', '.join(f'{x:.1f}' for x in st['first'])} | {st['first_median']:.1f} | {fmt(st['first_less_pct'], 2)}% |")
        md += ["", f"- failures: {b.get('failures')}", ""]

    par = []
    try:
        par = [json.loads(l) for l in open(out / "parallel.jsonl")]
    except OSError:
        pass
    if par:
        g = {}
        for x in par:
            g.setdefault((x["variant"], x["threads"], x["mode"]), []).append(x)
        st = {f"{k[0]}/{k[1]}:{k[2]}": {"busy_median": med([x["busy_ms"] for x in xs]),
                                         "wall_median": med([x["wall_ms"] for x in xs]), "n": len(xs)}
              for k, xs in g.items()}
        s["parallel"] = st
        md += ["## Cache contention smoke test (median ms per traversal)", "",
               "busy = summed block-execution time over all threads; wall = traversal wall time.", "",
               "| variant | config | busy | wall | busy less than baseline |", "|---|---|---:|---:|---:|"]
        for k, xs in sorted(g.items()):
            base = st.get(f"baseline/{k[1]}:{k[2]}", {}).get("busy_median")
            cur = st[f"{k[0]}/{k[1]}:{k[2]}"]
            md.append(f"| {k[0]} | {k[1]}:{k[2]} | {cur['busy_median']:.1f} | {cur['wall_median']:.1f} | {fmt(pct_less(base, cur['busy_median']), 2)}% |")
        md.append("")

    prof = read_json(out / "profile.json")
    if prof is not None:
        s["profile"] = {k: {x: y for x, y in e.items() if x not in ("top",)} for k, e in prof.get("variants", {}).items()} if prof.get("available") else prof
        md += ["## Sampled Keccak share (perf, leaf symbols, timed traversals only)", ""]
        if not prof.get("available"):
            md.append(f"- not sampled: {prof.get('reason')}")
        for k, e in prof.get("variants", {}).items():
            md.append(f"- `{k}`: Keccak leaf {e.get('keccak_leaf_pct')}% of samples ({e.get('keccak_leaf_pct_of_block_interval')}% of the block-execution interval), "
                      f"cache leaf {e.get('cache_leaf_pct')}%, samples {e.get('samples')}")
        md.append("")
    (out / "summary.json").write_text(json.dumps(s, indent=2))
    (out / "summary.md").write_text("\n".join(md) + "\n")
    print("\n".join(md))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("validate", "micro", "blocks", "parallel", "profile", "summarize"):
        p = sub.add_parser(name)
        p.add_argument("--out", required=True)
        if name != "summarize":
            p.add_argument("--bin", required=True)
        if name in ("micro", "blocks", "profile"):
            p.add_argument("--core", type=int, default=None)
        if name in ("validate", "micro"):
            p.add_argument("--isas", default="generic")
        if name in ("validate", "blocks", "parallel", "profile"):
            p.add_argument("--variants", default="baseline,asm,cache,asm-cache")
        if name in ("micro", "blocks", "parallel"):
            p.add_argument("--rounds", type=int, default=2)
        if name in ("blocks", "parallel", "profile"):
            p.add_argument("--runs", type=int, default=3)
        if name == "micro":
            p.add_argument("--reps", type=int, default=7)
        if name == "parallel":
            p.add_argument("--configs", required=True)
        if name == "profile":
            p.add_argument("--freq", type=int, default=1999)
        if name == "validate":
            p.add_argument("--expect-blocks", type=int, default=263)
            p.add_argument("--expect-included", type=int, default=27483)
            p.add_argument("--expect-refused", type=int, default=703)
            p.add_argument("--expect-gas", type=int, default=5827717115)
        if name == "summarize":
            p.add_argument("--arch", required=True)
    a = ap.parse_args()
    globals()[f"cmd_{a.cmd}"](a)


if __name__ == "__main__":
    main()
