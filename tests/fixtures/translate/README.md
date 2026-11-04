# Translate fixtures (real frames)

Frames from OpenCircle's own 0.1.3 test recordings (Windows 10 VM, 1920x1080), cut to the
text. `lines.json` = the lines PP-OCR read on the whole frame (shifted to the cut);
`translations.json` = fixed English for them; `<name>_expected.png` = the golden render
(`src/translate/tests/evidence.rs`; `OC_BLESS=1` rewrites them).

- `subtitle.png`: Sintel, with its official Spanish subtitle, paused in a browser.
  Sintel © Blender Foundation | durian.blender.org | CC BY 3.0
  (https://creativecommons.org/licenses/by/3.0/).
- `sign.png`: a forest refuge sign authored for the recording (OpenCircle, MIT).
- `web.png`: Spanish Wikipedia, "Luna" (https://es.wikipedia.org/wiki/Luna), Wikipedia
  contributors, CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/).
- `game_expected.png`: the golden of a game settings dialog drawn by the test itself
  (`src/translate/tests/game.rs`; no game's art), OpenCircle, MIT.
- `subtitle2.png`: Sintel again (the second recording: the subtitle straight on the film).
  Sintel © Blender Foundation | durian.blender.org | CC BY 3.0.

The English in `translations.json` is the real on-device output (OPUS-MT es→en, the app's
pinned model) for every block it was run on.
- `player.png`: Sintel's source frame with its Spanish subtitle (`sintel-spanish.mp4`) inside a
  video-page layout drawn for the test (no site's art or logos; the page text is our own English).
  Sintel © Blender Foundation | durian.blender.org | CC BY 3.0. Its English is the film's own
  English subtitle line, not MT output.

## games/
Three original game screens made for OpenCircle's tests by its owner (generated art, no game's
assets), downscaled to 1280x720: `quest.jpg` (German cutscene + quest HUD), `gacha.jpg` (French
event banner), `menu.jpg` (Spanish settings menu). `<name>.win.json` = Windows OCR's read (Win10
VM, the 2560x1440 frame, frame px), `<name>.pp.json` = PP-OCR's read of the same frame
(`src/translate/tests/games.rs`). OpenCircle, MIT.
- The scored corpus: the three above plus ten more of the owner's original screens (`inventory`
  de, `shop` it, `novel` fr, `battle` es, `questlog` pt, `skills` ru, `loading` ja, `pause` de,
  `racing` fr, `cards` ar), 1280x720 each; `corpus.json` = how each element must be handled
  (`src/translate/tests/corpus.rs`). OpenCircle, MIT.
