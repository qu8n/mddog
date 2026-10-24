#!/usr/bin/env python3
"""Print median [IQR] (n) per metric, file, mode, and binary. Usage: table.py FILE.jsonl"""
import json
import sys
from collections import defaultdict

METRICS = ["first_paint", "scroll_total", "scroll_p50", "search_enter", "search_n", "resize", "reload"]
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
g = defaultdict(list)
bins = []
for r in rows:
    if r["bin"] not in bins:
        bins.append(r["bin"])
    for m in METRICS:
        if r.get(m) is not None:
            g[(r["file"], r["mode"], m, r["bin"])].append(r[m])


def q(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(p * len(xs)))]


def cell(xs):
    if not xs:
        return "-"
    return f"{q(xs, .5):6.1f} [{q(xs, .25):.1f}-{q(xs, .75):.1f}] n={len(xs)}"


files = sorted({r["file"] for r in rows})
print("| file | mode | metric | " + " | ".join(bins) + " | delta |")
print("|---" * (4 + len(bins)) + "|")
for f in files:
    for mode in ["cold", "warm"]:
        for m in METRICS:
            cols = [g[(f, mode, m, b)] for b in bins]
            if not any(cols):
                continue
            d = ""
            if len(bins) == 2 and cols[0] and cols[1]:
                a, b = q(cols[0], .5), q(cols[1], .5)
                d = f"{(b - a) / a * 100:+.0f}%" if a else ""
            print(f"| {f} | {mode} | {m} | " + " | ".join(cell(c) for c in cols) + f" | {d} |")
