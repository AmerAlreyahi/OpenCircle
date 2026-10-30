# Releasing OpenCircle

Only the owner publishes. The workflow (`.github/workflows/release.yml`) builds
and signs everything and creates a **draft**; it never publishes.

## Stable release, step by step

1. **Docs match the release.** README, `docs/privacy.md` and
   `docs/getting-started.md` describe what this version really does (features,
   shortcuts, network actions, install steps).
2. **CI is green on `main`**, including the installer job (`gh run list -b main`).
3. **Test machine pass.** Install the build on a clean Windows 10 test machine
   without WebView2 and run the release gate (capture, search, copy text,
   translate, snip, settings, install and uninstall).
4. **Version.** `Cargo.toml` stays at `0.0.0` in git: the workflow stamps the
   version from the tag on the runner only. Nothing to bump; pick the tag.
5. **Tag.** `git tag v<ver>` and push the tag (for example `v0.2.0`). A manual
   "Run workflow" is a dry run: it builds and uploads workflow artifacts and
   creates no release.
6. **The workflow** builds Windows (setup exe, portable exe; the MSI is built only to sit inside the setup and is not uploaded) and Linux
   (AppImage, deb, rpm), writes `SHA256SUMS.txt`, signs it with minisign
   (`SHA256SUMS.txt.minisig`, trusted comment `OpenCircle v<ver>`, checked by
   the app; this format is frozen), and creates a draft release.
7. **Code signing.** When available (SignPath is not active yet), sign the
   Windows files as the README's Code signing policy says, before checksums.
   Until then the files are unsigned and the README explains SmartScreen.
8. **Release notes.** Write them in the draft from
   `.github/RELEASE_NOTES_TEMPLATE.md`.
9. **Update test on the draft (blocks publishing).** Before the owner is asked
   to publish, the DRAFT's assets are served from a local server on the Windows
   test machine, and the previous published version updates itself to them
   through `OPENCIRCLE_TEST_UPDATE_URL` (published builds honour it). Both ways
   users install: the previous Setup exe (installed in Program Files, run as a
   normal user) and the previous portable exe. Pass = each one ends on the new
   version, starts and shows the notes. Any failure: no publish.
   Script: `scripts/update-gate.ps1` (run elevated in the test machine's desktop):
   `gh release download v<prev> -p "OpenCircle-Setup-*" -p "*-x64-portable.exe" -D prev`,
   `gh release download v<new> -D draft`, then
   `update-gate.ps1 -Previous prev -New draft [-Notes notes.md] [-Shots shots]`; it prints
   PASS / FAIL per install kind and exits non-zero on any FAIL. A Setup install of a
   version before 0.1.2 can't update itself (#151: it can't write Program Files); from
   0.1.2 on it offers "Install now" (one admin prompt), which the script clicks and approves.
10. **Publish by hand.** The owner reads the draft and presses Publish.
11. **Post-release check** (on the test machine):
    - Install the previous version, let it auto-update to the new one, check
      the new version starts and its release notes show.
    - Rollback check: a new version that fails to start must be rolled back by
      the updater (`update::roll_back`, `src/update/swap.rs`).
    - Re-download the published assets; verify them against `SHA256SUMS.txt`
      and verify the signature of `SHA256SUMS.txt` with the public key in
      `src/update/verify.rs`.

## Frozen: the update channel

Asset names, `SHA256SUMS.txt`, `SHA256SUMS.txt.minisig` and the trusted comment
`OpenCircle v<ver>` are what installed copies depend on. Do not change them.

## Beta releases

- Tag `v<ver>-beta.<n>` (for example `v0.2.0-beta.1`). The same workflow runs;
  the draft is created as a **pre-release**. Publish it by hand like any release.
- The binaries are stamped `<ver>` (the MSI needs a numeric version), so a beta
  looks like the final version to itself. A tester installs the final release by
  hand when it comes.
- **Stable users never get a beta.** The updater asks only GitHub's "latest
  release" (which excludes pre-releases and drafts), refuses pre-releases, and
  ignores any tag that is not `X.Y.Z`. A test (`a_beta_prerelease_is_never_picked`)
  fails if that changes.
- There is **no beta opt-in in the app** for now. Testers download betas from the
  releases page by hand.

## Tester retest of a beta

1. Open the beta's release notes: they list the fixed issues.
2. Download the beta by hand; check it against `SHA256SUMS.txt`.
3. For each fixed issue (the board's "Needs your check" items), repeat the steps
   from the issue on the new build. Comment on the issue: fixed, or still
   broken with what you saw (and "Copy diagnostics" from Settings if useful).
4. Fixed and confirmed: the issue is closed at the next stable release.

## The update signing key

- The private minisign key exists only in the `release` environment's secret
  (usable by `v*` tag runs only) and in the maintainer's offline backup. There is
  no repository-level copy. The public key is built into the app
  (`src/update/verify.rs`).
- Every installed copy checks that `SHA256SUMS.txt.minisig` verifies with that key
  and that its trusted comment is exactly `OpenCircle v<version>` of the release it
  downloads. A replayed signature from another version, a missing comment or an
  older version is refused.
- **Backup:** keep an offline copy of the private key (for example in a password
  manager and on an encrypted USB stick). Losing it means installed copies can no
  longer auto-update.
- **Rotation** (key lost or exposed, or planned):
  1. Make a new key pair.
  2. Release N, still signed with the OLD key, ships the NEW public key.
     Installed copies verify N with the old key, update, and from then on trust
     the new key.
  3. From release N+1 on, sign with the new key: replace the `release`
     environment secret, then delete the old private key.
  4. If the old key was exposed, say so in N's release notes. Copies that skip
     release N must update by hand once.
