#!/usr/bin/env python3
"""Rebuilds the setup's and the MSI wizard's artwork (Sapphire look) from our logo.

Inputs: assets/setup/logo.png (the design's vector logo in the Sapphire tones,
rendered at 3x of its 46 px). Outputs:
  assets/setup/back.png         the emblem's halo and the tile's shadow (blurs
                                our CPU renderer can't draw), window top-aligned
  assets/setup/glow-accent.png  the glow under the blue Install button
  assets/setup/glow-success.png the glow under the green Launch button
  assets/installer/banner.bmp   WiX banner, 493x58 (inner wizard pages)
  assets/installer/dialog.bmp   WiX welcome / finish image, 493x312
Colours and sizes mirror ui/theme.rs `sapphire` and ui/setup.rs; change them together.
Needs Pillow. Run from the repo root: python3 scripts/installer-art.py
"""
from PIL import Image, ImageDraw, ImageFilter

BG = (12, 23, 46)          # sapphire::BG (#0c172e)
ELEVATED = (28, 54, 100)   # sapphire::ELEVATED (#1c3664)
ACCENT = (56, 189, 248)    # sapphire::ACCENT (#38bdf8)
SUCCESS = (52, 211, 153)   # sapphire::SUCCESS (emerald-400)
LIGHT = (240, 247, 255)    # sapphire::TEXT (#f0f7ff), the wizard's light side

logo = Image.open("assets/setup/logo.png").convert("RGBA")


def blurred(size, shape, colour, alpha, sigma):
    """`shape(draw)` filled at `alpha` in `colour`, Gaussian-blurred (CSS blur = sigma)."""
    mask = Image.new("L", size, 0)
    shape(ImageDraw.Draw(mask))
    mask = mask.filter(ImageFilter.GaussianBlur(sigma))
    mask = mask.point(lambda v: round(v * alpha))
    out = Image.new("RGBA", size, colour + (0,))
    out.putalpha(mask)
    return out


# The emblem (setup.rs): a 68 px tile whose centre is 82 px below the top of the
# window's inside (its 1 px edge), a 96 px halo behind it (blur-2xl = 40 px), the
# tile's shadow 0 14px 34px rgba(0,0,0,.45). 256 px wide, cut at that top.
W, CY = 256, 82
H = CY + 128
halo = blurred((W, H), lambda d: d.ellipse((W / 2 - 48, CY - 48, W / 2 + 48, CY + 48), 255),
               ACCENT, 0.20, 40)
shadow = blurred((W, H), lambda d: d.rounded_rectangle(
    (W / 2 - 34, CY - 34 + 14, W / 2 + 34, CY + 34 + 14), 20, 255), (0, 0, 0), 0.45, 17)
Image.alpha_composite(halo, shadow).save("assets/setup/back.png", optimize=True)

# The buttons' glow: 0 10px 28px at .32 (accent) / .28 (green) under a 370x44,
# radius 12 button; 418 px wide (the window's inside), the button 42 px from its top.
for name, colour, alpha in (("accent", ACCENT, 0.32), ("success", SUCCESS, 0.28)):
    glow = blurred((418, 128), lambda d: d.rounded_rectangle((24, 42 + 10, 394, 86 + 10), 12, 255),
                   colour, alpha, 14)
    glow.save(f"assets/setup/glow-{name}.png", optimize=True)


def gradient(size, top, bottom):
    out = Image.new("RGB", size)
    for y in range(size[1]):
        t = y / max(1, size[1] - 1)
        c = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom))
        ImageDraw.Draw(out).line((0, y, size[0], y), c)
    return out


def emblem(px):
    """The logo on its halo, `px` wide, for the wizard (over the dark panel)."""
    canvas = Image.new("RGBA", (px * 2, px * 2), (0, 0, 0, 0))
    canvas = Image.alpha_composite(canvas, blurred(canvas.size, lambda d: d.ellipse(
        (px * 0.45, px * 0.45, px * 1.55, px * 1.55), 255), ACCENT, 0.30, px * 0.35))
    mark = logo.resize((px, px), Image.LANCZOS)
    canvas.alpha_composite(mark, (px // 2, px // 2))
    return canvas


# Welcome / finish: the dark Sapphire panel on the left (WixUI's texts start at
# 180 px), the light page on the right, a hairline of the accent between them.
dialog = Image.new("RGB", (493, 312), LIGHT)
panel = gradient((164, 312), BG, ELEVATED)
art = emblem(84)
panel.paste(art, ((164 - art.width) // 2, 96 - art.height // 2), art)
dialog.paste(panel, (0, 0))
ImageDraw.Draw(dialog).line((164, 0, 164, 311), ACCENT)
dialog.save("assets/installer/dialog.bmp")

# Inner pages: the light strip WixUI writes the page title on, the logo on a
# small dark tile at the right.
banner = Image.new("RGB", (493, 58), LIGHT)
tile = Image.new("RGBA", (46, 46), (0, 0, 0, 0))
ImageDraw.Draw(tile).rounded_rectangle((0, 0, 45, 45), 12, BG + (255,))
tile.alpha_composite(logo.resize((34, 34), Image.LANCZOS), (6, 6))
banner.paste(tile, (493 - 46 - 8, 6), tile)
banner.save("assets/installer/banner.bmp")
