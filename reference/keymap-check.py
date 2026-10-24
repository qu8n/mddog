"""Check that every test named in docs/behavior.md's tables ran and passed.

Usage: python3 keymap-check.py  (runs `cargo test --release` from the repo root)
"""
import os
import re
import subprocess
import sys

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
section = open(os.path.join(root, "docs", "behavior.md")).read()
rows = [l for l in section.splitlines() if l.startswith("| ") and not l.startswith("| #") and not l.startswith("|---")]
out = subprocess.run(["cargo", "test", "--release"], cwd=root, capture_output=True, text=True).stdout
passed = set(re.findall(r"^test (?:\S+::)*(\w+) \.\.\. ok$", out, re.M))
missing = []
checked = 0
for r in rows:
    cell = r.rstrip("|").split("|")[-1]
    names = re.findall(r"`([a-z]\w*)`", cell)
    checked += len(names)
    for n in names:
        if n not in passed:
            missing.append((r.split("|")[1].strip(), n))
print(f"{len(rows)} rows, {checked} test names checked")
for row, n in missing:
    print(f"row {row}: {n} did not pass")
sys.exit(1 if missing else 0)
