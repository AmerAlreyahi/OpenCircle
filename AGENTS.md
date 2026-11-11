# OpenCircle — agent guide

OpenCircle brings Android's "Circle to Search" to the desktop (Windows and
Linux). Press a shortcut, the screen freezes, circle or drag over anything,
and search it (Google Lens), read it (OCR), translate it, or ask an AI about
it. A second mode snips and annotates screenshots (region, window, full,
scrolling; arrows, text, blur, steps; copy, save, pin). Stack: **pure Rust,
one binary, UI in `iced`** (no web view — lightweight is a hard
requirement). Spikes live in `spikes/` (throwaway). The plan is the private
master roadmap (see CLAUDE.local.md; older docs/internal plans are superseded), the UI/UX spec in `docs/internal/DESIGN.md`
(both local only). Status: **v0.1.5 published (2026-11-10); next: v0.1.6 hotfix (About page crash), then 0.1.7 Languages + live border, then Game translation (0.2.0).**

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
- Frozen contracts (since v0.1.0): the CLI (`opencircle capture …`, `run …`,
  `quit`) and the config file format: migrate, never break. The update channel
  (GitHub release assets, SHA256SUMS.txt + .minisig, trusted comment
  "OpenCircle v<ver>") is frozen too: installed copies depend on it.

## Architecture

Planned layout (details and change recipes: `docs/ARCHITECTURE.md`, written with
the first code):

| Part | Role |
|---|---|
| `src/capture/` | Screen capture per platform (xcap / windows-capture / portal) |
| `src/hotkey/` | Global shortcuts (`global-hotkey`; Wayland GlobalShortcuts portal; CLI fallback) |
| `src/actions/` | One file per action: lens, ocr, translate, ai, copy, save. Registry = one list (action bar and quick actions share it) |
| `src/translate/` | OCR → erase → translate → redraw pipeline; live layer |
| `src/ai/` | Two API clients only: Anthropic Messages, OpenAI-compatible (covers OpenAI, Gemini, Ollama, LM Studio, OpenRouter); plus the no-key browser provider (Google AI Mode in the user's browser, default) |
| `src/ui/` | iced windows: overlay, action bar, panels, snip editor (one file per annotation tool), settings, onboarding; `kit/` = the one UI kit and the only colour tokens (`theme.rs` = config-to-kit glue) |
| `src/config/` | The one config reader/validator (TOML): mod.rs types, parse, migrate, look, store (the one writer) |

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
