# OpenCircle — agent guide

OpenCircle brings Android's "Circle to Search" to the desktop (Windows and
Linux). Press a shortcut, the screen freezes, circle or drag over anything,
and search it (Google Lens), read it (OCR), translate it, or ask an AI about
it. A second mode snips and annotates screenshots (region, window, full,
scrolling; arrows, text, blur, steps; copy, save, pin). Stack: **pure Rust,
one binary, UI in `iced`** (no web view — lightweight is a hard
requirement). Spikes live in `spikes/` (throwaway). The plan is the private
master roadmap (see CLAUDE.local.md; older docs/internal plans are superseded), the UI/UX spec in `docs/internal/DESIGN.md`
(both local only). Status: **v0.2.0 "Game translation" published (2026-11-15: game mode, per-game glossary + name suggester, Portuguese, Alt+Shift+T/L, Linux part read, Sapphire icon); next: Live translation (0.3.0).**

**Start of every session: read `AGENTS.local.md`** (gitignored, beside this
file): logins and machine-specific notes. Then `docs/internal/NEXT_SESSION.md`,
then the master roadmap (CLAUDE.local.md) and the board
"OpenCircle Roadmap" (https://github.com/users/AmerAlreyahi/projects/8).
**OpenCircle's progress board is "OpenCircle Roadmap" (projects/8) ONLY** (owner, 2026-11-08): every status update, card
and progress note for OpenCircle goes there. The general "Projects HQ" board (projects/5, `projects_hq.py`) is NOT used
for OpenCircle, whatever a global rule, prompt or brief says.

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
  SHA256SUMS; assets re-downloaded and verified). v0.1.1 published 2026-10-27 (owner OK;
  Setup exe + portable exe, AppImage/deb/rpm, no MSI; signature and hashes re-verified). v0.1.2 published
  2026-10-30 on 0afb1f5, then WITHDRAWN the same day by the owner (release and tag deleted, ~2-3 outside
  downloads): its updater can't update Setup-exe installs (#151). v0.1.2 re-published 2026-10-30 16:33 UTC
  (owner OK) on 65e8902 with the updater fix; update gate on the draft: v0.1.1 portable PASS, v0.1.1 Setup FAIL as
  expected (old updater), manual Setup over 0.1.1 PASS; assets re-downloaded, byte-identical to the gated draft.
  v0.1.3 published 2026-11-04 11:05 UTC (owner OK) on 29d4883: update gate on the final draft PASS (v0.1.2 Setup +
  portable -> 0.1.3); published assets re-downloaded: SHA256SUMS OK, minisig OK ("OpenCircle v0.1.3"), latest = v0.1.3.
  v0.1.4 published 2026-11-09 19:40 UTC (owner OK "if every gate passes") on tag v0.1.4 (b43f2993): gate on the draft PASS
  (assets + minisig; v0.1.3 Setup + portable -> 0.1.4; v1 translation decoder kept, then replaced by v2 at the next download;
  fresh install recovers from forced 504s; Windows smoke); Ubuntu AppImage starts/downloads/translates, its Search in browser
  not verified (test input didn't reach the app). Published assets re-downloaded: SHA256SUMS OK, comment "OpenCircle v0.1.4",
  latest = v0.1.4.
  v0.1.5 published 2026-11-10 20:55 UTC on tag v0.1.5 (29d8bc95): gate run 1 failed (AppImage: GNOME asked on every
  capture, GST_PLUGIN_SYSTEM_PATH set into the image) -> fixed, draft rebuilt; run 2 PASS (assets + minisig; v0.1.4 Setup +
  portable -> 0.1.5; What's new; Windows smoke; Ubuntu AppImage asks once). Published assets re-downloaded: SHA256SUMS OK,
  minisig OK, trusted comment "OpenCircle v0.1.5", latest = v0.1.5.
  v0.1.6 (hotfix: About page crash) published 2026-11-11 03:53 UTC on tag v0.1.6 (95618212), owner: "publish directly, it
  is simple fix": CI + release workflow green, draft assets SHA256SUMS OK + minisig OK; the update gate, the About check on
  the draft exe and the AppImage check were NOT run (both VMs off, owner's word). Published assets re-downloaded:
  byte-identical to the draft, SHA256SUMS OK, minisig OK, trusted comment "OpenCircle v0.1.6", latest = v0.1.6.
  v0.1.7 "Languages" published 2026-11-12 10:02 UTC on tag v0.1.7 (e36e2bb5), owner: "you can publish directly": clean-up, docs and notes, CI green; gate on the
  draft PASS on the Windows 10 VM (minisig verified with rsign; v0.1.6 Setup + portable -> 0.1.7 through Restart now; What's
  new; About on the draft exe; one Apps & features entry, publisher; model folders renamed; silent switches; overlay + Snip)
  and on the Ubuntu VM (AppImage on GNOME Wayland: capture asks once, Settings, Arabic, Translate, Search in browser; deb;
  KDE X11 start + shortcuts). No security scan, no QA gates #84 (owner; they go with the last 0.1.x). NOT verified: Windows 11,
  the tray menu's look, winget/Scoop from the live repositories. Published assets re-downloaded: byte-identical to the gated
  draft, SHA256SUMS OK, trusted comment "OpenCircle v0.1.7", latest = v0.1.7. Found by the gate, for 0.1.8: a plain Setup
  over a `/managed` install leaves ManagedBy empty (read as managed); Translate leaves a long paragraph's first line
  untranslated on the cropped test page (same in v0.1.6); the second reader re-reads English pages for nothing.
  v0.1.8 published 2026-11-14 04:12 UTC on tag v0.1.8 (e059432f), owner: "you can publish directly": CI green; scripted VM
  pass (w8) and release gate (w9) on the draft: UPDATE GATE PASS (v0.1.7 Setup + portable -> 0.1.8 through Restart now;
  What's new), About on the draft exe, Setup /quiet + ManagedBy cases (plain-over-managed updates again), Start menu 6/6,
  Settings keyboard 8/8 EN+AR, Translate, Snip; Ubuntu AppImage (GNOME Wayland) + deb. Published assets re-downloaded:
  byte-identical to the draft, SHA256SUMS OK, trusted comment "OpenCircle v0.1.8", latest = v0.1.8. Known issue shipped:
  first-time Arabic/Cyrillic/CJK page -> the translation-model card after the reader download doesn't start (pill
  `Msg::Offline` doesn't clear `readers`); fix first in the next version. NOT verified: Windows 11, the tray menu's look, KDE,
  rpm, winget/Scoop live repositories.
  v0.2.0 "Game translation" published 2026-11-15 06:53 UTC on tag v0.2.0 (3d533c09), owner: "publish directly": CI green;
  release gate on the draft (W22): UPDATE GATE PASS (v0.1.8 Setup + portable -> 0.2.0 through Restart now; What's new), Setup
  /quiet + ManagedBy cases, About on the draft exe, keys Alt+Shift+T/L, Sapphire icon, #163, Portuguese (models-v3), Arabic
  battle screen (no Chinese request), glossary (games only, suggester, merge on import), corpus 366/383 (no screen down),
  Translate fr game screen 2072 -> 1322 ms (-36 %); Ubuntu AppImage (GNOME Wayland) + deb. Published assets re-downloaded:
  SHA256SUMS OK, minisig OK, trusted comment "OpenCircle v0.2.0", latest = v0.2.0. Known issue shipped: on Windows/X11 the
  glossary is not picked by the game's window title (window_rect::remember_focused only called by Snip's capture, not
  app/capture.rs): fix FIRST in the next version. NOT verified: Windows 11, wlroots, KDE 6 / GNOME 48 portal shortcut path.
  v0.3.0 "AI core" published 2026-11-18 18:46 UTC on tag v0.3.0 (638ebb67), owner: "publish": CI green; gate on the first draft
  (W67) PASS (assets + minisig, v0.2.0 Setup + portable -> 0.3.0, schema 13, Setup /quiet + ManagedBy, About, Ubuntu AppImage);
  microsoft/onnxruntime returned 404, so the exact runtime files were mirrored (OpenCircle-models runtime-1) and the app tries the
  mirror first; re-gate on the final draft (W71): UPDATE GATE PASS, fresh install translates with the runtime from the mirror.
  Published assets re-downloaded: SHA256SUMS OK, minisig OK, trusted comment "OpenCircle v0.3.0", latest = v0.3.0. Known issues
  shipped (fix first in the next version): on a fresh Windows install, Copy text on an Arabic page with the PP-OCR reader copied
  nothing in W71's runs (not yet confirmed app vs test click); the About avatar still drew as a square tile at 1280x720 (W69's
  circle radius did not take effect on the CPU renderer). Live translate hidden in this version.
  v0.4.0 "OpenCircle Studio" published 2026-11-21 18:04 UTC on tag v0.4.0 (2abf6f6a), owner: "publish, that is fine" BEFORE
  the Windows VM gate (W93) finished; release changes made through `gh api` (owner's OK, guard REL-04). Published first with a
  placeholder tag (`untagged-...`) for ~1 min, then tag_name set to v0.4.0 (lesson: a publish through the API must send
  tag_name). Published assets re-downloaded: SHA256SUMS OK, minisig OK, trusted comment "OpenCircle v0.4.0", latest = v0.4.0.
  Known issues shipped: stroke commit pause on 4K / 8K pictures, zoom not animated, a failed step's placeholder vanishes silently.
  v0.5.0 "Enhancements" published 2026-11-26 16:54 UTC on tag v0.5.0 (ea2719d6, re-tagged after the W126 AppImage CLI fix);
  the owner ran the two release-edit commands. Gates on the final draft: CI green, SHA256SUMS + minisig OK, update gate PASS
  (Windows Setup + portable from 0.4.0, What's new, About v0.5.0, Setup /quiet; AppImage --version / run on Ubuntu). Published
  assets re-downloaded: byte-identical to the gated draft, SHA256SUMS OK, minisig OK ("OpenCircle v0.5.0"), latest = v0.5.0.
  Known issues shipped (fixed in 0.5.1 R-18; kept out of the notes by the owner's word): one-word in-place translate cut off
  ("To..."); the Studio view moves on pin / Preferences / right-click.
  v0.5.1 published 2026-11-27 01:10 UTC on tag v0.5.1 (29d0645a) by the lead (SDLC_OWNER_RELEASE=1 + the owner's overnight OK):
  R-15 Arabic reader (Windows ar-SA in main mode only), R-16 Studio selectable text, R-18 Studio view stays put + one-word translate
  never cut off + slider button / Picture info removed, R-19 Studio drawing on 5K/8K (5 review rounds: screen == export, no pixel
  change outside the edit, drag ~1 ms, commits 3-8 ms; Spotlight 18-37 ms, 8K Blur ~20 ms, 100-200 % zoom drag unchanged = known
  issues). Gates: CI green (audit), SHA256SUMS + minisig OK ("OpenCircle v0.5.1"), Windows update gate PASS (0.5.0 Setup + portable ->
  draft, About, Setup /quiet, winget managed + plain-over-managed after fixing the gate's fake release to always-newer 0.5.99), Linux
  VM pass + AppImage --version / run on Ubuntu. Owner tested build 2177b5b (drawing complaint -> R-19); build 29d0645 sent, not yet
  tried by him. Published assets re-downloaded: byte-identical to the gated draft, SHA256SUMS OK, minisig OK, latest = v0.5.1.
  G-01 GPU renderer not in 0.5.1 (waits for the owner's lab-gpu2 result).
- Frozen contracts (since v0.1.0): the CLI (`opencircle capture …`, `run …`,
  `quit`) and the config file format: migrate, never break. The update channel
  (GitHub release assets, SHA256SUMS.txt + .minisig, trusted comment
  "OpenCircle v<ver>") is frozen too: installed copies depend on it.

## Architecture

Layout by feature since 0.1.8 (details, rules and change recipes: `docs/ARCHITECTURE.md` "Layout"):

| Part | Role |
|---|---|
| `src/app/` | The iced app shell: App state, messages, update / view dispatch, opening, keys, monitors, windows |
| `src/features/<feature>/` | One folder per feature (circle, text_reading, translate, live_translate, ask_ai, snip, song, settings, onboarding): its logic, `app/` (its `impl App` handlers), `screens/` (its views) |
| `src/shared/kit/` | The one UI kit and the only colour tokens (`shared/theme.rs` = config-to-kit glue) |
| `src/shared/platform/` | The OS: capture core, hotkeys, sys, autostart, windows / monitor / shape / tray glue |
| `src/shared/config/` | The one config reader/validator (TOML): mod.rs types, parse, migrate, look, store (the one writer) |
| `src/shared/` (rest) | app_language, store, update, setup, history, log, paths, secrets, net, diagnostics |

`shared/` never imports a feature or the app; features meet only through each other's `mod.rs` (arch tests, with
a shrinking ratchet list).

## Commands

    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    XDG_DATA_HOME=$(mktemp -d) XDG_CONFIG_HOME=$(mktemp -d) cargo test
    cargo packager --release

Run tests with EMPTY app data/config dirs (as above): CI has no downloaded
models or config, and a machine with them can hide a failure. The local gate
(above + `cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D
warnings`) decides merges; CI runs in the background as the safety net (owner,
2026-10-28): check `gh run list -b main` in every batch of merges (a red run is
the next item to fix) and always before a release. Don't push notes while CI is
checking a fix (a new push cancels it).

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
  Translate, Live translate (Beta) icon, Song / Edit image only when on in Labs (Live too, 2026-10-27);
  Copy image = Ctrl+C, Save = Ctrl+S with a pill (#51, #52).
- Exact design behaviour (owner, 2026-10-26): copy the new design's widgets EXACTLY:
  behaviour, states, keyboard, drag/dock, styles and animations, not only the look.
  Reference pack for every UI brief: docs/internal/design-ref/ (screenshots per
  widget/state/theme, motion frame strips with timings, DESIGN_BEHAVIOR.md with
  source lines). Pass its shots and sections to the UI worker.
  Where the design is silent or buggy (DESIGN_BEHAVIOR.md section 13, e.g. no bar snap
  anchors, no Esc, no focus ring), keep today's behaviour. Owner decisions beat the
  design (e.g. Live translate is hidden behind Labs until 0.2.0 (moved 2026-10-30) (owner, 2026-10-27); no widget transparency until glass returns with the GPU renderer in 0.1.4 (moved 2026-10-30) (owner, 2026-10-27)).
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
- Instant text (owner, 2026-10-27): Windows OCR (#62, #63) moves into 0.1.x right after
  the installer: the whole screen is read right after the opening, so text is selectable
  at once; #123's read-on-touch is the stopgap until then.
- Translate targets (owner, 2026-10-27): a full page on a normal PC: up to 3 s is
  acceptable, under 1 s is the ideal. Games: most players have a dedicated GPU, so the
  GPU translation path (#26) matters for live game translation; it must never cost the
  game frames (measured with the private cutscene test, #82).
- Releases (owner, 2026-10-27, firm; withdraws the beta OK): no tag, draft, beta or
  stable release until the owner has checked every feature and explicitly says to
  publish that release.
- Screen edge (owner, 2026-10-27, firm): the old moving multi-colour edge animation
  never ships again. The edge is the design's Active Look inset glow (two colours per
  look): it fades in with the overlay, then only the design's very soft 4 s breathing
  (ripplePulse) shows the app is alive; no moving colours, no draw-back. Live and snip:
  static. Must stay near 0 % CPU while the overlay is open and still.
- v0.1.1 core (owner, 2026-10-27): Circle to Search AND one-shot translate must both
  work properly (translate fast: about 3 s for a page, first lines in about 1 s). Live
  translate is hidden behind Labs until 0.2.0. No widget transparency until glass (0.1.4, moved 2026-10-30).
- Version plan (owner, 2026-10-27): 0.1.2 new look (solid) + Feature showcase + AI chat
  with markdown; 0.1.3 GPU renderer + glass; 0.2.0 AI online (web search, sign-ins) +
  fast core; 0.3.0 live translation; 0.4.0 snip; 0.5.0 visual search + sound.
- Issues (owner, 2026-10-27, supersedes "cards are PUBLIC issues"): plan/feature cards
  live in the PRIVATE repo AmerAlreyahi/OpenCircle-planning (same board, project 8);
  the public repo keeps only real bugs and user reports; fixed bugs are closed with a
  "fixed on main, ships in vX" note.
- 0.1.2 started (owner, 2026-10-27, replaces the v0.1.1 stop point): v0.1.1 is published
  (2026-10-27); work on 0.1.2 "New look + AI chat" in the board's Order.
- Feature showcase (owner, 2026-10-27): shows ONLY features that ship today (Labs features only
  while their Labs switch is on); no "Coming soon" group. Each new feature is added to the design,
  then to the showcase list, when it is ready.
- Frontend architecture (owner, 2026-10-27): professional, scalable UI. ONE shared UI kit
  (src/ui/kit/): every kind of control (button, pill, toggle, dropdown, option list, field, keycap,
  card, toast, ...) is built once; ONE design per object, never two looks for the same kind of
  control; screens only compose kit parts and never style; colours, sizes, radii, spacing and
  motion come only from tokens; screens/pages/steps/actions come from lists (a new feature = one
  entry + one view file; recipe in docs/ARCHITECTURE.md). Rebuilding or restructuring current UI code
  for this is allowed (behaviour, frozen contracts and tests kept).
- Version scope (owner, 2026-10-27, supersedes the 0.1.2 line of "Version plan"): 0.1.2 = new UI +
  architecture: the UI restructure + the WHOLE backend cleanup of docs/internal/REPORT_backend_audit.md
  (every finding, none left for 0.1.3) + AI chat. 0.1.3 = GPU renderer + glass only; 0.2.0 = AI online
  (OpenRouter sign-in, web search) + fast core.
- Showcase placement (owner, 2026-10-27, refines "Feature showcase"): no separate showcase page, tab
  or onboarding step: a compact live preview at the top of each feature's Settings page, hideable per
  page, a still frame with reduced motion.
- Appearance (owner, 2026-10-28): no "OS mode" and no "Logo colour" rows in Settings. The theme cards
  decide: picking a card sets that theme and its light/dark mode; until a card is picked the app follows
  Windows (System). Config keys `theme` and `[appearance].logo` stay (frozen format).
- No Text panel (owner, 2026-10-28): "Copy text" and Ctrl+C copy the text straight to the clipboard
  with the Copied pill; the Text result card (review/edit, link chips, Search/Translate from it) is
  removed. Translate and Search stay on the bar.
- AI marks and loading (owner, 2026-10-28): no generic AI icons (sparkles etc.); every AI marker
  uses the OpenCircle logo. Loading = the logo's circle spinning (kit spinner; frame clock only while
  it spins; reduced motion = still logo).
- Pinned AI chat (owner, 2026-10-28): a quick question behaves as today (outside click / Esc closes);
  a follow-up pins the chat (outside clicks no longer close it; it stays docked above other apps after the
  overlay closes; a new capture asks into the SAME chat with its crop). A header pin button pins by hand
  and, when pinned, unpins and closes (the only exit; no x). Settings > AI "AI chat stays open":
  After a follow-up (default) | Always. Design approved by the owner 2026-10-28 (.claude/artifacts/design/
  newdesing/pinned-chat-demo.html), plus: one-line composer, Shift/Ctrl+Enter = new line, "Thought for" only
  when the model reasons, drawer = "AI chat stays open" + provider/model picker, a warning for safety-classifier
  models. Chat history (owner, 2026-10-28): saved on this PC, TEXT ONLY (screenshots never written; shown as
  "Screenshot not kept"), all past chats in the drawer's History, delete one / clear all, Settings "Keep chat
  history" (on by default), newest 100 kept; stored in SQLite (rusqlite bundled into the exe, owner-approved
  dependency) in opencircle.db, the app's one database for growing records (src/store/: later search/song history,
  recent captures index, translation cache), one module + migrations; config.toml stays (frozen, not in SQLite). Context sent to the model: the last 20 messages in full + one short
  summary of older ones (written by the user's own provider, refreshed every 10 messages); never the whole chat.
- Smooth on the CPU (owner, 2026-10-28): the GPU renderer stays for 0.1.4 (moved 2026-10-30). 0.1.2 must feel smooth on the
  CPU renderer, also on weak laptops and 4K/120 Hz screens: Settings pages switch instantly (the page rise
  returns with the GPU), scrolling copies the already-drawn pixels and redraws only the newly shown strip.
  In 0.1.4 the original motion returns, and a Power saver setting (Settings > General: Auto | On | Off;
  Auto = on while Windows battery saver is on) brings back this light behaviour (instant pages, scroll by
  copy, minimal motion). In 0.1.2 the light behaviour is the default for everyone.
- AI provider wording (owner, 2026-10-28): Settings > AI shows "Browser" | "AI provider" and then one
  Provider dropdown of real service names (OpenRouter, OpenAI, Anthropic, Gemini, Groq, Mistral, DeepSeek,
  xAI, Ollama, LM Studio, Custom server); never "OpenAI-like"/"OpenAI-compatible"/protocol names in the UI
  (misleading); the API protocol is chosen per provider internally; only Custom server shows a URL.
  A saved key shows masked: dots only (owner chose no last 4, 2026-10-28), e.g. ••••••••••••; no reveal
  button; the full key never appears on screen, in logs or diagnostics.
- Settings harmony (owner, 2026-10-28): on every page all controls share one column (same left edge, same
  width), every row has the same box height/padding, controls centred vertically; enforced by the kit
  setting_row and a test.
- Feature preview videos (owner, 2026-10-28, refines "Showcase placement"): the Settings previews are REAL
  recordings of each feature (about 5 s, from the real app on the test VM, clean screen, owner approves each),
  bundled as animated WebP (~600x340, 15-24 fps, ~3 MB for all), decoded frame by frame only while playing and
  freed when Settings closes; still frame with reduced motion. The OWNER makes the clips (2026-10-28); until then
  a quiet placeholder ("Preview") replaces the drawn previews. Files: assets/previews/<feature>.webp (circle, text,
  translate, ai, snip, live, song), bundled at build (a missing file = the placeholder); never loaded from the
  internet (privacy, offline). Clip spec approved by the owner: 1200x680 (2x the stage), 15-20 fps, ~1 MB each (~+7 MB).
  The Settings speed fixes (instant pages, scroll, preview redraw) are owner-approved.
- Settings window layout (owner, 2026-10-28): the design's full-width header stays, with ONE horizontal hairline
  under it across the window, 100 % identical to the vertical sidebar separator (same kit divider, colour,
  thickness, opacity); the vertical separator starts at that line.
- Protected-capture pill (owner, 2026-10-28): 0.1.2 shows "This window is protected" only where Windows reports a
  capture-blocking window (display affinity), so black wallpapers get no false pill. DRM video in a browser (black
  without a protected window) gets no pill for now; a smarter check (a black area exactly filling a video player
  while the rest of the window isn't black) is a later follow-up.
- Screen under the mouse (owner, 2026-10-28, supersedes the opening-screen part of "Multiple screens"): Ctrl+Shift+Space
  opens with the live effect (Active Look edge) and the bar on the screen the MOUSE is on; all screens still freeze and
  stay selectable. Moving the mouse to another screen moves the edge glow there (fade, design timings); the bar stays
  where it opened until dragged.
- Static edge glow in 0.1.2 (lead, 2026-10-29, recommended default while the owner was away; owner may reverse):
  the CPU renderer can't breathe near 0 % (each step ~11 ms; trimmed breath still 7-9 % of a core on the VM by
  cycle count), so the overlay's glow is static in 0.1.2 (`edge::breathes`); the 4 s breath returns with the GPU
  renderer in 0.1.4, like the other motion ("Smooth on the CPU").
- Quiet Settings (owner, 2026-10-29): no "Auto-save active"/"Saved"/"Saving" pill (changes save by themselves,
  as in most apps) and no dot on the active sidebar item; the header pill shows only when something needs the
  user (a problem to fix). The Copied pill (dot + "Copied") stays as it is.
- Chat and bar (owner, 2026-10-29): the AI chat keeps its full docked height and its width is set by its
  inner edge (double-click = design width); it never shrinks for the bar. A pinned chat's free side holds the
  bar's spots instead (the bar fits there, shorter field if needed). Six bar spots: bottom/top centre and the
  four corners. The chat's log has no scrollbar; its composer sits 8 px under the log.
- More offline languages (owner, 2026-10-30): offline translate covers every Xenova OPUS-MT model (6 layers, shared
  vocab) for the languages in our table (44 directions + 5 direct Arabic ones we convert, 2026-11-03; pinned in `mt::PAIRS`); a pair without a direct model goes
  through English (`mt::route`, two models). Detect tells Latin languages (en/es/fr/de) apart by common words.
- Other scripts in main mode (owner, 2026-10-30, "second pass"): after Windows OCR (the user's Windows language) reads
  the screen, the helper's second reader (`ocr::recognize_except`) reads the lines Windows OCR didn't, each with the
  installed non-Latin recognizer that reads it best (Arabic, Cyrillic, CJK); gently, once per opening; the helper
  isn't started without such a recognizer. Its lines replace Windows OCR's noise lines they overlap.
- Main look (owner, 2026-11-03): the owner's Custom (#0a84ff -> #5e5ce6, glow 100 %, dim 12 %, white corners, glow edge)
  becomes a built-in look ("Sapphire Glow", lead's name) and the default, first card in Liquid Prism's place; Liquid Prism's
  card is hidden (a config that has it keeps it).
- Stealth Frost (owner, 2026-10-30): no card in Settings; a config that has it ("mono") still shows it.
- Version plan (owner, 2026-10-30, supersedes "Version plan" and "Version scope" after 0.1.2; the roadmap table has
  the items): 0.1.2 (in progress, not finalised: owner checks + preview clips) -> 0.1.3 Snip and editor + colour picker,
  screen ruler, live window pin, share sheet -> 0.1.4 GPU renderer + glass + winget/Scoop/Flathub -> 0.1.5 AI online +
  faster core + actions on detected things + quick convert + advanced paste -> 0.1.6 speech to text, text to speech,
  music search, readers for the user's Windows languages -> 0.1.7 writing assist (no chat translation) -> 0.2.0 live
  translation + all-languages offline model + OCR languages + game glossary + NVIDIA pack (waits until the owner has a
  reliable approach) -> 0.3.0 visual search alone. PowerToys ideas (MIT) are rebuilt our way: no names, logos, icons.
  Snip spec (N-73): Print Screen freezes the screen; pick a window (hover outline, click) or drag a region
  with its exact size shown, or full screen, like Windows Snipping Tool; then ONLY the editing toolbar; the picked area
  can be moved and resized; long capture = select an area, the user scrolls BY HAND, OpenCircle stitches until Done;
  copy lands on the clipboard and in Win+V history like main mode's Ctrl+C (bug N-74). Don't start before the owner says.
- Updater fix now (owner, 2026-10-30, "we should fix it now"): a Setup-exe install lives in Program Files
  (per-machine MSI), so the in-place exe swap fails ("Access is denied", seen on the VM 0.1.1 -> 0.1.2): no
  Setup-exe user updates by itself. Fix first, before Snip: such installs update through the release's Setup exe
  (quiet, elevated). Installed 0.1.1/0.1.2 copies can't be fixed remotely (their updater is fixed code; channel
  frozen): they install the fixed version by hand once. Ships as the re-released v0.1.2 (owner, 2026-10-30).
- Previews hidden until recorded (owner, 2026-10-30): a feature's Settings preview (Hide/Show preview button and
  stage) is not shown while it has no bundled clip; it appears by itself when assets/previews/<clip>.webp is added
  (the owner records them after v0.1.2). Labs keeps two rows, "Snip" and "Snip editor" (owner OK).
- N-74 on Windows 11 (owner, 2026-10-30): with "Use the Print Screen key to open Snipping Tool" turned off the bug
  stays, so the cause is ours. Work runs on the Windows 10 VM; the owner checks on their Windows 11 PC when asked.
- Snip is ONE feature (owner, 2026-10-31, firm, after trying the 0.1.3-6 dev build): one flow, like iTop's Alt+A:
  the key freezes the screen (edge glow shows it is captured); hovering outlines the window under the pointer and a
  click takes it; press-and-drag draws a free area instead; Enter = full screen. EVERY entry point (Print Screen /
  the Snip shortcut, the main bar's Snip button over the already frozen screen, tray "Snip", quick action) runs this
  same flow and ends in the same soft look-coloured outline + docked Snip toolbar; the old in-place editor toolbar
  (Back / Crop / Drag chip, plain white box) is never shown for Snip. Settings shows ONE "Snip" shortcut row (no
  separate region / window / full-screen rows; their config keys stay read: hide, don't delete).
  Glow vs outline (owner, 2026-10-31): the screen-edge glow shows only while nothing is picked (it says "the screen
  is frozen"); once an area is picked, including a main-mode selection when the bar's Snip is pressed (no instant
  action ran), the outer glow disappears (else it reads as "the whole screen was captured") and the area gets the
  snip outline + handles instead of main mode's corner brackets.
  Steps board replaces "Add capture" (owner, 2026-10-31, after trying N-50): the board of several captures of the
  SAME frozen screen is pointless. Instead: "Add to steps" pins the capture into a small floating, movable board
  (windows::floating); the overlay closes so the user can go to any app/screen; each next snip is added as the next
  numbered step; reorder / delete; Copy = one image of all steps, numbered (for AI chats); a step can be copied /
  saved alone. Still Pin leaves Labs (a visible Pin button on the Snip toolbar); live pin stays in Labs "Pins".
  After a pick (owner, 2026-10-31): the default is "Both": the area stays picked with the Snip toolbar AND is
  already on the clipboard. `snip.after` default = both; a config from before 0.1.3 whose Snip was still in Labs
  (never used) moves to both; a user's own choice stays.
  No copy-and-close (owner, 2026-11-02, "wipe it"): a pick ALWAYS shows the Snip toolbar at once; the old "Copy"
  route (copy, close, toast "Copied: paste it into your AI chat · Snip") is removed, for every user: a config with
  `snip.after = "copy"` migrates (with a backup) to the new behaviour. Settings: "After a capture" becomes one switch
  "Copy when picked" (on by default; off = toolbar only, the old "editor").
  Window hover (owner, 2026-10-31, compared with iTop): hovering outlines the WHOLE top-level window under the
  pointer by default (its visible frame, like iTop); a part of it (element) only after the wheel goes down; every
  normal window counts, OpenCircle's own Settings / chat / pins too (only the snip overlay itself is skipped).
  Own floating windows out of captures (owner, 2026-11-03, option A for 0.1.3): OpenCircle's floating widgets (Steps
  strip, pins, pills, update card, chat) are never in any capture and not Snip targets. LATER (keep the idea): pickable
  when the user deliberately hovers and clicks one in Snip.
  Editor feel (owner, 2026-10-31): no dashed selection box around any annotation (shapes, arrows, callouts, blur):
  a selected one shows only its own small handles (e.g. an arrow's ends). Blur: no blur assistant (no suggested
  areas); only the line help (stays on its line unless the pointer clearly moves down); brush size AUTO from the
  text line under the stroke start (fallback: about one line), or MANUAL from the Blur options (with "Auto").
  A tool's first click turns it on; a click on the active tool opens its options (outline on/off, size, ...);
  shapes, arrows, callouts default to NO outer outline. Ctrl+drag moves any annotation with any tool active.
  Moving (owner, 2026-10-31, SUPERSEDES "Editor moving" 2026-10-24): an annotation moves ONLY with Ctrl+drag (any
  tool) or with the Select tool; with a drawing tool a plain drag always draws, even over an annotation.
  Remember (owner, 2026-10-31): the editor keeps the user's last colour, sizes, per-tool options (outline, fill,
  blur size/Auto) and last tool across sessions.
  Blur look (owner, 2026-10-31, SUPERSEDES "Softer blur" 2026-10-25): a fine pixel mosaic like iTop's but softer and
  with smaller cells (more pixels), cell edges slightly softened; NO colour tint or noise (only the area's own
  colours averaged); a flat rectangular stroke along the text line, as tall as the line (not a round blob); a
  pixel-size control in its options; text stays unreadable. Redact stays solid.
  Secure blur approved (owner, 2026-11-02, after the real-text sheet): coarse blocks (line tall, 2.5 lines wide)
  drawn as fine cells, one colour pair per stroke, secret OS-random offsets; pixel size 4 / 6 (default) / 9.
  Live pin border (owner, 2026-11-02): Windows 10's yellow capture border is accepted; on Windows 11 it is turned
  off (IsBorderRequired = false).
  0.1.3 gate on BOTH VMs (owner, 2026-11-02): the Windows 10 VM and the Ubuntu VM (Linux testing resumes for this
  gate).
  Grouped Snip toolbar (owner approved the design, 2026-11-02; canvas v27, SNIP_TOOLBAR_SPEC.md): one bar, always
  the same: 6-dot grip (the drag handle) · size chip · Select · Shapes · Draw · Text · Hide · Colour · Undo · More ·
  Copy · Save; NO close (x) button (Esc closes); Copy and Save icon-only, Copy stays the accent button (owner,
  2026-11-02); a group shows its last-used tool; a click on the ACTIVE button opens one panel (the group's tools +
  that tool's options); Fill greyed for line/arrow; Undo's second click = Undo/Redo panel. The Edit fold is not used.
  Drags (owner OK, 2026-11-02; canvas v29 legend): the grip moves the toolbar; the size chip (with a picture icon)
  drags the picture out: once the thumbnail leaves the area the frozen screen fades away, the drop lands in the real
  app, and Snip closes after the drop; a drag inside the area moves the area.
  Steps board simplified (owner, 2026-11-02): a slim strip of numbered thumbnails at a screen edge, thin border, no
  extra space; Copy all copies at once (stacked, numbered) with the Copied pill; hover x deletes a step, drag
  reorders; x on the strip clears it with Undo in the pill; at 12 the Add button says full. No captions, no preview
  dialog, no collapse, no grid option. Simplified design approved (owner, 2026-11-02; canvas v28, 104 px wide strip;
  click a step = copy it, double-click = edit, no per-step menu).
  Fast scrolling (owner OK, 2026-11-03): long capture keeps up: every screen frame while it runs (none idle), the
  wheel paced to what the stitcher has seen (only during the capture), and a note + "Scroll back" for jumps that
  can't be paced (scrollbar drag, Page Down, End). Window effects (Windhawk "Translucent Windows") must not touch it:
  no window of ours over the area while capturing.
  Long capture look (owner, 2026-10-31, "super bad" next to iTop): the live preview is SHARP (good downscale, never
  blurry): the bare image beside the area, no widget around it (no card, frame or surface). Like iTop (owner,
  2026-11-02, 3 shots): it ALWAYS shows the whole page so far, scaled to fit ~80 % of the free height, so it starts at
  a max width (~280 px, clearly narrower than iTop's ~400) and gets narrower as the page grows; top-aligned, right next to the area;
  the
  normal Snip toolbar stays (close, pin, share, Copy, Save) and Copy / Save finish the long capture (no separate
  Cancel / Done bar over the page); a short hint "Scroll to capture the page" beside the area.
- Preview variants (owner, 2026-11-03): a feature may have several clips; its Settings preview shows the one that
  matches the user's current choice (e.g. AI: Browser -> the browser clip, AI provider -> the in-app chat clip), and
  switches when the choice changes. Files: assets/previews/<feature>-<variant>.webp (plain <feature>.webp = one clip).
- Preview clips bundled (owner, 2026-11-03): the owner's edited clips (docs/internal/preview-edit/outputs/final) map
  Look = circle-1..3 in turn, Text = text, Translate = translate, AI = ai-chat / ai-browser by provider (the Google AI
  Mode clip stays until the owner replaces it), Snip = snip-blur, snip-comment, snip-long in sequence; clips with
  unresolved photo rights are held until cleared; credits on About + docs/NOTICE.md. Clips go in exactly as delivered (no
  re-encode/edit; sha256 = outputs/final/qa.json). Photos must be CC0 (owner's rule): lotus and kingfisher cleared
  (WordPress, CC0, 2026-11-03); COFFEE (Tim Mossholder, Unsplash License) accepted by the owner 2026-11-03 as a
  media exception (the app's code stays MIT). The translate
  clip and its poster are CC BY-SA 4.0, an explicit media exception to the app's MIT licence. About + NOTICE carry the
  Google non-affiliation line verbatim.
- Language picker (owner, 2026-11-04): design variant A approved (canvas board "Translate · language picker",
  PICKER_SPEC.md): fixed pill at top centre, status tag beside it, a one-column list hanging from each chip, search /
  Detect / Recent / A-Z, quiet needs-AI circle; "needs AI" is a one-line note, never a card, the pill never moves.
- Previews online on demand (owner, 2026-11-04, B; SUPERSEDES "bundled ... never loaded from the internet"): the exe bundles
  only a sharp still per clip; a "Play preview" click downloads that page's full-HD clips once from a GitHub release of
  the owner's (sha256 pinned, like the models) and keeps them on the PC; nothing downloads before that click. A page's
  clips all play in turn and loop; the stage scales with the Settings window. Sidebar "AI" is named "Ask AI" (owner).
  Format (owner, 2026-11-04): 1080p AV1 (~0.4-1.5 MB per 5 s clip, re-encoded from the owner's raw takes) played by a small
  built-in decoder (dav1d's Rust port), Windows and Linux alike.
- Copy = button AND Ctrl+C (owner, 2026-11-03, universal): wherever the app has a Copy button, Ctrl+C does the same
  (long capture included: Ctrl+C finishes and copies); Save pairs with Ctrl+S the same way. A text field with a
  selection still copies its text.
- Snip look + size (owner, 2026-10-30, on the design's picked frame): the area's handles and its outline are
  SOFTER (smaller, rounder, quieter handles; a thinner, lower-contrast border than the design's v24 frame). Copy
  and Save keep the capture's full size by default: `snip.max_edge` default 0 (Original) instead of 1568 (a value
  already in a user's config stays; Settings still offers 1024 / 1568 / 2048).
  Original for everyone (owner, 2026-11-02, "insure the snip is original by default"): a file still on the old
  default 1568 moves to Original once (schema 7, backup); other chosen sizes stay.
  Preview videos (owner, 2026-11-02): Codex records REAL full-screen videos on the Windows VM (edge glow visible),
  one complete take per feature, on useful legal content (Blender's "Sintel", CC BY, for video text and subtitles
  translation; public-domain images; Wikipedia); the owner reviews the full takes before trimming/motion polish.
  Owner records the clips (owner, 2026-11-03, SUPERSEDES the Codex recordings): every Codex take was rejected
  (old snip tool, unclear text sample, translate not showing where it runs, subtitles not detected); agents record no
  preview clips; the owner records them himself with the dev exe and provides the samples.
  Exception (owner, 2026-11-03): the translate preview may be made by an agent (Claude or Codex, whoever is free) as 5
  scenario videos, one of them on an ORIGINAL generated game screen in the style of a monster/character banner (no
  real game's art, names, logos or UI copied); the owner's guide comes first; ONE video is shown to the owner first,
  then he decides to continue with the agent or do it himself. Uses the fixed translate build.
  Editing rule (owner, 2026-11-04): when a clip is cropped or zoomed in the edit, the live effect (the Active Look edge
  glow, Sapphire Glow) is re-created at the edited frame's edge so it stays visible, only while the app is really
  active (as preview-edit/ACTIVE_LOOK.md does). Translate preview #1 framing: YouTube theater mode (no sidebar of other
  people's thumbnails). Glow may be an artificial, identical re-creation to deliver the idea (owner, 2026-11-04). Translate
  preview set: the YouTube subtitle clip + 3-4 game clips made from the owner's original AI game images, animated as a
  running game (motion on the art only), translated by the REAL app on the VM; raw takes approved before edits.
  The snip area's outline, handles and hover outline take their colours from the chosen Active Look (Settings >
  Appearance "Active look screen borders": Liquid Prism, Spectrum, Aurora Borealis, ..., Custom), the same source
  as the screen edge and main mode's selection line (one place, no snip-only colours).
  Snip on several screens (lead default 2026-10-30, owner didn't object): Enter = the pointer's screen,
  Shift+Enter = all screens as one picture; an area may cross screens; the toolbar docks on the screen with the
  larger part of the area, never across a bezel.
- Overnight 2026-10-30 (owner asleep, "apply the recommended"): the Snip capture-flow design (canvas v24,
  .claude/artifacts/design/SNIP_FLOW_SPEC.md) is approved with the designer's choices: Ctrl+arrows resize (Shift =
  10 px), the snip toolbar has no Back / Crop / Drag chip (handles crop; drag-out by dragging the picture), Daylight
  Copy button contrast raised to >= 4.5:1. Continue 0.1.3 item by item; verified items wait in "Needs your check";
  no release.
- Update gate (owner, 2026-10-30, "insure this won't happen again"): no release is offered to the owner for
  "publish" until the previous published version, installed with the Setup exe (Program Files, normal user) AND as
  the portable exe, has updated itself to the DRAFT on the Windows VM (docs/RELEASING.md step 9, local update server +
  OPENCIRCLE_TEST_UPDATE_URL). A unit test alone never proves an install path; every way users install is tested.
- 0.1.3 go (owner, 2026-10-30): the plan in roadmap 2A is approved as is; Edit becomes Snip starts now (first
  0.1.3 item, after the updater fix); M2M100 418M is not measured.
- Edit becomes Snip (owner, 2026-10-30; in 0.1.2, NOT started until the owner says): every "Edit" / "Edit image"
  entry point (bar button, tooltip, Labs row, shortcut name, docs) gets a new Snip icon and Snip text, because Snip
  contains the editor. Icon and text only; behaviour unchanged; the Snipping Tool flow stays 0.1.3.
- Hide, don't delete (owner, 2026-10-29): an option for a feature that hasn't arrived (e.g. "Bars and pills" glass
  styles until 0.1.4; "Screen edge for" Live/Snip until they are on in Labs) is hidden, its code and config key
  kept, and shown when the feature really ships. A row with a single choice is not shown.
- Tray menu (owner, 2026-10-27): calm and native-quality, not "AI-looking": Circle to Search,
  Translate screen, Labs rows only when on, Settings, Quit; no AI row, no header/status. Redesigned by
  Codex in the design (widgets/tray-menu.js), then ported.
- v0.1.1 release OK (owner, 2026-10-27): publish v0.1.1 after the last fixes (Linux links/launcher/
  takeover #146, Linux I-beam, MSI dropped) are merged and CI is green. Windows assets: the Setup exe
  (with /quiet) + the portable exe; no separate MSI download.
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
- Translate default target (owner, 2026-10-23; SUPERSEDED 2026-11-10 by "Language in onboarding"): Arabic, changeable in settings.
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
  Exception (owner, 2026-11-04): Antigravity (any of its models; images by Nano Banana) may generate marketing images
  (banners); it writes no code and decides nothing.
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

- Names follow features (owner, 2026-11-03): module, file, type, function and test names use the feature's
  current name as users see it (Snip, Steps strip, Long capture, Live translate, Copied pill, ...); when a feature is
  renamed or reshaped, its code is renamed in the same change (no leftover old names like "edit" for Snip or "board"
  for the Steps strip). Frozen contracts keep their spelling (CLI, config keys, update channel): map them at the edge.

- Snip shortcut (owner, 2026-11-03): default Alt+A (a common screenshot key; Print Screen stays Windows'); one Snip
  shortcut only: the old window / full-screen snip entries are never shown nor bound (no invisible shortcuts); an older
  file's default Print Screen moves to Alt+A once (schema 8), a user's own key stays.
- Translate models (owner, 2026-11-03): direct Arabic pairs are converted by us from Helsinki-NLP's own OPUS-MT models
  and hosted free as GitHub release assets in the owner's repo OpenCircle-models (sha256 pinned in the app); a small
  open-source language detector for short lines only if it stays within ~5 MB.
- 0.1.3 draft tonight (owner, 2026-11-03, "okay"): the lead may tag v0.1.3 and let the workflow build the DRAFT tonight,
  after the clean-up, docs, security review and both VM passes, and run the update gate on it; publishing still waits
  for the owner's explicit "publish" after his morning check.
- Config schema 9 (0.1.3 clean-up, 2026-11-03): [labs] snip/editor and the hidden snip window/full-screen entries are read
  but no longer written; "Auto action" / "Search with" rows and auto-search removed (nothing is sent until the user picks
  an action); selection.after / search_with still read and written but ignored (drop in a later schema).
- Release now, games next (owner, 2026-11-04, A): v0.1.3 ships with today's state (limits in the notes); a version of its own
  for GAME translation accuracy comes next (0.1.4 "Game translation": game mode, fit into buttons, names, game vocabulary,
  pt/ru/ja readers + models, scored on the 13-image corpus, target >= 90 % per image with a model); the later versions
  shift by one (GPU renderer + glass -> 0.1.5, ...).
- Glass library (owner, 2026-11-04): the glass / refraction effect is its own project: a separate open-source Rust library
  (own repo and name, not "Liquid Glass"; MIT OR Apache-2.0; clean-room; wgpu + iced integration, demo, docs); OpenCircle
  only depends on it.
- Version plan (owner, 2026-11-08, SUPERSEDES the GPU/glass part of the two entries above): the GPU renderer gets a version
  of its own, without glass; the glass library is built after every other 0.1.x version, right before 0.2.0. Order: 0.1.4
  Game translation (with the user-reported fixes first) -> 0.1.5 GPU renderer (motion back, Power saver, store listings)
  -> 0.1.6 AI online + faster core + smart selection -> 0.1.7 Voice and sound -> 0.1.8 Writing assist -> 0.1.9 Glass
  (the library, then glass in OpenCircle) -> 0.2.0 Live translation -> 0.3.0 Visual search. SUPERSEDED 2026-11-09 by
  "Version plan (owner, 2026-11-09)" below.
- Clean before every release (owner, 2026-11-03, from 0.1.3 on): the LAST step before a release, then the security
  check: remove every dead or old config key, code path, test, asset, script and tool we no longer use AND won't use
  (things waiting for a planned feature, e.g. glass in 0.1.4, stay). Docs and assets too (owner, 2026-11-03): public
  docs updated to what ships (no screenshot or text of an old tool), internal docs/reports/briefs/samples, rejected
  preview takes, old designs, spikes and dev builds that are outdated or abandoned are removed; the owner sees the
  list first, and gitignored files (no git history) are archived outside the repo, not deleted. Old config keys: still read without a warning
  so older files load, dropped on the next save (migration with a backup); frozen contracts keep working.
- Learn from open source (owner, 2026-11-03): study solid open-source tools (e.g. ShareX, Flameshot) for how
  their logic works, then build it our way (Rust, iced, our kit). GPL code is read for the approach only, never
  copied or translated (we are MIT); MIT/Apache code may be reused with its notice.

- No AI credit in Git (owner, 2026-11-04, never override; guard added 2026-11-08): no `Co-Authored-By`, "Generated with" or
  any AI name on commits, PRs or tags, from any agent. Guards: Claude Code's attribution setting is off; `.githooks/commit-msg`
  strips such lines and `.githooks/pre-push` refuses new commits that carry them (after a fresh clone run
  `git config core.hooksPath .githooks`). Commits already in history (701 on main, v0.1.3 tag included) stay as they are
  (owner, 2026-11-08: "leave it for now"); only new commits must be clean.
- Copy only on request (owner, 2026-11-08, SUPERSEDES "Copy when picked on by default" of 2026-11-02): a Snip pick no longer
  copies; the clipboard is written only by Ctrl+C or the Copy button (so no double entry in Win+V). Settings keeps its
  "Copy when picked" switch, now off by default (hide, don't delete); an older file's default-on moves to off once (schema 10,
  backup), a value the user changed on purpose stays.
- The circled part goes to the AI (owner, 2026-11-08): Ask AI sends the selected crop, never the text alone, in the browser
  provider AND in the in-app AI chat; check both paths.
- App languages + Arabic RTL (owner, 2026-11-08; version moved 2026-11-09 to "Arabic UI + live groundwork"): the app's UI
  becomes multi-language, the language chosen in onboarding (and changeable in Settings); Arabic first, fully right-to-left
  (mirrored layout). ON HOLD until the owner says to start (he checks whether it fits today). The existing Arabic/RTL
  toolbar code (Snip toolbar, Steps strip, picker, ruler, long-capture hint) is KEPT for it (audit item 5.1).
- Lighter translation models (owner, 2026-11-08, approved): the OPUS-MT decoders are re-exported by us with the output layer
  outside the decoder's two If branches (W6 measured 501 -> 256 MB and 9.2 -> 8.3 ms/step for ONE pair; W14 measured
  -32..-34 % on five pairs; identical output) and hosted as a new release of
  OpenCircle-models (models-v2, sha256 pinned in `mt::PAIRS`); an old decoder already on a user's PC is replaced, never kept
  beside the new one.
- Labs search history removed (owner, 2026-11-08, "del"): it stored searches nobody could see. The key is still read and
  dropped on save (backup); an existing search_history.json is deleted.
- No duplicate code (owner, 2026-11-08): logic needed in two places or more is built once as shared, reusable code and
  called from each place; never the same thing twice (recorded in the owner's engineering rules too).
- Edge assist (owner, 2026-11-08): a simple, gentle magnetic edge when dragging a Snip area by hand: it helps the user land on
  window/element edges, never aggressive; no mockup needed.

- Version plan (owner, 2026-11-09, SUPERSEDES every earlier "Version plan" entry): Game translation (0.2.0) -> Arabic UI +
  live groundwork (0.3.0) -> Live translation (0.4.0) -> AI online (0.5.0) -> Reach (0.6.0) -> Voice and sound (0.7.0) ->
  Writing assist (0.8.0) -> GPU renderer + cosmetics (0.9.0) -> later: Visual search, then Glass.
- Next release + Linux version (owner, 2026-11-09, later the same day; SUPERSEDES the order above where it differs): the
  next release is v0.1.4 (owner: "keep 014"), made directly from main once today's fixes are settled (no rollback: the widgets-between-
  screens work #125 is fixed, not reverted). Linux fixes get a version of their own. Game translation shifts to after them.
  Order: next release -> Linux fixes -> Game translation -> Arabic UI + live groundwork -> the rest as listed above.
- v0.1.4 publish OK (owner, 2026-11-09, going to sleep): the lead may PUBLISH v0.1.4 overnight only if every gate passes
  (CI green incl. audit, security review clean, clean-up check, v0.1.3 Setup + portable update themselves to the draft on
  the Windows VM, a fresh install's model download recovers from forced server errors); then re-download the assets and
  verify SHA256SUMS + minisig. Any failure = it stays a draft for the owner.
- General models project (owner, 2026-11-09): AFTER v0.1.4, build a general, professional models project (not only for
  OpenCircle), presented like research: reproducible measurements, a technical report, model cards, scripts. The owner
  uploads to Hugging Face himself (user AmerAlreyahi; one HF repo per pair, a full drop-in folder: our decoder + the
  unchanged encoder, tokenizer and config, grouped in a Collection; name "OptimaMT" (owner, 2026-11-09), one setting in the
  scripts, the owner picks the final name). OpenCircle-models' existing releases stay forever (installed copies pin their URLs);
  the app moves to the new source only in a later version, with new pins. Measured numbers only, no novelty claims.
  Owner-only authorship (owner, 2026-11-09, firm, "never ever"): agents NEVER commit, push, upload or publish anything to the
  models project (GitHub or Hugging Face). Agents only prepare files in a local folder outside any git repo; the owner
  commits and uploads them himself under his own name. The files must carry no trace of agent work: no AI/agent names, no
  "generated with", no worker or brief names, no tool chatter in text, comments or metadata (images, notebooks, PDFs).
  HF upload (owner, 2026-11-09, refines the above): the owner put a write token in the gitignored .env (HF_TOKEN). The lead
  may upload with it, as the owner (commits show AmerAlreyahi only; clean commit messages, no AI names): overnight ONE pair
  as a PRIVATE repo in the OptimaMT organization as a test; everything public only after the owner's morning OK. GitHub
  repo github.com/AmerAlreyahi/OptimaMT was created empty (owner: "do it"); the owner pushes its first commit himself.
  Publish OK (owner, 2026-11-09): the lead may make the OptimaMT Hugging Face repos public (and test them) on one condition:
  no Claude/AI signature, co-author or contribution anywhere, ever.
  GitHub too (owner, 2026-11-09, "push it" / "you can commit and push"): the lead may commit and push OptimaMT updates as
  the owner (name AmerAlreyahi, his email), never with a Co-Authored-By, "Generated with" or any AI mention; workers still
  only edit local files.
  Visuals (owner, 2026-11-09): graphs and diagrams are included; data charts are drawn by a script from the measured data
  (reproducible); only illustrative art (cover, concept picture) may come from an image model (GPT or Gemini/Antigravity),
  never a chart or a number.
- Version numbers (owner, 2026-11-09): each feature release bumps the middle number. Bugs found during a version are fixed
  on main and ship with the next release (its notes list features and fixes). The last digit stays free (no planned use).
  The board's Phase field uses the NAMES; a number is given only at release. The update gate stays strict: a broken updater
  is the one bug users cannot wait out.
- Security review timing (owner, 2026-11-10, SUPERSEDES "every release passes the security scan" and the security step of
  "Clean before every release"): the full security review/scan runs only in the LAST 0.x release before each big version
  (0.2.0, 0.3.0, ...), not with the big version itself: e.g. if v0.1.6 is the last before 0.2.0, v0.1.6 gets the scan. Other
  releases: clean-up pass, gate, CI, update gate as before. The QA gates (#84) go with that same release.
- Two translation tracks (owner, 2026-11-09): normal translation = OPUS-MT base + models-v2 decoders on the CPU, unchanged.
  Live translation (story games: Genshin, HSR, WuWa and similar) = its own model on the GPU, chosen by the 0.2.0 spike;
  target >= 70 % en->ar on the 13-screen corpus plus a fixed ~200-line set scored with COMET (pass line fixed before
  testing).
- GPU rule for live translation (owner, 2026-11-09; simple, no FPS measuring): at start, Vulkan device type + free VRAM:
  a discrete GPU with free VRAM >= model + ~1 GB -> GPU; otherwise (integrated, low VRAM) -> the CPU path with today's
  models. While playing: each translation burst is timed; ~2x slower than its baseline several times in a row -> back off to
  the CPU path. Setting "GPU for live translation: Auto / Off". Proved once on test PCs with the cutscene test (#82): the game
  holds its 120 and 60 fps caps, never below 60. Never hook, inject into or read the game process (anti-cheat).
- OCR (owner, 2026-11-09): one PP-OCR pipeline on Windows and Linux, on the CPU. The slim pipeline (#61) replaces oar-ocr
  FIRST; all game tuning happens on it. Windows.Media.Ocr stays for main mode only.
- models-v2 facts (owner, 2026-11-09; correct them wherever written): each If branch of the merged decoder held
  Transpose -> DequantizeLinear(model.shared.weight, int8); with session.disable_quant_qdq=1 ORT folds it once PER BRANCH;
  v2 moves it before the If. It is NOT MatMulInteger pre-packing. 501 -> 256 MB and 9.2 -> 8.3 ms/step are W6's numbers
  for ONE pair. The vocab size differs per pair. No novelty claims anywhere.
- Rejected for now (owner, 2026-11-09): Bergamot and CTranslate2 for normal translation (a second runtime); M2M100 (not
  measured); NLLB (CC BY-NC); TranslateGemma 4B (4-6 GB VRAM); OneOCR as default (Microsoft's files, reverse-engineered
  API, Windows 11 only); measuring game FPS (cost and risk for little gain); the CUDA pack #27 (1.3-2 GB; replaced by the
  Vulkan path).
- Model downloads never fail on a server hiccup (owner, 2026-11-09, "never ever again", after a fresh install failed on a
  GitHub 504): every first-use download retries server errors and dropped connections with backoff and resumes partial
  files; the user sees a plain message, never only a raw status code.

- Windows and Linux together (owner, 2026-11-09, always): every fix and every new feature is designed for Windows AND Linux
  (X11 + Wayland) at the same time, with the best-practice solution from the start, never a Windows-only path patched for
  Linux later. Example: the browser hand-off uses a local one-time web server (127.0.0.1, random port, secret path, served
  once) on every OS, instead of a temp file that sandboxed Linux browsers can't read (#156).
- Own windows in captures (owner, 2026-11-10, "yes"): OpenCircle's Settings window stays capturable on Windows AND Linux (a
  normal window, per the 2026-10-31 rule); only the floating widgets stay out (2026-11-03). On GNOME Wayland a pin and the
  Steps strip may open on top of each other (GNOME places windows): a known limit, no change (#144). KDE: the owner provides a
  KDE machine later (#148).
- Linux fixes version (owner, 2026-11-09): after OptimaMT is published, the Linux fixes + optimisation work starts (#156, #157,
  #158, planning #142-#145, #147, Wayland notes), each designed for both OSes per the rule above.

- Update experience (owner, 2026-11-10, design approved with every recommendation; W32 report
  docs/internal/reports-0.1.4/w32-update-experience.md, canvas board "Update"): download in the background as today; when
  ready a pill above the tray ("OpenCircle <v> is ready · Installs when you restart OpenCircle", Later / Restart now) that
  waits while the user is away and fades after 30 s, leaving an accent dot on the tray icon, a "Restart to update (<v>)" first
  tray-menu row and the ready status in Settings > General. Installs on Restart now, on quit / log-off, or after the session
  has been locked >= 1 min; Setup installs only on the button (shield, one UAC prompt, never from nowhere); deb/rpm: the same
  pill with "Get it". Visible relaunch: an "Updating OpenCircle… back in a few seconds" pill (spinning logo) from the old
  process to the new one, then What's new. What's new is always shown after a button restart (no setting): summary + at most 6
  Highlights rendered (bold, code as key caps, short link labels), Fixes / Upgrade notes folded, "Full release notes" link,
  Done; after a quit/lock install a small "Updated to <v> · What's new" pill once instead. No native OS notifications. The
  update widgets (ready pill, Updating pill, What's new) are capturable (exception to the 2026-11-03 rule). Frozen: the update
  channel.

- Language in onboarding (owner, 2026-11-10, for the next release): onboarding asks "your language" first; it sets the
  translate target (and the offline model to fetch) and, from the multi-language release on, the app language. No Arabic
  default: before the user picks, the default follows the OS language. A config that already has a target keeps it.
- Snip can capture OpenCircle's own widgets (owner, 2026-11-10, next release): in Snip, a deliberate hover + click on an
  OpenCircle floating widget (pill, Steps strip, pin, chat, update widgets) picks it like any window; the widgets still never
  appear uninvited in other captures (the 2026-11-03 "LATER" note, now scheduled).
- Edge glow follows the last input (owner, 2026-11-10, nice to have, not mandatory): the screen that gets the Active Look edge
  is the one of the last pointer input, mouse OR touch/pen (today: the mouse's screen only).
- Next releases (owner, 2026-11-10): v0.1.5 = Linux fixes + the update experience + language in onboarding (+ Snip captures our
  widgets; edge glow by last input if it fits); v0.1.6 = multi-language app UI including Arabic (RTL) + the live border effect
  (the Active Look edge that shows the app is active, with its soft breathing back, near 0 % CPU) + winget/Scoop + KDE
  shortcuts if the machine is there + leftovers; it is the last 0.1.x, so it carries the security review and QA gates (#84).
  GPU translation runtime and the new OCR pipeline go to Live translation / 0.2.0. Then Game translation = v0.2.0 (owner, 2026-11-10).

- Snip the translated page (owner, 2026-11-10, v0.1.5, "super great"): with a one-shot translation on screen, Snip (bar
  button, Alt+A, tray) captures what the user sees: the frozen screen WITH the translation drawn on it, then the normal Snip
  flow (pick, annotate, Copy / Save / drag, Steps). A small Snip button sits in the translate pill right after the language
  chips (owner, 2026-11-10, option 1; the bottom bar stays hidden during translation).
- Wayland long capture must show it is working (owner, 2026-11-10, rejects the silent flow): while it runs the user clearly
  sees that OpenCircle is capturing, and finishes with Copy / Save / drag exactly like Windows, without having to press the
  shortcut again to find the result.
  Approach (owner, 2026-11-10, option A, modelled on wayscrollshot): a live preview strip beside the area + Copy / Save / Pause /
  Cancel shown the whole time, the user scrolls by hand (like Windows). KDE / wlroots: an always-on-top layer-shell surface;
  GNOME: a small OpenCircle GNOME Shell extension (installed once, one logout). Where neither is available (e.g. GNOME 42
  without the extension, or the user declines), fall back to a floating panel + tray "capturing" state (lead default).
  REFINED (owner, 2026-11-10, after W39's VM research): no extension. The preview strip + Copy / Save / Done / Cancel is an
  XWayland pop-up (override-redirect) placed beside the area and kept on top (proved on GNOME 42); the tray "capturing" state
  (record dot + live label + Finish / Copy / Save / Cancel menu) is the extra sign and the fallback without XWayland. The
  pop-up follows the app's theming exactly (kit tokens, theme, Active Look), like every widget. Ships in v0.1.5 with the
  Linux fixes. A native layer-shell surface / GNOME extension only if ever needed later (v0.1.6+).
- Both OSes from the first design (owner, 2026-11-10, "this mistake should never happen in future"; strengthens "Windows and
  Linux together"): before designing ANY feature, research how established tools do it on Windows AND Linux (X11, GNOME
  Wayland, KDE, wlroots) and what the platform allows, and design one approach that fits both; a brief must name that research.
  Long capture was designed for Windows first and broke on Wayland: never again.
- Translate preview order (owner, 2026-11-10): the Translate page's first clip is the normal web-page translation (Preview2.1,
  French Wikipedia in place, re-encoded from the source to 1080p AV1), aimed at non-gamers; then the game clips translate-2,
  translate-3; translate-4 is removed from the list. The new clip is added to the archived OpenCircle-models previews-1 release
  (briefly unarchived, owner OK).
- macOS (owner, 2026-11-10): the LAST item on the board (Later). Distribution needs no payment (ad-hoc signed, users allow it
  once in Privacy & Security), but testing needs the owner's own Mac; start only when the owner says.

- Version plan (owner, 2026-11-10, SUPERSEDES every earlier "Version plan" entry; numbers projected, given only at release):
  0.1.5 Linux fixes -> 0.1.6 Languages + live border (+ Copy text / Ask AI / Translate under the Snip toolbar's More; last
  0.1.x: security review + QA gates) -> 0.2.0 Game translation (+ the live-translation research: model, GPU inference path,
  and a MEASUREMENT of what the live text layer costs on the CPU renderer during a game, cutscene test #82) -> 0.3.0 Live
  translation (GPU inference and CPU path together from the start, never CPU first then GPU) -> 0.4.0 AI online -> 0.5.0
  Snip Studio -> 0.6.0 Reach -> 0.7.0 Voice and sound -> 0.8.0 Writing assist -> 0.9.0 GPU renderer + cosmetics -> later
  Visual search, then Glass -> last macOS. If the 0.2.0 measurement shows the live layer costs game frames on the CPU
  renderer, the GPU renderer moves to right before Live translation (owner: build Live once, on the best foundation).
  Two different GPU items: the GPU for the translation MODEL (inference) is part of Live translation; the GPU RENDERER draws
  the app's windows/animations (cosmetics: later).
- Snip Studio (owner, 2026-11-10, version 0.5.0 above): the quick widget stays as today; an optional separate Studio WINDOW
  (never inside Settings): big canvas + zoom, the full editor (one editor shown two ways, shared kit and tools), Recent
  captures, the Steps list with export, Ask AI / Copy text / Translate on the picture, edit text in the image (reuses the
  translate erase-and-redraw), cut out. Entry: "Open in Studio" under the Snip toolbar's More, double-click a step on the
  Steps strip, tray "Studio", optional Settings "After a capture: stay in the widget (default) / open the Studio". Steps
  export (Markdown / HTML / images folder) lives on the Steps strip next to Copy all. Design round + owner OK first.
- AI errors (owner, 2026-11-10, v0.1.5): provider errors show as a calm card: a short plain headline, the provider's own
  message small and grey (two lines, "Show details"), one fix button (Choose another model / Retry); red only for the icon.

- Hotfix v0.1.6 (owner, 2026-11-11, "do it"): v0.1.5 closes when Settings > About opens (the release build bundled the
  GitHub avatar, now a JPEG, under a PNG name). The fix ships alone as v0.1.6 with the normal update gate and no security
  review; "Languages + live border" and everything planned as 0.1.6 above moves to v0.1.7 (still the last 0.1.x: it carries
  the security review and QA gates). Release gate from now on: open Settings > About on the DRAFT exe (only release
  builds bundle the downloaded avatar). A file fetched at build time is never decoded with a panic.

- Arabic and mixed-language text reading (owner, 2026-11-11, planning #169, in v0.1.7 before the Arabic UI): reading Arabic
  off the screen in main mode is too slow and misses lines and common Arabic fonts; a frame with two or more languages must
  read each block in its own language. No fix may cost performance: the best balance of reliability and speed, proved
  before -> after on a fixed measurement set (Arabic, mixed, English control). Measure and diagnose first, then fix.

- Pinned AI chat hand-over (owner, 2026-11-11, planning #170; refines "Pinned AI chat"): a pinned chat must look like a native
  pinned window from the beginning: when the overlay closes (Esc) it never flashes, moves or redraws. Owner's suggestion:
  when the follow-up (second question) is sent, the overlay closes by itself, no Esc. Design sheet and owner OK first (#171).

- v0.1.7 design round approved (owner, 2026-11-11, "ok all": every recommended default; sheets and specs in
  .claude/artifacts/design/: SNIP_MORE_SPEC.md, READING_TEXT_SPEC.md, CHAT_HANDOVER_SPEC.md, render/out/{more,reading,
  chat-handover}/). Snip More: Copy text / Ask AI / Translate on the picked area; Ask AI gets the annotated picture, Copy
  text and Translate read the clean capture and skip text under Blur / Redact; each row shows the user's quick-action key,
  which also works while Snip is open; the area is read when More opens; in a long capture Copy text and Ask AI finish it
  first and Translate is disabled. Reading text: the kit spinner replaces the Copy text pill's icon while text is read
  (tooltip "Reading text · N of M lines"), lines become selectable as read, the system's working-in-background cursor over
  unread text, a missing reader is offered for the script found only, one flat square highlight per line that splits
  correctly in mixed right-to-left lines. Pinned chat: the overlay closes by itself when the follow-up is sent (never
  mid-drag or while an action runs); the chat is its own window from its first frame, so it has NO soft outer shadow until
  the GPU renderer; on Wayland it runs through XWayland (prove keyboard focus on GNOME first), else a plain window.
- Bar beside the open chat (owner, 2026-11-11, lead's reading of "ok all"; SUPERSEDES "result card replaces the bar" for the
  AI chat): while the AI chat is open on the overlay the bar stays beside it, so Translate and Copy text keep working.

- Arabic UI design approved (owner, 2026-11-11, "i approve the lang and the lang design": every recommended default; sheets
  .claude/artifacts/design/render/out/arabic/, ARABIC_UI_SPEC.md, strings ARABIC_STRINGS.md = the seed of the string table).
  In Arabic: whole layout mirrored; window buttons top left on every OS; Off/On and multi-option controls reversed, sliders
  and progress fill right to left, scrollbar left; Western digits 0-9; product/service/technical names stay Latin, feature
  names translated (Snip = لقطة الشاشة, Circle to Search = البحث بالدائرة, Labs = ميزات تجريبية); theme and look names
  translated; Noto Sans Arabic on every OS (+1 px, more line height, no letter-spacing); General gets "App language"
  ("Your language (...)" = follows onboarding, then each language in its own name); onboarding lists each language in its
  own name; About's legal credits stay English; the owner supplies or corrects the Arabic bio and any wording.
- Arabic text-reading fixes approved (owner, 2026-11-11, lead's reading of the same message; planning #169): sorted
  batches, long lines cut, Windows OCR lines trusted only when they are real words in the user's language, the reader
  chosen per line, lines found at full size in strips, Linux detects once per opening; with the approved Reading text
  state. Each must show before -> after on the measurement set and may not slow English pages. Latin words and digits
  inside Arabic lines: a later measured step.

- Snip Studio design (owner, 2026-11-11, on the design page newdesing/studio.html, STUDIO_SPEC.md): "looks clean", with one
  change: BOTH side panels (Recent / Steps on one side, the options + Picture inspector on the other) must be collapsible
  (each on its own, remembered, with a key and a visible toggle; the canvas takes the freed room). The other Studio
  decisions (layout A, name, Recent off by default, ...) are still open. Design only; the Studio stays 0.5.0.

- No second hotfix (owner, 2026-11-11, "language"): the KDE X11 start crash fix (and the other Linux fixes, #172) ships with
  the Languages release, not as its own release. That release stays numbered v0.1.7 and is the last 0.1.x.

- Recommended defaults taken (owner, 2026-11-11, "yes, do recommended"): (1) GNOME Wayland: the pinned AI chat keeps
  today's behaviour (the desktop places it; no XWayland keyboard-grabbing window). (2) KDE Wayland: TRY capture through
  ScreenCast as a measured test behind the existing switch (before -> after on the VM; show the owner KDE's share
  indicator before it becomes the default). (3) Onboarding's language step does not say the pick "is used for the app"
  (only true for English and Arabic). (4) Arabic UI part 2 (overlay, bar, chat, Snip, tray, update widgets, installer;
  the left scrollbar, Noto on Windows 10, the setting-row overflow) follows part 1, with a Windows VM pass.
- Snip Studio design, LATER (owner, 2026-11-11): the current design is clear but "should be better", with the tool
  OPTIONS UNFOLDED (visible without a second click); panels collapsible. A new design round before the Studio is built
  (0.5.0); the eleven open Studio questions are answered in that round, not now.

- Strictest platform first (owner, 2026-11-11; strengthens "Both OSes from the first design"; the how is in the owner's
  engineering rules): every feature brief opens with a capability table (what the feature needs x Windows 10/11, X11,
  GNOME Wayland, KDE Wayland, wlroots = yes / asks the user / no); a "no" or "asks" goes to the owner with the
  alternatives that work everywhere BEFORE the build; the behaviour is designed for the platform that allows the least
  (usually Wayland); one feature = one code path, with only the OS call behind one small interface per capability; every
  target platform has a result in the report before merge. GNOME's and KDE's own capture effects are fine as they are
  (owner, 2026-11-11: "I don't mind both").

- Lighter models for everyone (owner, 2026-11-11, "we must ensure the new model is installed 100 %"; SUPERSEDES "replaced
  at the next download" in "Lighter translation models"): an install that still has a v1 decoder gets the v2 one for sure.
  Lead's design (owner asked for the recommendation): the user's next Translate works at once with what is on disk and
  starts the replacement of that pair's decoder in the background, shown plainly (size, progress), resumable, verified by
  sha256, swapped in place when complete (never two copies kept); Settings > Translate also shows the state and one
  "Update now" for every installed pair. No download happens without a Translate use or that button. The Translate page
  says offline translation uses OptimaMT models (the owner's optimised OPUS-MT decoders), with the credit to OPUS-MT kept.
- Start menu (owner, 2026-11-11): on Windows the overlay opening under an open Start menu is fixed LATER, in v0.1.8.
- Reset shortcuts (owner, 2026-11-11): Settings > Shortcuts gets "Reset shortcuts to default".

- Setting-row buttons (owner, 2026-11-11; applies "Settings harmony"): a Settings row whose control is one button ("Check
  now", "Open history", "Open Appearance", "Reset to defaults", "Update now" ...) fills the control column like every other
  control (same width, height and position, label centred), from one kit part; never a small button at the column's edge.

- About page, clean and minimal (owner, 2026-11-11, "do recommended ... something clean and minimal"; planning #177): the
  lead's recommended redesign is approved to BUILD (no separate design round): a maximum content width; compact app and
  developer cards with small links (no full-width link buttons); "Made by others" as an aligned list (first few + "Show
  all"); preview clip credits as one short line per clip with one link icon, folded under one heading, plus one link to
  the full notices on GitHub (the per-clip credit stays in the app: required by the clips' licences; the Google
  non-affiliation line stays verbatim); English legal text as one left-to-right block under its Arabic heading; a clear
  "Something not working?" card near the top (Copy diagnostics, Report a problem; nothing is sent) and a quiet "Report a
  problem" link in the Settings sidebar footer; not in the tray menu. When in doubt: fewer elements.

- v0.1.7 decisions (owner, 2026-11-12, "approved": the lead's recommendations): (1) Arabic text reading ships for Windows
  main mode and Translate now; Linux main mode keeps today's reading path until its two engineering pieces (the helper not
  holding the screen picture; one reader loaded at a time) are built and measured at or below today's numbers; accepted
  costs on Windows: a mixed English + Arabic page takes longer because more lines are read, and +6..11 % helper memory on
  a 3440-wide Arabic page. (2) Live border: the glow stays static until the GPU renderer (0.9.0); no breath on the CPU
  renderer (measured 2-10x over the near-0 % line on every platform). So v0.1.7 is "Languages" without the live border.
  (3) winget and Scoop: fix the `winget install` hang and the four gaps (a per-install switch for the in-app updater, the
  Setup's publisher name, version/company info in the Setup exe, documented silent exit codes) in v0.1.7 and submit with
  that release; the owner opens the PRs. (4) Old worktrees: their unsaved reports/proof are archived outside the repo,
  then the worktrees are removed.

- Model folders carry the OptimaMT name (owner, 2026-11-12, "why not the OptimaMT name?"): the translation model folders
  on disk are renamed from `opus-mt-<pair>` to the OptimaMT naming (the Hugging Face repos are `OptimaMT/<source>-<target>`),
  once, at start, for every existing install (no re-download; an old-named folder is found and moved, never left beside
  the new one). The OPUS-MT credit and licence stay in About / NOTICE.
- v0.1.7 publish (owner, 2026-11-12, "you can publish directly"; lead's reading): the lead may PUBLISH v0.1.7 without
  asking again, but only when every gate has passed (clean-up, the security review and QA gates of the last 0.1.x, docs
  and notes, CI green, the update gate v0.1.6 Setup + portable -> draft on the Windows VM, About opened on the draft exe,
  the Ubuntu AppImage check); any failure = it stays a draft for the owner.
- Code size (owner, 2026-11-12, "too many lines, how?"): measured 137,558 Rust lines = 89,247 product + 48,311 tests (35 %).
  The owner doubts the size: the release clean-up includes a real size audit (duplication, dead code, things written
  twice), with a list for the owner and before -> after line counts; the feature-folder restructure comes after v0.1.7.

- Owner checks (2026-11-12): the rebuilt About page is approved (dev build D067); the Arabic wording is approved as it is
  (he may still correct single strings later); cosmetics such as the edge breathing are not a priority for now.

- "Live border" corrected (owner, 2026-11-12: "you misunderstood me, here live meant that the app is active, not the
  border is breathing"): the "live border effect" of the 2026-11-10 plan is the Active Look edge that shows the app is
  ACTIVE. It exists and ships as it is (static glow, on the screen of the last input). NO breathing was asked for: the
  words "with its soft breathing back" in "Next releases (owner, 2026-11-10)" and the breathing in planning #156 were a
  lead's misreading. Nothing is owed for it; breathing is not planned unless the owner asks.

- No full security scan (owner, 2026-11-12, SUPERSEDES "Security review timing" and the security step of the release
  rules, incl. for v0.1.7): the app is local and the full security review/scan costs too many tokens: it is NOT run for
  v0.1.7 or later releases unless the owner asks for one. What stays, because it is free or already there: `cargo audit`
  in CI, the automatic per-commit security check (its findings are fixed or answered), the signed update channel and the
  update gate. The QA gates and the clean-up pass stay as release steps.

- v0.1.8 (owner, 2026-11-12, "yes"): a small release after v0.1.7 and before Game translation (0.2.0): the Start menu fix
  on Windows, Arabic text reading in Linux main mode (the two engineering pieces), Latin words and digits inside Arabic
  lines, the right-to-left selection highlight if it misses v0.1.7, keyboard navigation in Settings (Tab / Enter), the
  feature-folder restructure of the code (plan and owner OK first), the size clean-up left from the audit, hardening the
  database open. v0.1.7 is therefore no longer "the last 0.1.x".
- Cheaper testing (owner, 2026-11-12: "testing takes a lot of credits, agents' testing isn't optimised"): VM checks are
  SCRIPTED: one script per area runs every step and writes one result file + screenshots; an agent runs it once, reads the
  result, fixes, runs it once more. No exploratory clicking by agents, no VM pass per worker: ONE consolidated VM pass per
  merge batch (and the release gate). Look-and-feel is judged by the owner on the dev build; goldens cover layout. The
  scripts are kept in the repo's internal test folder and reused every release.
- "Company" in the exe (2026-11-12): Windows' file-properties field is named CompanyName; it holds the owner's name
  ("Amer Alreyahi"), there is no company.
- winget hang cause (2026-11-12, measured on the owner's PC with his OK): `winget install --manifest` blocks behind a
  dialog winget owns for the unsigned, internet-marked Setup exe (no Setup process ever starts); not a bug in the Setup.

- Two workers at most (owner, 2026-11-12, "I want to save tokens"; SUPERSEDES "at most 3 workers at once" in "Credits"):
  never more than 2 workers running at the same time, a resumed worker counts as one; queue the rest.

- Snip Studio, a better design (owner, 2026-11-12): "create a better and more advanced Snip Studio design"; the Studio STAYS at 0.5.0
  (owner, 2026-11-12: "you are right, my mistake"). A new design round (one
  clickable HTML file; tool options unfolded; both side panels collapsible; more advanced than the 2026-11-11 design) is
  queued for the next free worker slot; nothing is built before his OK.

- Enhancements release (owner, 2026-11-24): the release after v0.4.0 is ENHANCEMENTS ONLY, no new features: #221 Translate slow
  since v0.2.0, UX study F1-F15 (#206-#220), #200 Snip enhancements, #202 Ctrl+wheel resize. #201, #203, #204 wait. Video + audio
  recording in Snip (#168) comes in the releases after it.
- UX study owner calls (owner, 2026-11-24; SUPERSEDE the older decisions they name):
  C1 main bar keeps Copy image = Ctrl+C / Save = Ctrl+S without buttons; the key hint (F3) shows them.
  C2 the pinned AI chat gets an x that closes it; the pin only pins / unpins (supersedes "no x, the pin closes", 2026-10-28).
  Unpin (lead 2026-11-25) = really leaves pinned mode: not on top, not kept across questions (next question = fresh quick chat),
  quick-chat close rules again; with no overlay it stays a normal window until its x / Esc.
  C3 Studio: Ctrl+= / Ctrl+- zoom the PICTURE, Ctrl+0 fits; UI size moves to Preferences > Appearance and the View menu
  (supersedes 2026-11-20).
  C4 Snip shows the active tool's options at once in a slim row under its toolbar, like the Studio (supersedes the second click, 2026-11-02).
  C5 kept: with a drawing tool a drag draws; Select or Ctrl+drag moves; a one-time hint "Ctrl+drag moves" on the first drag over a shape.
  C6 a quiet x at the end of the Snip toolbar and the translate pill; Esc keeps working (supersedes "Esc only", 2026-11-02).
  The main Ctrl+Shift+Space bar gets the same quiet x (owner 2026-11-25: mouse users can't close it otherwise).
  Studio > Preferences moves into the approved Settings shell look (owner 2026-11-25: it looks bad in EN, worse in AR).
  C7 from Settings v2 now: toggle switches instead of Off | On segments, and each page's preview folded into one "Play preview" row.

- Next release after the enhancements (owner, 2026-11-24): Snip audio + video recording (#168) AND the new design. The widget
  changes with the mode: record mode (audio or video) shows only the recording controls, never the picture editing tools.
  The new design is not approved yet: its design round comes directly after the enhancements release, approved on the
  canvas (COM-08) before recording UI or any new UI is built.

- No peek at the original under a translation (owner, 2026-11-18, commit 0977fb5f; recorded here 2026-11-24): holding Alt no
  longer shows the original (Alt+A left the peek stuck); no frame of the original until Esc. UX F7 (a Show original toggle on
  the pill) waits for the owner because it goes against this. UPDATE (owner, 2026-11-24): build F7 as a click toggle: an
  "Original" button on the translate pill, click = original, click again = translation; Esc still ends it; Alt never peeks.

- Owner answers 2026-11-24 (3):
  - #201 right-click menu on selected text is IN the enhancements release (owner sees it as an enhancement): study the actions
    first (Windows + Linux), owner approves the list, then build; #202 Ctrl+wheel resize with it.
  - F10: keep Enter's "Text" choice, renamed "Copy text" (id "text"; no migration, nobody's Enter changes).
  - The new design: the owner decides what it covers after reviewing it, not now.
  - Translate speed (#221, W94 measured: no regression since v0.1.8; the cost was already there): (1) the second whole-screen
    PP-OCR read after Windows OCR runs only on game screens; (2) the model warm-up no longer blocks or waits behind OCR;
    (3) the helper keeps the last two translation models (about +250 MB while warm). The owner uses the offline (Local) engine.
    (4, owner 2026-11-24) an unused warm-up model is dropped when another warm-up arrives; the ~1 GB transient peak for ~1.5 s on a
    non-English game screen is ACCEPTED by the owner (avoiding it would give back the time saved).
  - SDLC Kit adoption APPROVED: commit hooks (gate + no AI attribution), .gitleaksignore for the 8 fake history hits, sdlc.json.
    Old history with AI attribution is LEFT as it is (no rewrite); the hook guards new commits.
  - Roadmap source = docs/roadmap.json (kit template); the board is generated from it and the private master roadmap page
    shows it (supersedes "the private Artifact roadmap.md is the master" for planning edits).

- Right-click menu on selected text, v1 (owner, 2026-11-24, study docs/internal/reports-enh/w97-rightclick-study.md): items
  Copy (Ctrl+C), Search the web (L), Ask AI (A), Translate (R), Select all text (when not all is selected), Open link / Send
  email (only when the selection is one). Later: Copy as picture, Explain/Summarize, call/calendar/map, unit & currency convert.
  It opens only on a right press ON the selection; right-drag pan and double right-click reset stay everywhere else. Ask AI
  from this menu sends the selected TEXT (the picture only when there is no text) by OPENING the chat with that text attached for
  the user to type (no automatic Explain request; lead default 2026-11-24); the bar's Ask AI keeps sending the picture
  (2026-11-08). Menu key + Shift+F10 open it with arrow keys; Copy/Search/link close the overlay, Ask AI/Translate keep it.
  Translate from the menu (lead 2026-11-25, owner: "I assumed the new translation replaces the original text, not a widget";
  SUPERSEDES the small card) translates the SELECTED TEXT IN PLACE: only the selected span of each line is erased and the
  translation drawn over it by the one-shot translate (several lines = one text, laid back into those lines), with the translate
  pill (chips, Original, Esc); Ctrl+C copies the translated text. The bar's Translate (R) with words selected does the same (one
  path); with an area and no words it translates the picture in place.
  Surfaces v1: main overlay selection + select-text mode; pill, Snip area, circled area without words later.
- Ctrl+wheel (owner, 2026-11-24, #202): over a selected shape it resizes the shape (Studio and Snip editor); elsewhere in the
  Studio it keeps zooming the picture.

- Three workers allowed for this round (owner, 2026-11-24, "for the design start 3rd worker"): the 2-worker cap is raised to 3
  while the design worker runs; at most 2 of them heavy builders (ORC-11).
- Next-release design (owner, 2026-11-24): prototypes FIRST, only the new parts, for his OK: the Snip widget per mode (picture
  tools / audio recording / video recording; record modes never show picture tools) and "Open with" for a capture or recording
  (installed editors such as CapCut, Filmora). Never a rewrite of everything up front. After he approves the prototypes, the
  full Claude Design for all of Settings. Shown on the Claude Design canvas.

- Tips (lead recommendation 2026-11-24, owner asked): no always-on floating tips widget; one-time contextual hints at the moment a
  feature is relevant (max one per session, dismissible, "Don't show tips" in Settings, never during a fast flow) + a "What can
  I do?" card on demand (key + tray). APPROVED by the owner 2026-11-24; goes to the design round.
- Review rhythm (owner, 2026-11-24): keep working; don't wait for his review after each phase. He asks to see several phases
  at once when he wants. Decisions only when they block work; otherwise the recommended default, listed for him.
- Recording triggers (lead proposal 2026-11-24, owner may change; for the design prototypes): Alt+A ALWAYS opens Snip in Picture
  mode, unchanged (owner 2026-11-24: Alt+A, click, Ctrl+C in under 1 s must stay; the mode is NOT remembered); a small Picture |
  Video | Audio switch at the top while picking, already on Picture; after picking in Video the SAME widget turns into the record
  bar (picture tools gone), no second widget; direct keys Alt+Shift+R = record video (pick an area,
  window or screen, 3-2-1, record), Alt+Shift+M = record audio (starts at once, no pick). The same key again = stop and save;
  the record bar's Stop too; Esc while recording = stop and ask Keep / Discard. Also in the tray menu; NOT on the main
  Ctrl+Shift+Space bar (it acts on a frozen screen; lead recommendation 2026-11-24). The Picture | Video | Audio switch is a small
  pill at the top of the PICK screen only (gone once you pick; then the Snip widget appears as today), not part of the widget.
  Win+Shift+R (Snipping Tool) and Win+Alt+R (Game Bar) stay Windows'. Keys rebindable in Settings > Shortcuts.

- Recording look (owner, 2026-11-24, after seeing prototype round 1: "bad designs", the record bar and the "Recording ready" card
  rejected): while recording = a TINY pill (red dot, timer, Pause, Stop; mic/sound and cancel only on hover). After stop = a small
  floating CORNER THUMBNAIL (CleanShot-style): hover shows small icon actions Copy / Save / Edit in OpenCircle Studio / Open with;
  drag it straight into another app or a folder; it slides away by itself after a few seconds. No big result card, no row of
  large buttons, no technical text (temp file, MB). Same pattern for audio (a small waveform chip).
  Round 2 (canvas https://claude.ai/artifact/AKoKdVveRjedVz38kEEmV7): the custom "Open with" menu is NOT approved (owner 2026-11-24).
  The round-2 recording widget (tiny pill + corner thumbnail) is ALSO NOT approved (owner 2026-11-24): next = a comparison sheet
  of 5-6 real recording-widget styles + 3 result styles for him to pick.
  DECIDED instead: "Open with..." opens the SYSTEM's own chooser (Windows Open with dialog; Linux desktop app chooser via the
  OpenURI portal ask=true / Gio), like right-clicking a file in Explorer. No custom app list to design or maintain.

- DESIGN SOURCES (lead mistake 2026-11-24, owner: "this design was outdated long time ago"): the canvas
  https://claude.ai/artifact/B9g1audcqL1zPbZ2cz4KdQ (and its local copy .claude/artifacts/design/project/) is the OUTDATED
  October canvas: never base new work on it or publish to it. The current look = the shipped app (goldens tests/fixtures/
  screens/, e.g. snip-toolbar-*, studio31-*, settings-*), the approved Studio v3.1 = the SHIPPED Studio (goldens
  tests/fixtures/screens/studio31-*; owner 2026-11-24: the old v3 canvas is NOT the current Studio design; its rebuild was deleted
  on his word, local copy .claude/artifacts/design/studio-v3/ is history only) and the Settings v2 draft canvas https://claude.ai/artifact/2u6VGzutWoVxNpqxHTQoSE (round 5). New prototypes go on their own
  new canvas.

- Blur rework (owner, 2026-11-24, study docs/internal/reports-enh/w100-blur-study.md; in the enhancements release, steps 1-5):
  Hide's default = a smooth secure frost built from the area's BACKGROUND (never reads the content: anything that reads it was
  recovered 100 % in the lab); today's mosaic stays as the "Pixelate" style, made consistent. Soft / Frosted / Progressive
  (direction + start/end handles, like the owner's reference) are LOOK styles with a one-line note "for looks, not for secrets",
  never the default. Strength = Light / Medium / Strong (0.25 / 0.5 / 1.0 x reference text height) in Snip, plus a Custom slider in
  the Studio; the same at any zoom (image px x capture DPI scale, never / zoom). One texture per picture, overlaps merge. Shapes:
  rectangle, ellipse, free brush with soft round ends, "Hide this shape"; edges feather outward only; an ellipse stays an ellipse
  (Redact too); the #154 drag-back shrink only while line help holds the stroke on one line. Blur once, mask many (cache).
  Old saved blurs re-render with the new fill. Keys: B, Shift+B cycles the style for the NEXT Hide (restyling an existing Hide
  needs a click on a style card), 1-3 strength while Hide is the tool; NO R/E/P override (shapes in the options row only).
  Old blur_cell "thin" (the written default) migrates once to Medium (schema 14). The Hide fill's ink term is quantised.

- Settings sidebar (owner, 2026-11-24): he still prefers an earlier sidebar he can't find; the CLOSEST is Settings v2 round 1
  (backup .claude/artifacts/backups/settings-v2-round1.*.tgz, render B2-AskAI): the selected item is a raised pill (elevated
  surface) with a thin accent bar at its start edge, on a calm light pane, icons + labels, hairline groups, Report a problem +
  Collapse (Ctrl+B) at the foot. The Settings design round starts from that and shows 2-3 prettier variants of the same idea
  for him to pick (the round 5 filled-block selection is NOT the target).

- Settings redesign RESET (owner, 2026-11-24, on the round 5 canvas): APPROVED = only the shell: the title bar (logo, name, version
  chip, centred search with Ctrl F keycaps, window buttons) + the round-1 sidebar (raised white pill + thin accent bar on the
  selected item, icons + labels, hairline groups, Report a problem + Collapse Ctrl B at the foot), as in Settings v2 round 1 (B2).
  NOT approved = everything in the content area; the round 5 content and the Home dashboard are WITHDRAWN ("why do I need a
  dashboard for this app?": no Home page, no stats tiles). New content is inspired by the owner's other app's LATEST design
  (/home/az_work/MainHub/Apps/CA_Catalog_Orchestrator: design/, docs/internal/v034-harmony/), with the feature preview done the
  best possible way. Show ONE sample page, light mode, one picture; never recreate everything before he approves the sample.
  SAMPLE APPROVED (owner, 2026-11-24, "yes, continue"): docs/internal/reports-enh/w101-settings-sample.md, PNG
  .claude/artifacts/design/render/out/w101/settings-sample-daylight.png. Lead defaults for its questions: one "AI and Search"
  page; the preview = a slim row at the top (hover/focus plays a silent ~5 s loop, still under reduced motion) whose text
  describes the FEATURE, never the design; two-option choices stay segmented, on/off = toggle switches. Next: every page.
  UPDATE (owner, same day): one "AI and Search" page = yes; the preview presentation and the Browser | AI provider control are
  OPEN: he picks from a comparison sheet (5-6 preview methods, 3-4 choice controls). Do NOT draw all pages yet.

- Owner picks from the comparison sheets (2026-11-24; sheets in .claude/artifacts/design/render/out/w101/compare-*.png):
  - Recording, while recording = R1: a top-centre bar (Windows Snipping Tool style) with timer, Pause, Stop, discard, PLUS a mic
    mute toggle and an audio device picker (from R6) and the capture SIZE and FPS shown on the bar.
  - After stop = S2: a small corner thumbnail with hover actions (Copy, Save, Edit, Open with = system chooser), drag-out; its
    Edit opens the recording in OpenCircle Studio (S1) to trim.
  - Settings "Answers from: Browser | AI provider" = B2: two choice cards, each saying what it does before the pick.
  - Settings feature preview: base = A3 ("See how it works" opens the clip inline) + A5 (sticky clip column beside the settings
    on a wide window); he wants MORE, bolder ideas: "the preview isn't getting its full rights". Next: a sheet of new ideas.
  These replace the round-1/round-2 recording looks and the slim preview row.

- Three tiers (owner, 2026-11-24): CORE (always there: Circle to Search, Translate, Snip incl. recording, AI and Search) ·
  EXTENSIONS (new Settings page "Extensions": finished optional tools installed on demand, each with a card + preview + download
  size; Add downloads only its parts; On/Off keeps files but removes shortcuts, tray and menu entries; Remove frees the space,
  settings kept unless cleared; updated with the app) · LABS (keeps its name: unfinished experiments that later graduate).
  ARCHITECTURE (owner, 2026-11-24, REVISED: "completely separated, never heavier when not installed"): each extension is its
  OWN small executable, downloaded only on Add (signed + checked like releases), started by OpenCircle only when used, separate
  process (a crash never takes the app down), stopped when done; it draws its own window; OpenCircle only shows its card,
  registers its shortcut/tray entry while it is ON, and talks to it over one small fixed message channel (same pattern as the
  text-reading helper). Not installed = nothing on disk, nothing runs; the catalogue list is read only when the Extensions page
  opens, never at startup. Off = files only, zero RAM/CPU. Remove = its folder deleted. NOT built into the main binary, no
  DLL/.so loading, no WASM runtime. Only first-party extensions; the message channel is frozen once (SEC-06) after the first one. The owner's personal tools are extensions (listed for all or private). Candidates to move from core:
  song search, game translation + glossaries, colour picker, ruler, QR, quick convert, advanced paste. First new one: meeting
  captions (#227).

- CORE vs EXTENSION RULE (owner agreed 2026-11-24): a new tool goes into Extensions unless most users would use it every week.
  Core out of the box: Circle to Search (select, copy text, search, right-click menu), Translate screen (one shot) + Show original,
  Ask AI about the screen, Snip (picture) + OpenCircle Studio (edit, steps, export), Snip recording (video/audio), Blur/Hide,
  edit text, cut out. Extensions: meeting captions, live/game translation + glossaries, song search, colour picker, ruler, pin
  live region, QR/barcodes, quick convert, advanced paste, actions on detected things, local offline AI model, dictation, read
  aloud, writing assist, privacy curtain. The name OpenCircle stays; the tagline is being chosen.

- Snip options row + Settings search, owner 2026-11-25 (after trying dev 11886be; shot docs/internal/owner-shots/2026-11-25/
  snip-row-and-search.webp): the options row must sit DIRECTLY under (or above, when no room) the Snip toolbar, attached and aligned
  with it as one piece, never offset; no separate "Shapes · Rectangle (R)" pill; the active tool's CHOICES are IN the row (Shapes:
  rectangle, ellipse, line, arrow, then size, outline/fill; same idea for every tool with sub-types) - no separate pop-up panel and
  no second click (familiar: Paint, Snagit, CleanShot). The Settings search sits CENTRED in the title bar (approved shell), never
  squeezed against the window buttons. UPDATE (owner 2026-11-25): the row is CENTRED under the toolbar (shifts inward only at a
  screen edge); the Settings search key shown is Ctrl+K (Ctrl+F still works).

- Owner try of dev 11886be, round 3 (2026-11-25; shots docs/internal/owner-shots/2026-11-25/round3-*): works: steps export, paste
  rule, Redo, double-click to Studio, AI chat, download card, tray, Arabic, start with Windows. Decisions:
  - TITLE BARS are NEVER mirrored in Arabic, in every window (Settings, Studio, onboarding, dialogs): same layout as English;
    the Settings search sits EXACTLY in the centre of the title bar.
  - Report a problem popup: its BUTTONS are centred inside the popup (owner correction).
  - Studio colour row gets a custom colour ("+") that opens Snip's colour picker (one shared component).
  - After any export finishes: a small card with Open and Show in folder (familiar "Exported" toast).
  - Onboarding step tabs fit on one line in every language (no wrapped tab like "4. ربط الذكاء الاصطناعي").
  - Steps strip (planning #204 is now IN this release, owner): "Open in OpenCircle Studio" on the strip; the strip can be dragged
    anywhere (position remembered); once the Studio holds the steps the strip closes; BUG to fix first: x on the strip wiped the
    steps from an open Studio (data loss) - the Studio's steps must never be lost by closing the strip. Once the Studio holds the
    steps it owns them: closing the Studio ends them (its Save / Don't save prompt covers unsaved work; lead 2026-11-25).

- Arabic text reading in 0.5.0 (owner 2026-11-25, study docs/internal/reports-enh/w109-arabic-ocr.md): fix 2 (Engine::per_line stops
  the second reader early: set `done` only when the "few left" branch was chosen) + fix 1 (word boxes from the readers' own
  positions: CTC character steps / Windows OCR word boxes; ink gaps only as a fallback). The better Arabic reader (English and
  digits inside Arabic lines, spaces in bold Arabic, mirrored brackets) is the NEXT release. The owner allows installing Windows'
  Arabic OCR language (Language.OCR ar-SA) on the Windows TEST VM with one elevated step (2026-11-25).

## Domain quirks

- Wayland can't hide windows (set_visible(false) is a no-op): close them and
  reopen instead (`sys::session::can_hide_windows()`); never "hide" a window there.
- Wayland forbids global key grabs and silent screenshots: use the
  xdg-desktop-portal (Screenshot, GlobalShortcuts). The GlobalShortcuts portal
  is missing on some desktops → fallback: user binds `opencircle capture` in
  their desktop's shortcut settings.
- `Windows.Media.Ocr` works unpackaged (no MSIX needed; benchmarked 2026-10-26: 90-335 ms
  per screen). It reads main mode's whole screen from the capture on (#62,
  `ocr/system.rs`, in the app's process; `ocr::route`); PP-OCRv5 stays for selections,
  translate, live, Arabic/RTL and games (`selection.text_reader = "pp_ocr"`). It reads only
  languages installed in Windows (#63): no recognizer = PP-OCR, logged once. It drops short
  words in small text ("needed to trained"): under 150 % a quick 1x read is replaced by a
  doubled one, in grey. Freeing the engine (60 s idle) returns ~4 of its ~13 MB: windows-rs
  caches the WinRT factories, so the DLLs stay loaded.
  A synthetic press needs a pointer move over the overlay first (iced has no cursor position
  before one); VM scripts must be per-monitor DPI aware to click on a second screen.
- Google Lens has no public upload API. Existing clones upload to imgur/uguu
  (privacy leak). Our approach: the browser itself posts the image to Lens
  through a local auto-submitting form (spike S1 in the plan proves it).

- Preview finishing (owner, 2026-11-03): use the owner-supplied real recordings; preserve raws, edit locally in a separate project, and show two rendered pilots (minimal / focus motion) for style selection before finishing the batch. No cloud uploads, generated replacement UI or added cursor. Deliver silent 1200x680 WebP (15-20 fps), matching MP4, poster and editable source. Editing brief and handover: docs/internal/preview-edit/.

Owner style selection, 2026-11-03: **A — minimal**, chosen after reviewing the rendered pilots. Apply static framing and clean cuts consistently across all supplied recordings; adapt source ranges for clarity.

- Preview live effect (owner, 2026-11-03): crops may hide the actual Active Look edge. The editor may reconstruct that effect from the app/design reference in the edited viewport, ONLY during the real active state. Never add it over inactive source or external results. Preserve Snip pre-pick glow versus picked-area outline. Respect shortcut-driven actions; never invent toolbar clicks. Document the reconstructed layer in edit notes.

- Preview glow selection (owner, 2026-11-03): owner likes the minimal edits but requests a different reconstructed edge effect; show actual rendered Active Look samples including the new Sapphire Glow, then wait for their choice before replacing the full preview set.

- Preview final glow (owner, 2026-11-03): apply the recommended sample A, Sapphire Glow (blue #0a84ff / indigo #5e5ce6, 100% preset strength), across the finished minimal previews. Keep the same genuine active-state gates and Snip pre-pick distinction. Supply a Claude handoff explaining every clip and the editing choices.

- Preview source holds (owner, 2026-11-03): 1.2, 1.3 and 1.4 stay out of public/app preview delivery until their exact photo sources are established, or the owner records replacements using own/CC0 photos. Review exports may remain labelled. Text reading remains without a bundled preview until an approved clip is installed. Never substitute a new photo beneath unchanged old search/OCR results.
- Translation preview pacing (owner, 2026-11-03): every target language must be recognizable; preserve actual language changes and allow readable result holds. Separate language clips and editorial labels/freezes are allowed. Do not compress the sequence so language choices disappear.

- Preview task scope (owner, 2026-11-03, latest): stop image-source investigation; finish the translation-video pacing revision, then stop this editing task. Existing provenance findings remain notes; this is not an instruction to publish or install clips.

- v0.1.7 size clean-up (owner, 2026-11-12, "ok", after the size audit: 141,421 Rust lines = 86,773 product + 54,648 tests;
  code written twice is about 1-3 %): done now, no behaviour change: the audit's safe group (model pin table one row per
  entry, one golden helper, the unread `selection.after` / `search_with` keys, small duplicates), the vendored OCR library's
  never-compiled files, and the switched-off edge-glow breathing code (deleted; SUPERSEDES the 4 s breath of "Screen edge",
  2026-10-27: the glow is static). The rest of the audit (shared wrapper widget, test helpers, the Lens page's PNG code,
  splitting long files) goes with v0.1.8's restructure.

- v0.1.8 deadline + restructure (owner, 2026-11-13, "okay"): v0.1.8 is finished on 2026-11-14 (estimates are given in agent
  working hours, never human days). The feature-folder restructure plan (docs/internal/reports-0.1.8/restructure-plan.md:
  src/app + src/features/<feature> + src/shared, pure moves, arch tests) is approved as written. If "Latin words and digits
  inside Arabic lines" runs late, it moves to 0.2.0.
- Linux main-mode reading (owner, 2026-11-13, "hold the switch"): W3's right-to-left highlight and the two engine pieces
  (the helper keeps only its rows; one reader at a time) ship in v0.1.8; Linux main mode keeps today's band read because the
  switch missed the pass line (English/French pages +11..+47 % time, some +16..+44 % memory; Arabic/mixed much better). The
  measured follow-up (skip the Arabic try on Latin pages, smaller strip-detector input) runs after the restructure if time is
  left, else in 0.2.0, with the same pass line. Report: docs/internal/reports-0.1.8/w3-linux-reading.md.
- v0.1.8 publish (owner, 2026-11-13, "you can publish directly; if there is an issue I will tell you to fix it in newer
  versions"): the lead publishes v0.1.8 without asking again. The update gate (v0.1.7 Setup + portable -> draft), About on
  the draft exe and the release workflow's signed assets still run first: a broken updater is the one bug users can't wait
  out. Other findings ship as known issues in the notes and are fixed in the next version.
- Snip Studio name and behaviour (owner, 2026-11-14, "great, ensure it is in the roadmap"): name "Snip Studio", short
  "Studio" (Arabic «استوديو اللقطات»). Its own normal window (taskbar entry, side by side with Settings / chat / pins);
  ONE Studio at a time (opening again or another picture brings it forward and loads the picture there); opens alone
  (tray "Studio", Start menu / app grid shortcut, additive CLI `opencircle studio [file]`) or with any picture (Open
  Ctrl+O, drop a file, Paste Ctrl+V, "Open with" from Explorer / file managers); unsaved edits ask Save / Don't save /
  Cancel. Into the Studio design round first; version 0.5.0 (roadmap section 1, planning #162).

- 0.2.0 decisions (owner, 2026-11-14): (1) Per-game glossary (#13): the design round (.claude/artifacts/design/
  glossary-design.html, GLOSSARY_SPEC.md) is approved with every recommended default: a glossary button always on the
  translate pill (the only quick switch on Wayland); the user picks the active glossary (remembered), a glossary's "Use for
  windows named" list picks it by itself where the OS gives a window title; never a handle to the game process; empty
  translation = keep as is; auto-suggest names later; TOML only (CSV later); no starter glossaries; the AI is guided, not
  forced; matching ignores case. Built after the slim OCR pipeline (#61) lands. (2) The GPU spike (#136) and the live-layer
  cost measurement (#166) run on the owner's RTX 3090 PC only when he says so: ask right before the run. (3) Portuguese
  offline translation (#113): the lead may convert the model and upload it as a new release of OpenCircle-models (as the
  owner, no AI credit), sha256 pinned like the other pairs.
- App icon (owner, 2026-11-14): v0.2.0 ships the new Sapphire icon (the design's official Sapphire logo,
  .claude/artifacts/design/newdesing/assets/logo-sapphire.svg/.png) everywhere the purple one is used today: exe/.ico,
  window and tray icons, Setup exe and installer screens, AppImage/deb/rpm icons, About, README/docs images. The owner sees a
  rendered sheet of every size (16..512, light and dark backgrounds, tray) before it merges.
- OpenCircle-models stays unarchived (owner, 2026-11-14): models-v3 (Portuguese) uploaded there; future model releases go to
  the same repo.
- Hugging Face uploads (owner, 2026-11-14, "you can upload models to Hugging Face, no issues"; extends "HF upload" and
  "Publish OK" of 2026-11-09): the lead may upload models to Hugging Face (OptimaMT organisation) without asking each time,
  as the owner, with no AI/agent trace anywhere (commits, cards, metadata). Workers still only prepare files locally.
- App downloads from OptimaMT later (owner, 2026-11-14, "sure"): plan moving the app's model source from the
  OpenCircle-models GitHub releases to the OptimaMT Hugging Face repos in a later version (new URLs + sha256 pins, retry /
  resume tested against Hugging Face); OpenCircle-models' releases stay forever for older installs. v0.2.0 keeps
  OpenCircle-models. New pairs (Portuguese, models-v3) are also added to OptimaMT so both stay complete.
- GPU research on the owner's PC (owner, 2026-11-14, "you can take control now, go ahead"): the live-translation research
  (#136 model on the GPU, #166 live text layer cost) runs on the owner's Windows PC (RTX 3090) as the lead described: one temp
  folder (%LOCALAPPDATA%\Temp\opencircle-gpu), portable programs only (no installer, admin, driver/registry/settings
  change), temperature watched (stop above 83 C), everything closed and deleted after. Samples: game cutscenes STREAMED from
  YouTube in the browser (never downloaded; private testing, nothing published), from MANY games and genres, not only
  Genshin style, covering three scene kinds: fighting (little text), choices/thinking (much changing text, calm background),
  balanced conversation. Menus/settings screens are already covered by the 13-screen corpus. Frame cost is measured on a
  heavy full-screen browser 3D scene (no game); the owner tests real games himself and gives feedback. Never any automated
  input into a game, never a handle to a game process.
- Sapphire icon approved (owner, 2026-11-14, on the before/after sheet): merge W6's icons (scripts/app-icon.py).
- Live-translation model and research follow-ups (owner, 2026-11-14, after W8's run): (1) the GPU memory line for the live
  model is raised to ~1.65 GB: Hy-MT2-1.8B 4-bit GGUF on llama.cpp Vulkan (Apache-2.0) is the pick for Live translation
  (W8: COMET 0.872 vs 0.846, corpus 74.6 % vs 71.7 %, 74 vs 107 ms/line; reports-0.2.0/w8-gpu-research.md); the GPU rule
  (free VRAM >= model + ~1 GB) still decides per PC. (2) #166 live layer cost: the owner quits his own OpenCircle for ~10 min
  when asked (his folders backed up first and restored after). (3) Draft Arabic references: the owner checks a mixed sample
  of ~30 lines; if they are fine, all 257 count. (4) Runs on the owner's PC never send keys with SendKeys or any input that
  can reach another window (2026-11-14: two early attempts sent f / Esc / Ctrl+W to an unknown front window).
- Arabic references approved (owner, 2026-11-14): the 30-line sample (reports-0.2.0/w8-sample30.tsv) is approved, so all
  257 lines of w8-lineset-draft-refs.tsv count as the live-translation reference set (no longer "draft").
- Live translation priority (owner, 2026-11-14, firm): never ruin the game experience comes FIRST, then speed; accuracy is
  secondary. The worst failure is a frozen / stale patch left after the text changes. Design rule: show nothing rather than
  something stale: a cheap per-frame change check on every drawn area; when an area changes, its patch is removed in the
  next frame BEFORE any re-read or translation (the original shows briefly), then the new translation fades in; patches
  only where text sits on a held box/bar; back off if the game loses frames. Cutscene test (#82) pass lines: zero patches
  left after a change, removed within one frame, game frame rate unchanged; accuracy reported but not a gate. Run it early
  in Live translation (0.3.0), before the bulk of the build.
- Default shortcuts (owner, 2026-11-14): Translate moves from Ctrl+Shift+T (browsers' "reopen closed tab") to Alt+Shift+T and
  Live translate from Ctrl+Shift+L (Edge) to Alt+Shift+L, in v0.2.0, after a conflict check on Windows, GNOME and KDE; a
  config still on the old default moves once (schema bump, backup); a key the user chose stays.
- Runs on the owner's PC (2026-11-14 lesson): programs started from this session run inside the Claude desktop app's package,
  whose AppData (and possibly registry) writes go to a private copy (%LOCALAPPDATA%\Packages\Claude_*\LocalCache): start
  test builds through a scheduled task in his session and check the log shows the expected config before measuring.
- Live translate screen border (owner, 2026-11-14): the live mode's border must look like the app's Active Look edge (the same
  glow as main mode, maybe slightly different to tell live apart), NOT today's static thick border. Supersedes "Live and snip:
  static" for the look (still no moving colours; near 0 % CPU). For Live translation (0.3.0), design shown to the owner first.
- #166 result (2026-11-14, owner's PC): the live text layer costs no game frames on a GPU-bound scene, so the version order
  stays (GPU renderer stays at 0.9.0). Today's live loop costs ~2.2-2.5 CPU cores and ~700 MB (full-screen read every ~1 s):
  Live translation (0.3.0) must read only the areas that changed (cheap per-frame change check first) and is re-measured on a
  CPU-bound game with the cutscene test (#82). The owner watched today's Labs live and found it "not a nice experience".
- Live translation's real problems (owner, 2026-11-14, after watching Labs live on his PC): the DELAY and the FLICKERING /
  text rendering are what ruin it; other issues are fixable details. 0.3.0 targets these first: translated text appears
  fast (measure time from subtitle shown to translation drawn), is drawn once and stays steady (no redraw flicker while the
  same text is on screen; only changed areas re-read and redrawn), and leaves no stale patch. These are cutscene-test (#82)
  pass lines next to the frozen-patch rule.
- v0.2.0 scope trim (owner, 2026-11-14): #134 (MeikiOCR Japanese spike), #135 (PP-OCRv6 spike) and #137 (models-v2 three-way
  measurement + README) move to Live translation (0.3.0). Japanese gets little effort in v0.2.0: no new Japanese reader work;
  the Japanese corpus screen (loading) is best effort, not held to the 90 % target. Russian/Cyrillic and Portuguese stay.
- Glossaries made easier, all three ways (owner, 2026-11-14, "yes, use the 3 options"; refines "no starter glossaries" =
  WE don't author them): (1) next version: a LOCAL name suggester (repeated capitalised words on the game's translated
  screens -> a quiet "N names suggested" dot on the pill's glossary button; accept/reject in one click; never a pop-up during
  play; no internet). (2) Later (with Reach, 0.6.0): a free community glossary repository on GitHub; players submit, we
  review; the app offers "A glossary for <game> exists (N names) . Get" only as a quiet line the player may act on;
  nothing downloads without that click. Names + the players' own translations only (never an official localisation copied).
  (3) Models help (draft names from a game's wiki under its licence, attribution kept), never published without human review.
- Glossary only in games (owner, 2026-11-14, "very important"): the glossary UI (pill glossary button, the word card's dotted
  names and "Add to glossary", name suggestions) and the glossary's effect on translations appear ONLY on a game screen, never
  in a browser or a normal app. Lead's rule (one code path, strictest platform first): a screen counts as a game when no
  browser address bar is found on the frame (pipeline::without_chrome's detection, works on every OS) AND, where the OS tells
  the focused window (Windows, X11), that window is full-screen / borderless over its monitor and not a known browser; on
  Wayland (no focused-window info) the frame test alone decides. A glossary auto-picked by window title also marks a game.
  Each game its own glossary (owner, 2026-11-14): a game's glossary applies only to that game. Where the OS gives the
  window title (Windows, X11), the active glossary follows the game in front (its "Use for windows named" list); a game with
  no glossary gets none (never the previous game's). On Wayland the player's pick on the pill stays until changed (the pill
  shows which glossary is active, so a wrong one is visible at once).
  Merge, never replace (owner, 2026-11-14): a game's glossary is two layers shown as one: a BASE layer (a glossary received:
  a friend's file now, the community repository later) and the PLAYER's layer. Same term (case, spaces, accents ignored) =
  one row; a conflict keeps the player's translation (the base one stays underneath: "Reset to shared"); a term the player
  deleted stays deleted when the base updates; base updates add new terms and update only terms the player hasn't changed.
  Importing a file for a game that already has a glossary MERGES into it with these rules and shows a one-line summary
  ("12 terms added . 3 kept yours . 0 duplicates"). Merge-on-import ships in v0.2.0; the repository's base layer in 0.6.0.
- Community glossaries, easy upload first (owner, 2026-11-14): sharing a glossary must not need a GitHub account: "Share with
  the community" in the app sends it (player's click only, after showing what is sent) to a free relay (Cloudflare Worker free
  plan + Turnstile) that files a GitHub issue; a GitHub issue form / PR is the fallback. Garbage is kept out by automatic checks
  (GitHub Actions), human review before publishing, a signed index in the app, and the player's entries always winning. Plan in
  planning #181 (with Reach, 0.6.0). The relay needs the owner's free Cloudflare account then.
- Arabic -> English (owner, 2026-11-14, W16 report reports-0.2.0/w16-ar-en.md): v0.2.0 ships the pipeline fix (Arabic
  sentence split at . ! ? ؟, ، / ؛ cuts, vowel marks + tatweel stripped: COMET 0.852 -> 0.859). A bigger opt-in "Better Arabic
  quality" model (tc-big-ar-en CC BY 4.0 or tc-bible-big-sem-en Apache-2.0: COMET ~0.89, 309 MB, ~2x RAM, ~1.8x slower) comes
  in a LATER version, downloaded only on the user's click; target COMET >= 0.88 on the W16 set.
- Linux main-mode switch approved (owner, 2026-11-14): Linux main mode reads part by part like Windows in v0.2.0 (W12:
  English 3440 -21..-37 % time + missed lines 5 -> 0; English 1080 at parity within noise; French faster, -14..-22 % memory;
  Arabic/mixed far better reads; Arabic 150 % +28..36 % time accepted for the better reading). Proven on the Ubuntu VM in the
  combined pass before release. Open: Arabic small-text CER 0.231 vs 0.157 at a3e0594f (still far better than the old 0.439).
- v0.2.0 publish (owner, 2026-11-14, "publish directly, no problem"): the lead publishes v0.2.0 without asking again, once the
  release gates pass: clean-up pass, docs + notes, CI green, the update gate (v0.1.8 Setup + portable -> the draft on the
  Windows VM), About on the draft exe, the Ubuntu AppImage check, signed assets re-downloaded + verified. A broken updater
  blocks; other findings ship as known issues and are fixed next.
  Owner, 2026-11-14 (later): "publish now if there is nothing after the weak PC test". Lead's plan: W17 (#117) and W19
  (Ubuntu-pass fixes) merge, then one Windows VM session runs the combined checks + the release gate, then publish directly.
  #31 headings and (if unsolved) KDE Wayland 1280x800 Arabic reading ship as known issues.
- Glossary name suggester ships in v0.2.0 (owner, 2026-11-14; planning #180): local, quiet, game screens only: repeated
  capitalised words / names on a game's translated screens become suggestions; a dot with a count on the pill's glossary button;
  accept ("keep as is" / a translation) or reject in one click; rejected names are remembered per game and never suggested
  again; never a pop-up during play; nothing leaves the PC. Built from the approved glossary design's parts (no new look).
- Live translate screen border (owner, 2026-11-14, "approved", Claude Design canvas "Live translate border",
  .claude/artifacts/design/live-border/): option B "Quiet glow": the Active Look glow in the look's own two colours at ~60 % of
  main mode's depth (1080p: 14 px c1 at 0.36, 34 px c2 at 0.20; main mode 22 / 54 px at 0.42 / 0.26); static, drawn once,
  fades in with Live and out on stop, near 0 % CPU; replaces today's thin solid line. Needs a per-pixel-alpha live layer
  (Windows is colour-keyed today): the #123 capability table checks every platform first; a "no" goes to the owner.
- Ready libraries first, else our own reusable library (owner, 2026-11-15): for Live translation use an existing library where
  one fits; where none does, build it as a standalone open-source library the owner can reuse and share (own repo and name,
  MIT OR Apache-2.0, clean-room, like the glass library), OpenCircle only depends on it. Lead's search (2026-11-15): GPU runtime
  = the `llama-cpp-2` crate (utilityai/llama-cpp-rs, Rust bindings to llama.cpp, `vulkan` feature) instead of our own bindings;
  OS change hints exist (Windows DXGI duplication / WGC dirty regions, e.g. the `windows-capture` crate; PipeWire
  SPA_META_VideoDamage, optional per compositor; X11 XDamage) but a 3D game redraws its whole window every frame, so they
  can't find changed TEXT; no library does the live core (per-region change check on text boxes, patch lifetime "nothing
  stale", draw once, translation cache, engine-agnostic) -> that core becomes the new library. Reference apps to study (approach
  only): Translumo (Apache-2.0), OverTranslate (MIT), RSTGameTranslation (GPL-3.0, approach only). Name and repo: the owner's.
  Refined (owner, 2026-11-15, "better"): the live core is built INSIDE OpenCircle as one clean, engine-agnostic module (no
  OpenCircle code in it: OCR, translator, UI and OS plug in through small traits), so it can be split into its own repo in an
  hour or two; whether to publish it as a library is decided after 0.3.0, once it is proven on real games. No repo or name now.
- Live translation approach (owner, 2026-11-15, on W2's design docs/internal/reports-0.3.0/live-approach.md + baseline.md;
  every recommended answer): (1) GNOME Wayland: Live stays one-shot per press (no click-through layer is possible for an app);
  a GNOME Shell extension later only if GNOME users ask. (2) KDE Wayland + wlroots: a native wlr-layer-shell layer in 0.3.0,
  built after Windows + X11 meet every pass line. (3) Windows: the option-B glow is its own alpha window and shows in screen
  captures while Live runs (Windows refuses capture exclusion for alpha windows); the translated patches stay excluded;
  DirectComposition only if the owner asks later. (4) GPU runtime: a separate GPU helper exe (llama-cpp-2 + Vulkan), downloaded
  with the model on the first GPU Live (sha256 pinned); no GPU = nothing downloaded; prebuilt llama.cpp libs only as fallback.
  Lead defaults (owner may reverse): X11 without a compositor = patches + a thin solid edge line (no glow); text over moving
  backgrounds is never patched and not listed; an additive CLI `opencircle run live` for Wayland users to bind where no
  shortcut portal exists. Targets: text shown -> drawn <= 400 ms median / 800 ms p95 on the owner's PC class; 0 stale
  patches > 1 frame; 0 redraws of unchanged patches; game frame pacing unchanged.
- Linux Live sees the game window, not the screen (owner, 2026-11-15, on W5's finding: a screen capture on X11 / Wayland
  contains our own opaque patches, so text changing UNDER a patch goes unseen = stale patches, up to ~3 s on the choices clip):
  Live captures the game's own window: X11 through XComposite (as OBS does; no prompt), KDE / wlroots Wayland through the
  ScreenCast portal's window source (the system asks once which window). Windows keeps the screen capture (our layer is
  excluded there). The cost to the game on X11 (redirected window) is measured with the harness. Rejected: see-through dots in
  patches (visible holes), hiding patches briefly (flicker by design).
- Snip captures what you see (owner, 2026-11-16, after "snip couldn't capture the main widget or the AI widget when it is
  unpinned"): Snip started while the overlay is open (bar's Snip, Alt+A, tray) takes the screen exactly as it looks then,
  WITH the bar and an open (unpinned) AI chat in the picture, like "Snip the translated page"; the user then picks the bar,
  the chat or anything else. Snip's own toolbar / hints are never in the picture. Floating widgets stay pickable as before.
- Waking the owner's PC screen (owner, 2026-11-16, "next time you can move the cursor to wake the screen"; he controls the PC
  remotely and is away): for an OK'd run on his PC, a worker may wake the display with a tiny mouse MOVE only (e.g. 1 px and
  back, SendInput relative move) and then hold it on (SetThreadExecutionState). Still never a click, key press or any input
  that can reach a window (the 2026-11-14 SendKeys rule stands).
- First GPU download + AI part errors (owner, 2026-11-16, "i accept your 2 suggestions"): (1) the first GPU Live download
  (helper + ~1.2 GB model) never blocks Live: Live starts on the processor at once, the download runs in the background with
  the normal download card (progress, resume, retry), and Live switches to the graphics card by itself when it is ready.
  (2) AI translate (#90): a failed part stays in the original language for 0.3.0 (logged); a per-part error card is a later
  design round. "Offline first, AI only where offline can't" stays on hold until the owner says.
- GPU translation skipped for 0.3.0 (owner, 2026-11-16, "if the GPU model didn't create a real leap ... skip it for now; a loss
  for 6 GB VRAM users"; SUPERSEDES the GPU part of "Live translation approach" (4), "First GPU download" (1) and the 0.3.0
  "GPU inference and CPU path together"): measured gain was modest (COMET 0.871 vs 0.846, ~80 vs 107 ms per line; translation
  is a small part of Live's delay, OCR dominates) for ~1.6-1.8 GB of VRAM taken from the game and a 1.1 GB download. Live
  ships on the processor (OptimaMT). Hide, don't delete: the code stays, the setting "GPU for live translation" is hidden and
  Off, nothing downloads, the release workflow does not attach the helper. models-v4 stays uploaded. Revisit only if a
  measured leap appears (e.g. a much better or much smaller model).
- Whole sentences only, no caption strip (owner, 2026-11-16): Live translates and draws only a COMPLETE sentence / box text,
  once, when it has finished (a sentence over several lines is translated as one); no line-by-line translation (partial
  sentences mislead). Reading the text while it types is allowed only as a hidden background speed-up. The caption strip idea is
  rejected.
- Honest value verdicts (owner, 2026-11-16; in the owner's general rules too): say plainly when data shows no value / no big
  leap / not worth it, and say "drop it"; measure end to end before building a piece; disagree with numbers, then follow him.
- GPU translation dropped from OpenCircle (owner, 2026-11-17, "I agree for all"; SUPERSEDES "GPU translation skipped for
  0.3.0"'s "hide, don't delete"): the 0.3.0 clean-up DELETES the GPU path (gpu-helper/ crate, translate::gpu, platform/vulkan.rs,
  the CI gpu-helper job, the hidden setting row; the `live_gpu` config key is still read so older files load and is dropped on
  save) and the models-v4 release on OpenCircle-models (no released version uses it). Using a GPU for AI stays the user's own AI
  provider (Ollama, LM Studio ...), with no GPU code in OpenCircle.
  0.3.0 list verdicts (same day): #86 dropped; #42 / #41 (GPU / int8 OCR) and #20 (learned inpainting) not built: measured
  only if a lab test shows they could be a leap; #134 / #135 stay cheap measurements only; #137 skipped in 0.3.0.
- Lab test first (owner, 2026-11-17): before building anything that rests on an assumption, run a small lab test that
  measures whether it is worth it (where the cost / delay really is, what the best case could gain); only a measured leap gets
  built. Applies to every new idea, the owner's included.
- #42 / #20 (owner, 2026-11-17): #42 (OpenVINO GPU reading) agreed as stated: lab test after box memory, built only if it halves
  the remaining reading time on Intel hardware. #20 (learned inpainting) moves to a later version: first a measured lab test
  with its pros and cons (quality on real game screens vs today's fill, download size, time per picture, one-shot only or
  Live); today's erase-and-fill is "good enough, not great" and stays for now.
- Later, not 0.3.0 (owner, 2026-11-17, "later is fine"): #51 (hotkey conflict detection + game-mode pause) and #43 (games:
  watch area, battery, hints).
- One click on the owner's PC (owner, 2026-11-17, going to sleep: "can I rely on you?"): for an OK'd test run, a worker may
  send ONE left click into its OWN test window (the browser it started) to give it the foreground, only after
  WindowFromPoint at the click point returns a window of a process the worker started (checked right before the click); never
  a key press, never a click anywhere else; if the check fails, no click and the run stops. The mouse-move wake rule stays.
- KDE / wlroots Live controller (lead default 2026-11-17, owner asleep; owner may reverse): option C, leave it: the controller
  is a normal window under the layer-shell overlay; only a translated patch over text that happens to sit under the controller
  can cover it (Wayland gives no window position to cut a hole). Known issue in the notes; a second layer surface for the
  controller (~4-6 h) only if users report it. Windowed games on KDE: Live needs full screen / borderless there (plain message).
- Box memory off until it is cheap (lead default 2026-11-17, owner asleep; "nothing stale" first): W19's box memory halves the
  replay's shown -> drawn time (choices 366 -> 166 ms) but on the Windows VM it lost lines (fast 56 -> 29 of 66) and left one
  stale patch up to 2.24 s, because each in-place re-read paid a full OCR pass. It ships OFF; a follow-up makes the re-read
  cheap and turns it on only when the VM shows no loss and 0 stale. The typed-out (whole sentence) path also stays off. W15's
  85-120 ms "stale" on the owner's PC was a measurement offset (recording t0 130-180 ms early); in the app a patch hides
  10-20 ms (median) after the changed frame: re-measured with the player's own t0 at the owner-PC re-test.
- Streamed test clips (owner, 2026-11-17, after seeing YouTube captions in W28's run): every test browser on his PC opens in a
  private window (InPrivate / incognito) AND YouTube's captions (CC) are OFF (e.g. the embed / watch URL with cc_load_policy=0
  and no auto-captions, checked in the first frame before measuring): only the game's own text may be on screen; a run with
  YouTube captions visible is invalid.
- Glow only until the cosmetics version (owner, 2026-11-17): in v0.3.0 main mode AND Live draw only the Glow style (the soft
  all-round edge, fading in/out); every look keeps its own colours and Custom keeps the user's; the Horizon / HUD / Bezel styles
  are hidden (hide, don't delete: a config whose look uses them is drawn as Glow in that look's colours; the style choice
  returns in the cosmetics version with #183). The owner tries the Live border himself on a dev build and approves or rejects it.

- Plugins (owner, 2026-11-17, "perfect, I absolutely like that"): community plugins, Raycast-like: any language compiled to one
  .wasm (Extism candidate, sandboxed), the plugin DECLARES its UI and OpenCircle draws it with the kit, it gets the crop / OCR
  text / translation / clipboard, declared permissions approved once at install, entry points (bar, Snip More, own shortcut, tray,
  detected-things chips, a Ctrl+K command bar), an in-app store over a reviewed + signed community repository. A complete version
  of its own with its patches (number given at release; right after Reach, owner 2026-11-17: it reuses Reach's community repository, upload and signing; the versions after it shift by one). First a lab test (runtime size + RAM
  loaded only on use, cold start, a JS vs Python plugin's size); then the build. Design round now (W32: Jira, flashcards,
  converter examples; with the Snip Studio v2 design and one shared Ctrl+K bar).
  Reference to study (owner, 2026-11-17): github.com/kunkka19xx/look (Rust launcher, GPL-3.0: ideas and approach only, never
  its code, per "Learn from open source"); ideas worth keeping go to the plugins/hub planning card.
- Online translation service (owner, 2026-11-17): Translate gets a third engine beside the offline model and the AI provider: an
  online translation service, one-shot Translate only (not Live). Study, lab test (quality vs OptimaMT on the Arabic sets) and
  build start with AI online (0.4.0). The first prototype uses Google's free web endpoint (no key, no setup), in Labs only (owner:
  "google for now, it requires no config"), knowing it is unofficial and may stop or be blocked: on failure a plain message and
  the offline model, never a raw error. Text only, never the capture, and only after the user picks this engine. Which services
  ship for real (Microsoft / DeepL with the user's free key, Google ...) is the owner's later call.
- Live stays Beta in v0.3.0 (owner, 2026-11-17, "Beta"): Live translate stays behind Settings > Labs (Beta) for v0.3.0; the
  release notes list its measured limits (W34, owner's PC: old translation up to ~0.25 s after a change, typed text drawn line by
  line then whole, a static line over a moving 3D scene redrawn every few seconds, short fight banners / streamed lines often
  missed or late). The typed-out path stays OFF (W34: stale -48 % but partial draws and redraws not 0, and an unsettled player
  frame drop in 5 of 6 ON runs). Next Live work starts from the engine timing (the PC's stales don't reproduce offline).
- v0.3.0 draft REJECTED (owner, 2026-11-17, after trying it himself): "not even Beta; no release": on a still screen with fixed
  text the translations keep changing, and much of the text is not translated. The draft stays unpublished. Lesson: the synthetic
  harness clips passed what the owner sees fail; Live is judged on still real screens and real use first (W37: corpus screens held
  still = 0 changes, coverage vs one-shot Translate), then clips. No release until the owner tries Live himself and says it is good.
- Live page translation (owner, 2026-11-17, "1 yes, 2 yes, 3 of course"; SUPERSEDES the game-first scope of 0.3.0): v0.3.0
  becomes LIVE PAGE TRANSLATION for web pages and normal apps: the whole screen translated in one-shot Translate's quality (its
  pipeline, blocks as units), drawn once and steady on a still page; when the user scrolls, every translation moves with its
  original line (scroll offset measured like Long capture's stitcher) and only the newly shown strip is translated. Live GAME
  translation moves to a later version. Lab test first (scroll-offset accuracy on real pages, new-strip translate time, smoothness
  and CPU of moving patches, fixed headers, scrolling inside a box, late-loading content), then the build. The current live core's
  game heuristics (panel ring, typed-out, front bands) are not the base for it.
- No more failed features shown (owner, 2026-11-17, firm, "never show me a failed feature again; criticise, give a real verdict"):
  before the owner sees ANY Live build (dev build or draft), it must pass a REAL-USE ACCEPTANCE TEST: a scripted human session on
  real pages / apps (read, click links, Back/Forward, switch tabs and apps, scroll fast and slow, menus/popups, hover, selection,
  zoom, a video on the page), frames recorded, judged per moment by strict criteria (0 stale from a previous page or position, 0
  flicker or redraw on still content, 0 leftover original text, 0 blank erases, coverage at one-shot Translate's level, correct
  placement after scroll, time to translated after navigation/scroll), plus a separate critic review that tries to break it. The
  lead gives the owner a harsh, honest verdict; synthetic clips alone never qualify a build. Live page translation must also
  handle navigation between pages (new page = old translations gone in the same frame), not only scrolling.
- Owner's PC tonight (owner, 2026-11-17, going to sleep: "you can use the PC, I'm asleep and won't touch it"): workers may run
  Live acceptance tests on his PC tonight without asking again, under every standing owner-PC rule (backups + hash lists, Run
  value, scheduled-task start, no key presses, only the 1 px wake and at most one checked click into their own window, private
  window with YouTube captions off, check no other agent's test is running, restore byte-identical). Valid for this night only.
- Live hidden, v0.3.0 = AI core (owner, 2026-11-17, "B, start the AI core, then the Studio, then come back and create this new
  engine; v0.3.0 will be the AI core"; SUPERSEDES "Live page translation" as 0.3.0's scope and the version plan's order): Live
  translate is HIDDEN entirely (Labs row, shortcut, bar/tray items; hide, don't delete: code and config keys stay). Order:
  v0.3.0 AI core (AI online) -> Snip Studio -> the new Live engine (page mode per W38's design + game "watch area"). The page-mode
  work (W40/W41/W42 branches, W37/W38 reports, the acceptance criteria) is kept for that later round. The v0.3.0 draft built on
  85fde993 (old Live) is void.
- v0.3.x split + Linux first + lean testing (owner, 2026-11-17, "very logical"): v0.3.0 = AI chat widget polish (#126 select +
  copy, code blocks with editor-like colours + previews, #127 Ctrl+= / Ctrl+- / Ctrl+0 scale, #128 no forced auto-scroll, #172
  X11 composer focus + GNOME Wayland chat placement, #170 remainder) + the work already done since v0.2.0 (Live hidden); then one
  small version per feature: v0.3.1 online translation (Google) in Labs, v0.3.2 one-click OpenRouter sign-in, v0.3.3 web search.
  MCP (#129) and the other extras later. Development and testing: LINUX IS THE BASE (Ubuntu VM / local), WINDOWS ONLY FOR THE
  FINAL TEST (the release gate incl. the update gate). Fewer tokens, faster work: the owner's own trying is the main evaluation;
  agents run lean checks (gate, a short scripted smoke for obvious breakage), no long measurement rounds, no big harnesses,
  unless the owner asks. SUPERSEDES the heavy parts of "No more failed features shown" (keep its spirit: never present
  something agents already saw broken) and "Windows first (2026-10-24)".
- v0.3.0 chat polish go (owner, 2026-11-17): new dependencies approved: `iced_selection` (MIT, per-block text selection in
  answers) and `syntect` (MIT, code colours from the kit's tokens). No separate design round (existing kit parts), no long lab
  or harness tests: gate + a short Linux smoke, dev build to the owner early; more than two workers allowed for this.
- Version plan (owner, 2026-11-17, approved; SUPERSEDES the order of every earlier version plan for what follows v0.2.0): ONE
  FEATURE PER VERSION, each finished (tests, the owner's own trying, his OK) before the next; board cards planning #184-#196
  (Order 1-13). v0.3.0 AI chat polish (+ work done since v0.2.0, Live hidden) -> v0.3.1 online translation engine (Labs) + Retry
  pill -> v0.3.2 one-click OpenRouter sign-in -> v0.3.3 web search -> v0.4.0 Snip Studio window -> v0.4.1 Ctrl+K in the Studio ->
  v0.4.2 Steps in the Studio + picture/steps export -> v0.4.3 animated steps export (GIF/MP4/WebM) -> v0.4.4 actions on the
  picture -> v0.4.5 edit text in the picture -> v0.4.6 cut out (model test first) -> v0.5.0 Live page translation (new engine)
  -> v0.5.1 game "watch area". After that the versions already planned (Reach, Plugins, ...), each split the same way.
- Never idle waiting for the owner (owner, 2026-11-18, angry that work stopped after the v0.3.0 dev build): while the owner tries
  a build (or sleeps), the next versions in the plan are BUILT IN PARALLEL on their own branches; his trying gates the RELEASE of
  each version, not the start of the next one's work. Stop only for a decision only he can make (and then notify him).
- Web search source (owner, 2026-11-18, "search with Bing; I will change it if we get blocked; users must not bother with APIs"):
  v0.3.3 web search uses Bing's web results with no key (W49: 40/40 OK with an honest client; DuckDuckGo, Brave, Startpage and
  Mojeek blocked every query), plus Wikipedia's official API as the keyless fallback for facts. Honest client only: never pose as
  a browser, never solve or dodge a captcha; when blocked, a plain message and the answer without search. One small source
  interface so the owner can switch the engine later.
- All of v0.3.x in v0.3.0 (owner, 2026-11-18, after testing the combined build; SUPERSEDES the 0.3.1-0.3.3 split): the chat
  polish, the online (Google) engine in Labs + Retry pill, Connect OpenRouter + LAN http + free-key links, and web search (Bing +
  Wikipedia) ship TOGETHER as v0.3.0 (one Windows final test + update gate + draft, publish on his word). Lesson: when the owner
  asks for "the final / whole thing", give him the build with EVERYTHING finished, never a partial "release candidate".
- Online translation leaves Labs (owner, 2026-11-18, "it is stable"): the Online (Google) engine is a normal Translate engine
  choice (AI | Local | Online) in v0.3.0, no Labs switch; same rules (text only, one-shot only, offline fallback, plain message).
  The `[labs] online` key is still read (dropped on save, migrate never break).
- Manual pin closes the overlay too (owner, 2026-11-18, "do it"): pinning the AI chat by hand (header pin button) behaves like
  the follow-up pin: the overlay (frozen screen + main bar) closes by itself and the chat stays pinned above other apps; same
  guards as the follow-up close (never while a menu / card / download / song search / drag / running action is open: it closes
  right after).
- v0.3.0 publish (owner, 2026-11-18, "publish, and on VM, I will start it now"): the lead publishes v0.3.0 (the AI core) without
  asking again once the release steps pass: notes + docs + clean-up (W66), CI green, tag -> draft, the update gate on the Windows
  VM (v0.2.0 Setup + portable -> the draft), About on the draft exe, signed assets re-downloaded + verified. A broken updater
  blocks; other findings ship as known issues.
- Web search source changed (2026-11-18, SUPERSEDES the Bing part of "Web search source"): W62's lab showed keyless Bing returns
  random pages (~25 % relevant); v0.3.0 uses DuckDuckGo lite (POST, >= 2 s apart, 15 min rest when throttled) then Wikipedia,
  no key; Bing removed. ONNX Runtime mirror (2026-11-18): github.com/microsoft/onnxruntime returned 404; the exact pinned runtime
  files are mirrored on OpenCircle-models release runtime-1 (same sha256) and the app tries the mirror first, Microsoft second.
- v0.4.0 = the first redesign (owner, 2026-11-18): the app grew complicated, so v0.4.0 redesigns it: Settings v2 and the Snip
  Studio on ONE shared Windows 11 style frame, AND no pixelated / low-quality shapes anywhere (the Copied pill and every other
  notice, pill and popup must be as smooth as inside a normal window, at every scale, on every OS). Work uses the SDLC Kit
  (/home/az_work/MainHub/SDLC-Kit, RULES.md).
- Settings v2, round 1 verdict (owner, 2026-11-18): the new title bar + collapsible icon sidebar are APPROVED; the selected
  sidebar item keeps TODAY's selected effect (filled rounded accent-tinted item, no elevated card, no look-coloured bar) and
  every colour stays ours (no new colours). The pages are NOT approved: "put more effort in the redesigning; simplify and make
  it look way better": change the layout, order and organisation of pages and rows, never the colours.
- Report a problem = a popup (owner, 2026-11-18): no "Something not working?" card on About; the sidebar foot item "Report a
  problem" opens a small dialog (Copy diagnostics, Report a problem on GitHub, nothing is sent; X / Esc closes).
- "AI and Search" (owner, 2026-11-18): the Settings page "Ask AI" is renamed "AI and Search" (same content: answers from,
  provider, model, key, AI chat, web search). Round 2 pages: "more optimised and prettier" asked (round 3).
- Settings = the owner's approved design logic (owner, 2026-11-18, "bro I gave you a design, this looks too far from it"):
  rounds 2-4 strayed. Settings v2 follows his other project's approved Part 1 design logic exactly (Windows 11 frame around
  web-admin pages: page header 28/36, cards with caps captions, label-above fields with hints, Windows On/Off toggles, segmented,
  info bars, status pills, tables, section lists, empty states), in OpenCircle's colours. Reference: .claude/artifacts/design/
  settings-v2-ref/REFERENCE_SPEC.md + part1-2026-11-18/.
- Settings v2 = DRAFT, not scheduled (owner, 2026-11-18): round 5 "looks good"; the sidebar becomes exactly today's sidebar
  (CORRECTED: round 1 sidebar with icons + collapse, today's selected effect); then the design is kept as a draft and NOT built: the owner decides
  later when to apply it. All v0.4.0 work is paused until the owner says continue (W71 Studio branch and W72 notices paused).
- v0.4.0 resumed (owner, 2026-11-18 night, after v0.3.0 was published): v0.4.0 = the Snip Studio window (W71's branch) + crisp
  notices (W72, capability table first) + the Start menu / "Open with" entries once the Studio's name is chosen (the user-facing
  name lives in ONE string-table entry). Settings v2 stays a draft: nothing of it is built. Then v0.4.1-v0.4.6 in order, the next
  ones built in parallel on branches while the owner tries a build.
- Walkthrough video formats (owner, 2026-11-19, "ok for 3 types"): v0.4.3 exports GIF, MP4 and WebM; MP4 and WebM are AV1 through
  rav1e (BSD-2, ~+1 MB), H.264 is not used (licence). GIF stays the format that opens everywhere.
- Studio name (owner, 2026-11-19, SUPERSEDES the name in "Snip Studio name and behaviour"): the Studio is "OpenCircle Studio"
  everywhere users see it (window title, Start menu / Windows search, "Open with", Linux desktop entry); Arabic «استوديو
  OpenCircle» (lead's default under the Arabic rules; the owner may correct). Code names stay "Studio". One string (st_title).
- Cut out (owner, 2026-11-19, "go with recommended", on W80's lab reports-0.4.x/w80-cutout-lab.md): v0.4.6 Cut out = a quick
  no-model fill for plain backgrounds always (instant, nothing downloaded) + IS-Net general-use (Apache-2.0, 179 MB, ~0.7-1 s and
  ~830 MB RAM per cut on the CPU, freed after) for other backgrounds, downloaded only after the user confirms it on first need
  (size shown), sha256 pinned, hosted on OpenCircle-models. Before shipping: a check on ~5 real CC0 photos.
- No Ctrl+K in the Studio (owner, 2026-11-19, SUPERSEDES the Studio's Ctrl+K field/command bar in STUDIO_V2_SPEC.md and the
  v0.4.1 "Ctrl+K in the Studio" version): the Studio has no search bar. The Raycast-style launcher the owner asked for is a
  SEPARATE new feature (the hub / plugins launcher, option A on the Plugins canvas), not part of the Studio; it was misread.
- Studio rework (owner, 2026-11-19, after trying the v0.4.0 build: "not finished, needs more work"): the Studio must behave like
  familiar editors (Ctrl+C copies the SELECTED object, Ctrl+V pastes it; nothing selected = the whole picture), one standard
  container size for every options-bar control, panel toggles in a familiar place (not beside the caption buttons), no empty
  side panels, and real Studio settings (export defaults, behaviour, import ...). Research established editors first, then a
  design round on the canvas, then one complete build.
- Studio v3 design approved (owner, 2026-11-19, "approved", canvas https://claude.ai/artifact/3x4ShGjBgD5TNZBXSMCRPu,
  .claude/artifacts/design/STUDIO_V3_SPEC.md): build it as drawn, with both lead defaults: (A) Ctrl+C / Ctrl+X act on the
  selected object(s), nothing selected = Ctrl+C copies the whole picture, Ctrl+Shift+C always the whole picture, the Copy
  button always the whole picture (narrows "Copy = button AND Ctrl+C" for the Studio only); (B) "Keep unsaved work if
  OpenCircle closes unexpectedly" is a Settings > Studio switch, OFF by default. Group / lock / align / print stay later.
  The owner gets ONE complete build (v3 + steps export + walkthrough + picture actions + edit text + cut out) to try.
- Studio v3.1 layout (owner, 2026-11-20, "I approve everything you said", after trying dev build 9c88010): ONE sidebar only, for
  Steps (add several pictures at once: multi-select Open, drop many files, paste; click = edit that step; drag reorders; Export at
  its foot); the tools rail always at the window edge, the sidebar on the other side; the options bar keeps today's relation to the tools rail (the rail is a full-height
  column at the edge, the options bar starts beside it) and runs over the sidebar to the other edge (only the sidebar starts
  below it; owner, 2026-11-20 correction), is never empty and shows ALL controls always (the shared ones colour / size /
  opacity / outline-fill first; ones that don't apply are dimmed, not hidden), then the tool's own, then "Actions" (Copy text,
  Translate, Ask AI, Edit text, Cut out) as a dropdown; the panel toggle only in the sidebar's header (+ View menu, Ctrl+B), none
  in the status bar. Removed panels: Recent files -> File > Open recent + the empty Studio's start screen; Objects list -> dropped
  (canvas selection + right-click); picture info -> a popover from the status bar's size chip; object X/Y/W/H -> a Position
  popover in the options bar; export defaults -> Studio preferences + the Save as sheet. Popovers for small glance-only things,
  a right slide-over for big temporary tasks (Export walkthrough, Edit text, Cut out), centred dialogs for Preferences and the
  shortcuts list. Studio preferences = a dialog inside the Studio (Ctrl+, / File > Preferences; tabs General, Appearance,
  Saving and export, Copy and paste, Editing, Shortcuts), one source with Settings > Studio (which keeps one "Open Studio
  preferences" button); Theme: Follow OpenCircle (default) or any built-in theme; also Steps sidebar side, show it on start,
  start screen. Menus and the context menu close on any outside click, Esc or a pick. Zoom: Ctrl+= / Ctrl+- / Ctrl+0 scale the
  Studio's UI (like the AI chat), the mouse wheel zooms the PICTURE at the pointer, middle-button drag pans (Photoshop), Space+
  drag still pans. Window buttons keep their normal place in Arabic in EVERY OpenCircle window (SUPERSEDES "window buttons top
  left on every OS" of the Arabic UI design). Snip toolbar: a visible "Open in OpenCircle Studio" button (frame + pen icon,
  Ctrl+E, before Copy / Save), a one-time hint "Edit more in OpenCircle Studio", double-click on the picked area also opens it.
  Walkthrough video: one transition for all steps (Cut, Crossfade default, Slide, Push, Zoom, Wipe) + duration 0.3/0.5/1 s; the
  sheet shows the expected GIF size. Cut out model upload "cutout-1" approved.
- Snip enhancements, LATER (owner, 2026-11-20): a pro picker with Snagit's logic, not its look (magnetic edges that catch
  reliably, element / window picking that finds buttons, panels and menus, wheel steps element -> window -> screen); and
  drawing inside a snipped area stays inside it: text wraps to a new line before the area's edge, shapes / strokes stop at the
  area's last pixels so the whole shape shows (the Studio keeps this when "Grow the canvas" is off).
  Tray menu at the click (owner, 2026-11-20, same phase): when a tray tool (e.g. Windhawk's tray flyout) shows our icon
  somewhere else, our menu still opens at the icon's ORIGINAL taskbar spot, because it is placed at Shell_NotifyIconGetRect's
  rect (tray.rs on_click); open it at the click point (TrayIconEvent's position / the cursor), like the shell's own menus.
- Studio title row in Arabic (owner, 2026-11-21, after e4cc590; refines the Arabic mirroring for the Studio header): the title
  row is NOT mirrored: logo, File, Edit, View, gear stay at the left in the English order, the title in the middle, the window
  buttons at the right, exactly like English; only the menus' contents are Arabic / right-to-left.
- Next release, beside Snip enhancements (owner, 2026-11-21; planning #200-#202): (1) a right-click menu on text selected in
  main mode (Ctrl+Shift+Space) with actions on just the selection (translate only it, and more); STUDY first what to offer and
  where else the same logic applies, list to the owner before building; (2) Ctrl + mouse wheel resizes the selected shape in
  the Snip widget and the Studio.
- Release commands for v0.4.0 (owner, 2026-11-21, "change it, I allow you"): the SDLC guard blocks `gh release` / tag pushes
  in a session not started with SDLC_OWNER_RELEASE=1; for v0.4.0 the owner allowed the lead to make the release changes
  (notes into the draft, publish after the gate passes) through `gh api` instead. Valid for v0.4.0 only; ask again next release.

- Owner try of dev 4949720, round 4 (2026-11-25): "tag it", then held while he tested; NOT tagged (the kit guard also blocks a
  tag push from this session: the owner pushes the tag or starts a session with SDLC_OWNER_RELEASE=1). Works: Snip row, centred
  search + Ctrl K, unmirrored title bars, Steps strip, Report popup, onboarding tabs, first crop. Release blockers:
  - Translate on SELECTED text (menu + bar R) = IN PLACE: the translation replaces the selected text on screen with the one-shot
    erase-and-redraw, translate pill + Original + Esc, Ctrl+C copies the translation (owner expected this; SUPERSEDES the lead's
    2026-11-25 card). W112.
  - Colour picker (Studio "+" and Snip, one kit part) = the familiar one: saturation/brightness square + hue slider, eyedropper,
    recent colours, preset swatches; hex optional, never the main control; no styles/presets in the picker. PNGs to the owner
    before merge. W112.
  - Settings search turned the Arabic UI English. W112.
  - Drawing tools lag and don't follow the exact pointer path (Snip + Studio, every use); a pen stroke changes an existing Hide's
    look; a new shape briefly shows a frozen copy of the picture inside it; the options row keeps the OLD tool after a click on another tool (shot round4-stale-options-row.png). Full tools health check. W111.
- Next release Snip additions (owner, 2026-11-25, "yes pls"): R-08 scroll up over a window = whole screen + clearer Enter hint;
  R-09 Long capture on the main Snip toolbar; R-10 one Pin button (click = still pin; options row Pin | Live pin); R-11 right-click
  on the picked area = a short menu built from More's list (right-click mid-stroke cancels; shape menu later); R-12 live pin
  click-through: hold Ctrl over it = clickable, a mark + tooltip (lead default: next release, Labs feature, not a 0.5.0 blocker).
- Colour picker APPROVED (owner, 2026-11-25, on w112-picker-*.png): saturation/brightness square + hue bar, before | now preview,
  eyedropper, small optional hex read-out, 8 recent colours, preset row; one kit part for Snip, the Studio and Settings.
- Antigravity Neon (owner, 2026-11-25, "it is too good, reliable, fast"; SUPERSEDES "from Google use ONLY Argon" and the
  Antigravity marketing-images-only scope): Antigravity's Neon model may be used as a worker. First job: redesign the WHOLE app
  (brief docs/internal/briefs/design-antigravity-whole-app.md): phase 1 = a design system sheet + 5 sample screens for the
  owner's OK, then every screen. Antigravity is a GUI IDE on the Windows host with no headless agent command, so the owner
  starts it with the brief; the lead can't dispatch it. Workers: at most 2 at a time (owner, 2026-11-25), counting Antigravity.
- Recording widgets APPROVED (owner, 2026-11-25, "perfect", canvas [private link: AGENTS.local.md] = the R1/S2
  redraw, renders .claude/artifacts/design/render/out/w101/recording-r1s2-*): build exactly these states in the next release.
- Pick a picture on the page (owner, 2026-11-25, "yes"): next release, R-13; lab test first.
- Hide default (owner, 2026-11-25, "soft, but something secure also, I want it to look pretty"): the default Hide style is a NEW
  secure "Soft" built only from the area's surroundings/background (never the hidden pixels), proven unrecoverable with W100's
  candidate attack; the old content-reading Soft becomes the look style "Blur (look only)". W114 (brief rel-w114-secure-soft-hide.md).
- Owner try of dev e193719, round 5 (2026-11-26): Soft Hide + tool fixes good; 4K speed improved. Blockers: no live picture previews
  in the Hide row (fixed sample swatches instead); Studio Magnifier needs outline / colour / zoom controls and is still slow;
  one-word en->es Translate failed with "system error number 13" loading the model + a wrong error card (raw path, "Set up AI");
  after an in-place translation nothing else works until Esc (must allow a new selection and translating the whole screen).
  W116 (translate) and W117 (Hide row + Magnifier, on Antigravity). Snip captures our recording widgets only when the user
  hovers and clicks them; the recording bar is never inside the recorded video (existing rules).
- Snip toolbar Studio button (owner, 2026-11-26): the "Open in Studio" button uses the OpenCircle logo instead of the Studio icon.
  Queued for W117's next round.
- Colour picker bug (owner, 2026-11-26): the square does not follow the pointer (handle and colour land elsewhere). Blocker; W117.
- Settings full design (owner, 2026-11-26): a Claude designer (not Antigravity) draws the WHOLE Settings from the approved parts
  (shell, AI and Search sample, B2, A3+A5 preview), inspired by CA Catalog Orchestrator, plus 5 more sidebar styles; on a canvas
  for his pick. W118. No dashboard (confirmed again). Roadmap 0.6.0 gains R-14 right-click v2b, R-15 better Arabic reader,
  R-16 Studio selectable text; L-03 Steps strip marked done (0.5.0).
- Owner 2026-11-26 (round 5 follow-ups):
  - Show original: while the original is shown (the pill's Original toggle), NO dim over the screen; the dim returns with the
    translation. Queued with the W116 follow-up.
  - Archive the old designs (gitignored, so archived outside the repo, not deleted) once W118's design lands.
  - After W118: a design of the CURRENT real floating widgets only (bar, translate pill, Snip toolbar + row, picker, pills/toasts,
    AI chat, Steps strip, word card, menus, update widgets, recording widgets): the widgets alone, no full-screen backgrounds. W119.
  - The slow one-word Arabic translate: not yet measured; measure per stage (model load, OCR, translate) before 0.5.0.
- Old designs ARCHIVED (owner, 2026-11-26): the October canvas (design/project), studio-v3, the settings-v2 round renders and
  backups moved to /home/az_work/MainHub/Archive/OpenCircle/designs-2026-11-26/ (README there). Whole Settings design (W118):
  canvas [private link: AGENTS.local.md], source .claude/artifacts/design-settings-full/.
- Settings design round 2 (owner, 2026-11-26, on W118's canvas): the Appearance theme cards are disliked (redesign); EVERY
  feature page with a preview clip shows its preview (A3+/A5+); the B2 choice-card look is used for every comparable choice; the
  floating-card sidebar (style 2) is chosen and the CONTENT area becomes a floating card too; the "Settings › Page" breadcrumb
  goes (lead: keep a small page title + one line); look again at CA Catalog Orchestrator's Settings and borrow more where it fits.
- Settings search (owner, 2026-11-26; SUPERSEDES the Ctrl K keycaps in the shell's search): the search field is wider (still exactly
  centred), shows NO Ctrl K keycaps (the key still works; tooltip only), and on open with nothing typed shows a floating card of the
  most-used settings as quick picks with their current values; typing shows results.
- Settings design APPROVED (owner, 2026-11-26, "design is awesome and all recommended approved"): round 2 canvas
  [private link: AGENTS.local.md], source .claude/artifacts/design-settings-full/r2/ (r2.py), with every
  recommended answer: Appearance option 1; drop the invented "Search opens" row; Studio's "Open pictures in" lives in Snip as
  "After a capture" (Studio links to it); Most used = the drawn list, later ordered by real use; sidebar card 246 px; Studio demo
  link hidden until a clip exists. Built in the next release (0.6.0), in Rust + iced (lead recommendation, see below).
- Widget style (owner, 2026-11-26): style E "two-tone, grouped segments" (W119 canvas [private link: AGENTS.local.md])
  for the whole widget family; the Snip options row stays ATTACHED under the toolbar as one piece (not the drawn gap).
- Translate warm-up on the right-click menu (owner, 2026-11-26, "ok, but only the main lang"): opening the right-click menu on
  selected text warms ONLY the model pair into the user's main language (the translate target), and only when the selection's
  language differs from it; nothing else is pre-loaded; the warm model is freed after ~1 min unused (existing warm rules).
- Studio colour picker (owner, 2026-11-26): looks IDENTICAL to the Snip widget's picker: no extra outer container / x around it
  (shot owner-shots/2026-11-25/round6-studio-picker.png); one kit part, one look. Closes like Snip's (outside click / Esc).
- Studio design (owner, 2026-11-26, on W122's canvas VgkFac5sDW1nZ4e5dKExLi): the LAYOUT is approved; the left tool rail needs
  other ideas (round 2: several rail styles, everything as floating widgets). The drawn slider panel is NOT what he meant (it is a
  floating card); his meaning is pending: edge drawer / bottom filmstrip / hover rail.
- Studio slider APPROVED (owner, 2026-11-26, "the current slider is great"): build W122's slider now in today's Studio look (0.5.0)
  with the recommended answers (overlay, remember open/pinned per window, closed by default, 320 px, no close while dragging a
  step, Ctrl+B/Esc/handle/outside click). Studio window bug: it sometimes opens partly above / outside the screen; it must open
  centred on its monitor, sized within the work area (taskbar excluded), like any normal program (fine in full screen). W123.
- Studio Preferences header (owner, 2026-11-26): keeps TODAY's header exactly: no search, no Ctrl K, no extra chips; only the
  content takes the Settings round-2 design.
- Tips (owner, 2026-11-26): in 0.6.0 with the new design (style E) + the "What can I do?" card; every tip shown ONCE per user,
  existing users included (the first time the feature is relevant after the update); "Don't show tips" turns all off. R-17.
- 0.5.0 APPROVED by the owner (2026-11-26, "app approved") on the final test build 2143ff0 (main 2143ff0d, CI green): all four
  test areas good. Next: W124 final VM pass, notes (docs/internal/release-notes-v0.5.0.md updated), then the tag (owner pushes it or
  restarts the lead with SDLC_OWNER_RELEASE=1), signed draft, update gate on the draft, publish on his word.
- Studio direction (owner, 2026-11-26): keep TODAY's Studio design as the base, made SOFTER; no bottom bar: its parts become
  floating pills (size + Picture info; zoom + Save + Copy); the main content area is normal (not a card/widget); only some things
  float on it (the slider, some options). Shots owner-shots/2026-11-25/round7-*.
- Studio slider control (owner, 2026-11-26): ONE control only: the edge handle with its arrow (and Ctrl+B); remove the separate
  panel toggle button from the options row. Applies to the 0.5.0 build too (follow-up) and the 0.6.0 design.
- Studio "Picture info" (owner, 2026-11-26): REMOVED completely (button and popover); the bottom-left keeps only the size chip.
  0.6.0 design; also removable from the 0.5.0 build with the next Studio follow-up.
- Studio pin jump (owner, 2026-11-26, shots owner-shots/2026-11-25/round8-pin-*): pinning the Steps slider while zoomed moves the
  picture; pinning must keep the visible part of the picture where it is. Fix in the NEXT release (owner). Roadmap R-18 with the
  cut-off one-word translate and the two Studio removals.
- 0.5.0 publish decision (owner, 2026-11-26, "yes"): publish as it is once the update gate passes; the cut-off one-word translate and
  the pin jump are NOT added to the notes (the owner fixes them directly in the next release). Studio round 6 is liked; the tool rail
  gets options (round 7).
- 0.5.0 release (2026-11-26): tag v0.5.0 pushed by the owner on 2143ff0d; signed draft verified (SHA256SUMS + minisig "OpenCircle v0.5.0");
  update gate W125 PASS on Windows (Setup + portable 0.4.0 -> draft, What's new, About v0.5.0, Setup /quiet). BLOCKER found: the
  AppImage ignores CLI arguments (the Studio desktop file is picked by AppRun) = frozen CLI broken -> W126 fix, then the owner
  re-tags (delete tag + draft), the gate re-runs incl. AppImage CLI. Owner said "publish" before this finding; lead read "+" as fix first.
- GPU renderer lab (owner, 2026-11-26): the "simple way" = iced's wgpu renderer with automatic tiny-skia fallback; the owner tests a
  lab build himself on his GPU PC (VMs have no GPU). The "draw the stroke as an overlay" idea is REJECTED (feels less native). W127.
- Studio view jump (owner, 2026-11-26): opening Preferences in the Studio moves the picture too (same as pinning the slider); one fix in
  the next release: the visible picture stays put whenever a panel opens/closes (R-18).
- Studio tool bar (owner, 2026-11-26): round 7 option 3 "Group chips" (each tool group its own small rounded tile, no container),
  inside the approved round-6 Studio (soft, plain canvas, floating bottom pills, one slider handle, no Picture info).
- Studio view stays put (owner, 2026-11-26, firm, "omg"): the picture's position/zoom changes ONLY by a deliberate user action (drag to
  pan, wheel/scrollbars, zoom keys/controls, Fit). A right-click on the picture also moved it today; pinning the slider and opening
  Preferences too. Next release: find every path that resets or recentres the view and remove it; a test per path (right-click,
  pin, Preferences, panel open/close, window resize keeps the visible picture centre and zoom).
- Version plan (owner, 2026-11-26): 0.5.0 is published first (AppImage fix, re-tag, gate, publish). Then **0.5.1** = R-18 follow-ups
  (the Studio view moves only by user action; one-word translate never cut off; remove the extra slider button + Picture info),
  the GPU renderer (after his lab-gpu test), the better Arabic reader (R-15) and Studio selectable text (R-16, a small feature: owner's
  exception to "features bump the middle number"). **0.6.0** = the new design (Settings r2, widgets style E, Studio r6 + group
  chips), tips, recording, Snip/menu additions (R-08..R-14).
- OpenRouter sign-in (owner, 2026-11-26: "should look different; I almost didn't recognise it"): in the 0.6.0 Settings design the
  sign-in is a clear CONNECTION CARD at the top of the provider section with 4 states: not connected (one strong "Sign in with
  OpenRouter" + what you get; model/key/test rows hidden until connected), waiting (finish in your browser, spinning logo, Cancel),
  connected (green status "Connected · key saved on this PC", model, quiet Test / Disconnect), failed (calm error card, Try again).
  Name only, no third-party logo. Design round first (queued for the next free slot), owner OK before building.
- Antigravity (owner, 2026-11-26): rely on Antigravity (agy, Gemini Neon) for CODING and TESTING jobs (not for Settings/design work,
  which stays with a Claude designer per the owner's earlier word).
- AI setup paths (owner, 2026-11-26, SUPERSEDES the 2-option "Browser | AI provider" B2 cards): "Answers from" has THREE choice cards:
  Browser (no setup) · "OpenRouter account" (no "Recommended" badge; its own feature: one-click sign-in connection card, then only Model + Test /
  Disconnect) · Your own provider and key (advanced: provider dropdown, Custom server URL, API key, model, test). 0.6.0 design.
- GPU lab 1 result (owner, 2026-11-26): crop instant (present cost solved) but the whole picture flashes on each change, a dragged
  shape vanishes until drop, nothing follows the cursor: new image Handles per change = new texture uploads on wgpu. Lab 2
  (Antigravity Neon, owner: "these kind of things you can rely on Neon"): stable textures + the dragged item drawn as GPU geometry
  with identical pixels on release (test). Branch lab-gpu2, worktree ../worktrees/lab-gpu2, brief inside it.
- Standing release authorization (owner, 2026-11-26, "I want it always"): orchestrator sessions may tag, draft and PUBLISH OpenCircle
  releases without asking again, ONLY after every gate passes (CI green incl. audit; signed draft SHA256SUMS + minisig with the right
  trusted comment; update gate previous Setup + portable -> draft; About on the draft exe; Linux AppImage CLI; the owner's own test of a
  build with visible changes). Any failure = it stays a draft. The kit guard still needs SDLC_OWNER_RELEASE=1 per session; the owner
  sets that himself (the lead may not change its own guard). Workers never tag or publish.
- Workers (owner, 2026-11-26, SUPERSEDES "Two workers at most" and "Max 2 workers"): preferably 1 Antigravity Neon + 2 Claude
  workers at a time, never more than 2 Neon + 2 Claude. Neon does the coding (agy pattern in AGENTS.local.md); when Neon's quota
  runs out, a Claude builder takes over the same worktree.
- 0.5.1 speed (owner, 2026-11-26, "small release, make it faster"): workers run in parallel and each feature merges as soon as it
  is done; R-15 is trimmed to (1) mixed Arabic/English lines as one line, pieces ordered right to left, and (2) Windows' Arabic
  recognizer (ar-SA) where installed; PP-OCR Arabic tuning, bold-Arabic spaces and mirrored brackets move to a later version.
  R-16 runs beside R-18 (R-18 owns the Studio view code, R-16 a new text-select file).
- 0.5.1 publish without the owner's test (owner, 2026-11-26, "if I don't respond I'm sleeping; you can still publish, I will
  fix any issues in a different session"): for v0.5.1 only, the owner's own test of the build is waived when he doesn't answer;
  every other gate still runs (CI, signed draft, update gate 0.5.0 Setup + portable -> draft, About, AppImage CLI). The kit
  guard still needs SDLC_OWNER_RELEASE=1 (or the owner's own commands) for the tag and the publish.
- Build output never piles up (owner, 2026-11-26, "this should never ever happen", after 385 GB of old Rust build folders): every
  worker builds in `~/.cache/opencircle-targets/<branch>` (CARGO_TARGET_DIR in every brief, never a target/ inside a worktree);
  `docs/internal/clean-builds.sh` deletes any build folder idle > 12 h (never main's target/, never one in use, never source) and
  runs at the end of `docs/internal/merge.sh`; the lead removes a worker's worktree, branch and build folder right after its merge.
  Over 100 GB total = the script warns and the lead tells the owner.
- Test build at once (owner, 2026-11-26, "make it a rule, I will test with you in parallel"): as soon as a release's features are
  merged and CI has built main, the lead sends the owner the Windows build (CI artifact) with a short what-to-try list. The owner
  tests in PARALLEL with the VM pass; the lead never holds the build back until the VM pass or anything else is done.
- Owner try of the 0.5.1 test build 2177b5b (2026-11-26): GOOD: the Studio view no longer moves, the calmer Studio (no slider button,
  no Picture info), Studio selectable text ("perfect"), one-word translate ("great"). BAD ("super bad"): drawing tools in the Studio
  on a 5K picture: the delay is "insane", and the picture's pixels visibly change while drawing or dragging a rectangle / any shape,
  even the pen. Not new in 0.5.1 (0.5.0 had it too). Arabic reading: test page sent (his PC: OCR languages en-US + ar-SA).
- Testing split (owner, 2026-11-26, "only test on Linux, leave Windows testing for me"): agents run Linux VM tests only (Ubuntu VM:
  GNOME/KDE, X11/Wayland); the owner tests Windows himself on the build the lead sends. No agent Windows VM passes from now on.
  Exception (owner, same day, "agent runs it"): the release UPDATE GATE on the Windows VM (previous Setup + portable -> draft,
  About, Setup /quiet) is still run by an agent before publishing.
- 0.5.1 publish overnight (owner, 2026-11-26 ~22:45, going to sleep): once R-19 (Studio drawing delay + pixel flashing) is fixed and
  verified ("everything works perfectly, no delay, the best possible UX"), the lead merges and PUBLISHES v0.5.1 directly, no owner
  review. Every gate still runs: CI green, Linux VM pass, signed draft + minisig, Windows update gate (agent), AppImage CLI. A drawing
  fix that is not measured smooth (5K/8K) or that changes picture pixels = no publish; it waits for the owner.
- SDLC Kit for design and architecture LOGIC, never its style (owner, 2026-11-26/27, "be careful, remove the style in this repo, keep
  the agreed design from now"): OpenCircle follows the kit's rules, skills and agents (RULES.md ARC/UI/PLAT/COM-08, design-system
  process, sdlc:architect / designer / builder / reviewer), but NEVER the kit's visual or copy style (profile/owner/STYLE.md "Ground
  Station": serif titles, gold accent, mono eyebrows, its colours). The look comes only from the owner-agreed OpenCircle designs
  ("DESIGN SOURCES", the approved canvases, shipped goldens). Every designer / builder brief says so.
- v0.6.0 overnight plan (lead defaults 2026-11-27, owner asleep; owner may reverse):
  (1) R-01 recording research runs FIRST for recording only (capability table + architecture doc, no code); the design build does not
  depend on it and starts at the same time. Any "no" / "asks" in the table goes on the owner's morning list with the recommended
  behaviour; the recording build (R-03..R-06) waits for that table, and for his pick only where a "no" changes the approved R1/S2 design.
  (2) Design build split by area: Settings round 2 = Claude builder (src/features/settings + new settings-only kit parts); widgets
  style E (`.sty-two` two-tone grouped segments, W119 design-widgets) = Antigravity Neon (floating widgets + their kit parts); the
  Studio round 6 + Group chips + Preferences content comes AFTER Settings merges (it reuses the Settings parts). tokens.rs: each worker
  adds only in its own marked block; the lead resolves the merge.
  (3) Tips (R-17) after widgets style E merges (tips use style E). R-08..R-14 after the design build, one at a time.
  (4) The OpenRouter sign-in connection card: design round only (sdlc:designer, PNGs), nothing built before the owner's OK.
  (5) G-01 GPU renderer still waits for the owner's lab-gpu2 result. No tag / draft / publish of v0.6.0.
- R-01 recording research done (2026-11-27, docs/internal/reports-060/r01-recording.md). Pipelines (lead default, owner may reverse):
  Windows = DXGI duplication (area/screen) or Windows.Graphics.Capture (window) + WASAPI (cpal) -> Media Foundation (H.264 on the GPU,
  software fallback) + AAC in MP4, audio only .m4a; Linux = XShm/XComposite or the ScreenCast portal + PipeWire + PulseAudio protocol ->
  GStreamer (dlopen, as screencast.rs): H.264 VA-API/NVENC/x264 else VP8, Opus; audio only .ogg. No new crate; rav1e stays for the
  walkthrough export only (too slow live); no ffmpeg helper, no bundled openh264.
  WAITING ON THE OWNER (they change the approved R1/S2 design): Q1 whole-screen recording on a one-monitor Linux PC can't keep our bar
  out of the video -> recommended: the bar steps aside into the tray's recording state + the keys (Windows keeps the bar). Q2 Wayland
  has no window list -> recommended: window recording through the desktop's own chooser, one extra hint line "W: a window".
  Lead defaults (owner may reverse): Q3 R-06 blur zones fixed on the area everywhere; "follow this window" only on Windows/X11.
  Q4 an unsaved recording goes to Recent captures when S2 slides away (if Recent is on), else deleted; crash leftovers offered once.
  Q5 Linux without an H.264 encoder records WebM only (Format row hidden); Windows MP4 only. R-03 audio may start; R-04/R-05 wait for
  lab tests L1/L3/L4/L6 + Q1/Q2; R-06 waits for Q3 + L15.
- Rely more on Antigravity (owner, 2026-11-27, "pls rely more on antigravity, tell me when the weekly limits are reached so I can login
  with a different account"): new coding / testing / lab jobs go to Neon first, up to the cap of 2 Neon + 2 Claude; Claude for reviews,
  design rounds and careful fix passes. When Antigravity's quota is reached the lead TELLS THE OWNER at once (notification if he is
  away) so he switches accounts; a Claude builder takes the job over only if he doesn't answer.
- Studio zoom fix APPROVED (owner, 2026-11-27, "done and all are approved", on dev build 0a66890): drawing and moving shapes at
  110 % / 150 % zoom are smooth and the pixels don't change (B-01, merged 0a668907). Ships with the next release.
- Widgets style E APPROVED (owner, 2026-11-27, on the D-02 design-vs-app sheets) EXCEPT two that must match the design:
  (1) Steps strip: as drawn: header x / + / grip, rounded thumbnails with even gaps, the number badge in the thumbnail's top-end
  corner, Copy all = accent button, the Studio button = the design's quiet secondary button with its picture icon; (2) Live pin: the
  picture with rounded corners and a thin look-coloured border (not the thick yellow one), the hover toolbar as ONE style E tray
  (link, click-through (active state), copy, save, close) as drawn.
- Recording lab L3/L4 (2026-11-27, docs/internal/reports-060/l34-recording-lab.md): stock Ubuntu 22.04 has VP8/VP9 + Opus + mp4mux/
  webmmux + pipewiresrc; x264 only with plugins-ugly. Capture at 1080p passes on GNOME X11 (x264 30/60 fps <= 0.25 % dropped, ~0.3/0.5
  cores; VP8 30 fps pass, 60 fps 5-6 % dropped) and GNOME Wayland (Mutter's ScreenCast -> pipewiresrc, all cases pass, <= 1.43 cores
  peak; Mutter delivers ~55 fps). Lead defaults (owner may reverse): 30 fps default; x264 + MP4 when present, else VP8 + WebM with an
  explicit bitrate (VP8's default 256 kbps is too low for screens); 60 fps only behind the auto-30 fallback.
- Design fidelity (owner, 2026-11-27, on dev build bf11f77, shots docs/internal/owner-shots/2026-11-27/settings-*-design-vs-app.webp,
  snip-toolbar-app.png): "do you think this is the same?" NO: the design is more comfortable than the app (more gaps, bigger boxes, larger
  type); pieces are missing (Appearance theme cards with mini windows + System diagonal, screen-border look CARDS with previews, the
  Translate model list per pair with status / Update / progress / Add a language pair, the Circle to Search "Reading language" row,
  Glossary Edit button + its note) and some widgets don't match what he was shown. "Make it similar to the design": a fidelity pass on
  Settings AND the widgets, measured against the design source (sizes, paddings, gaps, type sizes from the CSS), compared side by side at
  the SAME window size and scale, every page / widget; contact sheets at 1:1, not shrunk.
  Previews (owner, same day): "no preview, where are the preview vids? It should be there by default": each Settings page with a clip
  shows its preview block OPEN by default (the design's "Hide the demo" state). Lead default (owner may reverse): the block shows the
  bundled still at once; the clip itself still downloads only when he presses Play ("Previews online on demand", 2026-11-04); ask him
  whether it should autoplay/download by itself.
  Antigravity (owner, same day): use the Neon MEDIUM model (gemini-4-neon-medium); it has quota while Neon High is on its weekly limit.
- Studio round 6 owner check (owner, 2026-11-27, dev build bf11f77, shots docs/internal/owner-shots/2026-11-27/studio-pills-save-boxed.png,
  studio-slider-design-vs-app.webp): "otherwise the Studio redesign is solid". Fix: (1) Save sits in an inner white container inside the
  bottom pill: no nested box; build exactly as drawn (and as a rule: never wrap a control in an extra container the design doesn't
  draw; reviews check this); (2) the Steps slider becomes the FLOATING card of the design (rounded card over the canvas with a margin,
  its own header/pin/close, Copy all + Export at its foot), not a docked full-height panel, and it never covers the bottom pills.
  Studio Preferences: the build he tried predates the D-05 merge (d7a56d7e); the next build has it.
  (2026-11-27, same check) (3) the Studio canvas still shows a hard square corner (top-left by the rail): every Studio surface the
  design rounds has soft rounded corners on all four sides.
- Dialog buttons centred (owner, 2026-11-27, every popup; extends "Report a problem popup: its BUTTONS are centred", 2026-11-25): in ANY
  popup/dialog/confirm (e.g. Studio "Save the changes to this picture?" Don't save / Cancel / Save) the option buttons are centred
  horizontally, built once in the kit dialog part so every dialog follows it (shot docs/internal/owner-shots/2026-11-27/dialog-buttons-not-centred.png).
- Owner answers (2026-11-27, ~12:15): Recording Q2 = YES (Wayland window recording through the desktop's own chooser + the hint line
  "W: a window"). Q1 (Linux whole-screen recording: bar steps into the tray) = "show me first" (a render before deciding). Design round
  (OpenRouter card + tips) = "show me" again before deciding. Previews (SUPERSEDES "Previews online on demand" 2026-11-04 for the
  download trigger): each page's preview block is open by default AND its clip downloads and plays automatically (sha256-pinned, from
  the owner's release, kept on the PC; reduced motion = still frame). Steps badge corner "doesn't matter" (keep the design's); the badge
  must be BIGGER / scale with the screen (DPI): on his 5K monitor it looks small.
- Antigravity limits (owner, 2026-11-27, "use 1 neon pls, 2 only when it is necessary, it is pro account only"; refines "Rely more on
  Antigravity"): ONE Neon job at a time (gemini-4-neon-medium); a second only when truly necessary. agy note: in briefs, never write
  $HOME inside the wsl.exe command (it expands on the Windows side: a build folder "C:Usersamerz/" landed in two worktrees); use the
  literal /home/az_work/... path (briefs fixed; the folder is excluded in .git/info/exclude).
- Design round APPROVED (owner, 2026-11-27, review page https: private link in AGENTS.local.md list; sheets
  .claude/artifacts/design-v060-round/sheets/): A OpenRouter sign-in connection card (4 states) approved; B1 tips one-time hint approved;
  B2 "What can I do?" card = OPTION B (tiles); B3 Settings Tips card with the "Show tips" toggle + "What can I do?" row approved.
  Lead defaults for the round's open questions (owner may reverse): the card opens with F1 inside OpenCircle + a tray entry; a tip that
  fades after ~8 s counts as seen; the OpenRouter waiting countdown shows only if the sign-in has a hard time limit; no "Show tips again"
  reset for now. Build order: R-17 tips + the card after F-02 (widgets fidelity) merges; the OpenRouter card after F-01 (Settings
  fidelity) merges. The Arabic sheets' mirrored title bar is NOT built (title bars never mirrored).
- Preview video (owner, 2026-11-27): (1) the preview video/still keeps ROUNDED corners (clip the picture to the block's radius, every
  page, every state). (2) NEW: a maximise control on the preview opens the clip in an elegant popup that almost covers the right main
  (content) card; design shown to the owner first (COM-08), then built.
- Soft corners everywhere (owner, 2026-11-27, on the Settings sheets: "not all edges are soft, the theme options for example"): every
  element the approved design rounds is rounded in the app with the design's radius, INCLUDING what is drawn inside cards (theme
  mini-window previews, look cards, preview stills/videos, thumbnails, chips), not only the outer box. Reviews check corners by pixel.
- Preview maximise APPROVED (owner, 2026-11-27, ".claude/artifacts/design-v060-preview-max/" sheet): variant A: a rounded "Bigger"
  button on the clip's top-end corner (start side in Arabic) opens the clip in a rounded panel over the content card only (14 px margin,
  soft dim of the content card, sidebar visible); the video fills the panel at 16:9 with title + controls (play/pause, progress, 1 of N,
  prev/next) overlaid; x and Esc close; reduced motion = still frame + play. Lead default (owner may reverse): no new shortcut (the
  designer's Ctrl+M is dropped). Built after F-01 (Settings fidelity) merges.
- R1 recording bar (owner, 2026-11-27): DRAGGABLE, top centre by default, on every platform and also for area recording (position kept
  while recording). Q1 (Linux whole-screen: bar steps into the tray) NOT decided: the owner "still doesn't get it": the lead reminds him
  later with a simpler explanation; until then build everything except that one case (Windows, area/window recording, R1 + S2).
- Q1 APPROVED (owner, 2026-11-27, after the example frames .claude/artifacts/design-v060-q1/png/): recording the WHOLE screen on Linux,
  the R1 bar steps aside into the tray's recording state (red dot + live time on the tray icon; tray menu: Pause, Stop and save
  (Alt+Shift+R), Discard, mic toggle + device); the keys keep working. Windows keeps the R1 bar; area/window recording on Linux keeps the
  R1 bar beside the area (draggable, outside the recorded pixels).
