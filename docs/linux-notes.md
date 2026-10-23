# Linux notes

OpenCircle runs on X11 and on Wayland. Wayland restricts what any app may do,
so a few things work differently.

## X11

Everything works the straightforward way: the screen is captured directly,
shortcuts are real global key grabs, the window list is available for Snip's
Window mode, and scrolling capture can scroll for you. Live translate is
supported.

## Wayland

| Need | How OpenCircle gets it |
|---|---|
| Screen capture | The desktop's Screenshot portal. The desktop may ask you to allow it. |
| Global shortcuts | The GlobalShortcuts portal (GNOME 48+, KDE Plasma 6, Hyprland's portal). The desktop asks you once. If the portal is missing or you decline, bind `opencircle capture` yourself: see [Shortcuts](shortcuts.md#wayland-without-the-shortcut-portal). |
| Snip modes | Region and Full only: there is no window list and apps cannot send scroll input, so Window and Scrolling are not offered. |
| Live translate | Not available; each press of the translate shortcut translates once. |

The portal paths are not yet verified on every desktop. If something fails, `opencircle` prints a line
explaining it on its standard error; run it from a terminal to read it.

## Tray

The tray icon needs AppIndicator support: `libayatana-appindicator3-1`
(Debian/Ubuntu) or `libayatana-appindicator-gtk3` (Fedora). GNOME also needs
the "AppIndicator and KStatusNotifierItem Support" extension to show tray
icons at all. Without a tray the app still works from its shortcuts and
`opencircle capture` / `opencircle run <action>`; Settings opens from the
tray, so on such a system edit `~/.config/opencircle/config.toml` instead.

## Keyring

AI keys are stored through the Secret Service (GNOME Keyring, KWallet). If none
is running OpenCircle tells you instead of saving the key anywhere else. A
local model needs no key.

## Packages and libraries

The `.deb` and `.rpm` declare their dependencies: GTK 3, AppIndicator, libxcb,
libX11 (with xcb), libXcursor, libXi, xkbcommon (and its X11 part), the Wayland
client library and `xdg-desktop-portal`. The AppImage needs the same on the host.

If you build from source, see the package list in the [README](../README.md#build-from-source).
