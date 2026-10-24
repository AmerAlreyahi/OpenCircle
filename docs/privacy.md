# Privacy

OpenCircle has no accounts and no telemetry, and it uploads nothing on its own.
Every network action is something you start.

## What leaves your computer, and when

| When you... | What is sent | To whom |
|---|---|---|
| Press **Search** | The selected area, as an image | Google Lens, from your own browser. OpenCircle writes a small local web page that holds the image and submits it to Lens when your browser opens it. No image-hosting service is involved. |
| Press **Send** in Ask AI / Explain, with the default **Google AI Mode (browser)** | Your question and the text read from your selection (text only; the screenshot is not uploaded unless you choose Lens) | Google, from your own browser, which OpenCircle opens at `google.com/search` with the question in the address. If you decline the one-time OCR download, only your prompt is sent. |
| Ask AI / Explain, with an API provider | The selected area and your questions | The AI provider you configured (or your local server) |
| Translate with AI | The text read from the screen (text only, not the image) | The AI provider you configured |
| Use **Text** for the first time | Nothing of yours. It downloads the OCR runtime and models (about 20 MB) | GitHub (ONNX Runtime releases and the oar-ocr model release), checked against pinned checksums |
| Anything else (circle, copy, save, snip, annotate, read text) | Nothing | Nobody |

Lens and your AI provider have their own privacy policies; OpenCircle cannot
control what they do with what you send.

## Keys

AI API keys are stored only in the operating system's keyring (Windows
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
- **OCR files** (runtime and models, no personal data) stay in your user data
  folder so they are downloaded once.
- **Settings** are in `config.toml` in your user config folder; they hold no
  keys.

## Offline use

Circle, Text (after the one-time download), Copy, Save and Snip work with no
network. Search, Ask AI and AI translation need one; a local model removes the
need for the last two (the browser provider always needs the web).
