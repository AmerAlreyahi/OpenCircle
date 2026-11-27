//! The canvas card's rounded shape over what is drawn in it. iced clips a widget's content to a
//! plain rectangle, so a picture larger than the card (any zoom past Fit, or scrolled) would
//! cover the card's rounded corners. These four corner pieces, in the window's fill and drawn
//! above the picture, its shapes and its selection, give the corners back: one small path per
//! corner, nothing copied from the picture.
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Path};
use iced::{Fill, Length, Rectangle, Renderer, Theme, Vector};

use super::Msg;
use crate::shared::kit::themes::c;

struct Corners(f32);

impl canvas::Program<Msg> for Corners {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<canvas::Geometry> {
        let (w, h, r) = (bounds.width, bounds.height, self.0);
        let piece = Path::new(|p| {
            p.move_to(iced::Point::ORIGIN);
            p.line_to(iced::Point::new(0.0, r));
            // a quarter circle as one Bezier curve (kappa 0.5523)
            let k = r * (1.0 - 0.552_284_8);
            p.bezier_curve_to(
                iced::Point::new(0.0, k),
                iced::Point::new(k, 0.0),
                iced::Point::new(r, 0.0),
            );
            p.close();
        });
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for (at, flip) in [
            ((0.0, 0.0), (1.0, 1.0)),
            ((w, 0.0), (-1.0, 1.0)),
            ((0.0, h), (1.0, -1.0)),
            ((w, h), (-1.0, -1.0)),
        ] {
            frame.with_save(|f| {
                f.translate(Vector::new(at.0, at.1));
                f.scale_nonuniform(Vector::new(flip.0, flip.1));
                f.fill(&piece, c().bg);
            });
        }
        vec![frame.into_geometry()]
    }
}

/// The four corner pieces, as a layer the size of the card (`radius` as the card is rounded).
pub fn over<'a>(radius: f32) -> iced::Element<'a, Msg> {
    canvas::Canvas::new(Corners(radius))
        .width(Length::Fill)
        .height(Fill)
        .into()
}
