//! The editor's canvas: the flattened image, the item being drawn over it, and
//! the input layer (pointer -> image px, keys -> editor messages) with the
//! chrome (crop preview, selection frame, handles, caret, a step's note).
//! Annotations themselves are painted by `editor::render`, never here; what
//! the pointer and keys mean is the editor's (`Editor::key`, `cursor`).
use iced::keyboard;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Fill, Frame, Geometry, LineDash, Path, Stroke, fill};
use iced::widget::{image, pin, stack};
use iced::{ContentFit, Element, Point, Rectangle, Renderer, Size, Theme};

use crate::editor::style::{DIM_OUTSIDE, HANDLE, NOTE_FILL, NOTE_FONT, NOTE_HINT, NOTE_INK};
use crate::editor::{Editor, Message};
use crate::ui::kit::themes::c;

/// The editing surface, `size()` big: for Edit in place, over the selection.
pub fn canvas(e: &Editor) -> Element<'_, Message> {
    let size = e.size();
    let view = e.doc.view();
    let mut layers: Vec<Element<'_, Message>> = vec![
        image(e.base.clone())
            .width(size.width)
            .height(size.height)
            .content_fit(ContentFit::Fill)
            .into(),
    ];
    // Always a layer here, live or not: the canvas must keep its index, or iced
    // gives it a fresh state mid-drag (the press is lost, the drag draws nothing).
    let live = e.live.as_ref().map(|(r, handle)| {
        let at = Point::new((r.x - view.x) * e.zoom, (r.y - view.y) * e.zoom);
        pin(image(handle.clone())
            .width(r.width * e.zoom)
            .height(r.height * e.zoom)
            .content_fit(ContentFit::Fill))
        .position(at)
    });
    layers.push(match live {
        Some(live) => live.into(),
        None => iced::widget::space().into(),
    });
    layers.push(
        iced::widget::canvas(Input { editor: e })
            .width(size.width)
            .height(size.height)
            .into(),
    );
    stack(layers).width(size.width).height(size.height).into()
}

struct Input<'a> {
    editor: &'a Editor,
}

#[derive(Default)]
struct Pointer {
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
            // Not captured: other widgets see modifiers too.
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
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
        _pointer: &Pointer,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let e = self.editor;
        let mut frame = Frame::new(renderer, bounds.size());
        let accent = Stroke::default().with_color(c().accent).with_width(1.5);

        if let Some(keep) = e.crop_preview() {
            let outside = Path::new(|p| {
                p.rectangle(Point::ORIGIN, bounds.size());
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

        // Line assist: a faint guide over the line the brush is held on.
        if let Some(r) = e.assist_band() {
            frame.fill_rectangle(r.position(), r.size(), c().accent.scale_alpha(0.35));
        }

        if let Some(f) = e.frame_rect() {
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
            } else {
                for (bend, at) in e.handle_marks() {
                    let knob = match bend {
                        true => Path::circle(at, 5.5),
                        false => {
                            Path::rectangle(at - iced::Vector::new(4.5, 4.5), Size::new(9.0, 9.0))
                        }
                    };
                    frame.fill(&knob, HANDLE);
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
        if e.picking {
            return mouse::Interaction::Crosshair;
        }
        e.cursor(e.to_image(at))
    }
}
