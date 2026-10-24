# OpenCircle — agent guide

OpenCircle brings Android's "Circle to Search" to the desktop (Windows and
Linux). Press a shortcut, the screen freezes, circle or drag over anything,
and search it (Google Lens), read it (OCR), translate it, or ask an AI about
it. A second mode snips and annotates screenshots (region, window, full,
scrolling; arrows, text, blur, steps; copy, save, pin). Stack: **pure Rust,
one binary, UI in `iced`** (no web view — lightweight is a hard
requirement). Spikes live in `spikes/` (throwaway). The plan lives
in `docs/internal/PLAN.md`, the UI/UX spec in `docs/internal/DESIGN.md`
(both local only). Status: **planning — no code until the owner approves the
plan.**

**Start of every session: read `AGENTS.local.md`** (gitignored, beside this
file): logins and machine-specific notes. Then `docs/internal/NEXT_SESSION.md`,
then this project's items on the Projects HQ board.

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
- Frozen contracts: none yet. Once released, the CLI (`opencircle capture …`)
  and the config file format become frozen (migrate, never break).

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
    XDG_DATA_HOME=$(mktemp -d) XDG_CONFIG_HOME=$(mktemp -d) cargo test
    cargo packager --release

Run tests with EMPTY app data/config dirs (as above): CI has no downloaded
models or config, and a machine with them can hide a failure. After every push,
check CI on main (`gh run list -b main`) before building on it.

(Exact scripts are created with the scaffold; update this block then.)

## Environments

Local dev only: WSL2 (this machine) for Linux builds, a Windows host for
Windows runs. CI runners: `ubuntu-22.04`, `windows-latest`. No servers, no
production systems.

## Owner decisions

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
- Releases (owner rules): unsigned binaries, GitHub Releases via Actions,
  owner approves each publish.
- Merging (owner, 2026-10-23): the lead merges verified work to main itself once
  CI + the local gate are green (and Windows-tested when it touches the UI).
- Progress tracking (owner, 2026-10-23): no GitHub Projects board for this repo;
  progress lives in the handover file only.
- Selection look (owner, 2026-10-24, supersedes 2026-10-23): like Google by
  default: the drawn circle snaps to a rounded box with corner brackets and the
  search runs automatically; "keep the drawn shape" and "wait for me" are options
  in Settings > Look & behaviour.
- Customization (owner, 2026-10-24): a Look & behaviour page: selection style,
  auto-search vs wait, search target, border style per mode (Google-style rainbow
  glow / thin gradient / none, thickness, animation, colours), activation glow, dim.
- Themes (owner, 2026-10-24): besides the app theme (Light/Dark/System), "active
  look" themes for when the tool is over the screen (glow, border, dim, brackets,
  bar style, accent), several presets (Google-like multi-colour default) + custom.
- Song search (owner, 2026-10-24): free fingerprint lookup (AcoustID) can't match
  clips (spike: 1/28), so song search is OPT-IN, bring your own key for a clip
  recognition service (AudD first); the button stays hidden until a key is set;
  a ~10 s clip of the chosen audio is sent to that service only on press (say so
  plainly); key in the OS keyring; history off by default.
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
- Results sheet (owner, 2026-10-24): explore a slide-up sheet showing Google's own
  page (Lens / AI Mode) in the system web view (WebView2 / WebKitGTK), loaded only
  while open; no scraping, no copying Google's UI. Placement (owner, 2026-10-24):
  on landscape screens it slides in from the side AWAY from the selection (auto;
  pinnable left/right), full height, resizable; bottom sheet only on portrait or
  narrow screens. Everything happens inside the card where the platform allows
  (Lens + AI Mode on Windows; on Linux Lens also opens in the card and a HUMAN
  solves Google's CAPTCHA there if shown; agents never solve/bypass CAPTCHAs;
  owner is testing how often it returns; fallback setting = browser).
  Google sign-in doesn't work in embedded web views, so history = an opt-in LOCAL
  OpenCircle search history + "Open in browser". All result panels follow the same rule, as FLOATING cards
  (owner, 2026-10-24): inset from the screen edges, rounded ~20 px, shadow,
  draggable + resizable, never a flat full-height slab.
- Translate default target (owner, 2026-10-23): Arabic, changeable in settings.
- No-key AI (owner, 2026-10-24): until a key is set, Ask AI opens the question
  (prompt + the selection's text) in Google Search AI Mode in the user's own
  browser; the image goes only via Lens. API providers stay as options.
- Screen border (owner, 2026-10-24): a thin gradient border around the captured
  screen; live translate animates it subtly; snip shows it static.
- Priorities (owner, 2026-10-24): when choosing between options, speed first,
  then lightweight, then free (no key, no account, no paid service) by default;
  paid or heavier options only as opt-in extras. E.g. live translate defaults to
  an on-device model, not an AI service.
- Test machines (owner, 2026-10-24): two test VMs, agents may do anything on them
  (access details in AGENTS.local.md). Prefer them over the owner's own PC.

## Domain quirks

- Wayland can't hide windows (set_visible(false) is a no-op): close them and
  reopen instead (`ui::can_hide_windows()`); never "hide" a window there.
- Wayland forbids global key grabs and silent screenshots: use the
  xdg-desktop-portal (Screenshot, GlobalShortcuts). The GlobalShortcuts portal
  is missing on some desktops → fallback: user binds `opencircle capture` in
  their desktop's shortcut settings.
- `Windows.Media.Ocr` officially needs package identity (MSIX) → we use the
  cross-platform PaddleOCR (ONNX Runtime) engine instead.
- Google Lens has no public upload API. Existing clones upload to imgur/uguu
  (privacy leak). Our approach: the browser itself posts the image to Lens
  through a local auto-submitting form (spike S1 in the plan proves it).
