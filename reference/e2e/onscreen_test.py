"""On-screen check: the Ghostty window capture of the preview must match the
tool's own PNG of the same viewport. Usage: python3 onscreen_test.py FILE"""
import os
import sys
import time

import numpy as np
from PIL import Image

from capture import grab, locate
from lib import BIN, keys, report, run, views, wait, workdir

src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "..", "fixtures", "gfm.md"))
d = workdir("onscreen")
trace, dump = os.path.join(d, "trace.log"), os.path.join(d, "view.png")
run(f"clear; MDDOG_TRACE={trace} MDDOG_DUMP_VIEW={dump} {BIN} {src}")
wait(lambda: len(views(trace)) >= 2 or (views(trace) and os.path.exists(dump)))
time.sleep(1.0)
keys("d")
wait(lambda: [v for v in views(trace) if v["scroll"] == 20])
time.sleep(0.6)
shot = grab(os.path.join(d, "window.png"))
own = np.asarray(Image.open(dump).convert("RGB")).astype(np.int16)
x, y = locate(shot, own)
crop = shot[y:y + own.shape[0], x:x + own.shape[1]]
Image.fromarray(crop.astype(np.uint8)).save(os.path.join(d, "crop.png"))
diff = np.abs(crop - own)
heat = (diff.max(axis=2) > 3).astype(np.uint8) * 255
Image.fromarray(heat).save(os.path.join(d, "heat.png"))
mismatch = float((diff.max(axis=2) > 3).mean())
keys("q")
ok = mismatch < 0.001
sys.exit(report("on-screen", ok, {"offset": [int(x), int(y)], "size": list(own.shape[:2]), "max_channel_diff": int(diff.max()), "pixels_over_3": mismatch, "dir": d}))
