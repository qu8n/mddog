#!/usr/bin/env python3
"""Scripted $EDITOR: add 40 lines at the top, save as write-to-temp then rename."""
import os
import sys

path = sys.argv[1]
text = open(path).read()
tmp = os.path.join(os.path.dirname(path), "." + os.path.basename(path) + ".tmp")
with open(tmp, "w") as f:
    f.write("".join(f"Inserted line {i}\n\n" for i in range(1, 21)) + text)
os.rename(tmp, path)
