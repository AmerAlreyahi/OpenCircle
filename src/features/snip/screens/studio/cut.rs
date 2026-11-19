//! The Studio's Cut out (v0.4.6, STUDIO_V2_SPEC S9): the picture's background
//! goes transparent as one undoable edit of its pixels (`Document::set_pixels`:
//! Ctrl+Z brings the background back). The options bar offers Before | After
//! to compare; the picture sits on a checkerboard while it has transparency.
//! The work (`features::cutout`) runs off the UI thread; this is what the
//! window keeps of it.
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame};
use iced::{Length, Point, Rectangle, Renderer, Size, Theme};

use super::Msg;
use crate::shared::kit::tokens::fixed::CHECKER;

/// A finished cut, for the compare view.
pub struct Cut {
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    /// The history's state right after the cut: any other edit (Undo too)
    /// ends the comparison.
    pub state: u64,
    pub showing_before: bool,
}

/// Where the cut stands in the host's work.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// Cutting (the row spins).
    Working,
    /// The model's download card is up; its Download runs the cut.
    Asking,
}

/// What the helper work answers.
#[derive(Debug, Clone)]
pub enum Done {
    /// The alpha, one byte a pixel.
    Alpha(Vec<u8>),
    /// A busy background and no model on disk yet.
    NeedsModel,
    Failed(String),
}

/// A picture with any see-through pixel.
pub fn has_alpha(rgba: &[u8]) -> bool {
    rgba.iter().skip(3).step_by(4).any(|&a| a < 255)
}

/// The squares behind the picture (logical px a side).
const SQUARE: f32 = 8.0;

pub struct Checker;

impl canvas::Program<Msg> for Checker {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), CHECKER.0);
        let (cols, rows) = (
            (bounds.width / SQUARE).ceil() as u32,
            (bounds.height / SQUARE).ceil() as u32,
        );
        for y in 0..rows {
            for x in (y % 2..cols).step_by(2) {
                let at = Point::new(x as f32 * SQUARE, y as f32 * SQUARE);
                let size = Size::new(
                    SQUARE.min(bounds.width - at.x),
                    SQUARE.min(bounds.height - at.y),
                );
                frame.fill_rectangle(at, size, CHECKER.1);
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The checkerboard as big as the shown picture.
pub fn checker<'a>(shown: Size) -> iced::Element<'a, Msg> {
    canvas::Canvas::new(Checker)
        .width(Length::Fixed(shown.width))
        .height(Length::Fixed(shown.height))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_is_found_anywhere() {
        let mut p = [1, 2, 3, 255].repeat(100);
        assert!(!has_alpha(&p));
        p[99 * 4 + 3] = 254;
        assert!(has_alpha(&p));
    }
}
