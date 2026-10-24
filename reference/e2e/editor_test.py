"""Editor check: scroll down, open $EDITOR, add 40 lines above the view, save
with write-then-rename (or :w in nvim), and confirm the same content is at
the top after the editor exits. Usage: python3 editor_test.py [script|nvim]"""
import os
import shutil
import sys
import time

from lib import BIN, keys, marks, report, run, text, views, wait, workdir

mode = sys.argv[1] if len(sys.argv) > 1 else "script"
d = workdir("editor-" + mode)
doc = os.path.join(d, "doc.md")
shutil.copy(os.path.join(os.path.dirname(__file__), "..", "..", "fixtures", "gfm.md"), doc)
trace = os.path.join(d, "trace.log")
editor = os.path.abspath(os.path.join(os.path.dirname(__file__), "add40.py")) if mode == "script" else "nvim -u NONE -i NONE"
run(f"clear; EDITOR='{editor}' MDDOG_TRACE={trace} {BIN} {doc}")
wait(lambda: views(trace))
keys("d")
keys("d")
before = wait(lambda: [v for v in views(trace) if v["scroll"] == 2 * (v["rows"] // 2)])
before = before[-1] if before else None
keys("e")
if mode == "nvim":
    wait(lambda: marks(trace, "editor start"))
    time.sleep(0.8)
    text(":0put =repeat(['nvim line'], 40)\r")
    time.sleep(0.3)
    text(":wq\r")
reload_at = wait(lambda: marks(trace, "editor reload"), timeout=30)
time.sleep(0.3)
after = views(trace)[-1] if views(trace) else None
keys("q")
lines_added = open(doc).read().count("\n") - open(os.path.join(os.path.dirname(__file__), "..", "..", "fixtures", "gfm.md")).read().count("\n")
ok = before and after and reload_at and before["top"] == after["top"] and lines_added == 40
sys.exit(report(f"editor-{mode}", ok, {"before": before, "after": after, "lines_added": lines_added}))
