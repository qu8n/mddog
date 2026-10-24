"""External change check: another process saves with write-then-rename; the
preview must re-render within 200 ms and keep the content at the top."""
import os
import shutil
import sys
import time

from lib import BIN, keys, marks, report, run, views, wait, workdir

d = workdir("external")
doc = os.path.join(d, "doc.md")
shutil.copy(os.path.join(os.path.dirname(__file__), "..", "..", "fixtures", "gfm.md"), doc)
trace = os.path.join(d, "trace.log")
run(f"clear; MDDOG_TRACE={trace} {BIN} {doc}")
wait(lambda: views(trace))
for _ in range(3):
    keys("d")
before = wait(lambda: [v for v in views(trace) if v["scroll"] == 3 * (v["rows"] // 2)])
before = before[-1] if before else None
time.sleep(0.5)
n_before = len(views(trace))
text = open(doc).read()
tmp = os.path.join(d, ".doc.md.tmp")
with open(tmp, "w") as f:
    f.write("".join(f"Line added by another process {i}\n\n" for i in range(1, 21)) + text)
t0 = time.time() * 1000
os.rename(tmp, doc)
reloaded = wait(lambda: [m for m in marks(trace, "reloaded") if m >= t0])
after_views = wait(lambda: views(trace)[n_before:])
after = after_views[-1] if after_views else None
keys("q")
latency = (reloaded[0] - t0) if reloaded else None
ok = before and after and latency is not None and latency < 200 and before["top"] == after["top"]
sys.exit(report("external-change", ok, {"latency_ms": latency, "before": before, "after": after}))
