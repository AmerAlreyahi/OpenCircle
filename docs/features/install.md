<p align="center">
  <img src="../../gallery/clips/clip-6-install.webp" alt="Install OpenCircle, launch it, and find it in the tray." width="960"><br>
  <a href="../../gallery/clips/clip-6-install.mp4">Watch in full quality</a>
</p>

# Install OpenCircle

Install OpenCircle on Windows, or use the portable executable; Linux packages are available as a preview.

## How to

1. Get the Windows Setup executable or portable executable from [Releases](https://github.com/AmerAlreyahi/OpenCircle/releases). Linux users can choose a `.deb`, `.AppImage`, or `.rpm`.
2. On Windows, run `OpenCircle-Setup-<version>-x64.exe`. Choose **Install OpenCircle**, or customise the install folder. Running Setup for an existing installation offers **Update OpenCircle** or **Repair OpenCircle**, depending on the version.
3. Choose whether to **Start with Windows** and launch OpenCircle. The portable executable runs without an install.
4. Follow the first-run prompts, including your update preference. A tray notice tells you OpenCircle is running and shows the shortcut.
5. Press `Ctrl+Shift+Space` to try it. You can change startup and update preferences in Settings.
6. To uninstall an installed Windows copy, use Windows' installed-apps list. The uninstall window lets you **Keep** or **Remove** settings, downloaded models, and saved keys. Keep is the default.

## Shortcuts

| Shortcut | What it does |
|---|---|
| `Ctrl+Shift+Space` | Open Circle to Search after launching |
| `Ctrl+Shift+T` | Translate the whole screen |
| `Esc` | Close the capture overlay; leave the question field first if focused |

## Tips

- OpenCircle starts with Windows by default and waits in the tray. Turn this off in Settings if you prefer to start it yourself.
- Automatic updates are opt-in on first run. When enabled, they download and install while OpenCircle is idle, then relaunch it and show the release notes.
- Updates must pass a signature check and a file checksum check before installation. The update check sends only the request, with no screen content, personal identifier, or telemetry.
- For Linux setup and desktop shortcut details, see [Linux notes](../linux-notes.md) and [Getting started](../getting-started.md).

## Limits

- The Windows Setup executable does not yet support silent installation with `OpenCircle-Setup-<version>-x64.exe /quiet [/dir="…"]`. Use the interactive setup.
- Update signatures verify downloads; they are separate from Windows code signing. Windows SmartScreen may warn about the unsigned executable. See [Signing](../signing.md).
- Linux is a preview. `.deb` and `.rpm` installations notify you about updates and link to the release rather than replacing the package themselves.
- Windows OCR needs an installed recognition language. Other text readers and offline translation models download on first use.

---

[Previous: Save and copy](save-and-copy.md) · [Next: Circle to Search](circle-to-search.md) · [All features](README.md) · [Back to README](../../README.md)
