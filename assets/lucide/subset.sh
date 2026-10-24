#!/bin/sh
# Rebuilds lucide-subset.ttf: only the Lucide icons OpenCircle uses (ISC, see LICENSE).
# To add an icon: look up its code in the font's info.json (e.g. "copy" -> \e0a2),
# add it below with a comment, run this script, commit the new .ttf.
# Needs uv (or: pip install fonttools, then call pyftsubset directly).
set -eu
cd "$(dirname "$0")"
VERSION=0.469.0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sfL "https://unpkg.com/lucide-static@$VERSION/font/lucide.ttf" -o "$tmp/lucide.ttf"
ICONS=$(printf '%s,' \
  U+E0A2 `# copy` \
  U+E0B6 `# download` \
  U+E154 `# search` \
  U+E416 `# sparkles` \
  U+E197 `# type` \
  U+E103 `# languages` \
  U+E1F8 `# pencil` \
  U+E04C `# arrow-left (editor: back)` \
  U+E1C2 `# mouse-pointer-2 (select)` \
  U+E16A `# square (rectangle)` \
  U+E07A `# circle (ellipse)` \
  U+E497 `# move-up-right (arrow)` \
  U+E133 `# pen-line (pen)` \
  U+E0F7 `# highlighter` \
  U+E1D0 `# list-ordered (numbered step)` \
  U+E29D `# focus (spotlight)` \
  U+E11A `# message-square (callout)` \
  U+E1B5 `# zoom-in (magnifier)` \
  U+E0AF `# crop` \
  U+E2A0 `# undo-2` \
  U+E29F `# redo-2` \
  U+E04E `# arrow-up` \
  U+E1B1 `# x` \
  U+E258 `# pin (editor: pin to screen)` \
  U+E125 `# music (song search)` \
  U+E11F `# minus (window header: minimise)` \
  U+E521 `# slash (line)` \
  U+E544 `# signpost (arrow with label)` \
  U+E182 `# tag (label)` \
  U+E123 `# mouse-pointer-click (pointer)` \
  U+E13E `# pipette (eyedropper)` \
  U+E104 `# layout-grid (board, grid layout)` \
  U+E1F6 `# image-plus (add capture)` \
  U+E43D `# rows-2 (stacked)` \
  U+E09C `# columns-2 (side by side)` \
  U+E0F2 `# hash (capture numbers)` \
  U+E10B `# list (legend)` \
  U+E0EE `# grip-vertical (drag-out)` \
  U+E0B8 `# droplet (blur brush)` \
  U+E16B `# square-asterisk (redact)` \
  U+E2B4 `# magnet (line assist)` \
  U+E145 `# radio (live translate)` \
)
uvx --from fonttools pyftsubset "$tmp/lucide.ttf" --unicodes="${ICONS%,}" \
  --output-file=lucide-subset.ttf --no-hinting --layout-features='' --name-IDs='*'
