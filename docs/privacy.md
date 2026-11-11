# Privacy

OpenCircle has no accounts and no telemetry, and it uploads nothing on its own.
Every network action is something you start.

## What leaves your computer, and when

| When you... | What is sent | To whom |
|---|---|---|
| Press **Search in browser** | The selected area, as an image; for selected text, only the text | Google Lens (or Google Search for text), in your default browser. The image goes straight from your browser to Lens (the browser you set as default); no image-hosting service is involved. A selection of one plain colour is never sent. |
| Ask AI / Explain with **Browser** (Settings > Ask AI; the default, no key): press `Enter` in the bar's AI field | **Include screen on** (the default) and something selected: the circled picture (the crop, as at **Search in browser**) and your question. **Include screen off**, or nothing selected: only your question. No text read from your screen is added. | Google, in your default browser. With the picture: OpenCircle hands your browser a one-time local page and your browser posts the picture and the question to Google Lens. Without it: Google AI Mode opens with the question in the address. Only when you press `Enter`; nothing is sent by OpenCircle itself. |
| Ask AI / Explain, with an **AI provider** (Settings > Ask AI) | Your question; the selected area as an image only while **Include screen** is on (turn it off and no image is sent). A chat sends the last 20 messages in full plus one short summary of older ones, which your own provider writes. The whole chat is never sent | The AI provider you configured (or your local server) |
| Click **Load models from** (your provider) in Settings > Ask AI > Model | Your key, to ask for the provider's list of models; nothing of your screen or chats | That provider only, only on that click |
| Translate with AI (only if you pick **AI** as the engine in Settings > Translate) | The text read from the screen (text only, not the image) | The AI provider you configured |
| Translate with the **Offline** engine (the default) | Nothing. The model runs on your PC | Nobody |
| Select text for the first time, after you agree (Linux; on Windows only when Windows has no text recognizer for your languages, or for Arabic; Settings > Text reading > Engine chooses) | Nothing of yours. It downloads the text reader (about 30 MB). On Windows, text is otherwise read by Windows' own built-in OCR, on your PC | GitHub (ONNX Runtime releases and the oar-ocr model release), checked against pinned checksums |
| Translate offline for the first time, after you agree | Nothing of yours. It downloads the translation model (about 120 MB, plus the OCR files if missing) | Hugging Face (a pinned commit of the OPUS-MT encoders and tokenizers) and GitHub (the lighter decoders and the five direct Arabic models, from `github.com/AmerAlreyahi/OpenCircle-models`, and the OCR files), checked against pinned checksums |
| Translate offline with a pair downloaded before v0.1.4, or click **Update now** in Settings > Translate > Translation models | Nothing of yours. It downloads that pair's lighter decoder (about 57-60 MB per pair; Update now: every pair that still has the older one), in the background after the translation is shown; the older file is then replaced | GitHub (`github.com/AmerAlreyahi/OpenCircle-models`, the models-v2 release: the OptimaMT decoders), checked against pinned checksums |
| Click **Play preview** on a Settings page | Nothing of yours. It downloads that page's preview clips (about 0.2-1 MB each), once | GitHub (`github.com/AmerAlreyahi/OpenCircle-models` releases), checked against pinned checksums |
| Press the **music button** (song search, only after you add your own key) | A ~10-second clip of the audio you choose (what this PC plays, or the microphone), plus your key. Then the song's cover image is downloaded | AudD (`api.audd.io`); the cover from Apple's image server (`mzstatic.com`) |
| Update check (asked on first run, where the switch starts on; you can turn it off there or later in Settings) | Only the request for the latest release: no account, no ID, no screen data | GitHub (the release files and `SHA256SUMS.txt` with its signature) |
| Press **Share** in Snip | The picture, to the app you pick in Windows' own share sheet | The app you choose; OpenCircle sends nothing itself |
| Anything else (circle, copy, save, snip, annotate, long capture, steps, pin, colour picker, ruler, read text) | Nothing | Nobody |

Lens and your AI provider have their own privacy policies; OpenCircle cannot
control what they do with what you send.

## Keys

AI API keys (one per provider) and the song-search key are stored only in the operating system's keyring (Windows
Credential Manager; Secret Service such as GNOME Keyring or KWallet on Linux).
They are not written to the config file, logs or any export, a saved key is shown only as dots, and a key is sent
only to the server of the provider it belongs to. A remote server must use `https://`; plain
`http://` is accepted only for `localhost`.

## What stays on disk

- **Log and crash files.** OpenCircle writes what it prints (start-up, errors,
  timings, never your text) to `logs/opencircle.log` in your local data folder
  (`%LOCALAPPDATA%\OpenCircle\logs` on Windows), up to 3 files of 1 MB. If it
  crashes, it writes `logs/crash-<time>.txt` (version, OS, the error, the last
  200 log lines; the newest 5 are kept). Keys, tokens and your home folder
  (shown as `~`) are removed from every line before it is written. Your
  captures, clipboard, recognised text, translations and prompts are not
  logged. **Nothing is sent**: the files stay on your PC until you delete them.
- **Copy diagnostics / Report a problem** (Settings > About). *Copy
  diagnostics* puts a report on your clipboard (version, Windows build,
  screens, a settings summary without keys or folders, which experiments are
  on, and the last 50 log lines) for you to paste where you choose. *Report a
  problem* opens a pre-filled GitHub issue in your browser with only the
  version and Windows build; nothing is submitted until you press the button
  on GitHub.

- **AI chat history.** Chats are saved on this PC in `opencircle.db` (SQLite) in
  your local data folder. Text only: screenshots are never written (a chat shows
  "Screenshot not kept"), though a crop's size may be kept as text. The newest
  100 chats are kept. **Settings > Ask AI > Keep chat history** is on by default;
  turning it off deletes the saved chats. Delete one chat, or **Clear all**, from
  the chat's History drawer. Nothing in it is sent anywhere except as the
  context described above.
- **Captures** are kept in memory. A screenshot is written to disk only when
  you press Save and choose a place. **Steps** and a long capture's pieces are
  memory only too. Copying puts the picture on the clipboard, where other apps
  (and Windows' clipboard history) can see it. **Settings > History > Recent
  captures** (off by default) keeps the last 20 on this PC; turning it off
  deletes them.
- **Lens page.** The web page that carries the image to Lens (and, for Ask AI
  in the browser, your question) is never written to disk: OpenCircle serves it
  from memory to your browser once, on this PC only (127.0.0.1, a random port
  and a secret address), only to your own user account's connections, and stops
  serving it after that one read, or after 2 minutes if the browser never asks.
- **Drag-out and Share** hand the picture to the app you pick as a temporary
  file, deleted about 30 seconds after the drop (at once when you quit); one
  left behind by a crash is removed the next time OpenCircle starts.
- **Wayland.** The screenshot file the desktop portal hands over is deleted
  right after it is read. Where OpenCircle captures through the ScreenCast
  portal (GNOME before 46, and long capture), it keeps only the desktop's
  permission token (no image) in `screencast-token` in your local data folder, so
  it doesn't ask again; the stream runs only during a capture.
- **OCR and offline translation files, and preview clips** (runtime, models
  and videos, no personal data) stay in your user data folder so they are downloaded once.
- **Song search** listens only while the "Listening…" pill shows; the clip
  stays in memory and is dropped once AudD has answered. Song history is off
  by default; when on, titles and artists (no audio) are kept in
  `song_history.json` in your local data folder.
- **Settings** are in `config.toml` in your user config folder; they hold no
  keys.
- **Setup and uninstall (Windows).** The setup installs offline and sends
  nothing; it unpacks its installer to a temporary folder that it deletes when
  it closes. Uninstalling asks whether your settings, downloaded models and
  saved keys go too; a silent uninstall keeps them.

## Offline use

Circle, select text and offline translation (after their one-time downloads),
Copy, Save and Snip (long capture, steps, colour picker, ruler) work with no network. Search, Ask AI and AI translation need
one; a local model removes the need for the last two (the browser provider
always needs the web).
