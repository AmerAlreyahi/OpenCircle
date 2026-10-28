# Feature preview clips

Each feature's page in Settings starts with a short preview: a real recording
of the feature, played once when the page opens (Replay plays it again), then
resting on its last frame. With reduced motion it shows its first frame only.
Until a clip exists, the stage shows a quiet "Preview" placeholder.

Clips are bundled into the app when it is built. They are never downloaded:
the app stays private and works offline.

## Files

One animated WebP per feature, in `assets/previews/`:

| File | Page |
|---|---|
| `circle.webp` | Look & behaviour (Circle to Search) |
| `text.webp` | Text reading |
| `translate.webp` | Translate |
| `ai.webp` | AI |
| `snip.webp` | Snip (Labs) |
| `live.webp` | Translate, Live translate (Labs) |
| `song.webp` | Song search (Labs) |

A missing file is the placeholder; nothing else changes. After adding or
replacing a clip, rebuild (the build picks up the folder's changes).

## Clip spec

- Animated WebP, played once (no loop needed: the player stops at the end).
- 1200x680 px (the stage draws it 200 px tall, about 353x200, so it stays
  sharp on high-DPI screens).
- 15-20 fps, about 5 seconds.
- About 1 MB each (lossy, quality around 70-80).
- Recorded from the real app on the test VM, on a clean screen (no desktop
  icons, taskbar or watermark), no private content.

## Exporting

From a screen recording (`in.mp4`), cut to the 5 seconds you want:

    ffmpeg -ss 00:00:02 -t 5 -i in.mp4 \
      -vf "fps=18,scale=1200:680:flags=lanczos" \
      -c:v libwebp -lossless 0 -q:v 75 -compression_level 6 \
      -loop 1 -an assets/previews/circle.webp

Check the size (`ls -lh assets/previews`): over about 1 MB, lower `-q:v` or
`fps`. `-loop 1` plays it once in browsers too; OpenCircle plays it once
either way.

## How it plays

The player (`src/ui/screens/settings/previews/player.rs`) decodes one frame at
a time while the clip plays, never the whole clip at once; at the end it drops
the decoder and keeps only the last frame. Leaving the page or closing
Settings frees it. The frame clock runs only while a clip plays.
