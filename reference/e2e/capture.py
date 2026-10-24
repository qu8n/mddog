import io
import os
import subprocess

import numpy as np
from PIL import Image, ImageCms

P3 = "/System/Library/ColorSync/Profiles/Display P3.icc"


def window_id(owner="Ghostty"):
    swift = os.path.join(os.path.dirname(__file__), "winlist")
    if not os.path.exists(swift):
        subprocess.run(["swiftc", "-O", swift + ".swift", "-o", swift], check=True)
    for line in subprocess.run([swift, owner], capture_output=True, text=True).stdout.splitlines():
        f = line.split("\t")
        if "onscreen=true" in line and "layer=0" in line:
            return f[0]
    raise SystemExit("no on-screen Ghostty window (is the screen locked?)")


def grab(path):
    subprocess.run(["caffeinate", "-u", "-t", "1"])
    r = subprocess.run(["screencapture", "-x", "-o", "-l", window_id(), path], capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit("screencapture failed: " + r.stderr.strip())
    im = Image.open(path)
    icc = im.info.get("icc_profile")
    rgb = im.convert("RGB")
    if icc:
        rgb = ImageCms.profileToProfile(rgb, ImageCms.ImageCmsProfile(io.BytesIO(icc)), ImageCms.getOpenProfile(P3), renderingIntent=0)
    return np.asarray(rgb).astype(np.int16)


def locate(haystack, needle):
    import cv2
    res = cv2.matchTemplate(haystack.astype(np.uint8), needle.astype(np.uint8), cv2.TM_SQDIFF)
    _, _, loc, _ = cv2.minMaxLoc(res)
    return loc
