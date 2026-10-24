# Getting started

## Install and start

Install from the [Releases](https://github.com/AmerAlreyahi/OpenCircle/releases)
page (see the [README](../README.md#install)). OpenCircle runs in the system
tray; starting it again while it runs just hands the request to the running
copy.

On first run a short welcome appears. It shows your shortcut, lets you pick an
AI provider (optional) and a theme. You can change all of it later in
**Settings** (tray menu).

## Circle something

1. Press `Ctrl+Shift+Space` (or choose **Circle to search** in the tray menu).
   The screen freezes under a light dim.
2. Circle, or drag a rectangle over what you want. A mostly straight drag
   selects the rectangle; any other stroke selects the area it encloses.
   Drawing again starts over.
3. A floating bar appears under the selection. Pick an action, or press its
   key:

| Action | Key | What it does |
|---|---|---|
| Search | `L` or `Enter` | Opens the selection in Google Lens, in your browser |
| Ask AI | `A` | Chat about the selection ([AI setup](ai-setup.md)) |
| Text | `T` | Reads the text so you can copy it |
| Edit | `E` | Turns the bar into the annotation toolbar |
| Copy | `Ctrl+C` | Copies the selection as an image |
| Save | `Ctrl+S` | Saves the selection as a PNG |

`Esc` closes everything.

By default the selection snaps to a rounded box with corner brackets (drag a
corner to resize it) and is searched with Google Lens at once; **More** under
the box brings the bar back, **Select text** highlights every word in it (drag
over words for Copy, Translate or Search). See Customize below to change this.

## Read text (OCR)

The first time you use **Text**, OpenCircle asks before downloading the
text-recognition files (about 20 MB, from GitHub). Nothing else is sent. They
are checked against pinned checksums and stored per user:

- Windows: `%LOCALAPPDATA%\OpenCircle\ocr`
- Linux: `~/.local/share/OpenCircle/ocr`

After that, reading works offline. The built-in recogniser currently reads
English (Latin-script) text. On a fresh Windows install the
[Microsoft Visual C++ Redistributable](https://aka.ms/vs/17/release/vc_redist.x64.exe)
may be needed; OpenCircle tells you if it is.

## Translate, ask AI, snip

- Translating the screen: [Translate](translate.md).
- Asking AI: add a key first, see [AI setup](ai-setup.md).
- Screenshots: [Snip and annotate](snip-and-annotate.md).
- Changing or adding shortcuts: [Shortcuts](shortcuts.md).

## Customize

**Settings > Look & behaviour**:

- **Selection**: snap to a box, or keep the shape you drew.
- **After selecting**: search at once, or show the action bar and wait.
- **Search with**: Lens, Ask AI or both.
- **Active look**: how the screen looks while OpenCircle is open. Spectrum
  (a soft glow in many colours, the default), Aurora (a thin violet-to-cyan
  line), Sunset, Ocean, Neon, Mono, Calm (no glow, light dim), or Custom: your
  own colours, glow strength, dim, activation glow, corner style, bar style and
  a screen edge per mode (circle, live translate, snip: glow, thin or none,
  thickness, moving or still).

Changes apply when you press Save; nothing needs a restart. The app theme
(Light, Dark, System) is under **Appearance**.

## Settings and config file

Settings (tray menu) has General, Appearance, Look & behaviour, Shortcuts, AI
and Translate. It writes a
plain TOML file you may also edit by hand:

- Windows: `%APPDATA%\opencircle\config.toml`
- Linux: `~/.config/opencircle/config.toml`

An invalid value falls back to its default with a warning on the log output
and never crashes the app. API keys are not in this file.

## Troubleshooting

- **Nothing happens on the shortcut.** Another app may own that key; pick a
  different one in Settings. On Wayland see [Linux notes](linux-notes.md).
- **No tray icon on Linux.** Install AppIndicator support (Debian/Ubuntu:
  `libayatana-appindicator3-1`, Fedora: `libayatana-appindicator-gtk3`). The
  shortcuts and `opencircle capture` still work.
- **Logs.** OpenCircle prints warnings to its standard error. Start it from a
  terminal to see them.
