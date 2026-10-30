//! The document: the image, its annotations (objects until export, never pixels),
//! the crop, the selection and the undo history (a command stack). `paint` flattens
//! any region of it with tiny-skia, the same rasteriser iced uses.
use iced::{Point, Rectangle, Size, Vector};
use tiny_skia::{
    FillRule, IntRect, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform,
};

use super::board::{self, Board};
use super::legend::{self, Legend};
use super::{
    arrow, blur, callout, highlighter, label, magnifier, pen, pointer, shape, spotlight, step, text,
};
use crate::actions::Crop;

/// The captured image: RGBA, straight alpha, `width * height * 4` bytes.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What an annotation is. Points are in image pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect {
        a: Point,
        b: Point,
    },
    Ellipse {
        a: Point,
        b: Point,
    },
    /// Quadratic curve a → b pulled toward `bend` (straight when `bend` is the
    /// midpoint). `head: false` = a plain line. `text` (if any) is a label on
    /// its tail `a`.
    Arrow {
        a: Point,
        b: Point,
        bend: Point,
        head: bool,
        text: String,
    },
    Pen(Vec<Point>),
    Highlighter(Vec<Point>),
    /// `at` = top-left, `size` = font px, `style` = font, weight, backdrop.
    Text {
        at: Point,
        text: String,
        size: f32,
        style: text::Style,
    },
    /// A numbered badge; its number is its place among the steps. Its `note`
    /// shows in the legend.
    Step {
        at: Point,
        radius: f32,
        note: String,
    },
    /// Text on a rounded fill of the colour; `at` = top-left, `size` = font px.
    Label {
        at: Point,
        text: String,
        size: f32,
    },
    /// A ring around `at` with a short arrow into it.
    Pointer {
        at: Point,
        radius: f32,
    },
    /// A blur brush stroke, `size` px tall. `seed`: OS-random per item, never
    /// shown; drives the noise that makes the blur irreversible (see `blur`).
    Blur {
        points: Vec<Point>,
        size: f32,
        seed: u64,
    },
    /// A solid box of the colour (passwords, keys: nothing left to recover).
    Redact {
        a: Point,
        b: Point,
    },
    /// Everything outside the union of all spotlights is dimmed.
    Spotlight {
        a: Point,
        b: Point,
    },
    /// Bubble centred on `at` with its text, tail pointing at `tip`.
    Callout {
        tip: Point,
        at: Point,
        text: String,
        size: f32,
    },
    /// Round lens over `at`: what is under it, zoomed.
    Magnifier {
        at: Point,
        radius: f32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub shape: Shape,
    /// RGBA. Annotations keep their colour in both themes.
    pub colour: [u8; 4],
    /// Stroke width, image px.
    pub width: f32,
}

impl Shape {
    /// Maps every point with `f` and every size (font, radius) by `k`.
    pub fn transform(&mut self, f: impl Fn(Point) -> Point, k: f32) {
        match self {
            Shape::Rect { a, b }
            | Shape::Ellipse { a, b }
            | Shape::Redact { a, b }
            | Shape::Spotlight { a, b } => (*a, *b) = (f(*a), f(*b)),
            Shape::Blur { points, size, .. } => {
                points.iter_mut().for_each(|p| *p = f(*p));
                *size = (*size * k).max(4.0);
            }
            Shape::Arrow { a, b, bend, .. } => (*a, *b, *bend) = (f(*a), f(*b), f(*bend)),
            Shape::Pen(points) | Shape::Highlighter(points) => {
                points.iter_mut().for_each(|p| *p = f(*p))
            }
            Shape::Text { at, size, .. } | Shape::Label { at, size, .. } => {
                (*at, *size) = (f(*at), (*size * k).max(6.0))
            }
            Shape::Step { at, radius, .. } | Shape::Pointer { at, radius } => {
                (*at, *radius) = (f(*at), (*radius * k).max(4.0))
            }
            Shape::Callout { tip, at, size, .. } => {
                (*tip, *at, *size) = (f(*tip), f(*at), (*size * k).max(6.0))
            }
            Shape::Magnifier { at, radius } => (*at, *radius) = (f(*at), (*radius * k).max(8.0)),
        }
    }
}

impl Item {
    /// The geometric box (selection frame, resize handles): no stroke padding.
    pub fn frame(&self) -> Rectangle {
        match &self.shape {
            Shape::Rect { a, b }
            | Shape::Ellipse { a, b }
            | Shape::Redact { a, b }
            | Shape::Spotlight { a, b } => span(&[*a, *b]),
            Shape::Blur { points, size, .. } => blur::frame(points, *size),
            Shape::Arrow {
                a, b, bend, text, ..
            } => {
                let line = span(&[*a, *b, arrow::middle(*a, *b, *bend)]);
                if text.is_empty() {
                    line
                } else {
                    let size = label::arrow_size(self.width);
                    line.union(&label::bubble(label::centred(*a, text, size), text, size))
                }
            }
            Shape::Pen(points) | Shape::Highlighter(points) => span(points),
            Shape::Text {
                at,
                text,
                size,
                style,
            } => Rectangle::new(*at, text::measure_styled(text, *size, *style)),
            Shape::Label { at, text, size } => label::bubble(*at, text, *size),
            Shape::Pointer { at, radius } => pointer::frame(*at, *radius),
            Shape::Step { at, radius, .. } | Shape::Magnifier { at, radius } => Rectangle::new(
                *at - iced::Vector::new(*radius, *radius),
                Size::new(2.0 * radius, 2.0 * radius),
            ),
            Shape::Callout {
                tip,
                at,
                text,
                size,
            } => {
                let bubble = callout::bubble(*at, text, *size);
                bubble.union(&span(&[*tip, *tip]))
            }
        }
    }

    /// Everything it paints (stroke, halo, arrow head): the region to repaint.
    pub fn bounds(&self) -> Rectangle {
        let pad = match &self.shape {
            Shape::Arrow { .. } | Shape::Pointer { .. } => {
                arrow::head_length(self.width) + self.width + 4.0
            }
            Shape::Highlighter(_) => self.width / 2.0 + 2.0,
            Shape::Redact { .. } | Shape::Spotlight { .. } => 1.0,
            Shape::Blur { size, .. } => blur::reach(*size),
            Shape::Text { size, .. } | Shape::Callout { size, .. } | Shape::Label { size, .. } => {
                size / 6.0 + 3.0
            }
            _ => self.width + 3.0,
        };
        self.frame().expand(pad)
    }
}

/// Bounding box of points (zero-size for one point).
pub fn span(points: &[Point]) -> Rectangle {
    let Some(&first) = points.first() else {
        return Rectangle::default();
    };
    let (min, max) = points.iter().fold((first, first), |(lo, hi), p| {
        (
            Point::new(lo.x.min(p.x), lo.y.min(p.y)),
            Point::new(hi.x.max(p.x), hi.y.max(p.y)),
        )
    });
    Rectangle::new(min, Size::new(max.x - min.x, max.y - min.y))
}

/// One undoable change. Undo applies the inverse.
#[derive(Clone, Debug)]
pub enum Edit {
    Insert(usize, Item),
    Delete(usize, Item),
    /// Index, before, after.
    Change(usize, Item, Item),
    /// Crop before, after (`None` = the whole image).
    Crop(Option<Rectangle>, Option<Rectangle>),
    /// The board (captures, layout) and everything on it, before and after.
    Board(Box<Sheet>, Box<Sheet>),
    /// Snip's live area (the crop) before and after, and how far every item
    /// moved with it (zero for a resize: the drawing stays on its pixels).
    Area(Rectangle, Rectangle, Vector),
}

/// A board's state: its layout and the items and crop that go with it.
#[derive(Clone, Debug)]
pub struct Sheet {
    pub board: Board,
    pub crop: Option<Rectangle>,
    pub items: Vec<Item>,
}

impl Edit {
    fn inverse(&self) -> Edit {
        match self.clone() {
            Edit::Insert(i, item) => Edit::Delete(i, item),
            Edit::Delete(i, item) => Edit::Insert(i, item),
            Edit::Change(i, before, after) => Edit::Change(i, after, before),
            Edit::Crop(before, after) => Edit::Crop(after, before),
            Edit::Board(before, after) => Edit::Board(after, before),
            Edit::Area(before, after, moved) => Edit::Area(after, before, -moved),
        }
    }

    fn apply(&self, doc: &mut Document) {
        match self {
            Edit::Insert(i, item) => {
                doc.items.insert(*i, item.clone());
                doc.selected = Some(*i);
            }
            Edit::Delete(i, _) => {
                doc.items.remove(*i);
                doc.selected = None;
            }
            Edit::Change(i, _, after) => {
                doc.items[*i] = after.clone();
                doc.selected = Some(*i);
            }
            Edit::Crop(_, after) => doc.crop = *after,
            Edit::Board(_, after) => {
                doc.board = after.board;
                doc.crop = after.crop;
                doc.items = after.items.clone();
                doc.selected = None;
                doc.compose();
            }
            Edit::Area(_, after, moved) => {
                doc.crop = Some(*after);
                doc.shift(*moved);
            }
        }
    }
}

pub struct Document {
    /// The capture, or the board of captures once there are more.
    pub image: Image,
    /// Bottom to top.
    pub items: Vec<Item>,
    pub selected: Option<usize>,
    /// The part of the image that is kept (non-destructive, undoable).
    pub crop: Option<Rectangle>,
    /// The steps' notes show in a legend.
    pub legend: bool,
    pub board: Board,
    /// The captures once a second one is added (`board.count` of them are on the
    /// board; the rest wait for a redo). Empty: `image` is the only capture.
    sources: Vec<Image>,
    /// Where each capture is on the board.
    tiles: Vec<Rectangle>,
    done: Vec<Edit>,
    undone: Vec<Edit>,
}

impl Document {
    pub fn new(image: Image) -> Self {
        let whole = Rectangle::new(
            Point::ORIGIN,
            Size::new(image.width as f32, image.height as f32),
        );
        Document {
            image,
            items: Vec::new(),
            selected: None,
            crop: None,
            legend: true,
            board: Board::default(),
            sources: Vec::new(),
            tiles: vec![whole],
            done: Vec::new(),
            undone: Vec::new(),
        }
    }

    /// Where each capture is on the board, image px.
    #[cfg(test)]
    pub fn tiles(&self) -> &[Rectangle] {
        &self.tiles
    }

    /// Adds a capture to the board (one undoable edit).
    pub fn append(&mut self, image: Image) {
        if self.sources.is_empty() {
            let first = std::mem::replace(
                &mut self.image,
                Image {
                    width: 0,
                    height: 0,
                    rgba: Vec::new(),
                },
            );
            self.sources.push(first);
        }
        self.sources.truncate(self.board.count); // drops captures undone earlier
        self.sources.push(image);
        let board = Board {
            count: self.board.count + 1,
            ..self.board
        };
        self.rearrange(board);
    }

    /// Lays the board out as `board` says; items move with their capture, the
    /// crop is reset (one undoable edit).
    pub fn rearrange(&mut self, board: Board) {
        let count = board.count.clamp(1, self.sources.len().max(1));
        let board = Board { count, ..board };
        if board == self.board || self.sources.is_empty() {
            return;
        }
        let sizes: Vec<_> = (self.sources.iter().take(count))
            .map(|s| (s.width, s.height))
            .collect();
        let (to, _) = board::place(&sizes, board.layout);
        let before = Sheet {
            board: self.board,
            crop: self.crop,
            items: self.items.clone(),
        };
        let after = Sheet {
            board,
            crop: None,
            items: board::remap(&self.items, &self.tiles, &to),
        };
        self.exec(Edit::Board(Box::new(before), Box::new(after)));
    }

    /// Rebuilds the board image from the captures.
    fn compose(&mut self) {
        if self.sources.is_empty() {
            return;
        }
        let (image, tiles) = board::compose(&self.sources[..self.board.count], &self.board);
        self.image = image;
        self.tiles = tiles;
    }

    /// The legend as it shows now (`None`: off, or no step has a note).
    pub fn legend_layout(&self) -> Option<Legend> {
        self.legend
            .then(|| {
                // On a board, beside or under a capture before the corners.
                let tiles = if self.board.count > 1 {
                    &self.tiles[..]
                } else {
                    &[]
                };
                legend::layout(self.view(), &self.items, tiles)
            })
            .flatten()
    }

    /// Paints `region` of the view with `layers`, and the legend where it overlaps.
    pub fn paint(
        &self,
        layers: &[(&Item, bool)],
        region: IntRect,
        legend: Option<&Legend>,
    ) -> Pixmap {
        let mut pm = paint(&self.image, layers, region);
        if let Some(legend) = legend.filter(|l| l.band == 0.0) {
            let mut layer = Layer {
                pm,
                origin: Point::new(region.x() as f32, region.y() as f32),
            };
            legend::paint(&mut layer, legend, self.view());
            pm = layer.pm;
        }
        pm
    }

    /// The whole view with `layers`, the legend in its corner or in a band under it.
    pub fn flatten(&self, layers: &[(&Item, bool)], legend: Option<&Legend>) -> Pixmap {
        let view = self.view();
        let pm = self.paint(layers, int_rect(view, view), legend);
        let Some(legend) = legend.filter(|l| l.band > 0.0) else {
            return pm;
        };
        let mut tall =
            Pixmap::new(pm.width(), pm.height() + legend.band as u32).expect("non-empty view");
        tall.draw_pixmap(
            0,
            0,
            pm.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        let mut layer = Layer {
            pm: tall,
            origin: view.position(),
        };
        legend::paint(&mut layer, legend, view);
        layer.pm
    }

    /// Applies `edit` and records it (clears the redo list).
    // ponytail: unlimited history; each entry holds one item (a pen stroke is
    // its points), so memory only matters after thousands of edits.
    pub fn exec(&mut self, edit: Edit) {
        edit.apply(self);
        self.done.push(edit);
        self.undone.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.done.pop() else {
            return false;
        };
        edit.inverse().apply(self);
        self.undone.push(edit);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.undone.pop() else {
            return false;
        };
        edit.apply(self);
        self.done.push(edit);
        true
    }

    /// Moves every item by `by` (image px).
    pub fn shift(&mut self, by: Vector) {
        if by != Vector::ZERO {
            for item in &mut self.items {
                item.shape.transform(|p| p + by, 1.0);
            }
        }
    }

    /// The crop becomes the whole image (its pixels cut out, the items moved
    /// with them): Snip's live area is fixed from here on (a board needs its
    /// captures as they are). Undo starts again from this state.
    pub fn bake(&mut self) {
        let Some(crop) = self.crop.take() else {
            return;
        };
        let whole = Rectangle::new(
            Point::ORIGIN,
            Size::new(self.image.width as f32, self.image.height as f32),
        );
        let r = int_rect(crop, whole);
        let (x, y, w, h) = (r.x() as usize, r.y() as usize, r.width(), r.height());
        let stride = self.image.width as usize * 4;
        let rgba = (y..y + h as usize)
            .flat_map(|row| &self.image.rgba[row * stride + x * 4..][..w as usize * 4])
            .copied()
            .collect();
        self.image = Image {
            width: w,
            height: h,
            rgba,
        };
        self.shift(Vector::new(-(x as f32), -(y as f32)));
        self.tiles = vec![Rectangle::new(Point::ORIGIN, Size::new(w as f32, h as f32))];
        self.done.clear();
        self.undone.clear();
    }

    /// The visible part of the image, image px.
    pub fn view(&self) -> Rectangle {
        self.crop.unwrap_or(Rectangle::new(
            Point::ORIGIN,
            Size::new(self.image.width as f32, self.image.height as f32),
        ))
    }
}

/// The flattened result (crop applied, every annotation drawn): what Copy and
/// Save get. Pure: no window, no state.
pub fn render(doc: &Document) -> Crop {
    let layers: Vec<_> = doc.items.iter().map(|item| (item, true)).collect();
    let pm = doc.flatten(&layers, doc.legend_layout().as_ref());
    let rgba = pm
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Crop {
        width: pm.width(),
        height: pm.height(),
        rgba,
    }
}

/// `r` in whole pixels (rounded outwards), inside `within`, at least 1x1.
pub fn int_rect(r: Rectangle, within: Rectangle) -> IntRect {
    let x0 = r.x.max(within.x).floor();
    let y0 = r.y.max(within.y).floor();
    let x1 = (r.x + r.width).min(within.x + within.width).ceil();
    let y1 = (r.y + r.height).min(within.y + within.height).ceil();
    let (x0, y0) = (
        x0.min(within.x + within.width - 1.0),
        y0.min(within.y + within.height - 1.0),
    );
    IntRect::from_xywh(
        x0 as i32,
        y0 as i32,
        (x1 - x0).max(1.0) as u32,
        (y1 - y0).max(1.0) as u32,
    )
    .expect("non-empty rect")
}

/// Where a tool paints: a pixmap showing the image region that starts at `origin`.
pub struct Layer {
    pub pm: Pixmap,
    pub origin: Point,
}

impl Layer {
    /// Image px → this pixmap's px.
    pub fn t(&self) -> Transform {
        Transform::from_translate(-self.origin.x, -self.origin.y)
    }

    pub fn stroke(&mut self, path: &Path, colour: [u8; 4], width: f32) {
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        let t = self.t();
        self.pm.stroke_path(path, &solid(colour), &stroke, t, None);
    }

    pub fn fill(&mut self, path: &Path, colour: [u8; 4]) {
        let t = self.t();
        self.pm
            .fill_path(path, &solid(colour), FillRule::Winding, t, None);
    }
}

pub fn solid(colour: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint::default();
    let [r, g, b, a] = colour;
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    paint
}

/// The halo that keeps a colour readable on any background: white, or dark
/// for light colours.
pub fn halo(colour: [u8; 4]) -> [u8; 4] {
    if light(colour) {
        [20, 20, 28, 200]
    } else {
        [255, 255, 255, 220]
    }
}

/// Text on a fill of `colour`: dark on light colours, white otherwise.
pub fn on(colour: [u8; 4]) -> [u8; 4] {
    if light(colour) {
        [20, 20, 28, 255]
    } else {
        [255, 255, 255, 255]
    }
}

fn light(c: [u8; 4]) -> bool {
    0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32 > 170.0
}

/// A rectangle path with rounded corners (`radius` clamped to fit).
pub fn rounded(r: Rectangle, radius: f32) -> Option<Path> {
    let k = radius.min(r.width / 2.0).min(r.height / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    let mut p = PathBuilder::new();
    p.move_to(x0 + k, y0);
    p.line_to(x1 - k, y0);
    p.quad_to(x1, y0, x1, y0 + k);
    p.line_to(x1, y1 - k);
    p.quad_to(x1, y1, x1 - k, y1);
    p.line_to(x0 + k, y1);
    p.quad_to(x0, y1, x0, y1 - k);
    p.line_to(x0, y0 + k);
    p.quad_to(x0, y0, x0 + k, y0);
    p.close();
    p.finish()
}

/// Paints `region` (image px, inside the image) of the image with `items` over it.
/// `(item, false)` = hidden (being edited live), but it still counts as a step.
pub fn paint(image: &Image, items: &[(&Item, bool)], region: IntRect) -> Pixmap {
    let (w, h) = (region.width(), region.height());
    let mut pm = Pixmap::new(w, h).expect("non-empty region");
    let stride = image.width as usize * 4;
    let (x, y) = (region.x() as usize, region.y() as usize);
    for (row, dst) in pm.data_mut().chunks_exact_mut(w as usize * 4).enumerate() {
        let start = (y + row) * stride + x * 4;
        dst.copy_from_slice(&image.rgba[start..start + dst.len()]);
        // Screen captures are opaque; premultiply anything that is not.
        for px in dst
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .filter(|px| px[3] != 255)
        {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
    }
    let mut layer = Layer {
        pm,
        origin: Point::new(region.x() as f32, region.y() as f32),
    };

    let holes: Vec<Rectangle> = items
        .iter()
        .filter(|(_, visible)| *visible)
        .filter_map(|(item, _)| match item.shape {
            Shape::Spotlight { a, b } => Some(span(&[a, b])),
            _ => None,
        })
        .collect();
    if !holes.is_empty() {
        spotlight::paint(&mut layer, &holes);
    }

    let mut steps = 0;
    for (item, visible) in items {
        if matches!(item.shape, Shape::Step { .. }) {
            steps += 1;
        }
        if !visible {
            continue;
        }
        let (c, w) = (item.colour, item.width);
        match &item.shape {
            Shape::Rect { a, b } => shape::paint_rect(&mut layer, *a, *b, c, w),
            Shape::Ellipse { a, b } => shape::paint_ellipse(&mut layer, *a, *b, c, w),
            Shape::Arrow {
                a,
                b,
                bend,
                head,
                text,
            } => {
                arrow::paint(&mut layer, *a, *b, *bend, *head, c, w);
                if !text.is_empty() {
                    let size = label::arrow_size(w);
                    label::paint(&mut layer, label::centred(*a, text, size), text, size, c);
                }
            }
            Shape::Pen(points) => pen::paint(&mut layer, points, c, w),
            Shape::Highlighter(points) => highlighter::paint(&mut layer, points, c, w),
            Shape::Text {
                at,
                text,
                size,
                style,
            } => text::paint_styled(&mut layer, *at, text, *size, c, *style),
            Shape::Step { at, radius, .. } => step::paint(&mut layer, *at, *radius, steps, c),
            Shape::Label { at, text, size } => label::paint(&mut layer, *at, text, *size, c),
            Shape::Pointer { at, radius } => pointer::paint(&mut layer, *at, *radius, c, w),
            Shape::Blur { points, size, seed } => blur::paint(&mut layer, points, *size, *seed),
            Shape::Redact { a, b } => {
                if let Some(path) = rounded(span(&[*a, *b]), 0.0) {
                    layer.fill(&path, [c[0], c[1], c[2], 255]);
                }
            }
            Shape::Spotlight { .. } => {}
            Shape::Callout {
                tip,
                at,
                text,
                size,
            } => callout::paint(&mut layer, *tip, *at, text, *size, c),
            Shape::Magnifier { at, radius } => magnifier::paint(&mut layer, *at, *radius, c, w),
        }
    }
    layer.pm
}
