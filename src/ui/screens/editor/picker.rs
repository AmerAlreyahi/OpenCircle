//! The full-screen area picker (Add capture): dims all but the dragged box.
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke, fill};
use iced::{Point, Rectangle, Renderer, Theme};

use crate::editor::style::{DIM_OUTSIDE, HANDLE};
use crate::editor::{self, Message};

pub struct Picker {
    pub drag: Option<(Point, Point)>,
}

impl canvas::Program<Message> for Picker {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        use iced::keyboard::{self, Key, key::Named};
        let message = match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Message::Press(cursor.position_in(bounds)?)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if self.drag.is_some() => {
                Message::Move(cursor.position_in(bounds)?)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Message::Release
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(Named::Escape),
                ..
            }) => Message::Back,
            _ => return None,
        };
        Some(Action::publish(message).and_capture())
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let keep = self.drag.map(|(a, b)| editor::span(&[a, b]));
        let outside = Path::new(|p| {
            p.rectangle(Point::ORIGIN, bounds.size());
            if let Some(r) = keep {
                p.rectangle(r.position(), r.size());
            }
        });
        frame.fill(
            &outside,
            canvas::Fill {
                style: canvas::Style::Solid(DIM_OUTSIDE),
                rule: fill::Rule::EvenOdd,
            },
        );
        if let Some(r) = keep {
            let edge = Stroke::default().with_color(HANDLE).with_width(1.5);
            frame.stroke(&Path::rectangle(r.position(), r.size()), edge);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        _bounds: Rectangle,
        _cursor: Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::Crosshair
    }
}
