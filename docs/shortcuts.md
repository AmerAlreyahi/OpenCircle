# Shortcuts

## Defaults

| Action | Default | What it does |
|---|---|---|
| Circle to search | `Ctrl+Shift+Space` | Freeze the screen, then select and act |
| Translate full screen | `Ctrl+Shift+T` | Translate the whole screen at once |
| Live translate | `Ctrl+Shift+L` | Toggle continuous translation |
| Explain with AI | `Ctrl+Shift+E` | Circle something; the AI explains it |
| Copy text | `Ctrl+Shift+X` | Circle something; its text is copied |
| Search with Lens | unset | Circle something; Lens opens |
| Snip a region | `Print Screen` | Drag or circle an area, then edit it |
| Snip a window | `Alt+Print Screen` | Click a window, then edit it |
| Snip the full screen | `Ctrl+Print Screen` | The whole screen, then edit it |

Snip mode also opens from the tray menu (**Snip**, the region mode) and with
`opencircle run snip`.

**If Print Screen does nothing.** On Windows, the system uses `Print Screen`
for its own screen capture. OpenCircle logs one line when it cannot register the
key. Free it in Windows Settings → Accessibility → Keyboard → turn off **Use the
Print screen key to open screen capture**, then restart OpenCircle. (Or give the
snip shortcuts other keys in Settings → Shortcuts; the rest keep working.) On
Linux, a desktop's own screenshot tool may hold the key: unbind it in your
desktop's keyboard shortcut settings.

While the overlay is open: `Esc` closes, `Enter` runs the default action
(Search unless you changed it in **Settings → General**), and the action keys
are `L` Search, `A` Ask AI, `T` Text, `E` Edit, `Ctrl+C` Copy, `Ctrl+S` Save.
In the editor, `Ctrl+P` pins the image to the screen (`P` alone is the Pen).

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
action = "copy_text"        # translate | ai_explain | copy_text | lens | snip | song_search | ai | text | edit | copy | save
shortcut = "Ctrl+Shift+X"   # "" = not bound
target = "circle"           # full_screen | circle | last_region | window
live = false                # translate only: keep translating
```

Targets: `full_screen` acts on the whole screen, `circle` lets you draw the
area first, `last_region` repeats the previous area (handy for a game's
subtitle bar). `window` is for Snip only: click a window. For `snip`, the target
picks the mode, which is how the three defaults differ:

```toml
[[quick_actions]]
action = "snip"
shortcut = "PrintScreen"
target = "circle"           # region

[[quick_actions]]
action = "snip"
shortcut = "Alt+PrintScreen"
target = "window"

[[quick_actions]]
action = "snip"
shortcut = "Ctrl+PrintScreen"
target = "full_screen"
```

Edit the file while OpenCircle is closed, or change shortcuts in Settings.

## Command line

The same actions can be triggered from a terminal, a launcher or your
desktop's own shortcut settings. They talk to the running copy (or start it):

```bash
opencircle capture
opencircle run snip
opencircle run snip --target window
opencircle run copy_text --target circle
opencircle run lens --target circle
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
- **KDE Plasma:** System Settings → Keyboard → Shortcuts.

## Wayland without the shortcut portal

Wayland does not let apps grab keys. OpenCircle asks the desktop through the
GlobalShortcuts portal (GNOME 48+, KDE Plasma 6) and the desktop asks you once.
That path has not been verified on every desktop. If it is missing or you
decline, bind the commands yourself. Use the full path to the binary if it is
not on your `PATH` (for an AppImage, the path of the file).

**GNOME:** Settings → Keyboard → View and Customize Shortcuts → Custom
Shortcuts → **+**. Name: "OpenCircle", command: `opencircle capture`, then set
the keys.

**KDE Plasma:** System Settings → Keyboard → Shortcuts → **Add New → Command
or Script**. Command: `opencircle capture`, then assign the keys.

**Hyprland / Sway:** add a binding to your config, for example
`bind = CTRL SHIFT, Space, exec, opencircle capture` (Hyprland) or
`bindsym Ctrl+Shift+space exec opencircle capture` (Sway).

Add one binding per action you want, e.g. `opencircle run translate`.
