<p align="center">
  <img src="assets/icons/256x256.png" width="128" alt="OpenCircle logo">
</p>

<h1 align="center">OpenCircle</h1>

<p align="center">
  <b>Circle anything on your screen — search it, translate it, ask AI about it, or snip it.</b><br>
  Fast, private and lightweight. For Windows and Linux.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-7a6cf0"></a>
  <img alt="Platforms" src="https://img.shields.io/badge/platforms-Windows%20%7C%20Linux-4fc9e0">
  <img alt="Status" src="https://img.shields.io/badge/status-early%20development-ff6b4a">
</p>

> **Status:** early development. There is no release yet; the features below
> are what the first version (v0.1) is being built to do.

<p align="center">
  <img src="gallery/00-translate-before-after.png" alt="Full-screen translate: before and after" width="900"><br>
  <sub>Design concept — the original text is replaced in place. More in the <a href="gallery/README.md">gallery</a>.</sub>
</p>

## What it does

Press a shortcut. The screen freezes. Circle, drag or tap anything — text,
images, video frames, games — and choose what to do with it.

- **Translate the whole screen, in place** — every piece of text is replaced
  where it stands, in the same colours and size, as if the app or game shipped
  in your language. A live mode keeps translating while you play. Hold `Alt`
  to peek at the original; click a word for its meaning in context.
- **Ask AI** — about what you circled, with follow-up questions. Use Claude,
  OpenAI, Gemini, or a local model through Ollama with your own key or none.
- **Search with Google Lens** — opens the results in your own browser. No
  third-party image hosts in between.
- **Copy text** from anything, even images and videos, read offline.
- **Snip and edit** — region, window, full-screen and scrolling
  screenshots. Any capture opens in the editor: blur private details, add
  smart shapes, curved arrows, text, callouts, numbered steps, spotlight,
  highlighter and magnifier; then copy, save or pin it to the screen.
- **Quick actions** — give any action its own shortcut and skip the menu
  (for example `Ctrl+Shift+T` translates the screen at once).
- **Light, dark and system themes.**

## Private by design

- Nothing leaves your computer until you choose an action.
- No accounts, no telemetry, no image-hosting services.
- API keys are kept in your system's password store, never in plain files.
- Text reading (OCR) runs on your machine.

## Lightweight

A single native program written in Rust — no browser engine inside. The
target is under 40 MB of memory while it waits in the tray, and under
150 ms from shortcut to frozen screen.

## Default shortcuts

| Action | Shortcut |
|---|---|
| Circle to search | `Ctrl+Shift+Space` |
| Translate the full screen | `Ctrl+Shift+T` |
| Live translate on / off | `Ctrl+Shift+L` |
| Circle → explain with AI | `Ctrl+Shift+E` |
| Circle → copy text | `Ctrl+Shift+X` |
| Snip a region | `Print Screen` |
| Scrolling snip | `Ctrl+Print Screen` |

All shortcuts can be changed in Settings. On Wayland, the desktop asks once
to allow them; where it can't, bind `opencircle capture` in your desktop's
keyboard settings.

## Platforms

- **Windows** 10 and 11
- **Linux** — X11 and Wayland (GNOME, KDE Plasma, Hyprland, Sway). Live
  translate and scrolling capture depend on what the Wayland compositor
  allows.

Games must run in borderless or windowed mode for live translate: no app can
draw over exclusive fullscreen.

## Install

Releases will be published on the [Releases](../../releases) page:
Windows `.exe` / `.msi`, Linux `.AppImage` / `.deb` / `.rpm`, with a
`SHA256SUMS.txt` to verify downloads.

Binaries are not code-signed, so Windows SmartScreen may warn on first run
("More info" → "Run anyway"). Organisations can sign the installer with their
own certificate before distributing it.

## Build from source

Requires a recent stable [Rust](https://rustup.rs) toolchain.

```bash
cargo build --release
```

Linux build dependencies (Debian/Ubuntu):

```bash
sudo apt-get install libxkbcommon-dev libwayland-dev libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev libgtk-3-dev libxdo-dev
```

## Contributing

Issues and pull requests are welcome. Please run the checks before sending a
pull request:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## License

[MIT](LICENSE)

OpenCircle is an independent project and is not affiliated with or endorsed
by Google. "Google Lens" is a trademark of Google LLC and is used only to name
the service OpenCircle can open for you.
