#!/usr/bin/env python3
"""Interactive viewer benchmark in a herdr test session.

Usage: viewer.py OUT.jsonl ROUNDS BIN [BIN...]

Each round runs every file and mode once per binary, alternating the binary
order between rounds. A run launches the viewer with MDDOG_TRACE and measures,
from the trace: first paint (ms since main), a scroll to the end with `f` (each
key waits for its frame), search Enter and `n` for "the", resize re-render
(set with stty, since a headless herdr server does not resize the pty), and
reload after an append to the file. Absolute binary paths only: the pane's
shell runs them. Summarize with table.py; compare MDDOG_DUMP_TILES output of
two builds with tiles.py.

Setup, once: start a named test session with its own HOME, make a workspace,
and run `tty` in its pane for VP_TTY:

  H=$TMPDIR/vp; mkdir -p $H/.config/herdr; touch $H/.config/herdr/config.toml
  HOME=$H HERDR_CONFIG_PATH=$H/.config/herdr/config.toml herdr --session vp server &
  HOME=$H HERDR_CONFIG_PATH=$H/.config/herdr/config.toml herdr --session vp workspace create --cwd .

Env: VP_TTY (required), VP_HOME (default $TMPDIR/vp), VP_PANE (default
w1:p1), VP_FILES, VP_MODES (cold,warm), VP_SCEN (scroll,search,resize,reload),
VP_RESIZES (default 4), VP_DUMP (a folder for tile dumps).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
HOME = os.environ.get("VP_HOME") or os.path.join(os.environ["TMPDIR"], "vp")
PANE = os.environ.get("VP_PANE", "w1:p1")
TTY = os.environ.get("VP_TTY", "")
FILES = os.environ.get("VP_FILES", "README.md,fixtures/gfm.md,ARCHITECTURE.md,docs/behavior.md").split(",")
MODES = os.environ.get("VP_MODES", "cold,warm").split(",")
SCEN = os.environ.get("VP_SCEN", "scroll,search,resize,reload").split(",")
BENCH = os.path.join(REPO, "target", "bench")
WORK = os.path.join(BENCH, "traces")


def herdr(*args):
    """Runs a herdr command in the test session. Exits the whole run on the first failure, since
    every later call would fail the same way and each run would wait out its timeout."""
    env = dict(os.environ, HOME=HOME, HERDR_CONFIG_PATH=os.path.join(HOME, ".config/herdr/config.toml"))
    # Set inside a herdr pane, this points at the user's own session, not the test one.
    env.pop("HERDR_SOCKET_PATH", None)
    r = subprocess.run(["herdr", "--session", "vp", *args], capture_output=True, text=True, env=env)
    if r.returncode != 0:
        msg = [f"herdr {' '.join(args[:2])} failed ({r.returncode}): {(r.stderr or r.stdout).strip()}"]
        if os.environ.get("XDG_CONFIG_HOME"):
            msg.append("XDG_CONFIG_HOME is set, and herdr looks for its socket under it. Unset it; mdd in the pane reads $VP_HOME/.config/mddog.")
        outer = os.environ.get("HERDR_CONFIG_PATH")
        if outer and outer != env["HERDR_CONFIG_PATH"]:
            msg.append(f"HERDR_CONFIG_PATH is {outer}, but the bench uses {env['HERDR_CONFIG_PATH']}. Start the vp session with VP_HOME's config.")
        sys.exit("\n".join(msg))
    return r


def keys(*ks):
    herdr("pane", "send-keys", PANE, *ks)


def lines(trace):
    if not os.path.exists(trace):
        return []
    out = []
    for l in open(trace):
        p = l.rstrip("\n").split(" ", 3)
        try:
            out.append((float(p[0]), float(p[1]), p[2], p[3] if len(p) > 3 else ""))
        except (ValueError, IndexError):
            pass
    return out


def views(trace):
    return [x for x in lines(trace) if x[2] == "view"]


def wait(pred, timeout=10.0):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(0.003)
    return None


def scroll_of(v):
    return int(re.search(r"scroll=(\d+)", v[3]).group(1))


def next_view(trace, n_before, timeout=5.0):
    """Wait for a view line after the first n_before views; return it."""
    got = wait(lambda: (lambda vs: vs[n_before:] or None)(views(trace)), timeout)
    return got[0] if got else None


def press(trace, key, timeout=5.0):
    """Send a key, return ms from the key line in the trace to the next view."""
    n_keys = sum(1 for x in lines(trace) if x[2] == "key")
    n_views = len(views(trace))
    keys(key)
    k = wait(lambda: (lambda ks: ks[n_keys:] or None)([x for x in lines(trace) if x[2] == "key"]), timeout)
    v = next_view(trace, n_views, timeout)
    if not k or not v:
        return None, v
    # Settle: let the loop finish follow-up work (prefetch tick) before the next key.
    return v[0] - k[0][0], v


def mark_to_view(trace, mark, n_lines_before, timeout=8.0):
    def find():
        ls = lines(trace)[n_lines_before:]
        for i, x in enumerate(ls):
            # The watcher also reports a no-op event after each reload; count only events that start a render.
            if mark == "file event" and not (i + 1 < len(ls) and ls[i + 1][3] == "page written"):
                continue
            if x[2] == "mark" and x[3] == mark:
                for y in ls[i + 1:]:
                    if y[2] == "view":
                        return y[0] - x[0]
        return None
    return wait(find, timeout)


def run_one(binary, mode, rel, tag):
    os.makedirs(WORK, exist_ok=True)
    trace = os.path.join(WORK, f"{tag}.log")
    if os.path.exists(trace):
        os.remove(trace)
    doc = os.path.join(BENCH, rel)
    env = "MDDOG_NO_SPARE=1 " if mode == "cold" else ""
    dump = os.environ.get("VP_DUMP")
    if dump:
        d = os.path.join(dump, tag)
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
        env += f"MDDOG_DUMP_TILES={d} "
    keys("ctrl+u")
    herdr("pane", "run", PANE, f"clear; {env}MDDOG_TRACE={trace} {binary} {doc}")
    r = {"bin": os.path.basename(binary), "mode": mode, "file": rel}
    fp = wait(lambda: [x for x in lines(trace) if x[2] == "mark" and x[3] == "first paint sent"], 15)
    if not fp:
        r["error"] = "no first paint"
        keys("q")
        return r
    r["first_paint"] = fp[0][1]
    # Let startup finish: file event, prefetch of the next tiles.
    time.sleep(1.2)
    if "scroll" in SCEN:
        steps, last, t0 = [], -1, None
        for _ in range(400):
            ms, v = press(trace, "f")
            if ms is None:
                break
            s = scroll_of(v)
            if s == last:
                break
            steps.append(ms)
            last = s
        r["scroll_total"] = sum(steps)
        r["scroll_steps"] = len(steps)
        r["scroll_p50"] = sorted(steps)[len(steps) // 2] if steps else None
        press(trace, "g")
        time.sleep(1.2)
    if "search" in SCEN:
        n_keys = sum(1 for x in lines(trace) if x[2] == "key")
        keys("/")
        if not wait(lambda: [x for x in lines(trace)[0:] if x[2] == "key"][n_keys:]):
            r["error"] = "search prompt"
            keys("q")
            return r
        time.sleep(0.05)
        herdr("pane", "send-text", PANE, "the")
        if not wait(lambda: len([x for x in lines(trace) if x[2] == "key"]) >= n_keys + 4):
            r["error"] = "search text"
            keys("esc", "q")
            return r
        ms, _ = press(trace, "enter")
        r["search_enter"] = ms
        ns = []
        for _ in range(15):
            ms, _ = press(trace, "n")
            if ms is not None:
                ns.append(ms)
            time.sleep(0.03)
        r["search_n"] = sorted(ns)[len(ns) // 2] if ns else None
        keys("esc")
        time.sleep(1.2)
    if "resize" in SCEN:
        rs = []
        for j in range(int(os.environ.get("VP_RESIZES", "4"))):
            n = len(lines(trace))
            # Headless herdr does not resize the pty on a layout change, so set its size directly.
            subprocess.run(["stty", "-f", TTY, "cols", "80" if j % 2 == 0 else "119"])
            ms = mark_to_view(trace, "resize", n)
            if ms is not None:
                rs.append(ms)
            time.sleep(0.4)
        r["resize"] = sorted(rs)[len(rs) // 2] if rs else None
    if "reload" in SCEN:
        orig = open(doc, "rb").read()
        rl = []
        try:
            for i in range(3):
                n = len(lines(trace))
                with open(doc, "ab") as f:
                    f.write(f"\nbench line {i}\n".encode())
                ms = mark_to_view(trace, "file event", n)
                if ms is not None:
                    rl.append(ms)
                time.sleep(0.6)
        finally:
            open(doc, "wb").write(orig)
            time.sleep(0.6)
        r["reload"] = sorted(rl)[len(rl) // 2] if rl else None
    keys("q")
    time.sleep(0.4)
    return r


def setup():
    shutil.rmtree(BENCH, ignore_errors=True)
    for rel in FILES:
        os.makedirs(os.path.dirname(os.path.join(BENCH, rel)), exist_ok=True)
        shutil.copy(os.path.join(REPO, rel), os.path.join(BENCH, rel))
    os.makedirs(os.path.join(BENCH, "fixtures"), exist_ok=True)
    shutil.copy(os.path.join(REPO, "fixtures/diagram.svg"), os.path.join(BENCH, "fixtures/diagram.svg"))


def main():
    out, runs, bins = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
    if "resize" in SCEN and not TTY:
        sys.exit("set VP_TTY to the test pane's tty (run `tty` in it)")
    setup()
    # Warm-up: one warm run per binary so a spare exists and caches are filled.
    for b in bins:
        run_one(b, "warm", FILES[0], "warmup")
        time.sleep(2)
    with open(out, "a") as f:
        for i in range(runs):
            order = bins if i % 2 == 0 else bins[::-1]
            for rel in FILES:
                for mode in MODES:
                    for b in order:
                        r = run_one(b, mode, rel, f"{os.path.basename(b)}-{mode}-{rel.replace('/', '_')}-{i}")
                        r["round"] = i
                        f.write(json.dumps(r) + "\n")
                        f.flush()
                        print(json.dumps(r), flush=True)
                        # A warm run starts the next spare; give it time to warm.
                        time.sleep(2.0 if mode == "warm" else 0.5)


if __name__ == "__main__":
    main()
