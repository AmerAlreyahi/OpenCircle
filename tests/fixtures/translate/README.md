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
