#!/usr/bin/env python3
"""Compare the MDDOG_DUMP_TILES folders of two runs: tiles.py DIR_A DIR_B.

Tiles are matched by send order, since image ids differ between runs. The PNG
encoder is deterministic, so equal files mean equal pixels."""
import hashlib
import os
import re
import sys


def sent(d):
    out = []
    for f in os.listdir(d):
        m = re.match(r"(band-)?(\d+)-(\d+)\.png$", f)
        if m:
            out.append((int(m.group(2)), m.group(1) or "", int(m.group(3)), os.path.join(d, f)))
    return sorted(out)


def digest(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


a, b = sent(sys.argv[1]), sent(sys.argv[2])
same = differ = 0
for x, y in zip(a, b):
    if x[1:3] != y[1:3]:
        print("order differs", x[1:3], y[1:3])
        differ += 1
    elif digest(x[3]) == digest(y[3]):
        same += 1
    else:
        print("pixels differ", x[1:3], os.path.basename(x[3]), os.path.basename(y[3]))
        differ += 1
print(f"{os.path.basename(sys.argv[1])}: {len(a)} vs {len(b)} tiles, {same} same, {differ} differ")
sys.exit(1 if differ or len(a) != len(b) else 0)
