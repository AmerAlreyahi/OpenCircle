#!/usr/bin/env python3
"""Writes tests/fixtures/previews/tiny-{a,b}.ivf: the preview player's test clips.

AV1 in IVF (as the real previews), 128x72 at 10 fps (100 ms a frame), each
frame one flat colour, so a test tells the frames apart by a pixel: a = red,
green, blue; b = white, black. Needs ffmpeg with libsvtav1.
Run from the repository root: python3 scripts/make-preview-fixture.py
"""
import subprocess
from pathlib import Path

CLIPS = {"a": ["0xdc2828", "0x28b43c", "0x2850dc"], "b": ["0xf0f0f0", "0x101010"]}
OUT = Path("tests/fixtures/previews")
OUT.mkdir(parents=True, exist_ok=True)
for name, colours in CLIPS.items():
    inputs = []
    for c in colours:
        inputs += ["-f", "lavfi", "-i", f"color=c={c}:s=128x72:r=10:d=0.1"]
    chain = "".join(f"[{i}:v]" for i in range(len(colours)))
    out = OUT / f"tiny-{name}.ivf"
    subprocess.run(["ffmpeg", "-v", "error", *inputs, "-filter_complex",
                    f"{chain}concat=n={len(colours)}:v=1,format=yuv420p",
                    "-c:v", "libsvtav1", "-crf", "8", "-preset", "8", "-g", "1",
                    "-colorspace", "bt709", "-color_range", "tv", "-y", str(out)], check=True)
    print(f"{out}: {out.stat().st_size} bytes")
