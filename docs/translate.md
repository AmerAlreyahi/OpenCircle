# Translate

Translate replaces the text on your screen with a translation, in place: the
original glyphs are erased from the picture and the translation is drawn where
they were, with matching colour, size and alignment. It is built for games, but
works on anything you can see.

## How it works

1. OpenCircle captures the screen.
2. Text is read on your PC (OCR).
3. The text is sent to your AI provider in sentence-sized blocks. Only text
   goes out, not the picture.
4. The original text is erased and the translation is drawn over the result.
   Names and items that the game shows in a highlight colour keep that colour
   when the AI marks them.

Translation uses the AI provider from [AI setup](ai-setup.md), which is best at
context; with a local model it stays on your PC. Pick the target language in
**Settings → Translate** (Arabic is the default; right-to-left languages are
laid out right-aligned).

The OCR step reads English (Latin-script) source text today.

## One-shot

Press `Ctrl+Shift+T`. The whole screen is translated at once and shown in
place. `Esc` closes it.

- **Hold `Alt`** to peek at the original; release to see the translation again.
- **Click a translated block** to open a card with the meaning, the
  translation, and a one-line explanation in the context of the scene. Press
  `A` there to continue as an AI chat.

## Live

Press `Ctrl+Shift+L` to turn live translate on or off. A click-through,
always-on-top layer re-captures about once a second and re-translates only the
text that changed, so game input is never blocked. A small status pill shows
the language pair.

- **Windows:** the layer is excluded from its own captures.
- **Linux/X11:** supported; the layer is click-through, and OpenCircle guards
  against reading its own output.
- **Wayland:** compositors generally do not allow such an overlay, so each press
  of the shortcut translates once instead.

## The game must be borderless or windowed

No application can draw over a game in exclusive fullscreen. Switch the game to
borderless ("fullscreen windowed") or windowed mode for live translate and
for the translation to appear over it.

## Limits

- Very large erased areas over busy art can show smudges; translation quality
  depends on the OCR and on the model you use.
- The text it read is sent to your AI provider. Use a local model to keep
  everything on your PC.
