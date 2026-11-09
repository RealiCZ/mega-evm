#!/usr/bin/env python3
"""Validate the same timed blocks and transactions and summarize repeated block-executor
measurements."""

import json
import math
import statistics
import sys
from pathlib import Path


def percentile(values, fraction):
    """Return a nearest-rank percentile, preserving observed sample values."""
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def summarize(engine_path, reference_path):
    sides = {}
    for side, path in [("engine", engine_path), ("reference", reference_path)]:
        rows = [json.loads(line) for line in Path(path).read_text().splitlines()]
        assert rows, "empty timing output"
        identities = [(r["set"], r["block"]) for r in rows]
        assert len(set(identities)) == len(identities), "duplicate block"
        runs = len(rows[0]["ms"])
        assert runs >= 3, "at least three measured rounds"
        for row in rows:
            assert row["txs"] == row["included"] + row["refused"]
            assert len(row["ms"]) == runs
            assert all(math.isfinite(x) and x > 0 for x in row["ms"])
        sides[side] = rows
    # The same blocks and transactions on both sides. What each side includes, refuses and charges
    # is its own: the block gate that ran first holds the engine's outcome to the reference's, a
    # difference of an accepted class included (a deposit's history gas, which can leave a later
    # transaction without room in the block's gas), so it is not compared again here.
    work = lambda rows: [(r["set"], r["block"], r["txs"]) for r in rows]
    assert work(sides["engine"]) == work(sides["reference"]), "timed blocks or transactions differ"
    totals = {}
    for side, rows in sides.items():
        rounds = [sum(samples) for samples in zip(*(r["ms"] for r in rows))]
        per_block = [statistics.median(r["ms"]) for r in rows]
        median = statistics.median(rounds)
        totals[side] = median
        gas = sum(r["gas"] for r in rows)
        print(
            f"{side}: blocks={len(rows)} txs={sum(r['txs'] for r in rows)} "
            f"included={sum(r['included'] for r in rows)} refused={sum(r['refused'] for r in rows)} "
            f"runs={len(rounds)} total_median_ms={median:.6f} "
            f"total_p95_ms={percentile(rounds, .95):.6f} "
            f"total_min_ms={min(rounds):.6f} total_max_ms={max(rounds):.6f} "
            f"block_median_ms={statistics.median(per_block):.6f} "
            f"block_p95_ms={percentile(per_block, .95):.6f} "
            f"receipt_gas={gas} gas_per_second={gas * 1000 / median:.3f}"
        )
    rows = sides["engine"]
    print(f"BENCH blocks={len(rows)} txs={sum(r['txs'] for r in rows)} "
          f"engine_ms={totals['engine']:.6f} reference_ms={totals['reference']:.6f}")


if __name__ == "__main__":
    summarize(*sys.argv[1:])
