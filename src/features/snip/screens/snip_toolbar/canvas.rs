//! The editor's canvas: the flattened image, the item being drawn over it, and
//! the input layer (pointer -> image px, keys -> editor messages) with the
//! chrome (crop preview, the selection's handles, caret, a step's note).
//! Annotations themselves are painted by `annotate::render`, never here; what
//! the pointer and keys mean is the editor's (`Annotator::key`, `cursor`).
use iced::keyboard;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Fill, Frame, Geometry, Path, Stroke, fill};
use iced::widget::{image, pin, stack};
use iced::{ContentFit, Element, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::features::snip::annotate::area::REACH;
use crate::features::snip::annotate::style::{
    BAND_ALPHA, DIM_OUTSIDE, GRID, HANDLE, NOTE_FILL, NOTE_FONT, NOTE_HINT, NOTE_INK,
};
use crate::features::snip::annotate::{Annotator, Group, Message, info};

/// Room around the image for Snip's live area handles (display px; none
/// otherwise): the canvas is this much bigger on every side.
pub fn margin(e: &Annotator) -> f32 {
    if e.movable { REACH } else { 0.0 }
}
use crate::shared::kit::area;
use crate::shared::kit::themes::c;
use crate::shared::touch::Touched;

/// The pixel grid shows from this zoom (800 %).
const GRID_FROM: f32 = 8.0;

/// How a window shows Snip's live area (`app::screens`' one unit): its px =
/// the starting screen's px * `k` + `at`; the image's top-left pixel is at
/// `origin` (starting screen's px). The starting screen: `ONE` (k 1, at 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenMap {
    pub k: f32,
    pub at: Vector,
    pub origin: Point,
}

impl ScreenMap {
    pub const ONE: ScreenMap = ScreenMap {
        k: 1.0,
        at: Vector::ZERO,
        origin: Point::ORIGIN,
    };

    /// A starting screen's point in this window.
    pub fn to_window(self, p: Point) -> Point {
        Point::new(p.x * self.k, p.y * self.k) + self.at
    }

    /// This window's point `p` in the editor's image px (`zoom` display px per image px).
    fn to_image(self, p: Point, zoom: f32) -> Point {
        let s = Point::new((p.x - self.at.x) / self.k, (p.y - self.at.y) / self.k);
        Point::new((s.x - self.origin.x) / zoom, (s.y - self.origin.y) / zoom)
    }
}

/// The editing surface, `size()` big: for Edit in place, over the selection.
/// `grown`: Snip's handles scaled in (0..1, the pick's motion); `map`: the
/// window it is drawn in (Snip's area may span screens), `k` times as big.
pub fn canvas(e: &Annotator, grown: f32, map: ScreenMap) -> Element<'_, Message> {
    let k = map.k;
    let size = e.size() * k;
    let view = e.doc.view();
    let m = margin(e) * k;
    let outer = Size::new(size.width + 2.0 * m, size.height + 2.0 * m);
    // (Snip's bare live area: the frozen frame under it shows through)
    let mut layers: Vec<Element<'_, Message>> = vec![match e.bare() {
        true => iced::widget::space().into(),
        false => pin(image(e.display.clone().unwrap_or_else(|| e.base.clone()))
            .filter_method(match e.crisp {
                true => iced::widget::image::FilterMethod::Nearest,
                false => iced::widget::image::FilterMethod::Linear,
            })
            .width(size.width)
            .height(size.height)
            .content_fit(ContentFit::Fill))
        .position(Point::new(m, m))
        .into(),
    }];
    // Always a layer here, live or not: the canvas must keep its index, or iced
    // gives it a fresh state mid-drag (the press is lost, the drag draws nothing).
    let live = e.live.as_ref().map(|(r, handle)| {
        let z = e.zoom * k;
        let at = Point::new((r.x - view.x) * z + m, (r.y - view.y) * z + m);
        pin(image(handle.clone())
            .width(r.width * z)
            .height(r.height * z)
            .content_fit(ContentFit::Fill))
        .position(at)
    });
    layers.push(match live {
        Some(live) => live.into(),
        None => iced::widget::space().into(),
    });
    layers.push(
        iced::widget::canvas(Input {
            editor: e,
            grown,
            map,
        })
        .width(outer.width)
        .height(outer.height)
        .into(),
    );
    stack(layers).width(outer.width).height(outer.height).into()
}

struct Input<'a> {
    editor: &'a Annotator,
    grown: f32,
    map: ScreenMap,
}

impl Input<'_> {
    /// Snip's live area: a window point in image px (the canvas moves with the area).
    fn image_at(&self, p: Point) -> Point {
        self.map.to_image(p, self.editor.zoom)
    }
}

#[derive(Default)]
struct Pointer {
    down: bool,
    /// Snip's area handle under the pointer (its edges): drawn hot.
    hot: Option<(i8, i8)>,
    /// Ctrl is held: a press moves what is under it (`Message::PressMove`).
    ctrl: bool,
    /// Alt is held (Studio): a press moves a copy (`Message::PressCopy`).
    alt: bool,
    /// Studio's pixel grid, drawn once per zoom / scroll / size (`grid_key`).
    grid: canvas::Cache,
    grid_key: std::cell::Cell<Option<[u32; 6]>>,
    /// The finger held (touch or pen acts as the left button: `shared::touch`).
    finger: Option<iced::touch::Finger>,
    /// Snip: when the last press landed (a second one soon after opens the Studio).
    last_press: Option<std::time::Instant>,
    /// The cursor's last place while a drag runs (the scrolled one, as the press read it):
    /// a move with no cursor keeps it, never the event's raw position.
    last_at: Option<Point>,
}

/// The picture pixel columns and rows whose lines cross `seen` (display px).
fn grid_span(seen: Rectangle, z: f32) -> ((i32, i32), (i32, i32)) {
    (
        (
            (seen.x / z).floor() as i32,
            ((seen.x + seen.width) / z).ceil() as i32,
        ),
        (
            (seen.y / z).floor() as i32,
            ((seen.y + seen.height) / z).ceil() as i32,
        ),
    )
}

impl Input<'_> {
    /// Studio's pixel grid: a hairline round every picture pixel from 800 %,
    /// only in the part the stage shows, kept until the zoom, the scroll or
    /// the size changes.
    fn pixel_grid(
        &self,
        pointer: &Pointer,
        renderer: &Renderer,
        bounds: Rectangle,
        margin: f32,
    ) -> Option<Geometry> {
        let e = self.editor;
        if !e.grid || e.zoom < GRID_FROM {
            return None;
        }
        let size = e.size();
        let all = Rectangle::with_size(size);
        let seen = match e.viewport {
            Some(v) => all.intersection(&v)?,
            None => all,
        };
        let key = [
            e.zoom.to_bits(),
            seen.x.to_bits(),
            seen.y.to_bits(),
            seen.width.to_bits(),
            seen.height.to_bits(),
            self.map.k.to_bits(),
        ];
        if pointer.grid_key.get() != Some(key) {
            pointer.grid.clear();
            pointer.grid_key.set(Some(key));
        }
        let z = e.zoom;
        Some(pointer.grid.draw(renderer, bounds.size(), |frame| {
            frame.scale(self.map.k);
            frame.translate(Vector::new(margin, margin));
            let line = Stroke::default().with_color(GRID).with_width(1.0);
            let ((c0, c1), (r0, r1)) = grid_span(seen, z);
            let path = Path::new(|p| {
                for i in c0..=c1 {
                    p.move_to(Point::new(i as f32 * z, seen.y));
                    p.line_to(Point::new(i as f32 * z, seen.y + seen.height));
                }
                for i in r0..=r1 {
                    p.move_to(Point::new(seen.x, i as f32 * z));
                    p.line_to(Point::new(seen.x + seen.width, i as f32 * z));
                }
            });
            frame.stroke(&path, line);
        }))
    }
}

/// Two presses this close are a double click (`last`: the previous press, cleared by a double).
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(350);

fn double_click(last: &mut Option<std::time::Instant>, now: std::time::Instant) -> bool {
    match last.take() {
        Some(before) if now.duration_since(before) <= DOUBLE_CLICK => true,
        _ => {
            *last = Some(now);
            false
        }
    }
}

impl canvas::Program<Message> for Input<'_> {
    type State = Pointer;

    fn update(
        &self,
        pointer: &mut Pointer,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        let e = self.editor;
        let (event, cursor) = match crate::shared::touch::as_mouse(event, &mut pointer.finger) {
            Some(Touched::Cancel) => {
                return std::mem::take(&mut pointer.down).then(|| Action::publish(Message::Cancel));
            }
            Some(Touched::Mouse(mouse, at)) => (canvas::Event::Mouse(mouse), Cursor::Available(at)),
            None => (event.clone(), cursor),
        };
        let message = match &event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?; // None = over the toolbar
                // a double click inside the picked area (Select tool, not on a handle or
                // its margin, not while a long capture runs): into the Studio
                if e.movable
                    && !e.capturing
                    && info(e.tool).group == Group::Select
                    && cursor.position().is_some_and(|c| {
                        let p = self.image_at(c);
                        e.doc.view().contains(p) && e.area_hot(Some(p)).is_none()
                    })
                    && double_click(&mut pointer.last_press, std::time::Instant::now())
                {
                    pointer.down = false;
                    return Some(Action::publish(Message::OpenStudio).and_capture());
                }
                pointer.down = true;
                pointer.last_at = cursor.position();
                let p = match e.movable {
                    true => self.image_at(cursor.position()?),
                    false => e.to_image(at),
                };
                match (pointer.ctrl, pointer.alt && e.studio) {
                    (_, true) => Message::PressCopy(p),
                    (true, _) => Message::PressMove(p),
                    _ => Message::Press(p),
                }
            }
            // Snip's live area: from the window's px, as the canvas moves with it.
            canvas::Event::Mouse(mouse::Event::CursorMoved { position })
                if pointer.down && e.movable =>
            {
                Message::Move(self.image_at(*position))
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if pointer.down => {
                // the cursor, not the event's raw position: inside a scroll area the cursor is
                // moved by the scroll offset as the bounds are, the raw position is not (the
                // press above reads the cursor too: both must agree)
                let at = cursor.position().or(pointer.last_at)?;
                pointer.last_at = Some(at);
                Message::Move(e.to_image(at - (bounds.position() - Point::ORIGIN)))
            }
            // Snip's area: the handle under the pointer shows hot (a redraw
            // only when it changes).
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) if e.movable => {
                let hot = (e.area_hot(Some(self.image_at(*position))))
                    .filter(|(_, held)| !held)
                    .map(|(edges, _)| edges);
                if hot == pointer.hot {
                    return None;
                }
                pointer.hot = hot;
                return Some(Action::request_redraw());
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if pointer.down =>
            {
                pointer.down = false;
                Message::Release
            }
            // Ctrl+wheel over the selection resizes it (the Studio's path, `annotate::wheel`)
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) if pointer.ctrl => {
                let at = cursor.position_in(bounds)?;
                let p = match e.movable {
                    true => self.image_at(cursor.position()?),
                    false => e.to_image(at),
                };
                if !e.wheel_over(p) {
                    return None;
                }
                let notches = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => {
                        *y / crate::features::snip::annotate::WHEEL_PX
                    }
                };
                return Some(Action::publish(Message::WheelResize(p, notches)).and_capture());
            }
            // Not captured: other widgets see modifiers too.
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                pointer.ctrl = m.command();
                pointer.alt = m.alt();
                return Some(Action::publish(Message::Shift(m.shift())));
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                text,
                ..
            }) => match e.key(
                key,
                key.to_latin(*physical_key),
                *modifiers,
                text.as_deref(),
            )? {
                // a paste lands at the pointer
                Message::Paste(None) => {
                    Message::Paste(cursor.position_in(bounds).map(|at| match e.movable {
                        true => self.image_at(cursor.position().unwrap_or(at)),
                        false => e.to_image(at),
                    }))
                }
                message => message,
            },
            _ => return None,
        };
        Some(Action::publish(message).and_capture())
    }

    fn draw(
        &self,
        pointer: &Pointer,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let e = self.editor;
        let mut frame = Frame::new(renderer, bounds.size());
        frame.scale(self.map.k);
        let accent = Stroke::default().with_color(c().accent).with_width(1.5);
        // Everything below in image-canvas px; the margin holds the area's handles.
        let m = margin(e);
        frame.translate(Vector::new(m, m));
        // Snip's area: its handles (the overlay draws its outline), in the
        // active look's colours, the held or hovered one hot.
        if e.movable {
            let r = Rectangle::with_size(e.size());
            let look = crate::shared::theme::accent_of(&crate::shared::config::look());
            let hot = match e.area_hot(None) {
                Some((edges, _)) => Some((edges, area::Handle::Held)),
                None => pointer.hot.map(|edges| (edges, area::Handle::Hot)),
            };
            area::handles(&mut frame, r, look, hot, self.grown);
        }

        // Studio's rubber band (Select tool): a soft box in the look's colour.
        if let Some(band) = e.band_rect() {
            frame.fill_rectangle(
                band.position(),
                band.size(),
                c().accent.scale_alpha(BAND_ALPHA),
            );
            frame.stroke(&Path::rectangle(band.position(), band.size()), accent);
        }
        if let Some(keep) = e.crop_preview() {
            let outside = Path::new(|p| {
                p.rectangle(Point::ORIGIN, e.size());
                p.rectangle(keep.position(), keep.size());
            });
            frame.fill(
                &outside,
                Fill {
                    style: canvas::Style::Solid(DIM_OUTSIDE),
                    rule: fill::Rule::EvenOdd,
                },
            );
            frame.stroke(
                &Path::rectangle(keep.position(), keep.size()),
                accent.with_color(HANDLE),
            );
        }

        // The selection: no frame (owner, 2026-10-31), only the item's own
        // small handles, soft, in the active look's colour (as Snip's area).
        if let Some((top, height)) = e.caret_rect() {
            if let Some(note) = e.note_pill() {
                // The step's note, in a small pill beside the badge.
                let pad = 6.0;
                frame.fill(
                    &Path::rounded_rectangle(
                        note.top - iced::Vector::new(pad, 2.0),
                        Size::new(note.text.width + 2.0 * pad, note.text.height + 4.0),
                        8.0.into(),
                    ),
                    NOTE_FILL,
                );
                frame.fill_text(canvas::Text {
                    content: note.shown,
                    position: note.top,
                    color: if note.empty { NOTE_HINT } else { NOTE_INK },
                    size: note.size.into(),
                    line_height: iced::widget::text::LineHeight::Absolute(note.line.into()),
                    font: NOTE_FONT,
                    ..canvas::Text::default()
                });
            }
            frame.fill_rectangle(top, Size::new(2.0, height), c().accent);
        }
        let marks = e.handle_marks();
        if !marks.is_empty() {
            let look = crate::shared::theme::accent_of(&crate::shared::config::look());
            for at in marks {
                area::handle(&mut frame, at, (1, 1), area::Handle::Rest, look.0);
            }
        }
        let mut out = Vec::with_capacity(2);
        if let Some(grid) = self.pixel_grid(pointer, renderer, bounds, m) {
            out.push(grid);
        }
        out.push(frame.into_geometry());
        out
    }

    fn mouse_interaction(
        &self,
        pointer: &Pointer,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        let e = self.editor;
        let Some(at) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if e.picking {
            return mouse::Interaction::Crosshair;
        }
        match (e.movable, cursor.position()) {
            (true, Some(p)) => e.cursor(self.image_at(p), pointer.ctrl),
            _ => e.cursor(e.to_image(at), pointer.ctrl),
        }
    }
}

#[cfg(test)]
mod key_tests;

#[cfg(test)]
mod tests {
    use iced::widget::row;
    use iced::{Point, Size, Vector};

    use super::{ScreenMap, canvas};
    use crate::features::snip::annotate::{Annotator, Image, Message, TOOLS, Tool};
    use crate::shared::kit::testing::golden;

    const W: u32 = 280;
    const H: u32 = 150;
    /// Between the two canvases of a golden.
    const GAP: f32 = 8.0;

    /// A light page with dark text-like bars (the blur, spotlight and
    /// magnifier have something to change).
    fn page() -> Image {
        let bar = |x: u32, y: u32| {
            (24..256).contains(&x) && (20..140).contains(&y) && (y - 20) % 24 < 8 && x < 256 - y / 2
        };
        let rgba = (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .flat_map(|(x, y)| match bar(x, y) {
                true => [52, 56, 70, 255],
                false => [236, 238, 243, 255],
            })
            .collect();
        Image {
            width: W,
            height: H,
            rgba,
        }
    }

    /// W98 202.7: Ctrl+wheel over the selection publishes the resize (a notch = one line = the
    /// shared WHEEL_PX of pixels); off it, or without Ctrl, the canvas leaves the wheel alone.
    #[test]
    fn w98_202_7_ctrl_wheel_over_the_selection_asks_for_a_resize() {
        use iced::keyboard;
        use iced::mouse::{self, Cursor};
        use iced::widget::canvas::Program;
        let mut e = Annotator::new(page(), 1.0);
        e.studio = true;
        e.update(Message::Tool(Tool::Rect));
        for m in [
            Message::Press(Point::new(40.0, 40.0)),
            Message::Move(Point::new(120.0, 90.0)),
            Message::Release,
            Message::SelectIndex(0, false),
        ] {
            e.update(m);
        }
        let input = super::Input {
            editor: &e,
            grown: 0.0,
            map: ScreenMap::ONE,
        };
        let bounds = iced::Rectangle::new(Point::ORIGIN, Size::new(W as f32, H as f32));
        let mut state = super::Pointer::default();
        let mut wheel = |at: Point, delta: mouse::ScrollDelta| {
            let ev = iced::Event::Mouse(mouse::Event::WheelScrolled { delta });
            input
                .update(&mut state, &ev, bounds, Cursor::Available(at))
                .and_then(|a| a.into_inner().0)
        };
        let over = Point::new(80.0, 60.0);
        let lines = mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 };
        assert!(wheel(over, lines).is_none(), "no Ctrl: nothing");
        let mods =
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL));
        let _ = input.update(&mut state, &mods, bounds, Cursor::Unavailable);
        let mut wheel = |at: Point, delta: mouse::ScrollDelta| {
            let ev = iced::Event::Mouse(mouse::Event::WheelScrolled { delta });
            input
                .update(&mut state, &ev, bounds, Cursor::Available(at))
                .and_then(|a| a.into_inner().0)
        };
        assert!(
            matches!(wheel(over, lines), Some(Message::WheelResize(p, n)) if p == over && n == 1.0)
        );
        let pixels = mouse::ScrollDelta::Pixels {
            x: 0.0,
            y: crate::features::snip::annotate::WHEEL_PX,
        };
        assert!(
            matches!(wheel(over, pixels), Some(Message::WheelResize(p, n)) if p == over && n == 1.0)
        );
        assert!(wheel(Point::new(10.0, H as f32 - 5.0), lines).is_none());
    }

    /// A stroke by finger is the same messages as by the left button.
    #[test]
    fn finger_draws_like_the_left_button() {
        use iced::mouse::{self, Cursor};
        use iced::touch::{Event as T, Finger};
        use iced::widget::canvas::Program;
        let e = Annotator::new(page(), 1.0);
        let input = super::Input {
            editor: &e,
            grown: 0.0,
            map: ScreenMap::ONE,
        };
        let bounds = iced::Rectangle::new(Point::ORIGIN, Size::new(W as f32, H as f32));
        let run = |events: &[iced::Event]| {
            let mut state = super::Pointer::default();
            let mut cursor = Cursor::Unavailable;
            let mut out = Vec::new();
            for ev in events {
                if let iced::Event::Mouse(mouse::Event::CursorMoved { position }) = ev {
                    cursor = Cursor::Available(*position);
                }
                let a = input.update(&mut state, ev, bounds, cursor);
                out.extend(a.and_then(|a| a.into_inner().0));
            }
            out
        };
        let id = Finger(1);
        let path = [pt(40.0, 40.0), pt(80.0, 50.0), pt(120.0, 90.0)];
        let mut touch = vec![iced::Event::Touch(T::FingerPressed {
            id,
            position: path[0],
        })];
        touch.extend(
            path[1..]
                .iter()
                .map(|p| iced::Event::Touch(T::FingerMoved { id, position: *p })),
        );
        touch.push(iced::Event::Touch(T::FingerLifted {
            id,
            position: path[2],
        }));
        let mut by_mouse = vec![
            iced::Event::Mouse(mouse::Event::CursorMoved { position: path[0] }),
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        ];
        by_mouse.extend(
            path[1..]
                .iter()
                .map(|p| iced::Event::Mouse(mouse::Event::CursorMoved { position: *p })),
        );
        by_mouse.push(iced::Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        )));
        let got = run(&touch);
        assert_eq!(got.len(), 4, "{got:?}");
        assert_eq!(got, run(&by_mouse));
    }

    /// Review 4: a move while the cursor is unavailable never falls back to the event's raw
    /// position (it ignores the scroll): it keeps the last scrolled one.
    #[test]
    fn w90c_a_move_without_a_cursor_never_uses_the_raw_position() {
        use iced::mouse::{self, Cursor};
        use iced::widget::canvas::Program;
        let e = Annotator::new(page(), 1.0);
        let input = super::Input {
            editor: &e,
            grown: 0.0,
            map: ScreenMap::ONE,
        };
        let bounds = iced::Rectangle::new(Point::ORIGIN, Size::new(W as f32, H as f32));
        let mut state = super::Pointer::default();
        let moved = |position| iced::Event::Mouse(mouse::Event::CursorMoved { position });
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let at = Cursor::Available(pt(40.0, 40.0));
        let _ = input.update(&mut state, &press, bounds, at);
        // no cursor: the raw (unscrolled) position is not used, the last one stays
        let got = input
            .update(
                &mut state,
                &moved(pt(190.0, 100.0)),
                bounds,
                Cursor::Unavailable,
            )
            .and_then(|a| a.into_inner().0);
        assert!(
            matches!(got, Some(Message::Move(p)) if (p.x - 40.0).abs() < 0.5),
            "{got:?}"
        );
        // the cursor is back: the move follows it
        let got = input
            .update(
                &mut state,
                &moved(pt(190.0, 100.0)),
                bounds,
                Cursor::Available(pt(60.0, 50.0)),
            )
            .and_then(|a| a.into_inner().0);
        assert!(
            matches!(got, Some(Message::Move(p)) if (p.x - 60.0).abs() < 0.5),
            "{got:?}"
        );
    }

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    /// One `tool` item drawn on `page` (typing `text` where it takes text),
    /// left selected as the tool leaves it; `done` = released (else mid-drag).
    fn drawn(tool: Tool, text: &str, done: bool) -> Annotator {
        let mut e = Annotator::new(page(), 1.0);
        e.assist = false; // not the user's config
        e.update(Message::Tool(tool));
        // (each placed so its text and handles stay inside the page)
        let (from, to) = match tool {
            Tool::Magnifier => (pt(110.0, 60.0), pt(150.0, 90.0)),
            Tool::ArrowLabel => (pt(150.0, 50.0), pt(200.0, 110.0)),
            _ => (pt(60.0, 40.0), pt(180.0, 110.0)),
        };
        let at = match tool {
            Tool::Text => pt(16.0, 60.0),
            Tool::Callout => pt(30.0, 135.0),
            _ => pt(70.0, 60.0),
        };
        match tool {
            Tool::Text | Tool::Label | Tool::Callout | Tool::Step | Tool::Pointer => {
                e.update(Message::Press(at));
                e.update(Message::Release);
                e.update(Message::Type(text.into()));
                e.update(Message::Finish);
            }
            _ => {
                e.update(Message::Press(from));
                e.update(Message::Move(from + Vector::new(40.0, 10.0)));
                e.update(Message::Move(to));
                if !done {
                    return e;
                }
                e.update(Message::Release);
                if tool == Tool::ArrowLabel {
                    e.update(Message::Type(text.into()));
                    e.update(Message::Finish);
                }
            }
        }
        // The blur's secret seed is OS-random per picture: fixed here, or no
        // two runs draw it alike.
        e.doc.seed = 7;
        e.refresh();
        e
    }

    /// Every editor tool on the canvas: as drawn (nothing selected) beside
    /// selected (its own handles only, no frame; pen, highlighter and blur
    /// have none), in Sapphire and Daylight; the text-bearing tools
    /// (text, label, callout, arrow label, step + its legend) in Latin and
    /// Arabic; Crop mid-drag (its dim and edges). Look at each one after an
    /// update.
    #[test]
    fn every_tool_matches_its_golden_images() {
        crate::shared::kit::text::load(); // the bundled faces the annotations use
        let mut cases: Vec<(String, Tool, &str)> = Vec::new();
        for info in (TOOLS.iter()).filter(|t| !matches!(t.tool, Tool::Select | Tool::Eraser)) {
            let name = format!("{:?}", info.tool).to_lowercase();
            let texty = matches!(
                info.tool,
                Tool::Text | Tool::Label | Tool::Callout | Tool::ArrowLabel | Tool::Step
            );
            cases.push((name.clone(), info.tool, "Open the menu"));
            if texty {
                cases.push((format!("{name}-arabic"), info.tool, "افتح القائمة"));
            }
        }
        golden::each_theme(|theme, failed| {
            for (name, tool, text) in &cases {
                let (plain, chosen) = if *tool == Tool::Crop {
                    (drawn(*tool, text, true), drawn(*tool, text, false))
                } else {
                    // (a step's note shows in the legend, on by default)
                    let mut plain = drawn(*tool, text, true);
                    let chosen = drawn(*tool, text, true);
                    plain.update(Message::Tool(Tool::Rect)); // ends any typing
                    plain.doc.selected = None;
                    (plain, chosen)
                };
                let (a, b) = (plain.size(), chosen.size()); // (a crop is smaller)
                let size = Size::new(a.width + GAP + b.width, a.height.max(b.height));
                let file = format!("editor-{name}-{}", theme.id);
                failed.extend(golden::check("editor", &file, size, &[1.0], || {
                    row![
                        canvas(&plain, 1.0, ScreenMap::ONE),
                        canvas(&chosen, 1.0, ScreenMap::ONE)
                    ]
                    .spacing(GAP)
                    .into()
                }));
            }
        });
    }
}

#[cfg(test)]
mod double_click_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// W91.2: a second press within the window is a double click; the third starts over.
    #[test]
    fn two_quick_presses_are_a_double_click() {
        let (mut last, t0) = (None, Instant::now());
        assert!(!double_click(&mut last, t0));
        assert!(double_click(&mut last, t0 + Duration::from_millis(200)));
        assert!(!double_click(&mut last, t0 + Duration::from_millis(300)));
        assert!(
            !double_click(&mut last, t0 + Duration::from_millis(900)),
            "too slow"
        );
    }
}
