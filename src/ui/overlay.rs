//! Overlay canvas (drawn over the frozen frame): dim, then the selection (a freehand
//! loop or a dragged rectangle) with a glowing gradient stroke.
use std::cell::Cell;
use std::time::Instant;

use iced::keyboard::{self, key::Named};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Fill, Geometry, Path, Stroke, fill, gradient, stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use super::theme;

/// A selection smaller than this (logical px, either side) is a tap, not a circle.
const MIN_SIDE: f32 = 12.0;

/// A press-drag is a rectangle if no point strays further than this fraction of
/// the start→end distance from the start→end segment (a loop strays much further).
const DRAG_STRAY: f32 = 0.2;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Press(Point),
    Move(Point),
    /// The pointer moved with no button down (only when `Overlay::hover`).
    Hover(Point),
    Release,
    /// Enter: the default action.
    Enter,
    /// A letter key (Latin, lower-case, keyboard-layout independent), with Ctrl or not.
    Key {
        letter: char,
        ctrl: bool,
    },
}

/// What a finished stroke selects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    /// Logical px. For a loop: its bounding box (the crop).
    pub area: Rectangle,
    /// A dragged rectangle (`area` is the shape) rather than a freehand loop.
    pub drag: bool,
}

/// The selection a stroke means, or `None` for a tap / thin line.
pub fn select(stroke: &[Point]) -> Option<Selection> {
    let drag = is_drag(stroke);
    let area = if drag {
        drag_rect(stroke)
    } else {
        fit_selection(stroke)?
    };
    (area.width >= MIN_SIDE && area.height >= MIN_SIDE).then_some(Selection { area, drag })
}

/// A mostly straight press-drag that doesn't loop back: every point stays near the
/// segment from the first point to the last.
// ponytail: one threshold, no timing/speed; tune DRAG_STRAY if real strokes misfire.
fn is_drag(stroke: &[Point]) -> bool {
    let (Some(&a), Some(&b)) = (stroke.first(), stroke.last()) else {
        return false;
    };
    let chord = a.distance(b);
    chord > 0.0
        && stroke
            .iter()
            .all(|&p| to_segment(p, a, b) <= DRAG_STRAY * chord)
}

/// Distance from `p` to the segment `a`–`b` (`a` ≠ `b`).
fn to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (ab, ap) = (b - a, p - a);
    let t = ((ap.x * ab.x + ap.y * ab.y) / (ab.x * ab.x + ab.y * ab.y)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The rectangle a drag spans: first and last points are opposite corners.
fn drag_rect(stroke: &[Point]) -> Rectangle {
    let (Some(&a), Some(&b)) = (stroke.first(), stroke.last()) else {
        return Rectangle::default();
    };
    let min = Point::new(a.x.min(b.x), a.y.min(b.y));
    Rectangle::new(min, Size::new((a.x - b.x).abs(), (a.y - b.y).abs()))
}

/// The selection a stroke means: its bounding box, or `None` for a tiny stroke.
fn fit_selection(stroke: &[Point]) -> Option<Rectangle> {
    let first = stroke.first()?;
    let (mut min, mut max) = (*first, *first);
    for p in stroke {
        min = Point::new(min.x.min(p.x), min.y.min(p.y));
        max = Point::new(max.x.max(p.x), max.y.max(p.y));
    }
    let size = Size::new(max.x - min.x, max.y - min.y);
    (size.width >= MIN_SIDE && size.height >= MIN_SIDE).then(|| Rectangle::new(min, size))
}

/// The stroke as quadratic curves `(control, end)` from `stroke[0]`, through the
/// segment midpoints: smooth, and never outside the stroke's bounding box.
fn smooth(stroke: &[Point]) -> Vec<(Point, Point)> {
    let mid = |a: Point, b: Point| Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let mut curves: Vec<_> = stroke.windows(2).map(|w| (w[0], mid(w[0], w[1]))).collect();
    curves.extend(stroke.last().map(|&p| (p, p)));
    curves
}

/// Adds the smoothed stroke to `p`; `closed` joins its end back to its start.
fn trace(p: &mut canvas::path::Builder, stroke: &[Point], closed: bool) {
    let Some(&first) = stroke.first() else { return };
    p.move_to(first);
    smooth(stroke)
        .into_iter()
        .for_each(|(control, end)| p.quadratic_curve_to(control, end));
    if closed {
        p.close();
    }
}

pub struct Overlay<'a> {
    pub stroke: &'a [Point],
    pub selection: Option<Selection>,
    /// Counts overlay openings: pointer state from an earlier opening is stale.
    pub opening: u32,
    pub first_draw: &'a Cell<Option<(Instant, Size)>>,
    /// Report moves without a button down too (snip's Window mode).
    pub hover: bool,
}

/// The pointer as the canvas saw it, event by event. Events can arrive in one batch
/// (behind a slow frame): `cursor` and the app state are then already stale.
#[derive(Default)]
pub struct Pointer {
    opening: u32,
    at: Option<Point>,
    down: bool,
}

impl canvas::Program<Event> for Overlay<'_> {
    type State = Pointer;

    fn update(
        &self,
        pointer: &mut Pointer,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Event>> {
        if pointer.opening != self.opening {
            *pointer = Pointer {
                opening: self.opening,
                ..Pointer::default()
            };
        }
        let event = match event {
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) => match key {
                keyboard::Key::Named(Named::Enter) => Event::Enter,
                _ => Event::Key {
                    letter: key.to_latin(*physical_key)?.to_ascii_lowercase(),
                    ctrl: modifiers.command(),
                },
            },
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let now = cursor.position_in(bounds)?; // None = over the action bar
                pointer.down = true;
                Event::Press(pointer.at.unwrap_or(now))
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let at = *position - (bounds.position() - Point::ORIGIN);
                pointer.at = Some(at);
                match (pointer.down, self.hover) {
                    (true, _) => Event::Move(at),
                    (false, true) => Event::Hover(at),
                    (false, false) => return None,
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if pointer.down =>
            {
                pointer.down = false;
                Event::Release
            }
            _ => return None,
        };
        Some(Action::publish(event).and_capture())
    }

    fn draw(
        &self,
        _pointer: &Pointer,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let screen = Rectangle::with_size(bounds.size());

        // After release, the clear region is the drawn shape itself (closed), not its
        // bounding box.
        // ponytail: even-odd re-dims areas circled twice; trim the stroke at its first
        // self-crossing if users circle twice.
        let closed = self.selection.is_some();
        // While drawing, a straight drag already shows as the rectangle it will select.
        let drag = self.selection.map_or(is_drag(self.stroke), |s| s.drag);
        let shape = |p: &mut canvas::path::Builder| {
            if drag {
                let r = drag_rect(self.stroke);
                p.rectangle(r.position(), r.size());
            } else {
                trace(p, self.stroke, closed);
            }
        };
        if closed {
            let outside = Path::new(|p| {
                p.rectangle(Point::ORIGIN, screen.size());
                shape(p);
            });
            frame.fill(
                &outside,
                Fill {
                    style: canvas::Style::Solid(theme::DIM_OUTSIDE),
                    rule: fill::Rule::EvenOdd,
                },
            );
        } else {
            frame.fill_rectangle(Point::ORIGIN, screen.size(), theme::DIM_IDLE);
        }

        if self.stroke.len() >= 2 {
            let path = Path::new(shape);
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

    fn mouse_interaction(&self, _: &Pointer, _: Rectangle, _: Cursor) -> mouse::Interaction {
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
    fn smoothed_stroke_stays_in_its_box_and_survives_odd_input() {
        // A figure eight: self-intersecting.
        let eight: Vec<Point> = (0..=64)
            .map(|i| {
                let a = i as f32 / 64.0 * std::f32::consts::TAU;
                Point::new(100.0 + 80.0 * a.sin(), 100.0 + 40.0 * (2.0 * a).sin())
            })
            .collect();
        let sel = fit_selection(&eight).unwrap();
        let curves = smooth(&eight);
        assert_eq!(curves.len(), eight.len());
        for (control, end) in &curves {
            for p in [control, end] {
                assert!(
                    sel.x - 0.01 <= p.x && p.x <= sel.x + sel.width + 0.01,
                    "{p:?}"
                );
                assert!(
                    sel.y - 0.01 <= p.y && p.y <= sel.y + sel.height + 0.01,
                    "{p:?}"
                );
            }
        }
        // Curves end at segment midpoints (not the raw points) and finish on the last point.
        let mid = Point::new(
            (eight[1].x + eight[2].x) / 2.0,
            (eight[1].y + eight[2].y) / 2.0,
        );
        assert_eq!(curves[1], (eight[1], mid));
        assert_eq!(curves.last(), Some(&(eight[64], eight[64])));

        // Degenerate strokes build a path without panicking.
        assert!(smooth(&[]).is_empty());
        let dot = [Point::new(5.0, 5.0)];
        for stroke in [&eight[..], &[], &dot, &[dot[0], dot[0]]] {
            let _ = Path::new(|p| trace(p, stroke, true));
        }
    }

    fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, turn: f32) -> Vec<Point> {
        (0..=64)
            .map(|i| {
                let a = i as f32 / 64.0 * turn * std::f32::consts::TAU;
                Point::new(cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect()
    }

    #[test]
    fn a_straight_drag_selects_the_rectangle_it_spans() {
        // Slow 45° mouse drag: a pixel staircase, up-left (end before start).
        let mut stairs = vec![Point::new(300.0, 200.0)];
        for i in 0..150 {
            let last = *stairs.last().unwrap();
            stairs.push(if i % 2 == 0 {
                Point::new(last.x - 1.0, last.y)
            } else {
                Point::new(last.x, last.y - 1.0)
            });
        }
        let sel = select(&stairs).unwrap();
        assert!(sel.drag);
        assert_eq!(
            sel.area,
            Rectangle::new(Point::new(225.0, 125.0), Size::new(75.0, 75.0))
        );

        // A wobbly, slightly bowed drag is still a drag.
        let wobbly: Vec<Point> = (0..=50)
            .map(|i| {
                let t = i as f32 / 50.0;
                let bow = 25.0 * (t * std::f32::consts::PI).sin();
                Point::new(100.0 + 300.0 * t + bow, 100.0 + 120.0 * t + (i % 3) as f32)
            })
            .collect();
        assert!(select(&wobbly).unwrap().drag);
    }

    #[test]
    fn loops_and_arcs_select_their_bounding_box() {
        for stroke in [
            ellipse(200.0, 100.0, 50.0, 30.0, 1.0),  // closed loop
            ellipse(200.0, 100.0, 50.0, 30.0, 0.85), // loop not quite closed
            ellipse(200.0, 100.0, 50.0, 50.0, 0.5),  // half circle (a "C")
        ] {
            let sel = select(&stroke).unwrap();
            assert!(!sel.drag, "{:?}", stroke.last());
            assert_eq!(Some(sel.area), fit_selection(&stroke));
        }
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
        // A flat drag is a line, not a rectangle.
        assert_eq!(
            select(&[Point::new(0.0, 0.0), Point::new(300.0, 4.0)]),
            None
        );
        assert_eq!(select(&[Point::new(5.0, 5.0)]), None);
    }
}
