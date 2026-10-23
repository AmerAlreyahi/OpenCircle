//! The document: the image, its annotations (objects until export, never pixels),
//! the crop, the selection and the undo history (a command stack). `paint` flattens
//! any region of it with tiny-skia, the same rasteriser iced uses.
use iced::{Point, Rectangle, Size};
use tiny_skia::{
    FillRule, IntRect, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform,
};

use super::{arrow, blur, callout, highlighter, magnifier, pen, shape, spotlight, step, text};
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
    /// midpoint). `head: false` = a plain line.
    Arrow {
        a: Point,
        b: Point,
        bend: Point,
        head: bool,
    },
    Pen(Vec<Point>),
    Highlighter(Vec<Point>),
    /// `at` = top-left, `size` = font px.
    Text {
        at: Point,
        text: String,
        size: f32,
    },
    /// A numbered badge; its number is its place among the steps.
    Step {
        at: Point,
        radius: f32,
    },
    Blur {
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
            | Shape::Blur { a, b }
            | Shape::Spotlight { a, b } => (*a, *b) = (f(*a), f(*b)),
            Shape::Arrow { a, b, bend, .. } => (*a, *b, *bend) = (f(*a), f(*b), f(*bend)),
            Shape::Pen(points) | Shape::Highlighter(points) => {
                points.iter_mut().for_each(|p| *p = f(*p))
            }
            Shape::Text { at, size, .. } => (*at, *size) = (f(*at), (*size * k).max(6.0)),
            Shape::Step { at, radius } => (*at, *radius) = (f(*at), (*radius * k).max(4.0)),
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
            | Shape::Blur { a, b }
            | Shape::Spotlight { a, b } => span(&[*a, *b]),
            Shape::Arrow { a, b, bend, .. } => span(&[*a, *b, arrow::middle(*a, *b, *bend)]),
            Shape::Pen(points) | Shape::Highlighter(points) => span(points),
            Shape::Text { at, text, size } => Rectangle::new(*at, text::measure(text, *size)),
            Shape::Step { at, radius } | Shape::Magnifier { at, radius } => Rectangle::new(
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
            Shape::Arrow { .. } => arrow::head_length(self.width) + self.width + 4.0,
            Shape::Highlighter(_) => self.width / 2.0 + 2.0,
            Shape::Blur { .. } | Shape::Spotlight { .. } => 1.0,
            Shape::Text { size, .. } | Shape::Callout { size, .. } => size / 6.0 + 3.0,
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
}

impl Edit {
    fn inverse(&self) -> Edit {
        match self.clone() {
            Edit::Insert(i, item) => Edit::Delete(i, item),
            Edit::Delete(i, item) => Edit::Insert(i, item),
            Edit::Change(i, before, after) => Edit::Change(i, after, before),
            Edit::Crop(before, after) => Edit::Crop(after, before),
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
        }
    }
}

pub struct Document {
    pub image: Image,
    /// Bottom to top.
    pub items: Vec<Item>,
    pub selected: Option<usize>,
    /// The part of the image that is kept (non-destructive, undoable).
    pub crop: Option<Rectangle>,
    done: Vec<Edit>,
    undone: Vec<Edit>,
}

impl Document {
    pub fn new(image: Image) -> Self {
        Document {
            image,
            items: Vec::new(),
            selected: None,
            crop: None,
            done: Vec::new(),
            undone: Vec::new(),
        }
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
    let region = int_rect(doc.view(), doc.view());
    let pm = paint(&doc.image, &layers, region);
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
            Shape::Arrow { a, b, bend, head } => {
                arrow::paint(&mut layer, *a, *b, *bend, *head, c, w)
            }
            Shape::Pen(points) => pen::paint(&mut layer, points, c, w),
            Shape::Highlighter(points) => highlighter::paint(&mut layer, points, c, w),
            Shape::Text { at, text, size } => text::paint(&mut layer, *at, text, *size, c, true),
            Shape::Step { at, radius } => step::paint(&mut layer, *at, *radius, steps, c),
            Shape::Blur { a, b } => blur::paint(&mut layer, span(&[*a, *b])),
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
