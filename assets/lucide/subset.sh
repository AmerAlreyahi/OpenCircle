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
  U+E070 `# check (update installed)` \
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
  U+E152 `# screen-share (include the screen in a question)` \
  U+E4D6 `# gallery-vertical-end (editor: scrolling capture)` \
  U+E0A5 `# corner-down-left (the AI badge's Enter)` \
  U+E179 `# star (About: Star OpenCircle)` \
  U+E14B `# rotate-ccw (a feature preview's Replay)` \
  U+E2B5 `# pin-off (the pinned AI chat: unpin)` \
  U+E151 `# scissors (Snip: the bar button)` \
  U+E158 `# share (Snip toolbar: the share sheet)` \
  U+E3B2 `# picture-in-picture-2 (Snip toolbar: live pin)` \
  U+E5A0 `# blend (live pin: opacity)` \
  U+E5EB `# mouse-pointer-ban (live pin: clicks pass through)` \
  U+E14E `# ruler (Snip toolbar: the screen ruler)` \
  U+E1C4 `# move-diagonal-2 (ruler: line)` \
  U+E1C5 `# move-horizontal (ruler: across)` \
  U+E1C6 `# move-vertical (ruler: up and down)` \
  U+E1CA `# square-dashed (ruler: box)` \
  U+E0F9 `# image (Snip toolbar: the size chip, drag the picture out)` \
  U+E0BA `# ellipsis (Snip toolbar: More)` \
  U+E23E `# list-plus (Snip toolbar: Add step N; More: Add to steps)` \
  U+E071 `# chevron-down (a list opener, closed)` \
  U+E074 `# chevron-up (a list opener, open)` \
  U+E04D `# arrow-right (the language pill: source -> target)` \
  U+E15B `# shield (update: Windows asks for permission)` \
  U+E0BD `# external-link (update: full release notes, Get it)` \
  U+E07B `# circle-alert (the AI error card)` \
  U+E548 `# book-a (the translate pill: the glossary)` \
  U+E28E `# eraser (Snip: the Eraser tool)` \
  U+E42A `# app-window (Snip Studio: its mark)` \
  U+E12D `# panel-left (Snip Studio: the library's toggle)` \
  U+E435 `# panel-right (Snip Studio: the picture panel's toggle)` \
  U+E246 `# folder-open (Snip Studio: Open)` \
  U+E3EB `# clipboard-paste (Snip Studio: Paste)` \
  U+E140 `# plus (Snip Studio: zoom in)` \
  U+E157 `# settings (Snip Studio: the preferences button)` \
  U+E0BE `# eye (the translate pill: show the original)` \
  U+E1DC `# palette (Settings round 2)` \
  U+E3FE `# circle-arrow-out-up-right (Settings round 2)` \
  U+E119 `# message-circle (Settings round 2)` \
  U+E283 `# keyboard (Settings round 2)` \
  U+E29B `# puzzle (Settings round 2)` \
  U+E0FE `# info (Settings round 2)` \
  U+E0DA `# flask-conical (Settings round 2)` \
  U+E0D6 `# flag (Settings round 2)` \
  U+E17B `# sun (Settings round 2)` \
  U+E121 `# moon (Settings round 2)` \
  U+E120 `# monitor (Settings round 2)` \
  U+E0EB `# globe (Settings round 2)` \
  U+E1B3 `# zap (Settings round 2)` \
  U+E183 `# target (Settings round 2)` \
  U+E13F `# play (Settings round 2)` \
  U+E148 `# refresh-cw (Settings round 2)` \
  U+E0AD `# cpu (Settings round 2)` \
  U+E051 `# arrow-up-right (Settings round 2)` \
  U+E131 `# pause (recording bar)` \
  U+E18D `# trash-2 (recording bar: discard)` \
  U+E11B `# mic (recording bar)` \
  U+E11C `# mic-off (recording bar: muted)` \
  U+E1A4 `# video (recording: the clip chip)` \
  U+E55E `# audio-lines (recording: the audio chip)` \
)
uvx --from fonttools pyftsubset "$tmp/lucide.ttf" --unicodes="${ICONS%,}" \
  --output-file=lucide-subset.ttf --no-hinting --layout-features='' --name-IDs='*'
