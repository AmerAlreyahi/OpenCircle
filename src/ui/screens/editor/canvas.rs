//! The editor's canvas: the flattened image, the item being drawn over it, and
//! the input layer (pointer -> image px, keys -> editor messages) with the
//! chrome (crop preview, the selection's handles, caret, a step's note).
//! Annotations themselves are painted by `editor::render`, never here; what
//! the pointer and keys mean is the editor's (`Editor::key`, `cursor`).
use iced::keyboard;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Fill, Frame, Geometry, Path, Stroke, fill};
use iced::widget::{image, pin, stack};
use iced::{ContentFit, Element, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::editor::area::REACH;
use crate::editor::style::{DIM_OUTSIDE, HANDLE, NOTE_FILL, NOTE_FONT, NOTE_HINT, NOTE_INK};
use crate::editor::{Editor, Message};

/// Room around the image for Snip's live area handles (display px; none
/// otherwise): the canvas is this much bigger on every side.
pub fn margin(e: &Editor) -> f32 {
    if e.movable { REACH } else { 0.0 }
}
use crate::ui::kit::area;
use crate::ui::kit::themes::c;

/// How a window shows Snip's live area (`app::screens`' one unit): its px =
/// the starting screen's px * `k` + `at`; the image's top-left pixel is at
/// `origin` (starting screen's px). The starting screen: `ONE` (k 1, at 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lens {
    pub k: f32,
    pub at: Vector,
    pub origin: Point,
}

impl Lens {
    pub const ONE: Lens = Lens {
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
/// `grown`: Snip's handles scaled in (0..1, the pick's motion); `lens`: the
/// window it is drawn in (Snip's area may span screens), `k` times as big.
pub fn canvas(e: &Editor, grown: f32, lens: Lens) -> Element<'_, Message> {
    let k = lens.k;
    let size = e.size() * k;
    let view = e.doc.view();
    let m = margin(e) * k;
    let outer = Size::new(size.width + 2.0 * m, size.height + 2.0 * m);
    // (Snip's bare live area: the frozen frame under it shows through)
    let mut layers: Vec<Element<'_, Message>> = vec![match e.bare() {
        true => iced::widget::space().into(),
        false => pin(image(e.base.clone())
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
            lens,
        })
        .width(outer.width)
        .height(outer.height)
        .into(),
    );
    stack(layers).width(outer.width).height(outer.height).into()
}

struct Input<'a> {
    editor: &'a Editor,
    grown: f32,
    lens: Lens,
}

impl Input<'_> {
    /// Snip's live area: a window point in image px (the canvas moves with the area).
    fn image_at(&self, p: Point) -> Point {
        self.lens.to_image(p, self.editor.zoom)
    }
}

#[derive(Default)]
struct Pointer {
    down: bool,
    /// Snip's area handle under the pointer (its edges): drawn hot.
    hot: Option<(i8, i8)>,
    /// Ctrl is held: a press moves what is under it (`Message::PressMove`).
    ctrl: bool,
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
        let message = match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?; // None = over the toolbar
                pointer.down = true;
                let p = match e.movable {
                    true => self.image_at(cursor.position()?),
                    false => e.to_image(at),
                };
                match pointer.ctrl {
                    true => Message::PressMove(p),
                    false => Message::Press(p),
                }
            }
            // Snip's live area: from the window's px, as the canvas moves with it.
            canvas::Event::Mouse(mouse::Event::CursorMoved { position })
                if pointer.down && e.movable =>
            {
                Message::Move(self.image_at(*position))
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) if pointer.down => {
                Message::Move(e.to_image(*position - (bounds.position() - Point::ORIGIN)))
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
            // Not captured: other widgets see modifiers too.
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                pointer.ctrl = m.command();
                return Some(Action::publish(Message::Shift(m.shift())));
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                text,
                ..
            }) => e.key(
                key,
                key.to_latin(*physical_key),
                *modifiers,
                text.as_deref(),
            )?,
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
        frame.scale(self.lens.k);
        let accent = Stroke::default().with_color(c().accent).with_width(1.5);
        // Everything below in image-canvas px; the margin holds the area's handles.
        let m = margin(e);
        frame.translate(Vector::new(m, m));
        // Snip's area: its handles (the overlay draws its outline), in the
        // active look's colours, the held or hovered one hot.
        if e.movable {
            let r = Rectangle::with_size(e.size());
            let look = crate::ui::theme::accent_of(&crate::config::look());
            let hot = match e.area_hot(None) {
                Some((edges, _)) => Some((edges, area::Handle::Held)),
                None => pointer.hot.map(|edges| (edges, area::Handle::Hot)),
            };
            area::handles(&mut frame, r, look, hot, self.grown);
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
            let look = crate::ui::theme::accent_of(&crate::config::look());
            for at in marks {
                area::handle(&mut frame, at, (1, 1), area::Handle::Rest, look.0);
            }
        }
        vec![frame.into_geometry()]
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
mod tests {
    use iced::widget::row;
    use iced::{Point, Size, Vector};

    use super::{Lens, canvas};
    use crate::editor::{Editor, Image, Message, TOOLS, Tool};
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

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

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    /// One `tool` item drawn on `page` (typing `text` where it takes text),
    /// left selected as the tool leaves it; `done` = released (else mid-drag).
    fn drawn(tool: Tool, text: &str, done: bool) -> Editor {
        let mut e = Editor::new(page(), 1.0);
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
        // The blur's secret seed is OS-random per stroke: fixed here, or no
        // two runs draw it alike.
        if let Some(crate::editor::Shape::Blur { seed, .. }) =
            e.doc.items.first_mut().map(|i| &mut i.shape)
        {
            *seed = 7;
            e.refresh();
        }
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
        crate::ui::kit::text::load(); // the bundled faces the annotations use
        let mut cases: Vec<(String, Tool, &str)> = Vec::new();
        for info in TOOLS.iter().filter(|t| t.tool != Tool::Select) {
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
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
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
                        canvas(&plain, 1.0, Lens::ONE),
                        canvas(&chosen, 1.0, Lens::ONE)
                    ]
                    .spacing(GAP)
                    .into()
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
