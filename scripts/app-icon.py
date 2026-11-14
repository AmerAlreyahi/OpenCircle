#!/usr/bin/env python3
"""Bakes the app icon (Sapphire) from assets/logo/logo.svg, the design's vector logo.

One source of truth: the SVG's 16-step ramp is swapped for the Sapphire ramp below (measured
from the design's official logo-sapphire.png), and each size is rendered natively by headless
Chromium (crisp at 16..32, no downscaling). Writes assets/icons/{32x32,64x64,128x128,256x256,
512x512}.png, opencircle.png (1024), opencircle.ico (16..256 frames) and assets/setup/logo.png.
Needs Pillow and a Chromium (CHROME=path; default: Playwright's cache).
Run from the repo root: python3 scripts/app-icon.py
"""
import glob, io, os, re, struct, subprocess, tempfile
from PIL import Image

SAPPHIRE = ["0a4d78", "115b89", "196fa2", "2285c1", "2b96d9", "32a5f0", "3aacfd", "47acfe",
            "55affe", "61b4fe", "71baff", "82c2ff", "92caff", "a0d2fe", "add8ff", "b8dcff"]
PNG = {"32x32": 32, "64x64": 64, "128x128": 128, "256x256": 256, "512x512": 512, "opencircle": 1024}
ICO = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SETUP = 138

svg = open("assets/logo/logo.svg").read()
for i, h in enumerate(SAPPHIRE):
    svg, n = re.subn(rf"(--logo-t{i}:\s*)#[0-9a-f]{{6}}", rf"\g<1>#{h}", svg)
    assert n == 1, i
chrome = os.environ.get("CHROME") or sorted(glob.glob(
    os.path.expanduser("~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome")))[-1]
tmp = tempfile.mkdtemp()
open(f"{tmp}/l.svg", "w").write(svg)


def render(n):
    """The logo n x n, transparent, drawn by Chromium at that size."""
    html, out = f"{tmp}/{n}.html", f"{tmp}/{n}.png"
    open(html, "w").write(f"<html><body style='margin:0;background:transparent'>"
                          f"<img src='l.svg' width={n} height={n} style='display:block'></body></html>")
    subprocess.run([chrome, "--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars",
                    "--allow-file-access-from-files", "--default-background-color=00000000",
                    "--window-size=1100,1100", f"--screenshot={out}", "file://" + html],
                   check=True, capture_output=True)
    return Image.open(out).convert("RGBA").crop((0, 0, n, n))


def png_bytes(im):
    b = io.BytesIO()
    im.save(b, "PNG", optimize=True)
    return b.getvalue()


for name, n in PNG.items():
    render(n).save(f"assets/icons/{name}.png", optimize=True)
render(SETUP).save("assets/setup/logo.png", optimize=True)

frames = [(n, png_bytes(render(n))) for n in ICO]
head = struct.pack("<HHH", 0, 1, len(frames))
off = 6 + 16 * len(frames)
dirs, data = b"", b""
for n, b in frames:
    dirs += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(b), off + len(data))
    data += b
open("assets/icons/opencircle.ico", "wb").write(head + dirs + data)
print("icons written")
