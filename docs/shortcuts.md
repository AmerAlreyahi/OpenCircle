# Shortcuts

## Defaults

The Song search shortcut is hidden (not registered) until you turn Song search on in
**Settings > Labs**. Live translate is a Labs feature too (unfinished, planned
for a later release): its shortcut, tray item and bar button appear only when it is on.

| Action | Default | What it does |
|---|---|---|
| Circle to search | `Ctrl+Shift+Space` | Freeze the screen, then select and act |
| Translate full screen | `Ctrl+Shift+T` | Translate the whole screen at once |
| Live translate (Labs) | `Ctrl+Shift+L` | Toggle continuous translation |
| Explain with AI | `Ctrl+Shift+E` | Circle something; the AI explains it |
| Copy text | `Ctrl+Shift+X` | Circle something; its text is copied |
| Search in browser | unset | Circle something; it opens in Google Lens in your browser |
| Snip | `Alt+A` | Hover a window and click, or drag an area; `Enter` = full screen |
| Colour picker | unset | Pick a colour from anywhere on screen |
| Screen ruler | unset | Measure distances on screen |
| Live pin (Labs) | unset | Pin a window's area, updating live |

Snip is one row in **Settings > Shortcuts**; there are no separate region,
window and full-screen rows (older entries in a config still work: see below).
Older configs that had Snip on `Print Screen` move to `Alt+A` once, unless a key
of your own stays.

Snip also opens from the tray menu (**Snip**), the bar's Snip button, and with
`opencircle run snip`. The colour picker and ruler have no default key, because
no combination is free on every PC; give them one in **Settings > Shortcuts**.
While Snip is open: `Enter` full screen (`Shift+Enter` all screens), `Esc` clears
the area and then closes, arrows nudge the area. After a pick: `Ctrl+C` copies,
`Ctrl+S` saves, `Ctrl+Z` / `Ctrl+Shift+Z` undo / redo, `Ctrl+V` pastes the clipboard's text
(into the text being typed, else as a new text), `Ctrl+Enter` adds to steps, `Ctrl+P` pins. The tool keys are in
[Snip and annotate](snip-and-annotate.md).

**Using Print Screen for Snip.** `Alt+A` is the default because Windows uses
`Print Screen` for its own screen capture. To use `Print Screen` anyway, give
Snip that key in **Settings > Shortcuts**, then free it in Windows Settings >
Accessibility > Keyboard: turn off **Use the Print screen key to open screen
capture**, and restart OpenCircle. Settings > Snip shows a note with a button
for this when Snip is on `Print Screen` and Windows holds the key. If `Alt+A`
is already taken by another app (some screenshot tools use it), the shortcut may
not register: give Snip another key; the rest keep working.

On Linux, a desktop's own screenshot tool may hold a key you choose: unbind it
in your desktop's keyboard shortcut settings. On GNOME (Wayland) the shell keeps
`Print Screen` for itself even when OpenCircle's shortcut shows as bound; to use
it for Snip, free it, then restart OpenCircle:

```bash
gsettings set org.gnome.shell.keybindings show-screenshot-ui "[]"   # Print Screen
# put GNOME's back: gsettings reset org.gnome.shell.keybindings show-screenshot-ui
```

OpenCircle prints this when it starts under GNOME.

**Print Screen for Snip through your desktop.** Where the desktop, not
OpenCircle, owns the keys (Wayland without the shortcut portal, or when you
prefer it), bind `Print Screen` to the command `opencircle run snip`:

- **GNOME:** free the key first (the `gsettings` line above: GNOME's own
  screenshot tool holds `Print Screen`), then Settings → Keyboard → View and
  Customize Shortcuts → Custom Shortcuts → **+**. Name "OpenCircle Snip",
  command `opencircle run snip`, shortcut `Print Screen`.
- **KDE Plasma:** System Settings → Keyboard → Shortcuts → Spectacle: clear its
  `Print` key. Then **Add New → Command or Script**, command
  `opencircle run snip`, and assign `Print`.

Use the full path if `opencircle` is not on your `PATH` (an AppImage's path).
Snip then opens as it does from `Alt+A`.

While the overlay is open: `Esc` closes, `Enter` runs the default action
(Search unless you changed it in **Settings → General**), and the action keys
are `L` Search in browser, `A` Ask AI, `T` Copy text, `E` Snip, `R` Translate.

**Selection box.** After you draw, the box can be moved (drag inside it, not over
text; `Alt`+drag moves it over text), resized by its edges and corners, and
nudged with the arrow keys (1 px, `Shift` 10 px; `Ctrl`+arrows resize from the
bottom-right). It stays on the screen.

**Typing always wins.** While a text box has the focus (the bar's AI field, a
card's question box or text), every key you press is typed into it: the letter
keys above do nothing then. They work again as soon as no text box has the
focus: press `Esc` once to leave the box (the overlay stays), and `Esc` again
closes it.

**Keyboard path.** `Tab` / `Shift+Tab` move the focus through the bar in the
order you see it: the logo, the AI field, the `AI ↵` badge, Search in browser,
Copy text, Translate, Live translate (Labs), then the other round buttons (a dimmed
button is skipped). `Enter` or `Space` presses the focused button; a click puts
the focus back on the screen.

`Ctrl+C` copies the selection (its picture, or its text when you selected text; in a
text box with text selected it copies that text instead) and
`Ctrl+S` (also from a text box) saves the picture in your save folder (Settings → General; the default is
Pictures/OpenCircle) as `OpenCircle-<date>-<time>.png`. There are no Copy or Save
buttons: a pill at the bottom right says "Copied" or "Saved to <folder>" for about two
seconds, or what went wrong (no green dot) if the clipboard was busy or the folder
could not be written.
In Snip, `Ctrl+P` pins the image to the screen (`P` alone is the Pen).

## Change a shortcut

Open **Settings → Shortcuts**, click a field, press the new keys. `Esc`
cancels; `Delete` clears a quick action. A shortcut needs at least one
modifier (`Ctrl`, `Alt`, `Shift`, `Super`); only function keys, `PrintScreen`,
`Pause` and `ScrollLock` may stand alone. One key combination does one job: if
two entries use the same keys, the second is left unbound with a warning.

## Quick actions

A quick action is a shortcut, a target and an action, stored in the config
file:

```toml
[[quick_actions]]
action = "copy_text"        # translate | ai_explain | copy_text | lens | snip | colour_picker | ruler | live_pin | song_search | ai | text | edit | copy | save
shortcut = "Ctrl+Shift+X"   # "" = not bound
target = "circle"           # full_screen | circle | last_region | window
live = false                # translate only: keep translating
```

Targets: `full_screen` acts on the whole screen, `circle` lets you draw the
area first, `last_region` repeats the previous area (handy for a game's
subtitle bar). Snip is one flow and takes no target:

```toml
[[quick_actions]]
action = "snip"
shortcut = "Alt+A"
target = "circle"
```

Older `Alt+PrintScreen` (window) and `Ctrl+PrintScreen` (full screen) entries
in a config keep working but are not shown in Settings.

Edit the file while OpenCircle is closed, or change shortcuts in Settings.

## Command line

The same actions can be triggered from a terminal, a launcher or your
desktop's own shortcut settings. They talk to the running copy (or start it):

```bash
opencircle capture
opencircle run snip
opencircle run copy_text --target circle
opencircle run lens --target circle   # "lens" is the command name for Search in browser
opencircle run song_search          # needs your key: see song-search.md
```

Targets are `full_screen`, `circle`, `last_region`, `window`; without
`--target` the first entry for that action in the config is used.

## Wayland: the desktop owns the keys

On Wayland the desktop, not OpenCircle, decides which key runs each shortcut.
The first start shows the desktop's own "Add Keyboard Shortcuts" dialog with
OpenCircle's keys; after that the desktop keeps what you confirmed. If you
change a key in OpenCircle's Settings, the desktop may keep its stored one
(GNOME does). Settings then shows, under that shortcut, the key that actually
works and where to change it:

- **GNOME:** Settings → Apps → OpenCircle → Global Shortcuts.
- **KDE Plasma:** System Settings → Shortcuts → OpenCircle (Plasma 6:
  System Settings → Keyboard → Shortcuts).

## Wayland without the shortcut portal

Wayland does not let apps grab keys. OpenCircle asks the desktop through the
GlobalShortcuts portal (GNOME 48+, KDE Plasma 6) and the desktop asks you once.
That path has not been verified on every desktop.

On GNOME without the portal (before GNOME 48, for example Ubuntu 22.04),
OpenCircle does this for you: it adds one entry per shortcut to GNOME's own
custom shortcuts ("OpenCircle: ..."), each running the command below, and keeps
them in step with its Settings. It never touches your own entries; a key one of
them already uses, or one GNOME keeps for itself, is refused in Settings with
the reason.

On KDE Plasma without the portal (Plasma 5, on Wayland and X11), OpenCircle
registers its keys with KDE's own shortcut service: they show in System Settings →
Shortcuts → OpenCircle, where you can change them (see
[Linux notes](linux-notes.md#kde-plasma)).

Elsewhere, if the portal is missing or you decline, Settings names the command
for each shortcut: bind it yourself. Use the full path to the binary if it is
not on your `PATH` (for an AppImage, the path of the file).

**GNOME (by hand):** Settings → Keyboard → View and Customize Shortcuts → Custom
Shortcuts → **+**. Name: "OpenCircle", command: `opencircle capture`, then set
the keys.

**KDE Plasma (by hand, only if you want more bindings):** System Settings →
Keyboard → Shortcuts → **Add New → Command or Script**. Command: `opencircle capture`, then assign the keys.

**Hyprland / Sway:** add a binding to your config, for example
`bind = CTRL SHIFT, Space, exec, opencircle capture` (Hyprland) or
`bindsym Ctrl+Shift+space exec opencircle capture` (Sway).

Add one binding per action you want, e.g. `opencircle run translate`.
