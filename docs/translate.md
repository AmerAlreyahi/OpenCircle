# Translate

Translate replaces the text on your screen with a translation, in place: the
original glyphs are erased from the picture and the translation is drawn where
they were, with matching colour, size and alignment. It is built for games, but
works on anything you can see.

## How it works

1. OpenCircle captures the screen.
2. Text is read on your PC (OCR).
3. The text is translated in sentence-sized blocks, by your AI provider or
   offline on your PC (see [Engines](#engines)). With AI, only text goes
   out, not the picture.
4. The original text is erased and the translation is drawn over the result.
   Names and items that the game shows in a highlight colour keep that colour
   when the AI marks them.

## Languages

Pick the languages in **Settings → Translate**: **Source** (what the text on
screen is in; *Detect automatically* is the default) and **Target** (what it
is translated into; Arabic is the default). Right-to-left targets (Arabic,
Persian, Urdu, Hebrew) are laid out right-aligned.

While translating, the pill at the top shows the pair (*Detect → Arabic*);
click either side to switch for this session, with your recent picks first
and a search field for the rest. Live translate's controller has the same
picker.

Reading the text (OCR) needs a model for the source's script. English is
the default; the first time you pick a source in another script,
OpenCircle asks before downloading its model (about 8 MB; Chinese and
Japanese about 17 MB):

| Script | Source languages |
|---|---|
| English | English (also *Detect automatically*) |
| Latin | French, German, Spanish, Portuguese, Italian, Dutch, Polish, Turkish, Indonesian, Vietnamese |
| Arabic | Arabic, Persian, Urdu |
| Cyrillic | Russian, Ukrainian |
| Chinese and Japanese | Chinese (Simplified, Traditional), Japanese |

*Detect automatically* reads with the English model; when the result looks
wrong (lines it couldn't read, or not in its script), it tries the other
models you have downloaded and keeps the best. So pick a source in another
script once (its model is then on your PC) and detection finds it from then
on. Hebrew, Hindi, Thai and Korean can be targets but not sources yet.

## Engines

Choose one in **Settings → Translate → Engine**.

- **AI** (the default) uses the provider from [AI setup](ai-setup.md). It is
  best at context and at marking names in their highlight colour; with a local
  model it stays on your PC. If no AI provider with a key is set up, OpenCircle
  translates offline instead, so live translate works without a key.
- **Offline** needs no key and no account, and nothing leaves your PC. It is a
  small neural translation model (OPUS-MT, by the University of Helsinki) that
  runs on your CPU: about a third of a second for 20 game lines, fast enough
  for live mode. It is more literal than AI, can miss the game meaning of a
  word ("Quit" as in leaving, "pelts"). One model per direction; the
  first time you use a direction, OpenCircle asks before downloading it
  (about 108-116 MB from Hugging Face, plus the text-recognition files if
  they are missing, checked against pinned checksums). Licences and credits
  are in `NOTICE`.

  | From | Into |
  |---|---|
  | English | Arabic, French, Spanish, German, Russian, Chinese (Simplified) |
  | Arabic, French, Chinese, Japanese | English |

  Any other pair is not available offline: OpenCircle says so and offers AI
  (it never sends the text anywhere you didn't choose). With *Detect
  automatically*, the direction follows the script of the text it read
  (Latin counts as English).

Offline, a highlighted name inside a line is drawn in the line's colour: the
model can't say which translated words are the name. A line drawn entirely
in a highlight colour keeps it.

Explain on a word card always asks your AI provider.

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
the language pair. If a model the languages need isn't on your PC yet, the
live controller asks first (with the download size) and live translate
starts once it is downloaded.

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
- Drawing a translation in a script OpenCircle doesn't bundle a font for
  (Cyrillic, Chinese, Japanese, Thai, ...) uses your system's fonts. Windows
  has them; on Linux install a font for that script (for example Noto Sans
  CJK).
- With AI, the text it read is sent to your AI provider. Use the offline
  engine or a local model to keep everything on your PC.
