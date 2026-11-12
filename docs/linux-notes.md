# Linux notes

Status: **preview.** Windows is the main target and has every feature; Linux
works on X11 and Wayland with the limits below.

OpenCircle runs on X11 and on Wayland. Wayland restricts what any app may do,
so a few things work differently.

## Install in one command

```bash
curl -fsSL https://raw.githubusercontent.com/AmerAlreyahi/OpenCircle/main/scripts/install.sh | sh
```

[`scripts/install.sh`](../scripts/install.sh) downloads the latest release's AppImage and
checks, before anything is installed, the minisign signature over `SHA256SUMS.txt` (the
key the in-app updater trusts, naming exactly that release) and then the AppImage's
SHA-256. It installs `~/.local/bin/opencircle` (no `sudo`), the app-menu entry
`com.opencircle.app.desktop` and its icon, and starts the app. It needs `curl` or `wget`
and OpenSSL 3, and is safe to run again to upgrade. `sh -s -- --uninstall` removes
what it installed (your settings stay).

## X11

Everything works the straightforward way: the screen is captured directly,
shortcuts are real global key grabs (on KDE Plasma: KDE's shortcut service, see
[KDE Plasma](#kde-plasma)), the window list is available so Snip can
outline and pick windows, and Long capture works while you scroll. Live translate is
supported.

## Wayland

| Need | How OpenCircle gets it |
|---|---|
| Screen capture | The desktop's Screenshot portal. On GNOME before 46 (for example Ubuntu 22.04), whose Screenshot portal asks on every capture, OpenCircle uses the ScreenCast portal instead: GNOME asks once (**Share**), then captures without asking again. That needs GStreamer's PipeWire plugin (`gstreamer1.0-pipewire` on Debian/Ubuntu); without it the Screenshot portal is used. |
| Global shortcuts | The GlobalShortcuts portal (GNOME 48+, KDE Plasma 6, Hyprland's portal). The desktop asks you once. On GNOME without the portal (before 48), OpenCircle adds its keys to GNOME's own custom shortcuts (named "OpenCircle: ..."), so they work with no dialog; a key GNOME keeps for itself (for example a `Super` shortcut) is refused in Settings with the reason. On KDE Plasma without the portal (Plasma 5), OpenCircle registers its keys with KDE's own shortcut service: see [KDE Plasma](#kde-plasma). Where there is no shortcut service at all, Settings names the command to bind yourself: see [Shortcuts](shortcuts.md#wayland-without-the-shortcut-portal). |
| Snip | There is no window list: drag an area, or click (or press `Enter`) to take the screen under the pointer. Long capture works (see below). Copy, Save, the toolbar and its tools, Steps, the colour picker and the screen ruler all work. Share (Windows' share sheet) is Windows only. |
| Search in browser, Ask AI (Browser) | The page that carries your picture to Google Lens is served once from 127.0.0.1, never written to a file, so snap and Flatpak browsers (for example Ubuntu's snap Firefox) open it too. |
| Live translate | Not available; each press of the translate shortcut translates once. |

The portal paths and the Snip flow above are verified on GNOME 42 (Ubuntu 22.04,
Wayland) and GNOME 50 (Ubuntu 26.04, Wayland); other desktops are not yet. If
something fails, `opencircle` prints a line explaining it on its standard error;
run it from a terminal to read it.

### Long capture on Wayland

Pick the area, then **More > Long capture**, and scroll the page yourself. The
desktop streams the screen to OpenCircle through the ScreenCast portal (allowed
once, like the screen capture), only while the capture runs. A small pop-up
beside the area shows the page so far, its height, and **Copy**, **Save**,
**Done** and **Cancel**; if the area fills the screen, the pop-up sits in its
top-right corner and is left out of the picture (drag just the part that
scrolls, so the pop-up is not over it). **Done** opens Snip with the page
picked, ready to mark up.

While it runs, the tray icon shows a record dot and the height so far, and its
menu has **Finish**, **Copy**, **Save** and **Cancel**. The pop-up needs
XWayland (GNOME, KDE and most wlroots desktops run it); without XWayland the
tray is the only sign. Long capture needs a ScreenCast portal of version 4 or
later and GStreamer's PipeWire plugin; without them its button is dimmed.

### KDE Plasma

On KDE Plasma (X11 and Wayland) OpenCircle registers every shortcut with KDE's own
shortcut service, the one KDE's apps use, unless the desktop offers the
GlobalShortcuts portal (Plasma 6 on Wayland), which is then used. The keys work
with no dialog and appear in System Settings → Shortcuts under **OpenCircle**:

- A key you change there is the one that works, also after a restart; OpenCircle's
  Settings shows it under that shortcut.
- A key you record in OpenCircle's Settings is written to KDE at once. A key another
  KDE action already uses is refused with its name (for example "Used by KWin:
  Switch to Desktop 1").
- A shortcut you clear in OpenCircle's Settings is removed from KDE, and
  `install.sh --uninstall` removes them all.
- While OpenCircle is not running, its keys stay in KDE's list but do nothing.

### Changing a shortcut

With the shortcut portal, GNOME keeps the key it stored for a shortcut and
ignores the one an app asks for later. So a key you change in OpenCircle's Settings reaches the desktop as a new
shortcut, and GNOME shows its "Add Keyboard Shortcuts" dialog once: accept it.
Keys you leave at OpenCircle's defaults keep their identity, so a key you set in
GNOME's own settings (Settings → Apps → OpenCircle → Global Shortcuts) stays.
Without the portal (older GNOME), a key you change in OpenCircle's Settings
takes effect at once in GNOME's custom shortcuts.

### First start

With the shortcut portal (GNOME 48+), the first start shows GNOME's own "Add
Keyboard Shortcuts" dialog once, listing OpenCircle's keys (including Snip's
`Alt+A`). Accept it; GNOME keeps what you confirmed. On older GNOME there is no
dialog: the keys appear in Settings → Keyboard → Custom Shortcuts, and OpenCircle
keeps them in step with its own Settings.

### Print Screen

Snip's default key is `Alt+A`, which GNOME leaves alone. GNOME's shell takes
`Print Screen` before any app's global shortcut, so if you give Snip that key it
does nothing until you free it with `gsettings`: see
[Shortcuts](shortcuts.md#defaults). You can also bind `Print Screen` to
`opencircle run snip` in GNOME's or KDE's own shortcut settings: see
[Print Screen for Snip through your desktop](shortcuts.md#defaults).

### Limits on Wayland

- Popups that belong to the open overlay (Snip's toolbar, its menus and size
  readouts, hints, the Copied pill after Copy or Save) are drawn inside it, with
  rounded corners, next to your selection. Windows that outlive the overlay (the
  Steps strip, pins, the update pills) are square: Wayland cannot cut a window to
  a rounded shape.
- GNOME places new windows itself, so a pin and the Steps strip may open on top
  of each other; drag one aside. The pinned AI chat is placed by GNOME too, and
  GNOME decides whether it stays above other windows.
- A shortcut pressed in GNOME's Activities overview first closes the overview,
  then opens the overlay with the keyboard (about a quarter of a second later).
- The Snip timer (Settings > Snip) shows no countdown on Wayland: the frozen
  screen closes, and the capture comes when the time is up.
- Circle to Search takes about half a second to appear (the portal screenshot),
  and reading text uses the downloaded reader, so a full page can take around ten
  seconds to become selectable.
- The first offline translation needs a model download (for example about 117 MB
  for German to English); a pair that goes through English needs two. A page
  takes several seconds on a CPU-only machine.

### App id and desktop file

The desktop grants shortcuts and screenshots by app id, and GNOME needs a
desktop file named after it. OpenCircle's app id is `com.opencircle.app`; the
`.deb` and `.rpm` install `/usr/share/applications/com.opencircle.app.desktop`.
A raw binary or an AppImage has none, so on its first start under Wayland it
writes `~/.local/share/applications/com.opencircle.app.desktop` (pointing at
the binary, or at the AppImage file) and its icon
`~/.local/share/icons/hicolor/128x128/apps/com.opencircle.app.png`, only if no
such desktop file exists yet. Delete both files to remove them; it writes them
again on its next Wayland start.

### Screenshot permission

GNOME asks once whether OpenCircle may take screenshots, and only while an
OpenCircle window is in front. The first-run setup has a Screenshots step for
that (**Allow**; **Share** on GNOME before 46, where GNOME's ScreenCast dialog
asks). If a capture is refused later (for example after **Deny**), a small
"OpenCircle needs screenshot permission" window opens; its **Allow** button
asks the desktop again (on GNOME it first clears the earlier "Deny", so GNOME
shows its dialog again). A capture that gets no answer within 20 seconds is
given up, so the next shortcut press works.

## Tray

The tray icon needs AppIndicator support: `libayatana-appindicator3-1`
(Debian/Ubuntu) or `libayatana-appindicator-gtk3` (Fedora). GNOME also needs
the "AppIndicator and KStatusNotifierItem Support" extension to show tray
icons at all. Without a tray (for example an AppImage on a desktop with no
tray), starting OpenCircle opens its Settings window (the first-run setup on
the first start), so you can always reach it; the shortcuts and `opencircle
capture` / `opencircle run <action>` work as usual.

## Keyring

AI keys are stored through the Secret Service (GNOME Keyring, KWallet). If none
is running OpenCircle tells you instead of saving the key anywhere else. A
local model needs no key.

## Packages and libraries

The `.deb` and `.rpm` declare their dependencies: GTK 3, AppIndicator, libxcb,
libX11 (with xcb), libXcursor, libXi, xkbcommon (and its X11 part), the Wayland
client library and `xdg-desktop-portal`. The AppImage needs the same on the host.

If you build from source, see the package list in the [README](../README.md#build-from-source).
