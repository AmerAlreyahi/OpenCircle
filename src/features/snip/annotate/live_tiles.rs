//! A long pen or highlighter stroke's live layer in fixed tiles (W113). One layer the size of the
//! stroke's box is painted, un-premultiplied, wrapped in a new image and drawn again every frame (4K:
//! 15 to 20 ms by the end of the stroke). Here the picture is cut into `TILE` px squares; a frame
//! repaints only the tiles the newest segments touch (each from the WHOLE stroke, clipped to the
//! tile, so a tile is exactly what one big layer would hold there), and every other tile keeps its
//! image: the renderer sees the same handle and repaints nothing for it.
use std::collections::BTreeMap;

use iced::widget::image;
use iced::{Point, Rectangle, Size};

use super::document::{Item, Shape, int_rect};
use super::{Annotator, straight};

/// A tile's side, image px.
const TILE: f32 = 512.0;
/// Below this many pixels (the view) a stroke is one layer: it is cheap, and a small picture is not
/// drawn through the Studio's pixel-for-pixel path that makes seams between tiles impossible.
pub(super) const MIN_PIXELS: f32 = 1_000_000.0;
/// The last points a new point can still change (the curve through the midpoints, the closing line).
const REPAINT_BACK: usize = 3;

/// The tiles painted so far, and the stroke they hold.
#[derive(Default)]
pub(super) struct LiveTiles {
    last: Option<Item>,
    tiles: BTreeMap<(i32, i32), (Rectangle, image::Handle)>,
}

impl LiveTiles {
    pub(super) fn clear(&mut self) {
        self.last = None;
        self.tiles.clear();
    }
}

fn points(item: &Item) -> Option<&[Point]> {
    match &item.shape {
        Shape::Pen(p) | Shape::Highlighter(p) => Some(p),
        _ => None,
    }
}

fn union(a: Rectangle, b: Rectangle) -> Rectangle {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = (
        (a.x + a.width).max(b.x + b.width),
        (a.y + a.height).max(b.y + b.height),
    );
    Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
}

fn meets(a: Rectangle, b: Rectangle) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// What of the stroke the new item changes: its newest points when it only grew, else both boxes.
fn dirty(old: Option<&Item>, new: &Item) -> Rectangle {
    let grown = old.and_then(|old| {
        let (o, n) = (points(old)?, points(new)?);
        let same = std::mem::discriminant(&old.shape) == std::mem::discriminant(&new.shape)
            && (old.colour, old.width) == (new.colour, new.width);
        (same && n.len() >= o.len() && n[..o.len()] == *o).then(|| {
            let mut tail = new.clone();
            if let Shape::Pen(p) | Shape::Highlighter(p) = &mut tail.shape {
                *p = n[o.len().saturating_sub(REPAINT_BACK)..].to_vec();
            }
            tail.bounds()
        })
    });
    grown.unwrap_or_else(|| old.map_or(new.bounds(), |old| union(old.bounds(), new.bounds())))
}

impl Annotator {
    /// The pending stroke is drawn as tiles: a pen or highlighter, on a picture of 1 MP or more,
    /// shown through the nearest path (the Studio's stage), and visible over the base.
    fn tiled(&self) -> bool {
        #[cfg(test)]
        if self.one_layer {
            return false;
        }
        let view = self.doc.view();
        self.live_on
            && self.crisp
            && view.width * view.height >= MIN_PIXELS
            && (self.pending.as_ref()).is_some_and(|p| points(&p.item).is_some())
    }

    /// The tiles of the pending stroke (painted now if they changed since last asked for); `None`
    /// when the stroke is not drawn as tiles (`live_layer` holds it).
    pub(crate) fn live_tiles(&self) -> Option<Vec<(Rectangle, image::Handle)>> {
        if !self.tiled() {
            self.tiles.borrow_mut().clear();
            return None;
        }
        if self.tiles_stale.replace(false) {
            self.paint_tiles();
        }
        Some(self.tiles.borrow().tiles.values().cloned().collect())
    }

    fn paint_tiles(&self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let (view, mut cache) = (self.doc.view(), self.tiles.borrow_mut());
        let changed = dirty(cache.last.as_ref(), &pending.item);
        let (now, mut layers) = (
            pending.item.bounds(),
            (self.doc.items.iter())
                .map(|item| (item, false))
                .collect::<Vec<_>>(),
        );
        match pending.index {
            Some(i) => layers[i] = (&pending.item, true),
            None => layers.push((&pending.item, true)),
        }
        let origin = Point::new(view.x.floor(), view.y.floor());
        let span = |from: f32, size: f32, at: f32| {
            let (lo, hi) = (
                ((from - at) / TILE).floor(),
                ((from + size - at) / TILE).floor(),
            );
            (lo.max(0.0) as i32)..=(hi.max(0.0) as i32)
        };
        let (cols, rows) = (
            span(changed.x, changed.width, origin.x),
            span(changed.y, changed.height, origin.y),
        );
        for j in rows {
            for i in cols.clone() {
                let tile = Rectangle::new(
                    Point::new(origin.x + i as f32 * TILE, origin.y + j as f32 * TILE),
                    Size::new(TILE, TILE),
                );
                let inside = Rectangle::new(
                    tile.position(),
                    Size::new(
                        TILE.min(view.x + view.width - tile.x),
                        TILE.min(view.y + view.height - tile.y),
                    ),
                );
                if inside.width < 1.0 || inside.height < 1.0 {
                    continue;
                }
                if !meets(tile, now) || !meets(tile, changed) {
                    if !meets(tile, now) {
                        cache.tiles.remove(&(i, j));
                    }
                    continue;
                }
                let r = int_rect(inside, view);
                let pm = self.doc.paint_bare(&layers, r);
                match pm.pixels().iter().any(|p| p.alpha() != 0) {
                    true => {
                        let at = Rectangle::new(
                            Point::new(r.x() as f32, r.y() as f32),
                            Size::new(r.width() as f32, r.height() as f32),
                        );
                        cache.tiles.insert((i, j), (at, straight(pm)));
                    }
                    false => {
                        cache.tiles.remove(&(i, j));
                    }
                }
            }
        }
        cache.last = Some(pending.item.clone());
    }
}
