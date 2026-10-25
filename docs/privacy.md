# Privacy

OpenCircle has no accounts and no telemetry, and it uploads nothing on its own.
Every network action is something you start.

## What leaves your computer, and when

| When you... | What is sent | To whom |
|---|---|---|
| Search (runs by itself after a selection, or with `Enter` in the question field) | The selected area, as an image | Google Lens, from your own browser. OpenCircle writes a small local web page that holds the image and submits it to Lens when your browser opens it. No image-hosting service is involved. |
| Press **Send** in Ask AI / Explain, with the default **Google AI Mode (no key)** | Your question and the text read from your selection (text only; the screenshot is not uploaded unless you choose Lens) | Google, from OpenCircle's results card, which loads AI Mode with the question in the address. The browser is used only if you press **Open in browser**. If you decline the one-time text reader download, only your prompt is sent. |
| Ask AI / Explain, with an API provider | The selected area and your questions | The AI provider you configured (or your local server) |
| Translate with AI | The text read from the screen (text only, not the image) | The AI provider you configured |
| Translate with the **Offline** engine | Nothing. The model runs on your PC | Nobody |
| Select text for the first time, after you agree | Nothing of yours. It downloads the text reader (about 30 MB) | GitHub (ONNX Runtime releases and the oar-ocr model release), checked against pinned checksums |
| Translate offline for the first time, after you agree | Nothing of yours. It downloads the translation model (about 120 MB, plus the OCR files if missing) | Hugging Face (a pinned commit of the OPUS-MT model) and GitHub, checked against pinned checksums |
| Press the **music button** (song search, only after you add your own key) | A ~10-second clip of the audio you choose (what this PC plays, or the microphone), plus your key. Then the song's cover image is downloaded | AudD (`api.audd.io`); the cover from Apple's image server (`mzstatic.com`) |
| Update check (only if you turned it on; off until you say yes on first run) | Only the request for the latest release: no account, no ID, no screen data | GitHub (the release files and `SHA256SUMS.txt` with its signature) |
| Anything else (circle, copy, save, snip, annotate, read text) | Nothing | Nobody |

Lens and your AI provider have their own privacy policies; OpenCircle cannot
control what they do with what you send.

## Keys

AI API keys and the song-search key are stored only in the operating system's keyring (Windows
Credential Manager; Secret Service such as GNOME Keyring or KWallet on Linux).
They are not written to the config file, logs or any export, and they are sent
only to the server you configured. A remote server must use `https://`; plain
`http://` is accepted only for `localhost`.

## What stays on disk

- **Captures** are kept in memory. A screenshot is written to disk only when
  you press Save and choose a place.
- **Lens page.** The temporary web page that carries the image to Lens is
  deleted as soon as the browser has had time to read it, when the overlay
  closes. A page left behind by a crash is removed the next time OpenCircle
  starts.
- **Wayland.** The screenshot file the desktop portal hands over is deleted
  right after it is read.
- **OCR and offline translation files** (runtime and models, no personal
  data) stay in your user data folder so they are downloaded once.
- **Song search** listens only while the "Listening…" pill shows; the clip
  stays in memory and is dropped once AudD has answered. Song history is off
  by default; when on, titles and artists (no audio) are kept in
  `song_history.json` in your local data folder.
- **Settings** are in `config.toml` in your user config folder; they hold no
  keys.

## Offline use

Circle, select text and offline translation (after their one-time downloads),
Copy, Save and Snip work with no network. Search, Ask AI and AI translation need
one; a local model removes the need for the last two (the browser provider
always needs the web).
