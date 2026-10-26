# OpenCircle — agent guide

OpenCircle brings Android's "Circle to Search" to the desktop (Windows and
Linux). Press a shortcut, the screen freezes, circle or drag over anything,
and search it (Google Lens), read it (OCR), translate it, or ask an AI about
it. A second mode snips and annotates screenshots (region, window, full,
scrolling; arrows, text, blur, steps; copy, save, pin). Stack: **pure Rust,
one binary, UI in `iced`** (no web view — lightweight is a hard
requirement). Spikes live in `spikes/` (throwaway). The plan is the private
master roadmap (see CLAUDE.local.md; older docs/internal plans are superseded), the UI/UX spec in `docs/internal/DESIGN.md`
(both local only). Status: **v0.1.0 published; work follows the master roadmap.**

**Start of every session: read `AGENTS.local.md`** (gitignored, beside this
file): logins and machine-specific notes. Then `docs/internal/NEXT_SESSION.md`,
then the master roadmap (CLAUDE.local.md) and the board
"OpenCircle Roadmap" (https://github.com/users/AmerAlreyahi/projects/8).

## General rules

The owner's general working rules are private. `AGENTS.local.md` says where
they are; read them at the start of every session. Project rules below win
where they differ.

## Never

- Never log, report, export or commit a secret (AI API keys live only in the
  OS keyring, never in the config file, logs or crash reports).
- Never send a capture anywhere the user didn't choose. Every network action
  (Lens, AI, translate) is an explicit user action or an explicit opt-in
  setting. No telemetry. No third-party image hosts (imgur, uguu, …).
- Never ship a capture to disk except where the user saves it; temp files are
  deleted when the overlay closes.
- Never copy Google's code, logos, icons or copy. Clean-room: we rebuild the
  behaviour, not the assets. Name, icon and visuals are our own.
- Never publish a GitHub release without the owner's OK for that release
  (the release workflow creates **drafts**).
- Releases: v0.1.0 published 2026-10-25 (owner OK; Windows + Linux preview; signed
  SHA256SUMS; assets re-downloaded and verified).
- Frozen contracts (since v0.1.0): the CLI (`opencircle capture …`, `run …`,
  `quit`) and the config file format: migrate, never break. The update channel
  (GitHub release assets, SHA256SUMS.txt + .minisig, trusted comment
  "OpenCircle v<ver>") is frozen too: installed copies depend on it.

## Architecture

Planned layout (details and change recipes: `ARCHITECTURE.md`, written with
the first code):

| Part | Role |
|---|---|
| `src/capture/` | Screen capture per platform (xcap / windows-capture / portal) |
| `src/hotkey/` | Global shortcuts (`global-hotkey`; Wayland GlobalShortcuts portal; CLI fallback) |
| `src/actions/` | One file per action: lens, ocr, translate, ai, copy, save. Registry = one list (action bar and quick actions share it) |
| `src/translate/` | OCR → erase → translate → redraw pipeline; live layer |
| `src/ai/` | Two API clients only: Anthropic Messages, OpenAI-compatible (covers OpenAI, Gemini, Ollama, LM Studio, OpenRouter); plus the no-key browser provider (Google AI Mode in the user's browser, default) |
| `src/ui/` | iced windows: overlay, action bar, panels, snip editor (one file per annotation tool), settings, onboarding; `theme.rs` = the only colour tokens |
| `src/config.rs` | The one config reader/validator (TOML) |

## Commands

    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo clippy --all-targets --features sheet -- -D warnings  # until N-02 removes `sheet`
    XDG_DATA_HOME=$(mktemp -d) XDG_CONFIG_HOME=$(mktemp -d) cargo test
    cargo packager --release   # --features sheet until N-02 removes it

Run tests with EMPTY app data/config dirs (as above): CI has no downloaded
models or config, and a machine with them can hide a failure. After every push,
check CI on main (`gh run list -b main`) before building on it.

(Exact scripts are created with the scaffold; update this block then.)

## Environments

Local dev only: WSL2 (this machine) for Linux builds, a Windows host for
Windows runs. CI runners: `ubuntu-22.04`, `windows-latest`. No servers, no
production systems.

## Owner decisions

- MASTER ROADMAP (owner, 2026-10-26): the private roadmap (link in the gitignored
  CLAUDE.local.md; local copy docs/internal/roadmap.md) overrides every older plan
  and any entry below that disagrees with it. Never follow its "Superseded" section.
  Work in the board's Order, ONE FEATURE AT A TIME, each made mature (tests, VM proof,
  owner OK) before the next (owner, 2026-10-26). A phase is done only when its
  Windows VM test gate passes; until the owner creates a Windows 11 VM, gates run on
  the Windows 10 VM with the Windows 11 part marked pending.
- UI questions (owner, 2026-10-26): the new design answers them
  (.claude/artifacts/design/newdesing*): build exactly what it shows. Bar: drag grip,
  AI field (replaces an Ask AI button), pills Search in browser (default) / Copy text /
  Translate, Live translate (Beta) icon, Song / Edit image only when on in Labs;
  Copy image = Ctrl+C, Save = Ctrl+S with a pill (#51, #52).
- Exact design behaviour (owner, 2026-10-26): copy the new design's widgets EXACTLY:
  behaviour, states, keyboard, drag/dock, styles and animations, not only the look.
  Reference pack for every UI brief: docs/internal/design-ref/ (screenshots per
  widget/state/theme, motion frame strips with timings, DESIGN_BEHAVIOR.md with
  source lines). Pass its shots and sections to the UI worker.
  Where the design is silent or buggy (DESIGN_BEHAVIOR.md section 13, e.g. no bar snap
  anchors, no Esc, no focus ring), keep today's behaviour. Owner decisions beat the
  design (e.g. Live translate (Beta) stays visible with Labs off).
- Overnight / owner away (2026-10-26): continue item by item in Order with the
  recommended defaults; leave each verified item in "Needs your check"; no releases.
- Board statuses (owner, 2026-10-26): Todo / In Progress / Needs your check / Done.
  "Needs your check" = built, gated and VM-verified by agents; the owner tries it on
  the test VM. "Done" only for what is really verified (owner-checked when it is
  something to try). Only the one active feature is In Progress.
- Superseded 2026-10-26 (removed from this file; see the roadmap): the Google results
  card / sheet / web view, Lens and AI Mode inside the app, auto-search after a
  selection, Frost/Graphite widget themes and in-app frosted glass (now the Liquid
  Glass design from the OS compositor, N-17/N-18), song search needing your own key
  (now AudD with no key first, N-34), the AI trial and any owner-paid service, the
  v0.1.x release themes and scopes, the one-exe sheet helper.
- Ground rule (owner, 2026-10-26): the owner pays nothing and makes no profit; nothing
  may depend on a paid owner service.
- Ask AI without a key (owner, 2026-10-26): until the AI chat (N-05) and OpenRouter
  sign-in (#39) land in 0.2.0, Ask AI with no key keeps opening Google AI Mode in the
  user's own browser (no web view); `ai/browser.rs` is deleted then.
- Installer (owner, 2026-10-26, #55): our own OpenCircle-Setup.exe in the new design
  (main theme Sapphire) installs the MSI underneath (Windows Installer external UI);
  the MSI stays a full installer (folder choice, re-skinned WiX wizard, silent
  install); NSIS is dropped; the portable exe stays.
- New design everywhere (owner, 2026-10-26): every visual is replaced by the new
  design (.claude/artifacts/design/newdesing*), with the LOGIC unchanged: all
  animations (core/motion.js), the live effect and every border ("Active Look",
  core/theme-context.js), the selection line colour (the stroke logic stays).
  Installer screens: widgets/installer.js. Lands with N-18/N-19 (#79/#80); #23 already
  uses the design's motion timings.
- Linux (owner, 2026-10-26): the 25 Oct Linux round stays on main; further Linux work
  is on hold until the owner resumes it.

- Repository (owner, 2026-10-23): public GitHub repo, created by the owner.
  Internal docs stay in gitignored docs/internal/.
- License (owner, 2026-10-23): MIT.
- Private rules (owner, 2026-10-23): the owner's engineering prompts are
  secret — never copy, quote or name them in tracked files.
- Stack (owner, 2026-10-23): pure Rust, UI in iced, no web view — the app
  must be super lightweight (target < 40 MB RAM idle). egui and Slint were
  considered: egui lacks bidi for mixed Arabic/English lines, Slint's RTL is
  unconfirmed.
- Platforms (owner, 2026-10-23): Windows and Linux first. macOS later, maybe.
- Full-screen translate (owner, 2026-10-23): top-priority feature, copy of
  Circle to Search's translate, built for games (in-place, live mode, tap to
  explain in context). Must feel excellent, not "good enough". The original
  text is erased (inpainted) and the translation drawn in its place with
  matched colour/size/alignment — never a box laid over it.
- Quick actions (owner, 2026-10-23): every action also gets a direct global
  shortcut that runs it without the floating action bar.
- Snip mode (owner, 2026-10-23): include a screenshot + annotate tool modelled
  on IObit's iTop Screenshot 1.2.3 (region/window/full/scrolling, annotate,
  copy/save), rebuilt clean-room from its behaviour, never from its binary.
  Scrolling capture is in v0.1; annotation must be more modern than iTop
  (smart shapes, curved arrows, callouts, spotlight — see DESIGN.md).
- Edit in place (owner, 2026-10-23): the action bar's Edit (`E`) turns the
  same floating bar into the editing toolbar (shapes, arrows, blur, text,
  steps, …) with a back arrow to the actions; works on any capture. One
  editor shared with Snip mode.
- Visual consistency (owner, 2026-10-23): controls of the same kind look
  identical — same width, height, radius, spacing and alignment (e.g. every
  shortcut field is the same width with one keycap per key). No crowded
  rows of options: group them behind one control (e.g. one colour button
  opening a picker instead of a row of swatches).
- Shared UI (owner, 2026-10-23): every widget and popup uses the same shared
  components (Surface, Pill, IconButton, TextButton, KeyCap, Segmented, …;
  list in DESIGN.md) styled only from theme.rs — same background, blur,
  border, radius and shadow everywhere.
- Themes (owner, 2026-10-23): Light, Dark and System, from one token set.
- Workflow (owner, 2026-10-23): planning first; execution starts only after
  the owner reviews the plan and adds rules.
- Releases (owner, 2026-10-26, supersedes "unsigned binaries"): GitHub Releases via
  Actions, owner approves each publish; free code signing only (SignPath Foundation,
  #122 then #76); if refused, stay unsigned and explain SmartScreen; minisign keeps
  protecting updates.
- Merging (owner, 2026-10-23): the lead merges verified work to main itself once
  CI + the local gate are green (and Windows-tested when it touches the UI).
- Skills first (owner, 2026-10-25): every task uses the fitting skills/plugins;
  if an important one is missing, the lead asks the owner to install it BEFORE the
  work starts (plugins load only in a new session) and says plainly when it
  can't do something itself.
- UI motion and polish (owner, 2026-10-24): for motion, transitions, loaders and
  UI polish in every release, workers use the VectorLab UI/UX skills (plugin);
  ported to iced, our tokens and shared components.
- Progress tracking (owner, 2026-10-26, supersedes 2026-10-24): the GitHub Project
  "OpenCircle Roadmap" (users/AmerAlreyahi/projects/8): fields Phase, Priority, Area,
  Kind, Order (the work sequence). Cards are PUBLIC issues (public-safe wording, no
  secrets/hosts/paths), footer "Roadmap: master roadmap (private)". The lead keeps
  it and the roadmap current and closes issues when done.
- Snip's main use (owner, 2026-10-24): explaining things to an AI with images.
  Optimise for capture → annotate → paste into an AI chat in seconds: copy after
  capture, AI-friendly size, image + OCR text, delay timer, snapping + loupe,
  combine captures, numbered legend, presets, drag-out, local recent captures.
- Windows + tooltips (owner, 2026-10-24): our own themed window header (logo,
  title, styled window buttons) instead of the OS title bar. Every control keeps
  an accessible label, but a VISIBLE tooltip only when it adds something (icon-
  only button, truncated text, shortcut hint), never repeating the visible text.
- No boxes behind floating UI (owner, 2026-10-24, "never ever"): every popup
  window (toast, pin, controller, prompt, card) goes through the one shared
  floating-window helper `windows::floating` (our CPU renderer has no alpha on
  any OS): Windows SetWindowRgn / X11 SHAPE cut the window to the rounded shape;
  Wayland can't cut a window, so popups there are square (prefer drawing them
  inside the overlay on Wayland). A test forbids creating windows
  outside the helpers. Verify popups over white AND dark pages on both VMs.
- Window capture (owner, 2026-10-24, like iTop): in the default snip mode, hovering
  outlines the window (or UI element) under the cursor live; click captures it,
  drag draws a free region; wheel walks element → window → monitor.
- Blur (owner, 2026-10-24): a soft Photoshop-style blur brush (no blocky mosaic),
  still irreversible (strong radius + noise), with line assist (the brush stays on
  the text line it started on unless the user clearly moves; Shift = free; can be
  turned off). Solid "Redact" stays as a separate preset.
- Multiple screens (owner, 2026-10-25): Circle to Search opens on the screen of the
  ACTIVE window (keyboard focus), falling back to the mouse's screen when there is no
  active window (desktop); snip can capture from any screen (the picker spans all
  screens, like iTop). Test on the Windows VM with a virtual second display.
- Softer blur (owner, 2026-10-25, supersedes "strong radius + noise" in Blur): the blur
  must look pretty: smooth like a background fade, no visible squares or blocks, no
  noise/grain, weaker-looking, yet text stays unrecognisable (it reads as "no text
  there", not as a smear).
- Translate default target (owner, 2026-10-23): Arabic, changeable in settings.
- Screen border (owner, 2026-10-24): a thin gradient border around the captured
  screen; live translate animates it subtly; snip shows it static.
- Priorities (owner, 2026-10-24): when choosing between options, speed first,
  then lightweight, then free (no key, no account, no paid service) by default;
  paid or heavier options only as opt-in extras. E.g. live translate defaults to
  an on-device model, not an AI service.
- Pre-release installers (owner, 2026-10-25): build test MSI/installers on the
  Windows test VM (Rust + cargo-packager + WiX may be installed there), not on
  GitHub, for faster pre-release testing. GitHub builds only the real releases.
- Test machines (owner, 2026-10-24): two test VMs, agents may do anything on them
  (access details in AGENTS.local.md). Prefer them over the owner's own PC.
- Windows first (owner, 2026-10-24, to save credits): VM testing and
  Linux-only fixes are on hold; Windows is the priority. Linux must still build and
  pass CI. Linux VM testing resumes later, on the owner's word.
- Lightweight means (owner, 2026-10-24): the least resources at ALL times, idle
  and busy (RAM, CPU, GPU, disk, download), not just a low idle footprint.
- Main mode like Circle to Search (owner, 2026-10-24; bar contents, auto-search and
  the Lens/AI card are SUPERSEDED 2026-10-26 by N-03/N-04: circle, then choose): when the overlay opens, a
  bottom floating widget shows the main actions at once ("Ask about your screen"
  field, translate the screen, song search when a key is set). Text on screen can
  be selected directly: drag along text selects words, tap selects a word;
  circling still selects an area, the same way Circle to Search behaves.
  Text selection works like a desktop (owner, 2026-10-24): I-beam cursor,
  selection to the exact character, flat square highlight, no handles,
  double-click a word, triple-click a line, Shift+click, Ctrl+C, Ctrl+A.
  ONE widget (owner, 2026-10-24): no action bar next to the selection; the
  bottom bar (drag snaps to the nearest anchor: bottom-centre default, top-centre,
  bottom-left, bottom-right; anchor remembered; moves aside so it never covers the
  selection; default a clear gap above the
  taskbar) holds everything as round icon buttons (no labels, tooltips);
  selection actions (Search, Text, Copy, Save) are always there, dimmed until
  something is selected. The Ask AI icon will become an OpenCircle-logo SVG later.
  The bar keeps exactly the same size in every state (selection actions shown
  dimmed until something is selected), and Edit turns it into the editor toolbar
  in place, same anchor and drag (owner, 2026-10-25).
  No Search / Ask AI / Text buttons in the bar (owner, 2026-10-25: Enter asks AI,
  Ctrl+C / direct selection copies text, search runs on its own; in "wait" mode
  Enter with an empty field searches). Ask AI and Lens use ONE card: no interim
  panel, same side and remembered size from the first frame.
  The bottom bar holds: the ask field, Ask AI, Translate screen, Live translate,
  Search music and Edit image (owner, 2026-10-24). Search music is always shown
  (supersedes "hidden until a key is set"): with no key it opens the key setup and
  sends nothing. Ask AI can also send a plain question without the screen (an
  "Include screen" toggle in the field, on by default).
- Live translate controller (owner, 2026-10-25): ONE compact controller in the
  bar's look, sized to what's visible, never over a window's tab bar.
- Start with the OS (owner, 2026-10-25): a Settings > General switch, ON by
  default (also in onboarding); the app manages the Run key / autostart entry.
- Copy = image only (owner, 2026-10-25, supersedes "image + OCR text"): copying a
  capture puts only the picture on the clipboard; text is copied only via text
  selection / Copy text.
- Other providers (owner, 2026-10-25): Codex and Google Antigravity may be used as
  workers; from Google use ONLY the "Argon" model, never other Google models.
  Exception (owner, 2026-10-25): Gemini Pro or Flash 3.8 may run Linux TEST passes
  (test only, no code), now and after each release; scope for now: widgets, Circle
  to Search, OCR, translation (no song search, no snip tool).
  Gemini ONLY runs the steps and captures screenshots: it never touches code, never
  decides, never judges; any verdict it gives is ignored. The lead passes the pictures
  to the owner, who approves or rejects himself.
- Settings (owner, 2026-10-25, future): changes save automatically (no Save
  button); the widget theme (Frost/Graphite, glass) styles the Settings window too.
- About page (owner, 2026-10-25): app info, model/library notices, and a developer
  card for the owner (avatar bundled, no runtime network; data refreshed from the
  owner's GitHub profile at each release by scripts/refresh-about.sh).
- Credits (owner, 2026-10-24): choose the cheaper way whenever quality barely
  changes: at most 3 workers at once, mid model for routine work (strong model
  only for security, hard bugs, design), one contact sheet per visual proof,
  short replies, only key videos.
- Editor moving (owner, 2026-10-24, like iTop): any annotation moves by holding and
  dragging it, with any tool active. Pen and highlighter strokes never show a
  selection box or handles.
- Auto-update (owner, 2026-10-24, in v0.1): the app watches GitHub Releases, tells
  the user an update exists, installs it quietly when idle, relaunches, then shows
  that release's notes. Opt-in asked on first run (one onboarding switch, pre-set
  on, changeable in Settings); the check sends nothing but the request. Every update
  must pass a minisign signature over SHA256SUMS (public key in the app, private key
  only in a GitHub Actions secret); no valid signature = no install. Binaries stay
  unsigned. Where self-replace isn't possible (deb/rpm), notify + link only.

## Domain quirks

- Wayland can't hide windows (set_visible(false) is a no-op): close them and
  reopen instead (`ui::can_hide_windows()`); never "hide" a window there.
- Wayland forbids global key grabs and silent screenshots: use the
  xdg-desktop-portal (Screenshot, GlobalShortcuts). The GlobalShortcuts portal
  is missing on some desktops → fallback: user binds `opencircle capture` in
  their desktop's shortcut settings.
- `Windows.Media.Ocr` works unpackaged (no MSIX needed; benchmarked 2026-10-26: 90-335 ms
  per screen). It is the fast engine on Windows (N-12, #62); PP-OCRv5 stays for selections,
  Arabic and games. It reads only languages installed in Windows (#63).
- Google Lens has no public upload API. Existing clones upload to imgur/uguu
  (privacy leak). Our approach: the browser itself posts the image to Lens
  through a local auto-submitting form (spike S1 in the plan proves it).
