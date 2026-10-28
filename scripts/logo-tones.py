#!/usr/bin/env python3
"""Bakes assets/logo/tones.png from assets/logo/logo.svg (the design's vector logo).

The SVG colours itself from a 16-step ramp (--logo-t0 darkest ... t15 brightest).
We render it once (headless Chromium, transparent) in its own purple ramp and store,
per pixel, WHERE on that ramp the colour sits (0..255 = tone 0..15 in 1/17 steps, so
gradients keep their smoothness) plus alpha. Output: 512x512 grey+alpha PNG.
src/ui/kit/logo.rs rebuilds the 16 colours from the theme accent at run time.
Needs Pillow and a Chromium (CHROME=path; default: Playwright's cache).
Run from the repo root: python3 scripts/logo-tones.py
"""
import glob, os, re, subprocess, tempfile
from PIL import Image

N = 512
svg = open("assets/logo/logo.svg").read()
ramp = [tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))
        for h in re.findall(r"--logo-t\d+:\s*#([0-9a-f]{6})", svg)[:16]]
assert len(ramp) == 16

chrome = os.environ.get("CHROME") or sorted(glob.glob(
    os.path.expanduser("~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome")))[-1]
tmp = tempfile.mkdtemp()
html = os.path.join(tmp, "l.html")
png = os.path.join(tmp, "l.png")
open(html, "w").write(
    f"<html><body style='margin:0;background:transparent'>"
    f"<img src='file://{os.path.abspath('assets/logo/logo.svg')}' width={N} height={N}></body></html>")
subprocess.run([chrome, "--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars",
                "--default-background-color=00000000", f"--window-size={N},{N}",
                f"--screenshot={png}", "file://" + html], check=True, capture_output=True)
src = Image.open(png).convert("RGBA")
assert src.size == (N, N), src.size


def position(c):
    """Fractional ramp position (0..15) of colour `c`: the nearest point on the polyline."""
    best = (1e18, 0.0)
    for i in range(15):
        a, b = ramp[i], ramp[i + 1]
        d = [b[k] - a[k] for k in range(3)]
        L = sum(x * x for x in d)
        t = max(0.0, min(1.0, sum((c[k] - a[k]) * d[k] for k in range(3)) / L))
        e = sum((a[k] + t * d[k] - c[k]) ** 2 for k in range(3))
        if e < best[0]:
            best = (e, i + t)
    return best[1]


cache = {}
out = Image.new("LA", (N, N))
px = out.load()
s = src.load()
for y in range(N):
    for x in range(N):
        r, g, b, a = s[x, y]
        if a == 0:
            px[x, y] = (0, 0)
            continue
        # unpremultiply is already done by PNG; quantise to share the search
        key = (r >> 1, g >> 1, b >> 1)
        if key not in cache:
            cache[key] = round(position((r, g, b)) * 17)
        px[x, y] = (cache[key], a)
out.save("assets/logo/tones.png", optimize=True)
print("tones.png", os.path.getsize("assets/logo/tones.png"), "bytes")
