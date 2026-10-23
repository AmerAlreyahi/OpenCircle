# Architecture

One Rust binary, UI in iced (tiny-skia CPU renderer). It runs as a resident tray
app: the overlay window is created hidden at start, so a trigger only has to
capture the screen and show it.

## Modules

| Module | Role |
|---|---|
| `main.rs` | Argument parsing, single-instance hand-off, starts tray + shortcut + UI. `Trigger` = what any source asks for. |
| `hotkey/` | Global shortcuts from the config (capture + one per quick action; `global-hotkey`, or the Wayland portal) and the per-user command socket behind `opencircle capture` / `opencircle run <action>`. `hotkey::reload(&Config)` re-registers after a settings change. |
| `capture/` | One frozen RGBA frame of the monitor under the cursor; one file per platform. |
| `ui/mod.rs` | The iced daemon: overlay window state, open / close (the multi-step `Hiding`), running actions. |
| `ui/overlay.rs` | The canvas over the frozen frame: dim, stroke, selection rules (loop vs. drag), mouse and key events. |
| `ui/bar.rs` | The floating action bar: the action registry as buttons, placed under/above the selection. |
| `ui/widgets.rs` | The shared components (`pill`, `surface`, `text_button`, `with_tip`). Only place that styles a widget. |
| `ui/theme.rs` | The only place colours live (Light/Dark palettes, accent, dims). |
| `ui/tray.rs` | Tray icon and menu. |
| `actions/mod.rs` | The action registry (`ACTIONS`), `crop`, `write_png`. One file per action. |
| `assets/lucide/` | Icon font: a subset of Lucide (ISC, see its LICENSE) with only the glyphs we use. |

## Data flow

1. **Trigger**: shortcut, tray or `opencircle capture` sends `Trigger::Capture` or `Trigger::Quick` into one channel.
2. **Capture**: `capture::monitor_under_cursor()` returns a `Frame` (RGBA, size, origin, scale).
3. **Overlay**: the RGBA bytes become the `image::Handle` drawn full-screen. The same
   bytes are later cropped by actions (shared, never copied whole).
4. **Selection**: the canvas reports press/move/release; on release `overlay::select`
   decides: a mostly straight drag = the rectangle it spans, anything else = the
   loop's bounding box. A new press starts over (redraw).
5. **Action**: a bar button, Enter (the first action) or the action's key sends
   `Message::Run(&Action)`. `App::run` crops the selection in physical pixels,
   closes the overlay (so dialogs aren't under it), then calls `action.run(crop)`.
6. **Close**: `close()` drops the frame; the window shrinks, then hides (`Hiding`).

## Change recipes

**Add an action** (e.g. Lens):
1. Create `src/actions/lens.rs` with `pub const ACTION: Action = Action { id, label,
   tip, key, icon, run }` and its `fn run(crop: Crop) -> Result<(), String>`
   (copy `copy.rs`). Keys: DESIGN.md "Shortcuts".
2. Add `mod lens;` and `lens::ACTION` to `ACTIONS` in `actions/mod.rs`, in
   action-bar order. The first entry is the Enter default.
3. Icon: the subset font already has search, sparkles, type, languages, pencil,
   copy, download (codes in `assets/lucide/subset.sh`). For another glyph, add its
   code to `subset.sh`, run it, commit the `.ttf`.
4. `cargo test`: the registry tests check ids, labels, tips and keys are unique and
   that the icon is in the font.
An action that needs to stay on screen (Ask AI, Text panels) will need `run` to
return something the UI acts on instead of closing; change that when the first
such action lands.

**Add a shared widget**: add a function to `ui/widgets.rs` that builds it from
iced primitives and styles it only with `theme.rs` tokens (add tokens there for
both palettes). Screens call the function; they never set a style themselves.

**Add a colour**: add it to both `DARK` and `LIGHT` in `theme.rs` (or a `const`
if it is theme-independent). Nowhere else.

## Test hooks (not for users)

- `OPENCIRCLE_TEST_FRAME=<8-bit RGBA PNG>`: capture returns this image instead of the screen.
- `OPENCIRCLE_TEST_SHOT=<out.png>`: Esc saves what the overlay shows before closing.
