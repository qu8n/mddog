"""Search check: a word on at least 3 screens; capture after each n and N and
confirm the expected match is in the viewport with the current-match color."""
import os
import re
import sys
import time

import numpy as np
from PIL import Image

from capture import grab, locate
from lib import BIN, keys, report, run, text, views, wait, workdir

src = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "fixtures", "gfm.md"))
word = sys.argv[1] if len(sys.argv) > 1 else "fixture"
d = workdir("search")
trace, dump = os.path.join(d, "trace.log"), os.path.join(d, "view.png")
run(f"clear; MDDOG_TRACE={trace} MDDOG_DUMP_VIEW={dump} {BIN} {src}")
wait(lambda: views(trace))
time.sleep(1.0)
CURRENT = np.array([0xF2, 0xCC, 0x60])


def matches():
    out = []
    for line in open(trace):
        m = re.search(r" match (\d+)/(\d+) rect=([\d.]+),([\d.]+),([\d.]+),([\d.]+)", line)
        if m:
            out.append((int(m.group(1)), int(m.group(2)), [float(m.group(i)) for i in range(3, 7)]))
    return out


keys("/")
text(word)
keys("enter")
wait(lambda: matches())
time.sleep(0.6)
total = matches()[0][1]
steps = ["start"] + ["n"] * (total + 1) + ["N"] * 3
results = []
for i, k in enumerate(steps):
    if k != "start":
        seen = len(matches())
        keys(k)
        wait(lambda: len(matches()) > seen)
        time.sleep(0.6)
    idx, _, rect = matches()[-1]
    v = views(trace)[-1]
    shot = grab(os.path.join(d, f"shot-{i:02d}.png"))
    own = np.asarray(Image.open(dump).convert("RGB")).astype(np.int16)
    x, y = locate(shot, own)
    s, top = v["scale"], (v["phase"] + v["scroll"] * v["cell_h"]) / v["scale"]
    rx, ry = int(x + rect[0] * s), int(y + (rect[1] - top) * s)
    region = shot[ry:ry + int(rect[3] * s), rx:rx + int(rect[2] * s)]
    in_view = rect[1] >= top and (rect[1] + rect[3] - top) * s <= own.shape[0]
    hits = float((np.abs(region - CURRENT).max(axis=2) < 24).mean()) if region.size else 0.0
    screen = int(rect[1] * s // (v["rows"] * v["cell_h"]))
    results.append({"key": k, "current": idx, "screen": screen, "in_view": in_view, "highlight_fraction": round(hits, 3)})
keys("q")
expected = [0]
for k in steps[1:]:
    expected.append((expected[-1] + (1 if k == "n" else -1)) % total)
order = [r["current"] for r in results]
screens = sorted({r["screen"] for r in results})
ok = len(screens) >= 3 and order == expected and all(r["in_view"] and r["highlight_fraction"] > 0.3 for r in results)
sys.exit(report("search", ok, {"word": word, "screens": screens, "order": order, "expected": expected, "steps": results}))
