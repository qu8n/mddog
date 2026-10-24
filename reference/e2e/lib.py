import json
import os
import re
import shutil
import subprocess
import time

PANE = os.environ.get("MDDOG_PANE", "wG1:p2")
BIN = os.environ.get("MDDOG_BIN") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "target", "release", "mdd")


def herdr(*args):
    return subprocess.run(["herdr", *args], capture_output=True, text=True)


def keys(*ks):
    herdr("pane", "send-keys", PANE, *ks)


def text(s):
    herdr("pane", "send-text", PANE, s)


def run(cmd):
    time.sleep(0.3)
    herdr("pane", "send-keys", PANE, "ctrl+u")
    herdr("pane", "run", PANE, cmd)


def views(trace):
    out = []
    if not os.path.exists(trace):
        return out
    for line in open(trace):
        parts = line.rstrip("\n").split(" ", 3)
        if len(parts) == 4 and parts[2] == "view":
            m = re.search(r"scroll=(\d+) phase=(\d+) placed=\[([^\]]*)\] resident=(\d+) held=(\d+) scale=([\d.]+) cell=(\d+)x(\d+) rows=(\d+) doc=\d+ top=(.*)$", parts[3])
            placed = [int(x) for x in m.group(3).split(",") if x.strip()]
            out.append({
                "epoch": float(parts[0]), "scroll": int(m.group(1)), "phase": int(m.group(2)), "placed": placed,
                "resident": int(m.group(4)), "held": int(m.group(5)), "scale": float(m.group(6)),
                "cell_w": int(m.group(7)), "cell_h": int(m.group(8)), "rows": int(m.group(9)), "top": m.group(10),
            })
    return out


def marks(trace, name):
    if not os.path.exists(trace):
        return []
    return [float(l.split(" ")[0]) for l in open(trace) if l.rstrip("\n").endswith("mark " + name)]


def wait(pred, timeout=15.0, step=0.02):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(step)
    return None


def workdir(name):
    d = os.path.join(os.environ.get("TMPDIR", "/tmp"), "mddog-e2e-" + name)
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    return d


def report(name, ok, detail):
    print(json.dumps({"test": name, "pass": bool(ok), **detail}, indent=1))
    return 0 if ok else 1
