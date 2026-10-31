//! Snip's screen tools (the colour picker, the ruler; `screens::tools`):
//! starting one, its overlay events and its messages.

use std::time::Duration;

use iced::widget::image;
use iced::{Point, Rectangle, Task, Vector};

use crate::actions::copy;
use crate::ui::app::background::background;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::screens::tools::{self, Tool, Which, picker, ruler};
use crate::ui::{overlay, snip};

use super::App;

impl App {
    /// The running screen tool.
    pub(in crate::ui) fn tool(&self) -> Option<&Tool> {
        match &self.o.purpose {
            Purpose::Snip(snip) => snip.tool.as_ref(),
            _ => None,
        }
    }

    fn tool_mut(&mut self) -> Option<&mut Tool> {
        match &mut self.o.purpose {
            Purpose::Snip(snip) => snip.tool.as_mut(),
            _ => None,
        }
    }

    /// Starts `which` over the frozen screens. From Snip's toolbar the picked
    /// area and its editor wait for Esc (kept as they are).
    pub(in crate::ui) fn start_tool(&mut self, which: Which) -> Task<Message> {
        if !matches!(self.o.purpose, Purpose::Snip(_)) {
            return Task::none();
        }
        let back = (self.o.selection.map(|s| s.area))
            .zip(self.o.edit.take())
            .map(|(area, edit)| (area, Box::new(edit)));
        self.o.selection = None;
        self.o.stroke.clear();
        self.o.drawing = false;
        let Purpose::Snip(snip) = &mut self.o.purpose else {
            return Task::none();
        };
        let mut tool = Tool::new(which, back);
        tool.at = snip.pointer; // (none yet from a shortcut: the first move says)
        snip.tool = Some(tool);
        snip.hinting = true;
        self.frame_times.borrow_mut().clear();
        let opening = self.opening;
        background(|tx| {
            std::thread::sleep(Duration::from_secs(snip::HINT_SECS));
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::Snip(snip::Msg::HintGone(opening)))
    }

    /// The frozen frame under `p` (the starting screen's logical px), that
    /// screen's scale and its view of the starting screen's px.
    pub(in crate::ui) fn frame_at(&self, p: Point) -> Option<(&image::Handle, f32, overlay::View)> {
        if Rectangle::with_size(self.o.screen).contains(p) {
            return Some((self.o.frame.as_ref()?, self.o.scale, self.o.view));
        }
        let start = self.o.place();
        (self.o.screens.iter())
            .find(|s| super::screens::area_of(start, s.place).contains(p))
            .map(|s| (&s.frame, s.place.scale, s.view))
    }

    /// An overlay event while a tool runs (points in the starting screen's
    /// logical px): the pointer and the arrows move it, a click picks or a
    /// drag measures, Enter / Ctrl+C copy.
    pub(in crate::ui) fn on_tool_event(&mut self, event: overlay::Event) -> Task<Message> {
        let Purpose::Snip(snip) = &mut self.o.purpose else {
            return Task::none();
        };
        let shift = snip.shift();
        let Some(which) = snip.tool.as_ref().map(|t| t.which) else {
            return Task::none();
        };
        match event {
            overlay::Event::Hover(p) | overlay::Event::Move(p) => {
                let (tool, p) = place(snip, p);
                tool.ruler.drag(p, shift);
                Task::none()
            }
            overlay::Event::Press { at, shift } => {
                snip.hinting = false;
                let (tool, p) = place(snip, at);
                match which {
                    // the frame times of the loupe's moves so far (the 0.1.3 gate)
                    Which::Picker => {
                        self.log_frames("colour picker");
                        self.copy_colour(shift)
                    }
                    Which::Ruler => {
                        tool.ruler.press(p);
                        self.frame_times.borrow_mut().clear();
                        Task::none()
                    }
                }
            }
            overlay::Event::Release => {
                let held = self.tool_mut().is_some_and(|t| {
                    let held = t.ruler.held;
                    t.ruler.release();
                    held
                });
                if held {
                    self.log_frames("ruler drag");
                }
                Task::none()
            }
            overlay::Event::Arrow { dx, dy, shift, .. } => {
                let Some(at) = self.tool().and_then(|t| t.at) else {
                    return Task::none();
                };
                // one physical px of the screen it is on (Shift: ten)
                let scale = self.frame_at(at).map_or(self.o.scale, |(_, s, _)| s);
                let step = if shift { 10.0 } else { 1.0 } / scale;
                let to = at + Vector::new(f32::from(dx), f32::from(dy)) * step;
                if let Some(tool) = self.tool_mut() {
                    tool.at = Some(to);
                }
                Task::none()
            }
            overlay::Event::Enter if which == Which::Picker => self.copy_colour(false),
            overlay::Event::ShiftEnter if which == Which::Picker => self.copy_colour(true),
            overlay::Event::Key {
                letter: 'c',
                ctrl: true,
            } => match which {
                Which::Picker => self.copy_colour(false),
                Which::Ruler => self.copy_measure(),
            },
            _ => Task::none(),
        }
    }

    /// The toolbar's messages, and Esc (`Close`).
    pub(in crate::ui) fn on_tool(&mut self, msg: tools::Msg) -> Task<Message> {
        match msg {
            tools::Msg::Mode(mode) => {
                if let Some(tool) = self.tool_mut() {
                    tool.ruler.set_mode(mode);
                }
                Task::none()
            }
            tools::Msg::Recopy(i) => match self.colours.get(i).copied() {
                Some(colour) => self.copy_picked(colour, false),
                None => Task::none(),
            },
            tools::Msg::CopiedGone(id) => {
                if let Some(tool) = self.tool_mut() {
                    tool.copied_gone(id);
                }
                Task::none()
            }
            tools::Msg::Close => {
                let Purpose::Snip(snip) = &mut self.o.purpose else {
                    return Task::none();
                };
                match snip.tool.take().and_then(|t| t.back) {
                    // back to the picked area and its editor
                    Some((area, edit)) => {
                        self.select(area);
                        self.o.edit = Some(*edit);
                        self.open_editor()
                    }
                    None => self.close(),
                }
            }
        }
    }

    /// The colour under the pointer to the clipboard: HEX, or `rgb`.
    fn copy_colour(&mut self, rgb: bool) -> Task<Message> {
        let colour = self.tool().and_then(|t| {
            let (frame, scale, view) = self.frame_at(t.at?)?;
            t.colour(frame, scale, view)
        });
        match colour {
            Some(colour) => self.copy_picked(colour, rgb),
            None => Task::none(),
        }
    }

    /// `colour` to the clipboard (HEX, or `rgb`), first in the recent ones.
    fn copy_picked(&mut self, colour: [u8; 3], rgb: bool) -> Task<Message> {
        picker::remember(&mut self.colours, colour);
        let text = match rgb {
            true => picker::rgb(colour),
            false => picker::hex(colour),
        };
        self.copy_tool_text(text)
    }

    /// The ruler's measure to the clipboard, as its readout says it.
    fn copy_measure(&mut self) -> Task<Message> {
        let measure = self.tool().and_then(|t| {
            let (frame, scale, view) = self.frame_at(t.at?)?;
            t.measure(frame, scale, view)
        });
        match measure {
            Some(m) => self.copy_tool_text(m.text()),
            None => Task::none(),
        }
    }

    /// `text` to the clipboard (on a thread: another app may hold it) with
    /// the Copied pill for `COPIED_MS`.
    fn copy_tool_text(&mut self, text: String) -> Task<Message> {
        let Some(tool) = self.tool_mut() else {
            return Task::none();
        };
        let id = tool.copied(format!("{} {text}", copy::COPIED_PILL));
        let copied = background(move |tx| {
            if let Err(e) = copy::text(text) {
                crate::log!("copy failed: {e}");
            }
            std::thread::sleep(Duration::from_millis(tools::COPIED_MS));
            let _ = tx.unbounded_send(());
        });
        copied.map(move |()| Message::Snip(snip::Msg::Tool(tools::Msg::CopiedGone(id))))
    }
}

/// The pointer at `p`: the tool's, snapped to window and control edges for a
/// box (as a snip's corners are; the snip's hover reads the controls there).
fn place(snip: &mut snip::Snip, p: Point) -> (&mut Tool, Point) {
    let boxed = (snip.tool.as_ref()).is_some_and(|t| t.ruler.mode == ruler::Mode::Box);
    // (the snip's hover walks every control under the pointer: only a box snaps)
    let p = match boxed {
        true => {
            snip.hover(p);
            snip.snap(p)
        }
        false => {
            snip.pointer = Some(p); // (the screen it is on draws the tool)
            p
        }
    };
    let tool = snip.tool.as_mut().expect("a tool runs");
    tool.at = Some(p);
    (tool, p)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use iced::{Point, Rectangle, Size};

    use super::super::tests::{open_app, snipping_on};
    use crate::actions::copy;
    use crate::editor;
    use crate::ui::app::App;
    use crate::ui::app::message::{Message, Purpose, Shot};
    use crate::ui::overlay::Event;
    use crate::ui::screens::tools::{self, Which, ruler};
    use crate::ui::snip::{self, Snip};

    const WINDOW: Rectangle = Rectangle {
        x: 100.0,
        y: 80.0,
        width: 300.0,
        height: 200.0,
    };

    /// What this test copied as text, once its thread has (within 10 s).
    fn copied() -> Option<String> {
        let until = Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match copy::copied::text() {
                Some(text) => return Some(text),
                None if Instant::now() > until => return None,
                None => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    }

    /// A snip at `scale` with `which` running (the frame's pixel at (x, y)
    /// is `[x, y, x >> 8]`, `tests::reopen`).
    fn running(which: Which, scale: f32) -> App {
        let mut app = match scale {
            1.0 => snipping_on(800, 600, WINDOW),
            _ => {
                let mut app = open_app(600, 600, scale);
                let place = app.place();
                app.o.purpose = Purpose::Snip(Box::new(Snip::new(Vec::new(), place, true, None)));
                app
            }
        };
        let _ = app.start_tool(which);
        app
    }

    fn press(app: &mut App, at: Point, shift: bool) {
        let _ = app.on_overlay(Event::Press { at, shift });
        let _ = app.on_overlay(Event::Release);
    }

    #[test]
    fn the_picker_copies_the_true_pixel_as_hex_or_rgb_at_any_scale() {
        let mut app = running(Which::Picker, 1.0);
        let _ = copy::copied::text();
        press(&mut app, Point::new(30.0, 40.0), false);
        assert_eq!(copied().as_deref(), Some("#1E2800"));
        assert!(app.tool().is_some(), "it stays for more picks");
        press(&mut app, Point::new(30.0, 40.0), true);
        assert_eq!(copied().as_deref(), Some("rgb(30, 40, 0)"));
        // the arrows move it one physical px (Shift: ten); Enter copies there
        let arrow = |dx, shift| Event::Arrow {
            dx,
            dy: 0,
            ctrl: false,
            shift,
        };
        let _ = app.on_overlay(arrow(1, false));
        let _ = app.on_overlay(Event::Enter);
        assert_eq!(copied().as_deref(), Some("#1F2800"));
        let _ = app.on_overlay(arrow(1, true));
        let _ = app.on_overlay(Event::Key {
            letter: 'c',
            ctrl: true,
        });
        assert_eq!(copied().as_deref(), Some("#292800"));
        assert_eq!(app.colours, [[41, 40, 0], [31, 40, 0], [30, 40, 0]]);
        // a swatch copies its colour again (and comes first)
        let _ = app.on_tool(tools::Msg::Recopy(2));
        assert_eq!(copied().as_deref(), Some("#1E2800"));
        assert_eq!(app.colours[0], [30, 40, 0]);
        // 150 %: a logical point is on the frame pixel 1.5x away; one arrow = one pixel
        let mut app = running(Which::Picker, 1.5);
        press(&mut app, Point::new(20.0, 10.0), false);
        assert_eq!(copied().as_deref(), Some("#1E0F00"));
        let _ = app.on_overlay(arrow(1, false));
        let _ = app.on_overlay(Event::Enter);
        assert_eq!(copied().as_deref(), Some("#1F0F00"));
    }

    #[test]
    fn the_ruler_measures_a_drag_and_ctrl_c_copies_it() {
        let ctrl_c = Event::Key {
            letter: 'c',
            ctrl: true,
        };
        let drag = |app: &mut App, a: Point, b: Point| {
            let _ = app.on_overlay(Event::Press {
                at: a,
                shift: false,
            });
            let _ = app.on_overlay(Event::Move(b));
            let _ = app.on_overlay(Event::Release);
        };
        let mut app = running(Which::Ruler, 1.0);
        let _ = copy::copied::text();
        drag(&mut app, Point::new(10.0, 10.0), Point::new(310.0, 410.0));
        let _ = app.on_overlay(ctrl_c);
        assert_eq!(copied().as_deref(), Some("500 px · Δx 300 · Δy 400"));
        assert!(app.o.selection.is_none(), "a measure, not a selection");
        let _ = app.on_tool(tools::Msg::Mode(ruler::Mode::Box));
        drag(&mut app, Point::new(10.0, 10.0), Point::new(210.0, 130.0));
        let _ = app.on_overlay(ctrl_c);
        assert_eq!(copied().as_deref(), Some("200 × 120 px"));
        // 150 %: physical px, and the scaled value
        let mut app = running(Which::Ruler, 1.5);
        let _ = app.on_tool(tools::Msg::Mode(ruler::Mode::Box));
        drag(&mut app, Point::new(10.0, 10.0), Point::new(210.0, 130.0));
        let _ = app.on_overlay(ctrl_c);
        assert_eq!(
            copied().as_deref(),
            Some("300 × 180 px · 200 × 120 at 150 %")
        );
    }

    #[test]
    fn esc_goes_back_to_the_picked_area_and_its_editor_or_closes() {
        let mut app = snipping_on(800, 600, WINDOW);
        let area = Rectangle::new(Point::new(120.0, 100.0), Size::new(200.0, 120.0));
        app.select(area);
        let crop = app.crop().unwrap();
        let _ = app.snip_routed(crop, true, snip::route(crate::config::SnipAfter::Editor));
        assert!(app.o.edit.is_some());
        let _ = app.on_editor(editor::Message::ScreenTool(Which::Ruler));
        assert!(app.tool().is_some_and(|t| t.which == Which::Ruler));
        assert!(
            app.o.edit.is_none() && app.o.selection.is_none(),
            "the screen is the tool's"
        );
        let _ = app.update(Message::Escape);
        assert!(app.tool().is_none());
        assert_eq!(app.o.selection.map(|s| s.area), Some(area));
        assert!(
            app.o.edit.as_ref().is_some_and(|e| e.is_open()),
            "its editor back"
        );
        // from its shortcut: Esc closes the overlay
        let mut app = running(Which::Picker, 1.0);
        let _ = app.update(Message::Escape);
        assert!(app.o.frame.is_none());
    }

    #[test]
    fn its_quick_action_opens_a_snip_with_the_tool_and_no_timer() {
        let mut app = App::idle(iced::window::Id::unique());
        app.tool_next = Some(Which::Picker);
        app.capturing = Some(3);
        let shot = Shot {
            image: iced::widget::image::Handle::from_rgba(4, 4, vec![9; 64]),
            scale: 1.0,
            origin: Point::ORIGIN,
            size: Size::new(4.0, 4.0),
            snip: Some((snip::Kind::Region, Vec::new(), None, Vec::new())),
        };
        let _ = app.update(Message::Captured(3, Instant::now(), Ok(shot)));
        assert!(app.tool().is_some_and(|t| t.which == Which::Picker));
        assert!(app.tool_next.is_none() && !app.snip_pill());
    }
}

/// The tools redraw round what moves (the 0.1.3 gate: frames p95 under 16 ms
/// on the VM): the loupe, the measure, the readouts; never the frozen screen.
#[cfg(test)]
mod frames {
    use iced::{Point, Rectangle, Size, Vector};

    use super::super::tests::snipping_on;
    use crate::ui::app::App;
    use crate::ui::frame_times::{Drawn, frames};
    use crate::ui::overlay::Event;
    use crate::ui::screens::tools::Which;

    fn moved(which: Which, n: u32) -> (Vec<Drawn>, App) {
        let window = Rectangle::new(Point::new(200.0, 150.0), Size::new(600.0, 400.0));
        let mut app = snipping_on(1716, 1271, window);
        let _ = app.start_tool(which);
        let (from, by) = (Point::new(120.0, 300.0), Vector::new(9.0, 4.0));
        let t = frames(&mut app, n, false, |app, i, _| {
            let at = from + by * i as f32;
            let e = match (which, i) {
                (Which::Ruler, 0) => Event::Press { at, shift: false },
                (Which::Ruler, _) => Event::Move(at),
                (Which::Picker, _) => Event::Hover(at),
            };
            let _ = app.on_overlay(e);
        });
        (t, app)
    }

    /// The messages a click at `at` on the overlay's window sends.
    fn clicked(app: &App, at: Point) -> Vec<crate::ui::app::message::Message> {
        use iced::mouse::{Button, Cursor, Event as Mouse};
        use iced_runtime::user_interface::{Cache, UserInterface};
        let mut renderer = crate::ui::kit::testing::shot::renderer();
        let mut ui = UserInterface::build(
            app.view(app.overlay),
            app.o.screen,
            Cache::default(),
            &mut renderer,
        );
        let mut out = Vec::new();
        let events = [
            iced::Event::Mouse(Mouse::CursorMoved { position: at }),
            iced::Event::Mouse(Mouse::ButtonPressed(Button::Left)),
            iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)),
        ];
        let mut clipboard = iced::advanced::clipboard::Null;
        let _ = ui.update(
            &events,
            Cursor::Available(at),
            &mut renderer,
            &mut clipboard,
            &mut out,
        );
        out
    }

    /// A click on the ruler's toolbar switches its mode and never measures.
    #[test]
    fn the_toolbar_takes_its_clicks() {
        use crate::ui::app::message::Message;
        use crate::ui::screens::tools::{Msg, ruler::Mode};
        let window = Rectangle::new(Point::new(200.0, 150.0), Size::new(600.0, 400.0));
        let mut app = snipping_on(1280, 720, window);
        let _ = app.start_tool(Which::Ruler);
        let _ = app.on_overlay(Event::Hover(Point::new(250.0, 450.0)));
        let mut modes = Vec::new();
        for x in (480..800).step_by(4) {
            for m in clicked(&app, Point::new(x as f32, 46.0)) {
                match m {
                    Message::Snip(crate::ui::snip::Msg::Tool(Msg::Mode(mode))) => modes.push(mode),
                    Message::Overlay(e @ Event::Press { .. }) => {
                        panic!("{x}: the toolbar let {e:?} through")
                    }
                    _ => {}
                }
            }
        }
        modes.dedup();
        assert_eq!(modes, [Mode::Line, Mode::Across, Mode::Down, Mode::Box]);
    }

    #[test]
    fn the_screen_tools_redraw_round_what_moves() {
        for which in [Which::Picker, Which::Ruler] {
            let (t, app) = moved(which, 30);
            let whole = app.o.screen.width * app.o.screen.height;
            // (~1-1.5 % a frame: the loupe or the measure, its readouts)
            for (i, d) in t.iter().enumerate().skip(2) {
                let area: f32 = d.damage.iter().map(|r| r.width * r.height).sum();
                let share = area / whole;
                assert!(share < 0.04, "{which:?} frame {i}: {:.1} %", share * 100.0);
            }
        }
    }
}
