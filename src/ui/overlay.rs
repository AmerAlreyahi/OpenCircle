//! Overlay canvas (drawn over the frozen frame): dim, freehand circle with a glowing
//! gradient stroke.
use std::cell::Cell;
use std::time::Instant;

use iced::keyboard::{self, key::Named};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Fill, Geometry, Path, Stroke, fill, gradient, stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use super::theme;

/// A selection smaller than this (logical px, either side) is a tap, not a circle.
const MIN_SIDE: f32 = 12.0;
const SELECTION_RADIUS: f32 = 16.0;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Press(Point),
    Move(Point),
    Release,
    Close,
}

/// The selection a stroke means: its bounding box, or `None` for a tiny stroke.
pub fn fit_selection(stroke: &[Point]) -> Option<Rectangle> {
    let first = stroke.first()?;
    let (mut min, mut max) = (*first, *first);
    for p in stroke {
        min = Point::new(min.x.min(p.x), min.y.min(p.y));
        max = Point::new(max.x.max(p.x), max.y.max(p.y));
    }
    let size = Size::new(max.x - min.x, max.y - min.y);
    (size.width >= MIN_SIDE && size.height >= MIN_SIDE).then(|| Rectangle::new(min, size))
}

pub struct Overlay<'a> {
    pub stroke: &'a [Point],
    pub drawing: bool,
    pub selection: Option<Rectangle>,
    pub first_draw: &'a Cell<Option<(Instant, Size)>>,
}

impl canvas::Program<Event> for Overlay<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Event>> {
        let event = match event {
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(Named::Escape),
                ..
            }) => Event::Close,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Event::Press(cursor.position_in(bounds)?)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if self.drawing => {
                Event::Move(cursor.position_in(bounds)?)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.drawing =>
            {
                Event::Release
            }
            _ => return None,
        };
        Some(Action::publish(event).and_capture())
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let screen = Rectangle::with_size(bounds.size());

        match self.selection {
            Some(sel) => {
                let outside = Path::new(|p| {
                    p.rectangle(Point::ORIGIN, screen.size());
                    p.rounded_rectangle(sel.position(), sel.size(), SELECTION_RADIUS.into());
                });
                frame.fill(
                    &outside,
                    Fill {
                        style: canvas::Style::Solid(theme::DIM_OUTSIDE),
                        rule: fill::Rule::EvenOdd,
                    },
                );
            }
            None => frame.fill_rectangle(Point::ORIGIN, screen.size(), theme::DIM_IDLE),
        }

        if self.stroke.len() >= 2 {
            let path = Path::new(|p| {
                p.move_to(self.stroke[0]);
                self.stroke[1..].iter().for_each(|&q| p.line_to(q));
            });
            let span = fit_selection(self.stroke).unwrap_or(screen);
            // Soft glow = the same stroke, wider and fainter, under the core line.
            for (width, alpha) in [(18.0, 0.10), (10.0, 0.22), (3.5, 1.0)] {
                frame.stroke(
                    &path,
                    Stroke {
                        style: gradient_style(span, alpha),
                        width,
                        line_cap: stroke::LineCap::Round,
                        line_join: stroke::LineJoin::Round,
                        ..Stroke::default()
                    },
                );
            }
        }

        // Logged once the window is really full-screen (WSLg maps it windowed first).
        if let Some((at, size)) = self.first_draw.get()
            && (bounds.width - size.width).abs() < 1.0
            && (bounds.height - size.height).abs() < 1.0
        {
            self.first_draw.set(None);
            eprintln!(
                "[opencircle] trigger -> first full-screen overlay frame drawn: {} ms",
                at.elapsed().as_millis()
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), _: Rectangle, _: Cursor) -> mouse::Interaction {
        mouse::Interaction::Crosshair
    }
}

fn gradient_style(span: Rectangle, alpha: f32) -> canvas::Style {
    let tint = |c: Color| c.scale_alpha(alpha);
    let linear = gradient::Linear::new(span.position(), span.position() + span.size().into())
        .add_stop(0.0, tint(theme::STROKE_START))
        .add_stop(1.0, tint(theme::STROKE_END));
    canvas::Style::Gradient(linear.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_fits_to_its_bounding_box() {
        let circle: Vec<Point> = (0..64)
            .map(|i| {
                let a = i as f32 / 64.0 * std::f32::consts::TAU;
                Point::new(200.0 + 50.0 * a.cos(), 100.0 + 30.0 * a.sin())
            })
            .collect();
        let sel = fit_selection(&circle).unwrap();
        assert!((sel.x - 150.0).abs() < 0.5 && (sel.y - 70.0).abs() < 0.5);
        assert!((sel.width - 100.0).abs() < 0.5 && (sel.height - 60.0).abs() < 0.5);
    }

    #[test]
    fn tiny_strokes_are_ignored() {
        assert_eq!(fit_selection(&[]), None);
        assert_eq!(fit_selection(&[Point::new(5.0, 5.0)]), None);
        // Long but flat: a line, not a circle.
        assert_eq!(
            fit_selection(&[Point::new(0.0, 0.0), Point::new(300.0, 4.0)]),
            None
        );
        assert!(fit_selection(&[Point::new(0.0, 0.0), Point::new(12.0, 12.0)]).is_some());
    }
}
