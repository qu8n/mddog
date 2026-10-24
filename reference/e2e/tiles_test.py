"""Tile check: every tile sent after a search or n shows the current match in
the current-match color, a new search re-sends the tiles that showed the old
query's highlights, and the resident byte count matches the tiles held."""
import os
import re
import sys
import time

import numpy as np
from PIL import Image

from lib import BIN, keys, report, run, text, views, wait, workdir

src = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "fixtures", "gfm.md"))
d = workdir("tiles")
trace, tiles = os.path.join(d, "trace.log"), os.path.join(d, "tiles")
os.makedirs(tiles)
run(f"clear; MDDOG_TRACE={trace} MDDOG_DUMP_TILES={tiles} {BIN} {src}")
wait(lambda: views(trace))
time.sleep(1.5)
CURRENT = np.array([0xF2, 0xCC, 0x60])


def matches():
    out = []
    for line in open(trace):
        m = re.search(r" match (\d+)/(\d+) rect=([\d.]+),([\d.]+),([\d.]+),([\d.]+)", line)
        if m:
            out.append((int(m.group(1)), [float(m.group(i)) for i in range(3, 7)]))
    return out


def highlighted(rect, v):
    s, y = v["scale"], rect[1] * v["scale"]
    held = []
    for name in os.listdir(tiles):
        tid, top = map(int, name[:-4].removeprefix("band-").split("-"))
        if tid in v["placed"] and top <= y and y + rect[3] * s <= top + Image.open(os.path.join(tiles, name)).height:
            held.append((name.startswith("band-"), top, name))
    if not held:
        return 0.0
    _, top, name = max(held)
    im = np.asarray(Image.open(os.path.join(tiles, name)).convert("RGB")).astype(np.int16)
    region = im[int(y - top):int(y - top + rect[3] * s), int(rect[0] * s):int((rect[0] + rect[2]) * s)]
    return round(float((np.abs(region - CURRENT).max(axis=2) < 24).mean()), 3) if region.size else 0.0


def settle(n_views, n_matches=None):
    wait(lambda: len(views(trace)) > n_views and (n_matches is None or len(matches()) > n_matches))
    time.sleep(0.4)
    return views(trace)[-1]


def search(word):
    n, m = len(views(trace)), len(matches())
    keys("/")
    text(word)
    keys("enter")
    return settle(n, m if word == "fixture" else None)


first = search("fixture")
colors = [highlighted(matches()[-1][1], first)]
empty = search("zzqxv")
resent = set(empty["placed"]) != set(first["placed"])
search("fixture")
for _ in range(25):
    n, m = len(views(trace)), len(matches())
    keys("n")
    v = settle(n, m)
    colors.append(highlighted(matches()[-1][1], v))
keys("esc")
time.sleep(0.6)
keys("q")
drift = [v for v in views(trace) if v["resident"] != v["held"]]
ok = resent and not drift and min(colors) > 0.3
sys.exit(report("tiles", ok, {"resent_after_empty_search": resent, "current_match_color": colors, "views": len(views(trace)), "drift": drift[:3]}))
