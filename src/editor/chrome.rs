//! What the canvas draws over the image (display px), so the view
//! (`ui::screens::editor::canvas`) needs none of the editor's internals.
use iced::{Point, Rectangle, Size};

use super::{Editor, Shape, select, text};

impl Editor {
    /// The crop being dragged.
    pub fn crop_preview(&self) -> Option<Rectangle> {
        let (a, b) = self.cropping?;
        Some(self.to_display(super::span(&[a, b])))
    }

    /// The text line the blur brush is held on.
    pub fn assist_band(&self) -> Option<Rectangle> {
        let b = self.lock.as_ref()?.band;
        let r = Rectangle::new(Point::new(b.x0, b.y0), Size::new(b.x1 - b.x0, b.height()));
        Some(self.to_display(r))
    }

    /// The selection frame of the framed item.
    pub fn frame_rect(&self) -> Option<Rectangle> {
        Some(self.to_display(self.framed()?.frame()))
    }

    /// The framed item's handles: (a bend knob, else a corner square, centre).
    pub fn handle_marks(&self) -> Vec<(bool, Point)> {
        let Some(item) = self.framed() else {
            return Vec::new();
        };
        select::handles(item)
            .into_iter()
            .map(|(h, at)| (matches!(h, select::Handle::Bend), self.to_display_point(at)))
            .collect()
    }

    /// The caret to draw while typing: its top and its height.
    pub fn caret_rect(&self) -> Option<(Point, f32)> {
        let (at, size) = self.caret()?;
        Some((
            self.to_display_point(at),
            text::line_height(size) * self.zoom,
        ))
    }

    /// The step note while it is typed, in a small pill beside the badge.
    pub fn note_pill(&self) -> Option<NotePill> {
        let item = self.framed().filter(|_| self.typing())?;
        let Shape::Step { at, radius, note } = &item.shape else {
            return None;
        };
        let (origin, size) = self.note_origin(*at, *radius);
        let shown = if note.is_empty() {
            "Note for the legend"
        } else {
            note
        };
        Some(NotePill {
            top: self.to_display_point(origin),
            text: text::measure(shown, size) * self.zoom,
            shown: shown.to_string(),
            empty: note.is_empty(),
            size: size * self.zoom,
            line: text::line_height(size) * self.zoom,
        })
    }
}

/// A step's note being typed (display px).
pub struct NotePill {
    pub top: Point,
    /// The text's measured size.
    pub text: Size,
    pub shown: String,
    /// Nothing typed yet: `shown` is the hint.
    pub empty: bool,
    pub size: f32,
    pub line: f32,
}
