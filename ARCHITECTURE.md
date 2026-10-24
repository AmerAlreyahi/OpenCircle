# Architecture

One Rust binary, UI in iced (tiny-skia CPU renderer). It runs as a resident tray
app: the overlay window is created hidden at start, so a trigger only has to
capture the screen and show it.

## Modules

| Module | Role |
|---|---|
| `main.rs` | Argument parsing, single-instance hand-off, starts tray + shortcut + UI. `Trigger` = what any source asks for. |
| `hotkey/` | Global shortcuts from the config (capture + one per quick action; `global-hotkey`, or the Wayland portal) and the per-user command socket behind `opencircle capture` / `opencircle run <action>`. `hotkey::reload(&Config)` re-registers after a settings change. |
| `config.rs` | The one config reader/validator. `config::current()` is the running app's config (one source of truth: panels read it, `set_current` replaces it when Settings saves). New settings: add the field + default + check in `parse` + a round-trip test, then a row in `ui/settings.rs`. `general.default_action` = what Enter runs (`actions::default_for`), `general.save_folder` = Save's start folder, `translate.live_interval_ms` = the live layer's period, `translate.source` / `target` = the languages (`auto` = detect). |
| `capture/` | One frozen RGBA frame of the monitor under the cursor; one file per platform. `window_rect.rs`: visible windows, topmost first (snip Window mode). `stitch.rs` + `scroll.rs`: scrolling capture. |
| `ocr/`, `mt/` | OCR (PP-OCRv5) and offline translation (OPUS-MT) on ONNX Runtime, only in the helper process `opencircle ocr-worker` (`ocr/worker.rs`: requests, the serve loop; models load on first use and stay). `ocr/session.rs`: the app side (framed, request ids, one request at a time, timeout -> kill -> next call respawns). `ocr::hold()` keeps ONE warm helper while held: live mode (`ui/live_layer.rs`) and an open overlay (`App::helper`) each hold one; the last drop kills it. Unheld calls get a child per call. |
| `translate/lang.rs` | The ONE language table (code, English and native name, script, RTL, OCR recognizer id): pickers, the AI prompt, RTL layout and recognizer choice read it. `ocr::models::RECOGNIZERS` = one pinned recognizer per script (en, latin, arabic, cyrillic, cjk; a source picks one, `auto` = English, then the other downloaded ones when it reads badly: `ocr::Read`); `mt::PAIRS` = one pinned OPUS-MT model per direction. New language: a row (+ a recognizer / pair entry if it has one). |
| `ui/mod.rs` | The iced daemon: overlay window state, capture on a thread, open / close (the multi-step `Hiding`), running actions, held Lens pages, `background` (thread work → messages). |
| `ui/overlay.rs` | The canvas over the frozen frame: dim, strokes, selection rules (`classify`: tap / scribble / drag / loop; `settle` = the selection and the stroke it keeps, a scribble's padded hull), corner handles (resize / move math), the Shift count badge, mouse / wheel / key events. `View` = the zoomed (1x–4x) and panned part of the frame: selections live in image px (the overlay at 1x), so crops are right at any zoom. |
| `ui/word.rs` | Tap a word: OCR lines around the tap (`recognize_in_child` on a crop, on a thread; no models = no-op), split into words by character share (RTL lines from the right); a second tap on the line makes a range. A press-drag from the highlight or a Shift+click stretches it across lines in reading order (`Picked::extend`; past the lines read, OCR reads a full-width band, `band` / `reread`); one highlight rect per line. Copy copies the text, a line break between lines. |
| `ui/bar.rs` | The floating action bar: the action registry as buttons, placed under/above the selection. |
| `editor/` | The annotation editor (one component for Edit in place and Snip): `Document` (image or board of captures, items, crop, undo stack; `render` flattens), one file per tool. Tool buttons can hold variants (`ToolInfo::slot`: Arrow > Line, labelled arrow; Text > Label; Step > Pointer; Blur > Redact), shown in a strip under the button. `legend.rs`: the steps' notes in a box (free corner, else a band under the image). `board.rs`: several captures as one image (stack / side / grid; items move with their capture). `blur.rs`: soft blur brush (sigma >= 0.6 x line height + OS-random noise: not invertible); `lines.rs`: line assist (text lines from glyph edges, no OCR; the stroke holds its line). `style.rs`: presets (keys 1-9) and copy/paste style. `dragout.rs`: OS drag of the flattened PNG (Windows only; temp file deleted with the editor/pin). Outputs the host wires: `AddCapture` (then `Editor::append`), `CopyWithText`. |
| `ui/edit.rs` | Edit in place: the editor (`crate::editor`) over the selection at its own pixels (zoom 1/scale), its toolbar where the bar was; Back keeps the edits (shown flattened) and later actions get the edited image. Also the fitted editor for a scrolling capture. |
| `ui/quick.rs` | Quick actions: id → what runs (registry action, copy_text, ai_explain), target → area (full_screen, circle, last_region per monitor; `window` is snip-only and counts as circle elsewhere). |
| `ui/translate.rs` | Translate in place (Ctrl+Shift+T = full screen; `R` in the bar = the selection): `Session` runs `translate::pipeline` on a thread with `actions::translate::deps()`, swaps the translated frame in only when every block is ready (an error leaves the original), Alt = peek, a tap on a block = word card (Explain = one streamed AI call with the block's text + crop; `A` = Ask AI panel with that chat). Missing OCR models: the Text panel's consent prompt (`panel::consent`). |
| `ui/live_layer.rs` | Live translate (Ctrl+Shift+L, tray item): a monitor-sized, always-on-top window made click-through by `live::prepare_layer`, see-through except the patches (`theme::LAYER_CLEAR`); a thread captures about once a second and runs `translate::live::Live::step`, one step at a time. Off = stop flag + window closed. Wayland / no layer: one-shot instead. Also opens the mini-controller (`ui/live_controller.rs`: language chip + list, Pause, Hide = dot, Stop; draggable, takes clicks, `live::prepare_controller` excludes it from capture on Windows, X11 captures get its spot blanked). |
| `ui/snip.rs` | Snip mode (`opencircle run snip`, tray; default keys Print Screen / Alt+ / Ctrl+ = the circle / window / full_screen target of the three default `snip` quick actions): Region · Window · Full · Scrolling switch, then the editor; scrolling runs on a thread while the overlay is a progress pill beside the area. |
| `ui/panel.rs` | Result panels (Search, Text, Ask AI): one `surface` docked right (bottom sheet on narrow screens); their state, views and background work. |
| `ui/border.rs` | The screen-edge border (violet → cyan, theme tokens) over the overlay (one shimmer sweep when it opens), Snip (static) and the live layer (a soft light circles the edge, 20 fps, its base layer). Eight strip images rendered by hand: a frame swaps only the strips whose pixels changed (the renderer redraws only those), and the colour-keyed layer gets no partial alpha. `ANIMATE` turns motion off; Windows' "show animations" setting is respected. `mask` blanks it out of live captures (X11 shows the layer). |
| `ui/picker.rs` | The compact language picker (source → target chips, list with search, recent first), shared by the live controller and the one-shot translate pill; picks last for the session. |
| `ui/widgets.rs` | The shared components (`pill`, `surface`, `text_button`, `icon_button`, `chip`, `wide_chip`, `drag_handle`, `input`, `editor`, `progress`, `bubble`, `scroll`, text styles, `with_tip`). Only place that styles a widget. |
| `ui/theme.rs` | The only place colours live (Light/Dark palettes, accent, dims). `apply(Theme)` = the Settings choice, `set_os(Mode)` = what the OS reports; System follows the OS. |
| `ui/tray.rs` | Tray icon and menu (Circle to search, Settings…, Quit). Without libayatana-appindicator (Linux) it logs one line and the app runs on shortcut / CLI. |
| `ui/windows.rs` | The normal windows beside the overlay: Settings and first-run Onboarding (one of each, a second request focuses it), their close handling, the OS-theme feed, and `apply(cfg)` after a save. Also the pinned images (editor Pin / `Ctrl+P`): any number of borderless always-on-top windows, each owning its image (drag, scroll = zoom, hover bar Drag (Windows) / Copy / Save / Close, `Esc` or double-click closes), and the Settings side effects (folder picker, project link). |
| `ui/settings.rs`, `ui/onboarding.rs` | The two screens as self-contained components (`State`, `update` returning an `Outcome`, `view`). |
| `actions/mod.rs` | The action registry (`ACTIONS`), `Run` (finish or open a panel), `crop`, `encode_png`, `write_png`. One file per action: `lens.rs` (page + its minimum life), `ask.rs` (setup, `Chat`, streaming, `PRESETS` = the suggested questions for every provider), `text.rs` (consent check, OCR, line joining), `translate.rs` (the pipeline's deps from config + keyring; the Text panel's Translate), `copy.rs`, `save.rs`. Not actions: `entities.rs` (links, emails, phone numbers as chips in the Text panel; click only) and `qr.rs` (QR decode + black-capture check, run off the UI thread when a selection is made; feeds the bar's QR chip and the "protected" pill). |
| `assets/lucide/` | Icon font: a subset of Lucide (ISC, see its LICENSE) with only the glyphs we use. |

## Data flow

1. **Trigger**: shortcut, tray or `opencircle capture` sends `Trigger::Capture` or `Trigger::Quick` into one channel.
2. **Capture**: `capture::monitor_under_cursor()` returns a `Frame` (RGBA, size, origin, scale).
   It runs on its own thread (a Wayland portal can wait minutes for the user's OK);
   `Message::Captured` then shows the pre-created window.
3. **Overlay**: the RGBA bytes become the `image::Handle` drawn full-screen. The same
   bytes are later cropped by actions (shared, never copied whole).
4. **Selection**: the canvas reports press/move/release (screen px; the App maps them
   to image px through `overlay::View`); on release `overlay::settle` decides: a zig-zag
   (3+ sharp corners) = a scribble (the box of its padded convex hull, which is also the
   clear area), a mostly straight drag = the rectangle it spans, anything else = the
   loop's bounding box, a tap = the word there (`ui/word.rs`). A new press starts over
   (redraw); a press on a corner handle resizes, on highlighted words a drag stretches
   them across lines (Shift+click too), elsewhere inside moves; Shift adds a selection (Ask AI / Text get every
   crop stacked in one image, the rest the box around all). The ask bar at the bottom
   opens Ask AI with its text.
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
- `OPENCIRCLE_TEST_SCROLL=<tall PNG>`: a scrolling snip reads this page (100 px per wheel notch)
  instead of scrolling the screen. With the editor open, its Esc (Back) takes the test shot.
- `OPENCIRCLE_TEST_LIVE_SHOT=<out.png>`: live translate saves its layer once the first patches (or the first error) are drawn.
- `OPENCIRCLE_TEST_SHOT=<out.png>`: Esc saves what the overlay shows before closing
  (shot ~150 ms after Esc, with no message in between: a relayout would drop the
  text editor's drawn text from the shot).
