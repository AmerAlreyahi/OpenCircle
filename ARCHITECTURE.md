# Architecture

One Rust binary, UI in iced (tiny-skia CPU renderer). It runs as a resident tray
app: the overlay window is created hidden at start, so a trigger only has to
capture the screen and show it.

## Modules

| Module | Role |
|---|---|
| `main.rs` | Argument parsing, single-instance hand-off, starts tray + shortcut + UI. `Trigger` = what any source asks for. |
| `hotkey/` | Global shortcuts from the config (capture + one per quick action; `global-hotkey`, or the Wayland portal) and the per-user command socket behind `opencircle capture` / `opencircle run <action>`. `hotkey::reload(&Config)` re-registers after a settings change. |
| `config.rs` | The one config reader/validator. `config::current()` is the running app's config (one source of truth: panels read it, `set_current` replaces it when Settings saves). |
| `capture/` | One frozen RGBA frame of the monitor under the cursor; one file per platform. |
| `ui/mod.rs` | The iced daemon: overlay window state, capture on a thread, open / close (the multi-step `Hiding`), running actions, held Lens pages, `background` (thread work → messages). |
| `ui/overlay.rs` | The canvas over the frozen frame: dim, stroke, selection rules (loop vs. drag), mouse and key events. |
| `ui/bar.rs` | The floating action bar: the action registry as buttons, placed under/above the selection. |
| `ui/panel.rs` | Result panels (Search, Text, Ask AI): one `surface` docked right (bottom sheet on narrow screens); their state, views and background work. |
| `ui/widgets.rs` | The shared components (`pill`, `surface`, `text_button`, `icon_button`, `chip`, `input`, `editor`, `progress`, `bubble`, `scroll`, text styles, `with_tip`). Only place that styles a widget. |
| `ui/theme.rs` | The only place colours live (Light/Dark palettes, accent, dims). `apply(Theme)` = the Settings choice, `set_os(Mode)` = what the OS reports; System follows the OS. |
| `ui/tray.rs` | Tray icon and menu (Circle to search, Settings…, Quit). Without libayatana-appindicator (Linux) it logs one line and the app runs on shortcut / CLI. |
| `ui/windows.rs` | The normal windows beside the overlay: Settings and first-run Onboarding (one of each, a second request focuses it), their close handling, the OS-theme feed, and `apply(cfg)` after a save. |
| `ui/settings.rs`, `ui/onboarding.rs` | The two screens as self-contained components (`State`, `update` returning an `Outcome`, `view`). |
| `actions/mod.rs` | The action registry (`ACTIONS`), `Run` (finish or open a panel), `crop`, `encode_png`, `write_png`. One file per action: `lens.rs` (page + its minimum life), `ask.rs` (setup, `Chat`, streaming), `text.rs` (consent check, OCR, line joining), `copy.rs`, `save.rs`. |
| `assets/lucide/` | Icon font: a subset of Lucide (ISC, see its LICENSE) with only the glyphs we use. |

## Data flow

1. **Trigger**: shortcut, tray or `opencircle capture` sends `Trigger::Capture` or `Trigger::Quick` into one channel.
2. **Capture**: `capture::monitor_under_cursor()` returns a `Frame` (RGBA, size, origin, scale).
   It runs on its own thread (a Wayland portal can wait minutes for the user's OK);
   `Message::Captured` then shows the pre-created window.
3. **Overlay**: the RGBA bytes become the `image::Handle` drawn full-screen. The same
   bytes are later cropped by actions (shared, never copied whole).
4. **Selection**: the canvas reports press/move/release; on release `overlay::select`
   decides: a mostly straight drag = the rectangle it spans, anything else = the
   loop's bounding box. A new press starts over (redraw).
5. **Action**: a bar button, Enter (the first action) or the action's key sends
   `Message::Run(&Action)`. `App::run` crops the selection in physical pixels, then:
   `Run::Finish(f)` closes the overlay (so dialogs aren't under it) and calls `f(crop)`;
   `Run::Panel(kind)` opens that result panel over the frozen frame (the bar hides,
   the selection stays). Slow work (PNG, browser launch, keyring, HTTP stream, model
   download, OCR) runs on a thread via `background`; results come back as
   `Message::Panel(id, panel::Msg)` and stale ids are ignored. Dropping a panel
   aborts its task, which disconnects the thread (an AI stream then stops).
6. **Close**: Esc (any focus) or a finishing action: `close()` drops the panel and the
   frame and releases Lens pages (deleted at once, or once 10 s old so the browser
   has read them); the window shrinks, then hides (`Hiding`). A press outside the
   panel starts a new selection and drops the panel.

## Change recipes

**Add an action** that finishes at once (like Copy):
1. Create `src/actions/<name>.rs` with `pub const ACTION: Action = Action { id, label,
   tip, key, icon, run: Run::Finish(run) }` and its `fn run(crop: Crop) -> Result<(), String>`
   (copy `copy.rs`; keep blocking work off the UI thread, see `save.rs`). Keys:
   DESIGN.md "Shortcuts".
2. Add `mod <name>;` and `<name>::ACTION` to `ACTIONS` in `actions/mod.rs`, in
   action-bar order. The first entry is the Enter default.
3. Icon: the subset font has search, sparkles, type, languages, pencil, copy,
   download, arrow-up, square, x (codes in `assets/lucide/subset.sh`). For another
   glyph, add its code to `subset.sh`, run it, commit the `.ttf`.
4. `cargo test`: the registry tests check ids, labels, tips and keys are unique and
   that the icon is in the font.

**Add an action that stays on screen** (a result panel, like Text):
1. As above, with `run: Run::Panel(Panel::<Kind>)`; add `<Kind>` to `actions::Panel`.
   The action file holds the work (blocking functions, pure logic + its tests).
2. In `ui/panel.rs`: a `State` variant, its start in `Panel::open` (slow work via
   `background`, keep the abort handle in `work` if it should stop with the panel),
   its `Msg`s in `Panel::update`, its body in `view`. Messages the App must act on
   (close, open another panel) are matched first in `App::on_panel`.

**Add a shared widget**: add a function to `ui/widgets.rs` that builds it from
iced primitives and styles it only with `theme.rs` tokens (add tokens there for
both palettes). Screens call the function; they never set a style themselves.

**Add a colour**: add it to both `DARK` and `LIGHT` in `theme.rs` (or a `const`
if it is theme-independent). Nowhere else.

## Test hooks (not for users)

- `OPENCIRCLE_TEST_FRAME=<8-bit RGBA PNG>`: capture returns this image instead of the screen.
- `OPENCIRCLE_TEST_SHOT=<out.png>`: Esc saves what the overlay shows before closing
  (shot ~150 ms after Esc, with no message in between: a relayout would drop the
  text editor's drawn text from the shot).
