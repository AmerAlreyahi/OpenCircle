//! Combine board: several captures laid out as one image (stacked, side by side
//! or in a grid, with a gap and optional number labels), so one picture can show
//! a whole flow. The board is the document's image; annotations live on it and
//! may span captures. A relayout moves each annotation with the capture it is on.
use iced::{Point, Rectangle, Size, Vector};

use super::document::{Image, Item, Layer};
use super::label;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Stack,
    Side,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Board {
    pub layout: Layout,
    /// A number label on each capture.
    pub numbers: bool,
    /// How many captures are on it.
    pub count: usize,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            layout: Layout::Stack,
            numbers: true,
            count: 1,
        }
    }
}

/// Between captures and around them, image px.
pub const GAP: u32 = 24;
/// Behind the captures.
pub const BACKGROUND: [u8; 4] = [236, 238, 243, 255];
const NUMBER: [u8; 4] = [28, 28, 36, 255];

/// Where each capture of `sizes` goes and the board's size. One capture is the
/// board itself (no gap). Captures are packed in rows (shelves) with `GAP`
/// between them: stacked = one per row, side by side = one row, grid = the
/// number of columns that makes the board closest to a 16:10 screen.
pub fn place(sizes: &[(u32, u32)], layout: Layout) -> (Vec<Rectangle>, Size) {
    if let [one] = sizes {
        let size = Size::new(one.0 as f32, one.1 as f32);
        return (vec![Rectangle::new(Point::ORIGIN, size)], size);
    }
    let n = sizes.len().max(1);
    match layout {
        Layout::Stack => shelves(sizes, 1),
        Layout::Side => shelves(sizes, n),
        Layout::Grid => (1..=n)
            .map(|cols| shelves(sizes, cols))
            .min_by(|(_, a), (_, b)| {
                let off = |s: &Size| (s.width / s.height - 1.6).abs();
                off(a).total_cmp(&off(b))
            })
            .expect("at least one layout"),
    }
}

/// `cols` captures per row, left to right, each row as tall as its tallest.
fn shelves(sizes: &[(u32, u32)], cols: usize) -> (Vec<Rectangle>, Size) {
    let mut tiles = Vec::with_capacity(sizes.len());
    let (mut y, mut width) = (GAP, 0);
    for row in sizes.chunks(cols.max(1)) {
        let mut x = GAP;
        for &(w, h) in row {
            tiles.push(Rectangle::new(
                Point::new(x as f32, y as f32),
                Size::new(w as f32, h as f32),
            ));
            x += w + GAP;
        }
        width = width.max(x);
        y += row.iter().map(|s| s.1).max().unwrap_or(0) + GAP;
    }
    (tiles, Size::new(width as f32, y as f32))
}

/// The board image of `sources` and where each one is on it.
pub fn compose(sources: &[Image], board: &Board) -> (Image, Vec<Rectangle>) {
    let sizes: Vec<_> = sources.iter().map(|s| (s.width, s.height)).collect();
    let (tiles, size) = place(&sizes, board.layout);
    let (w, h) = (size.width as u32, size.height as u32);
    let mut rgba = BACKGROUND.repeat((w * h) as usize);
    for (src, tile) in sources.iter().zip(&tiles) {
        let row = src.width as usize * 4;
        for y in 0..src.height as usize {
            let at = ((tile.y as usize + y) * w as usize + tile.x as usize) * 4;
            rgba[at..at + row].copy_from_slice(&src.rgba[y * row..(y + 1) * row]);
        }
    }
    if board.numbers && sources.len() > 1 {
        let pm = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap());
        let mut layer = Layer {
            pm: pm.expect("board pixmap"),
            origin: Point::ORIGIN,
        };
        for (i, tile) in tiles.iter().enumerate() {
            let size = (tile.width.min(tile.height) / 16.0).clamp(14.0, 22.0);
            let at = tile.position() + Vector::new(size * 0.5, size * 0.5);
            label::paint(&mut layer, at, &(i + 1).to_string(), size, NUMBER);
        }
        rgba = layer.pm.take(); // opaque: premultiplied = straight
    }
    (
        Image {
            width: w,
            height: h,
            rgba,
        },
        tiles,
    )
}

/// `items` moved from the layout `from` to `to` (same captures, same order, `to`
/// may have more): each moves with the capture under its centre, or the nearest.
pub fn remap(items: &[Item], from: &[Rectangle], to: &[Rectangle]) -> Vec<Item> {
    items
        .iter()
        .map(|item| {
            let c = item.frame().center();
            let distance = |r: &Rectangle| {
                let dx = (r.x - c.x).max(c.x - r.x - r.width).max(0.0);
                let dy = (r.y - c.y).max(c.y - r.y - r.height).max(0.0);
                dx.hypot(dy)
            };
            let nearest = (0..from.len().min(to.len()))
                .min_by(|&a, &b| distance(&from[a]).total_cmp(&distance(&from[b])));
            let mut item = item.clone();
            if let Some(i) = nearest {
                let by = to[i].position() - from[i].position();
                item.shape.transform(|p| p + by, 1.0);
            }
            item
        })
        .collect()
}
