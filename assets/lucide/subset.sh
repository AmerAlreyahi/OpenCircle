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
  U+E503 `# grid-2x2 (blur)` \
  U+E29D `# focus (spotlight)` \
  U+E11A `# message-square (callout)` \
  U+E1B5 `# zoom-in (magnifier)` \
  U+E0AF `# crop` \
  U+E2A0 `# undo-2` \
  U+E29F `# redo-2` \
  U+E04E `# arrow-up` \
  U+E1B1 `# x` \
  U+E258 `# pin (editor: pin to screen)` \
)
uvx --from fonttools pyftsubset "$tmp/lucide.ttf" --unicodes="${ICONS%,}" \
  --output-file=lucide-subset.ttf --no-hinting --layout-features='' --name-IDs='*'
