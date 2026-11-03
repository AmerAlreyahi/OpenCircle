//! Legend: the numbered steps' notes in one box, a badge and its note per row,
//! flattened into the image so whoever reads it (often an AI) gets the
//! explanation with the picture. It sits in a free corner of the view, or in a
//! white band added under the image when no corner is free.
use iced::{Point, Rectangle, Size};

use super::document::{Item, Layer, Shape, rounded};
use super::{step, text};

pub struct Row {
    pub number: u32,
    pub colour: [u8; 4],
    pub note: String,
}

pub struct Legend {
    pub rows: Vec<Row>,
    /// Font size, image px.
    pub size: f32,
    /// The box, image px.
    pub at: Rectangle,
    /// Height of the band added under the view (0 = the box is in a corner).
    pub band: f32,
}

const PAPER: [u8; 4] = [255, 255, 255, 240];
const BAND: [u8; 4] = [255, 255, 255, 255];
const EDGE: [u8; 4] = [0, 0, 0, 50];
const INK: [u8; 4] = [28, 28, 36, 255];

/// Every step that has a note, numbered by its place among all steps.
pub fn rows(items: &[Item]) -> Vec<Row> {
    let mut number = 0;
    items
        .iter()
        .filter_map(|item| {
            let Shape::Step { note, .. } = &item.shape else {
                return None;
            };
            number += 1;
            let note = note.trim();
            (!note.is_empty()).then(|| Row {
                number,
                colour: item.colour,
                note: note.replace('\n', " "),
            })
        })
        .collect()
}

/// The text size: readable at any capture size (on a board, the largest
/// capture's, not the whole board's).
fn font(view: Rectangle, tiles: &[Rectangle]) -> f32 {
    let basis = tiles
        .iter()
        .map(|t| t.width.min(t.height))
        .fold(0.0, f32::max);
    let basis = if tiles.is_empty() {
        view.width.min(view.height)
    } else {
        basis
    };
    (basis / 28.0).clamp(13.0, 28.0)
}

/// Badge radius, row height and the text's left edge, for a font size.
fn metrics(size: f32) -> (f32, f32, f32) {
    let r = size * 0.62;
    let row = text::line_height(size).max(2.0 * r) + size * 0.35;
    (r, row, size * 0.7 + 2.0 * r + size * 0.5)
}

fn box_size(rows: &[Row], size: f32) -> Size {
    let (_, row, text_x) = metrics(size);
    let widest = rows
        .iter()
        .map(|r| text::measure(&r.note, size).width)
        .fold(0.0, f32::max);
    let pad = size * 0.7;
    Size::new(
        text_x + widest + pad,
        2.0 * pad + rows.len() as f32 * row - size * 0.35,
    )
}

/// Where the legend goes in `view` among `items`, or `None` (no notes). On a
/// board (`tiles` = its captures) it first tries the space right of or under a
/// capture, so it sits next to the pictures instead of in a far corner.
pub fn layout(view: Rectangle, items: &[Item], tiles: &[Rectangle]) -> Option<Legend> {
    let rows = rows(items);
    if rows.is_empty() {
        return None;
    }
    let size = font(view, tiles);
    let b = box_size(&rows, size);
    let margin = size * 0.8;
    let (left, top) = (view.x + margin, view.y + margin);
    let right = view.x + view.width - margin - b.width;
    let bottom = view.y + view.height - margin - b.height;
    let fits = right >= left && bottom >= top;
    let gap = super::board::GAP as f32;
    let beside = tiles.iter().flat_map(|t| {
        [
            Point::new(t.x + t.width + gap, t.y),
            Point::new(t.x, t.y + t.height + gap),
        ]
    });
    // Bottom-right first: the usual place for a key, away from title bars.
    let corners = [(right, bottom), (left, bottom), (right, top), (left, top)]
        .into_iter()
        .filter(|_| fits)
        .map(|(x, y)| Point::new(x, y));
    let inside = view.expand(-margin / 2.0);
    let corner = beside
        .chain(corners)
        .map(|at| Rectangle::new(at, b))
        .find(|r| {
            let clear = r.expand(margin / 2.0);
            inside.contains(r.position())
                && inside.contains(Point::new(r.x + r.width, r.y + r.height))
                && !tiles.iter().any(|t| t.intersects(&clear))
                && !items.iter().any(|item| match item.shape {
                    Shape::Spotlight { .. } => false,
                    // A blur's bounds include what it reads; it paints its frame.
                    Shape::Blur { .. } => item.frame().intersects(&clear),
                    _ => item.bounds().intersects(&clear),
                })
        });
    Some(match corner {
        Some(at) => Legend {
            rows,
            size,
            at,
            band: 0.0,
        },
        None => Legend {
            rows,
            size,
            at: Rectangle::new(Point::new(left, view.y + view.height + margin), b),
            band: (b.height + 2.0 * margin).ceil(),
        },
    })
}

/// Paints the legend (and its band, if any) into `layer`; `view` = the image's view.
pub fn paint(layer: &mut Layer, legend: &Legend, view: Rectangle) {
    let size = legend.size;
    let at = legend.at;
    if legend.band > 0.0 {
        let band = Rectangle::new(
            Point::new(view.x, view.y + view.height),
            Size::new(view.width, legend.band),
        );
        if let Some(path) = rounded(band, 0.0) {
            layer.fill(&path, BAND);
        }
    } else if let Some(path) = rounded(at, size * 0.6) {
        layer.fill(&path, PAPER);
        layer.stroke(&path, EDGE, 1.0);
    }
    let (r, row, text_x) = metrics(size);
    let pad = size * 0.7;
    for (k, line) in legend.rows.iter().enumerate() {
        let cy = at.y + pad + k as f32 * row + (row - size * 0.35) / 2.0;
        step::paint(
            layer,
            Point::new(at.x + pad + r, cy),
            r,
            line.number,
            line.colour,
        );
        let top = cy - text::line_height(size) / 2.0;
        text::paint(
            layer,
            Point::new(at.x + text_x, top),
            &line.note,
            size,
            INK,
            false,
        );
    }
}
