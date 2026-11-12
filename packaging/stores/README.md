# Store manifests (winget, Scoop)

Prepared files only: nothing here is submitted anywhere. The owner submits.
Rules read on 2026-11-11 (links in the w22 report). Regenerate for any published release:

    scripts/store-manifests.sh 0.1.7 2026-12-01     # needs gh; reads that release's SHA256SUMS.txt
    python3 -I packaging/stores/validate.py          # schema check (python3-jsonschema, PyYAML)
    scripts/test-store-manifests.sh                  # v0.1.6 regenerates identically, the next release's form is right, a bad hash is rejected

| Store | What | Files | Who reviews |
|---|---|---|---|
| winget | the Setup exe (`exe`, `/quiet`, machine scope) | `winget/<ver>/` (3 files, schema 1.28.0) | automated pipeline + a moderator |
| Scoop (own bucket, now) | the portable exe | `scoop/opencircle.json` | nobody |
| Scoop Extras (later) | same file | same | maintainers; needs >= 100 stars and/or 50 forks (repo has 5 / 3 on 2026-11-11) |

Why Setup for winget and portable for Scoop: the Setup is what the app's own updater and
repair/uninstall understand (Program Files, Apps & features); Scoop wants a portable file it can
keep in `versions\<ver>`. Offering both in winget would make `winget install` pick by scope/type
ambiguously, so only the Setup goes there (a portable winget package can be added later as a
separate identifier if people ask).

## winget (first submission)

One-time, on the owner's PC:
1. Sign the Microsoft CLA: <https://cla.opensource.microsoft.com/microsoft/winget-pkgs> (log in as AmerAlreyahi).
2. Fork <https://github.com/microsoft/winget-pkgs> to AmerAlreyahi/winget-pkgs; clone it.
3. `winget install wingetcreate` (also useful later).

Test (Windows 10/11, elevated PowerShell, folder = `packaging/stores/winget/0.1.6`):

    winget validate --manifest <folder>
    winget settings --enable LocalManifestFiles
    winget install --manifest <folder>          # an unsigned exe from a local manifest shows SmartScreen: click More info > Run anyway (see "Why ... waits" below)
    winget list OpenCircle                       # must show Id ARP\Machine\X64\OpenCircle, version 0.1.6
    winget uninstall --id "ARP\Machine\X64\OpenCircle"
    # optional, isolated: Tools\SandboxTest.ps1 <folder> in the clone (Windows Sandbox, Pro/Enterprise)

Submit:

    git checkout -b AmerAlreyahi.OpenCircle-0.1.6 master
    mkdir -p manifests/a/AmerAlreyahi/OpenCircle/0.1.6
    cp <repo>/packaging/stores/winget/0.1.6/* manifests/a/AmerAlreyahi/OpenCircle/0.1.6/
    git add manifests && git commit -m "New package: AmerAlreyahi.OpenCircle version 0.1.6"
    git push -u origin AmerAlreyahi.OpenCircle-0.1.6        # then open the PR against microsoft/winget-pkgs master

PR title: `New package: AmerAlreyahi.OpenCircle version 0.1.6`. The PR template body has the
checklist: tick CLA signed, one manifest only, `winget validate` run, tested with
`winget install --manifest`, conforms to schema 1.28. (`wingetcreate new <url>` can do the whole
thing interactively instead, but it would overwrite our hand-tuned switches: use `update` later.)

What to expect: a GitHub App runs 10 checks (structure, schema, URLs, installer scan with Defender
and other engines, a silent install as a standard user in a clean VM, post-install scan). Labels like
`Validation-Completed` then a moderator reviews, and approval merges it automatically. A moderator can
re-run with a comment `@wingetbot run`. Typical wait: days.

Known risks:
- Unsigned installer. Signing is not a stated requirement; the risk is a heuristic/PUA hit in the
  installer scan (fails the PR whatever the intent; fix = submit the file to Microsoft as a false
  positive at <https://www.microsoft.com/wdsi/filesubmission> with the PR URL, then `@wingetbot run`).
  The 22 MB exe embeds an MSI and unpacks it to temp: the kind of behaviour heuristics dislike.
- The test runs as a standard user; our Setup needs admin (per-machine). The manifest says
  `ElevationRequirement: elevationRequired`, which is the documented answer. If the pipeline still
  rejects it: ask in the PR what it expects (we cannot install per user without a bigger change, item 1 below).
- Own updater: after install, the app's updater (when the user switched it on) downloads the next
  Setup and runs it elevated on "Restart now". winget later sees a newer DisplayVersion; nothing conflicts
  (`winget upgrade` runs the same Setup `/quiet`, which updates in place). If the in-app update ran first,
  winget reports "no upgrade". A moderator may ask about it: answer that the updater is opt-in, signed and
  user-initiated for Setup installs.
- ARP entry: the MSI hides its own Apps & features entry and writes one named `OpenCircle`
  (Publisher `opencircle` up to 0.1.6, `Amer Alreyahi` from 0.1.7): the manifest carries `AppsAndFeaturesEntries` for it.

Later releases (by hand): `scripts/store-manifests.sh <ver> <date>`, copy the folder to
`manifests/a/AmerAlreyahi/OpenCircle/<ver>/`, title `Update: AmerAlreyahi.OpenCircle version <ver>`;
or just `wingetcreate update AmerAlreyahi.OpenCircle --version <ver> --urls <Setup url> --submit`
(token with `public_repo`). Free automation: `winget-release.workflow.yml` in this folder (a DRAFT, not in
`.github/workflows/`): copy it there after the first version is merged, create a classic PAT with only
`public_repo`, store it as the repo secret `WINGET_TOKEN`. It fires on `released` (published, never drafts).
(vedantmgoyal9/winget-releaser is the common alternative; its last release was January 2025, so we use Microsoft's own tool.)

## Scoop

Now (option B, recommended): your own bucket, no review.
1. Create the public repo AmerAlreyahi/scoop-bucket from the template
   <https://github.com/ScoopInstaller/BucketTemplate> ("Use this template"). It ships the `ci.yml` and
   `excavator.yml` Actions (free): Excavator runs `checkver`/`autoupdate` every few hours and commits version bumps.
2. Put `packaging/stores/scoop/opencircle.json` at `bucket/opencircle.json`.
3. Users: `scoop bucket add opencircle https://github.com/AmerAlreyahi/scoop-bucket` then `scoop install opencircle`.
4. Test (Windows, Scoop installed): `scoop install .\bucket\opencircle.json`, run it, `scoop uninstall opencircle`;
   `.\bin\checkver.ps1 opencircle` and `.\bin\checkver.ps1 opencircle -f` from Scoop's checkout to prove autoupdate.

Later (Extras): <https://github.com/ScoopInstaller/Extras>. Required order: open a "Package Request" issue first
(the form asks for: reasonably well known, at least 100 stars and/or 50 forks, English interface/docs, latest stable,
standard install, some indication of popularity); wait for a maintainer to approve; fork, branch from `master`,
add `bucket/opencircle.json`; PR title `opencircle: Add version 0.1.6`; comment `/verify` on the PR. Version-only
bump PRs are refused: Excavator does bumps, so `checkver` + `autoupdate` must work. Prefer this only after the stars threshold;
until then option B.

Notes on the manifest:
- `url ...#/opencircle.exe` renames the download so the shim, shortcut and the CLI (`opencircle capture`, a frozen contract)
  all use `opencircle.exe`. `bin` puts it on PATH; `shortcuts` adds the Start menu entry.
- No `persist`: the app stores config in `%APPDATA%\opencircle` and data/models in `%LOCALAPPDATA%\opencircle`, never
  beside the exe, so there is nothing in Scoop's folder to persist; a `persist` of a path that does not exist would only
  create an empty folder. Reviewers prefer "portable via persist" only when the app writes beside itself.
- Updates: the portable exe's updater swaps the exe in place (inside `apps\opencircle\<ver>\`, reached through the `current`
  junction). That does not break `scoop update` (it downloads the new version into its own folder and re-points `current`), but
  the app would then be newer than Scoop thinks and the swap leaves marker files in the old version folder. Therefore the
  manifest's `notes` tell users to switch off "Keep OpenCircle up to date" (Settings > General; config `[update] auto = false`).
  From 0.1.7 the manifest's `post_install` writes `opencircle.managed` (beside the exe) and the app's updater is off for that
  copy (the notes no longer ask the user to switch it off). The 0.1.6 manifest keeps the old note: that exe does not read the file.
- `checkver: github` uses the latest non-prerelease release tag `v0.1.6`; `autoupdate.hash.url` points at the release's
  `SHA256SUMS.txt` (Scoop finds the line by file name).

## Other free channels (not prepared)

| Channel | Cost | Review | Signing / other needs |
|---|---|---|---|
| Chocolatey community repo | free | automated checks + a moderator, slow queue (days to weeks) | unsigned allowed but flagged by VirusTotal-style scans; needs a .nuspec + install script (can call Setup `/quiet`); API key from a free account |
| Flathub | free | PR to flathub/flathub, manual review | needs a Flatpak manifest and a build from source or a release tarball; app id `com.opencircle.app` (we already have AppStream metadata); Wayland/X11 portals fit; our own "no outside network surprises" is fine; sandbox permissions reviewed |
| AUR | free | none (user-maintained) | PKGBUILD (`opencircle-bin` from the AppImage/deb); needs an AUR account and SSH key; anyone may maintain it |
| Snap | free | store review for classic confinement; strict is automatic | snapcraft.yaml; screen capture and global shortcuts are hard under strict confinement, so likely classic (needs a manual request) |
| Homebrew cask | free | macOS only | not applicable until the macOS item starts |

## What the app does for stores (v0.1.7 and later; v0.1.6 predates all of it)

1. Per-machine only: a standard user cannot install the Setup silently (exit 1925 without elevation).
   winget elevates for `ElevationRequirement: elevationRequired`. A per-user install is a bigger change: only if the pipeline rejects.
2. One answer to "may this copy replace itself?": `update::Install::detect`. The HKLM value `ManagedBy` of the Setup install's Apps & features key (the MSI writes it from `MANAGEDBY=<name>`; admin-only, so no
   link-following write) or, for portable / Scoop copies, a file `opencircle.managed` beside the exe (text: `winget`, `scoop` or anything else) makes it "notify only", like deb / rpm: no download, no swap; the check still
   runs when the user's "Keep OpenCircle up to date" is on or on "Check now", and Settings > General says "Updates are managed
   by winget / Scoop / your package manager" and whether a newer version exists. The user's setting is not rewritten.
   Who writes it: Scoop's `post_install` (the manifest, 0.1.7 form: the user's own process); the Setup's `/managed=<name>` (through the MSI); a Linux package may ship the file.
   A marker that cannot be read counts as managed (a read failure never turns self-update on).
   winget installs keep the updater ON (no `/managed` in the manifest): winget accepts self-updating apps and re-reads the ARP
   DisplayVersion, the in-app update runs the same Setup winget would, and winget's pipeline test runs only the install. The
   cost is that the user may be offered the same version twice (the app's pill, then `winget upgrade` says nothing to do).
   If a winget moderator asks for it off, add `Custom: /managed=winget` to `InstallerSwitches` (one line in the script).
3. Publisher: the MSI's Manufacturer = Apps & features' Publisher = `Amer Alreyahi` (`publisher` in `[package.metadata.packager]`).
   The UpgradeCode comes from the binary name, not the publisher, so 0.1.6 upgrades in place; the HKCU keys keep their old name.
4. The exe's version resource carries FileVersion, ProductVersion (the release version), CompanyName, ProductName, FileDescription, LegalCopyright.
5. Silent switches and exit codes: README "Silent install"; `/managed=` is new, the codes were mapped (1223 -> 1602).
6. The two portable asset names (`OpenCircle-<v>-x64-portable.exe` and `OpenCircle_<v>_x64-portable.exe`, identical hashes):
   stores use the hyphen one; nothing to change (frozen channel).
7. Silent install time under winget's pipeline timeouts: not measured.

### Why `winget install --manifest` waits at "Starting package install" (measured, Windows 10 VM, winget 1.29.380)

Not the Setup: while it waits there is no Setup process. winget downloads the exe, marks it (Mark of the Web,
`ZoneId=3`, internet) and launches it with ShellExecute; for an UNSIGNED exe Windows shows SmartScreen's "Windows
protected your PC - Microsoft Defender SmartScreen prevented an unrecognized app from starting" (buttons "More info" /
"Don't run"; a `CHXSmartScreen` window the user may not see in a non-interactive session) and winget waits behind it.
Proof on the VM (one scripted pass): as is, winget hung 151 s with that dialog; the same manifest with the download not
marked (policy `SaveZoneInformation=1`) installed in 9 s ("Successfully installed"); the Setup copy with `ZoneId=3`
started by ShellExecute blocked on the prompt, the same file with `ZoneId=2` or no stream ran at once.
Why a local manifest differs from the repository: winget's `UpdateInstallerFileMotwIfApplicable` writes `ZoneId=2`
(Trusted) only for an installer whose package comes from a source with trust level Trusted (the default `winget`
community source), and `ZoneId=3` through the Attachment Execution Service for everything else, which a local
`--manifest` is. Measured: `winget download` of Notepad++ and of 7-Zip (7-Zip's MSI is not signed) from the community source
wrote `ZoneId=2`. So a package installed from the repository gets no prompt, signed or not.
Answer: v0.1.7 may be submitted unsigned; `winget install AmerAlreyahi.OpenCircle` from the repository should then
install without the prompt (not provable until the package is live; the first moderator run and a first install on a clean PC
are the proof). `winget install --manifest <local>` of an unsigned exe always shows the prompt (`Unblock-File` on the download is
useless: winget marks it again): click "More info" > "Run anyway", or test with the policy above. Code signing
(SignPath Foundation, applied for: README "Code signing policy") would remove the prompt for local installs and for
direct downloads too, and builds SmartScreen reputation; it is not required for winget.
There is no `/log` switch: a setup that runs as administrator must not write to a path the caller chose (planted-link risk).
