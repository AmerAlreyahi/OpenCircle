//! The input layer over the image: pointer → image px, keys → editor messages,
//! and the chrome (selection frame, handles, caret, crop preview). Annotations
//! themselves are painted by `document::paint`, never here.
use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Fill, Frame, Geometry, LineDash, Path, Stroke, fill};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use super::document::span;
use super::{Editor, Message, Shape, TOOLS, Tool, callout, select, text};
use crate::ui::theme;

pub struct Input<'a> {
    pub editor: &'a Editor,
}

#[derive(Default)]
pub struct Pointer {
    down: bool,
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
                Message::Press(e.to_image(at))
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) if pointer.down => {
                Message::Move(e.to_image(*position - (bounds.position() - Point::ORIGIN)))
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if pointer.down =>
            {
                pointer.down = false;
                Message::Release
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
        _pointer: &Pointer,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let e = self.editor;
        let mut frame = Frame::new(renderer, bounds.size());
        let accent = Stroke::default()
            .with_color(theme::ACCENT_START)
            .with_width(1.5);

        if let Some((a, b)) = e.cropping {
            let keep = e.to_display(span(&[a, b]));
            let outside = Path::new(|p| {
                p.rectangle(Point::ORIGIN, bounds.size());
                p.rectangle(keep.position(), keep.size());
            });
            frame.fill(
                &outside,
                Fill {
                    style: canvas::Style::Solid(theme::DIM_OUTSIDE),
                    rule: fill::Rule::EvenOdd,
                },
            );
            frame.stroke(
                &Path::rectangle(keep.position(), keep.size()),
                accent.with_color(Color::WHITE),
            );
        }

        let item = (e.pending.as_ref().map(|p| &p.item))
            .or_else(|| e.doc.selected.and_then(|i| e.doc.items.get(i)));
        if let Some(item) = item {
            let f = e.to_display(item.frame());
            frame.stroke(
                &Path::rectangle(f.position(), f.size()),
                Stroke {
                    line_dash: LineDash {
                        segments: &[5.0, 4.0],
                        offset: 0,
                    },
                    ..accent
                },
            );
            if e.typing() {
                let caret = match &item.shape {
                    Shape::Text { at, text, size } => Some((*at, text, *size)),
                    Shape::Callout { at, text, size, .. } => {
                        Some((callout::text_origin(*at, text, *size), text, *size))
                    }
                    _ => None,
                };
                if let Some((origin, s, size)) = caret {
                    let top = e.to_display_point(origin + text::caret(s, size));
                    let height = text::line_height(size) * e.zoom;
                    frame.fill_rectangle(top, Size::new(2.0, height), theme::ACCENT_START);
                }
            } else {
                for (handle, at) in select::handles(item) {
                    let at = e.to_display_point(at);
                    let knob = match handle {
                        select::Handle::Bend => Path::circle(at, 5.5),
                        select::Handle::Corner(_) => {
                            Path::rectangle(at - iced::Vector::new(4.5, 4.5), Size::new(9.0, 9.0))
                        }
                    };
                    frame.fill(&knob, Color::WHITE);
                    frame.stroke(&knob, accent);
                }
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _pointer: &Pointer,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        let e = self.editor;
        let Some(at) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        match e.tool {
            Tool::Select => {
                let reach = super::REACH / e.zoom;
                if select::hit(&e.doc.items, e.to_image(at), reach).is_some() {
                    mouse::Interaction::Grab
                } else {
                    mouse::Interaction::default()
                }
            }
            Tool::Text | Tool::Callout => mouse::Interaction::Text,
            _ => mouse::Interaction::Crosshair,
        }
    }
}

impl Editor {
    /// Canvas px → image px.
    fn to_image(&self, p: Point) -> Point {
        let v = self.doc.view();
        Point::new(v.x + p.x / self.zoom, v.y + p.y / self.zoom)
    }

    fn to_display_point(&self, p: Point) -> Point {
        let v = self.doc.view();
        Point::new((p.x - v.x) * self.zoom, (p.y - v.y) * self.zoom)
    }

    fn to_display(&self, r: Rectangle) -> Rectangle {
        Rectangle::new(self.to_display_point(r.position()), r.size() * self.zoom)
    }

    /// What a key does: typing while a text is edited; Esc closes the picker
    /// (its hex field has the keys); otherwise tool letters and shortcuts.
    pub(super) fn key(
        &self,
        key: &Key,
        latin: Option<char>,
        modifiers: Modifiers,
        text: Option<&str>,
    ) -> Option<Message> {
        if self.typing() {
            return match key {
                Key::Named(Named::Escape) => Some(Message::Finish),
                Key::Named(Named::Backspace) => Some(Message::Backspace),
                Key::Named(Named::Enter) => Some(Message::Type("\n".into())),
                _ if modifiers.command() => None,
                _ => text
                    .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                    .map(|t| Message::Type(t.to_string())),
            };
        }
        if self.picker {
            return matches!(key, Key::Named(Named::Escape)).then_some(Message::Picker);
        }
        match key {
            Key::Named(Named::Escape) => Some(Message::Back),
            Key::Named(Named::Delete | Named::Backspace) => Some(Message::Delete),
            _ => match (
                modifiers.command(),
                modifiers.shift(),
                latin?.to_ascii_lowercase(),
            ) {
                (true, false, 'z') => Some(Message::Undo),
                (true, true, 'z') | (true, _, 'y') => Some(Message::Redo),
                (true, _, 'c') => Some(Message::Copy),
                (true, _, 's') => Some(Message::Save),
                (false, _, letter) => TOOLS
                    .iter()
                    .find(|t| t.key == letter)
                    .map(|t| Message::Tool(t.tool)),
                _ => None,
            },
        }
    }
}
