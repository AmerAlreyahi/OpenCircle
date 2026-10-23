# Snip and annotate

Take a screenshot and mark it up. The same editor is also available on any
circled selection: press `E` in the action bar.

## Take a snip

Open Snip mode from the tray menu (**Snip**), with `opencircle run snip`, or
with a shortcut you set ([Shortcuts](shortcuts.md)). Choose a mode:

| Mode | How |
|---|---|
| Region | Drag a rectangle, or circle an area |
| Window | Click a window |
| Full | The whole screen |
| Scrolling | Drag over the scrolling area; OpenCircle scrolls it and stitches the pieces into one tall image. `Esc` or Stop keeps what was gathered. |

The editor then opens on the capture. On Wayland there is no window list and no
synthetic scrolling, so only Region and Full are offered.

## Annotate

Every tool has a key while the editor is open:

| Tool | Key |
|---|---|
| Select, move, resize | `V` |
| Crop | `X` |
| Rectangle / Ellipse | `R` / `O` |
| Arrow (drag the middle to curve it) | `A` |
| Pen (rough boxes, circles, lines and arrows snap to clean ones) | `P` |
| Highlighter | `H` |
| Text | `T` |
| Callout | `C` |
| Numbered step | `N` |
| Blur | `B` |
| Spotlight (dims everything else) | `S` |
| Magnifier | `M` |

One colour button opens the colour picker, with three stroke widths and a
custom colour. `Ctrl+Z` undoes, `Ctrl+Shift+Z` redoes. Annotations stay
editable until you copy or save. `Esc` goes back.

**Blur is irreversible in the saved image.** It is not a plain mosaic: each cell is
blurred, randomised and coarsely quantised, so the hidden text cannot be
recovered from the pixels.

## Copy or save

- **Copy** (`Ctrl+C`) puts the image on the clipboard.
- **Save** (`Ctrl+S`) opens the system save dialog and writes a PNG, where you
  choose.

Nothing is written to disk until you save.
