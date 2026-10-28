#!/usr/bin/env python3
"""Writes tests/fixtures/previews/tiny.webp: the preview player's test clip.

5 frames of 300 ms (1.5 s), 48x27, each frame one flat colour, lossless (so
the test can tell the frames apart by a pixel). Needs Pillow with WebP.
Run from the repository root: python3 scripts/make-preview-fixture.py
"""
from pathlib import Path

from PIL import Image

COLOURS = [(220, 40, 40), (40, 180, 60), (40, 80, 220), (230, 200, 30), (150, 50, 200)]
OUT = Path("tests/fixtures/previews/tiny.webp")

frames = [Image.new("RGB", (48, 27), c) for c in COLOURS]
OUT.parent.mkdir(parents=True, exist_ok=True)
frames[0].save(
    OUT,
    save_all=True,
    append_images=frames[1:],
    duration=300,
    loop=1,
    lossless=True,
)
print(f"{OUT}: {OUT.stat().st_size} bytes")
