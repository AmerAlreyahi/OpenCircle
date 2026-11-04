# Feature preview clips

Each feature's page in Settings starts with a short preview of the real app,
recorded and edited by the project owner. The app itself carries only a sharp
still picture of each clip. The full clips (1080p video) are downloaded the
first time you click **Play preview** on a page, never before, and kept on
your PC, so later visits play them at once, offline. A page's clips play one
after another in a loop while the page is open; the stage grows with the
Settings window. With reduced motion only the still is shown. A page with no
still shows no preview at all (and no Hide preview button).

## Files

| Clip | Page |
|---|---|
| `circle-1`, `-2`, `-3` | Look & behaviour, in turn |
| `text` | Text reading |
| `translate-1` ... `-4` | Translate, in turn |
| `ai-chat` | Ask AI, with an AI provider set |
| `ai-browser` | Ask AI, with Browser |
| `snip-blur`, `snip-comment`, `snip-long` | Snip, in turn |

`live` and `song` are not published yet, so those pages show no
preview.

- Still: `assets/previews/stills/<clip>.webp` (1640x922 WebP, ~25-75 KB: the clip's most telling sharp frame, the result moment),
  bundled at build time (`build.rs`); a page lists the clips it wants
  (`previews::HEROES`) and one without a still is just absent.
- Clip: `<clip>.ivf` (AV1 in IVF, 1920x1080, 20 fps, BT.709, ~0.2-0.9 MB),
  a GitHub release asset, pinned by size and SHA-256 in
  `src/ui/screens/settings/previews/online.rs` and saved under
  `<data>/OpenCircle/previews`. A wrong or partial file is deleted and fetched
  again on the next click.
- `assets/previews/<clip>.webp`: the animated versions the README shows
  (circle-1, text, snip-blur, snip-long); not bundled.

Clips with third-party material are credited in Settings > About and in
NOTICE (`pages::about::CLIP_CREDITS`): add a line there with any new clip.

## Making them

The owner's edits are compositions in a local editing project (not in this
repository). They are rendered at 1080p (Remotion `--scale 1.6`, 1920x1088,
cropped 4 px top and bottom to 1920x1080) and encoded with SVT-AV1:

    ffmpeg -framerate 20 -i frames/element-%03d.png -an \
      -vf "crop=1920:1080:0:4,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p" \
      -colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv \
      -c:v libsvtav1 -preset 4 -crf 28 -g 400 -svtav1-params tune=0 <clip>.ivf

The still is the edit's poster frame, cropped the same way, as WebP quality
82. A new clip = its line in `online.rs` (name, size, SHA-256), its still,
and the file added to the release.

## How it plays

`previews::player` runs one thread per preview: it decodes the still at the
stage's width in pixels, then (clips on disk, motion allowed) reads each clip,
checks it against its pin, and decodes it with rav1d (dav1d's Rust port,
`previews::av1`), turning each frame into RGBA at the stage's width (area
averaged, so text stays sharp), at most two frames ahead of the screen.
Leaving the page or closing Settings drops the player and ends the thread.
The frame clock runs only while a preview plays.
