//! What the canvas draws over the image (display px), so the view
//! (`features::snip::screens::snip_toolbar::canvas`) needs none of the editor's internals.
use iced::{Point, Rectangle, Size};

use super::{Annotator, Shape, select, text};

impl Annotator {
    /// The only item, a spotlight, being dragged: its hole (display px), clipped to the picture. The
    /// dim round it is filled by the canvas OVER the base (W113: a layer of the whole picture
    /// repainted at every move cost 47 ms on 4K). That is right only when the base holds the bare
    /// picture: the committed picture dims the picture and draws every other item on top, undimmed,
    /// so over any other item, or a Hide's patch, the canvas dim would darken them until the release.
    /// With anything else in the picture (`None`) the live layer paints the union as before.
    pub fn spotlight_hole(&self) -> Option<Rectangle> {
        let pending = self.pending.as_ref().filter(|_| self.live_on)?;
        let Shape::Spotlight { a, b } = pending.item.shape else {
            return None;
        };
        let alone = (self.doc.items.iter().enumerate()).all(|(i, _)| Some(i) == pending.index);
        let picture = Rectangle::with_size(self.size());
        (alone && self.patch.is_empty())
            .then(|| self.to_display(super::span(&[a, b])).intersection(&picture))
            .flatten()
    }

    /// The crop being dragged.
    pub fn crop_preview(&self) -> Option<Rectangle> {
        let (a, b) = self.cropping?;
        Some(self.to_display(super::span(&[a, b])))
    }

    /// Where the framed item's handles are (none while it is typed into).
    pub fn handle_marks(&self) -> Vec<Point> {
        // Several selected: each one's own small handles (none drag): an
        // arrow or line its two ends, a box its corners, a pen, highlighter
        // or blur stroke the corners of its box.
        if !self.doc.extra.is_empty() && self.pending.is_none() {
            return (self.doc.selection().iter())
                .flat_map(|&i| select::marks(&self.doc.items[i]))
                .map(|at| self.to_display_point(at))
                .collect();
        }
        let Some(item) = self.framed().filter(|_| !self.typing()) else {
            return Vec::new();
        };
        select::handles(item)
            .into_iter()
            .map(|(_, at)| self.to_display_point(at))
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
