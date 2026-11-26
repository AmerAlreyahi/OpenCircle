//! The document: the image, its annotations (objects until export, never pixels),
//! the crop, the selection and the undo history (a command stack). `paint` flattens
//! any region of it with tiny-skia, the same rasteriser iced uses.
use iced::{Point, Rectangle, Size, Vector};
use tiny_skia::{
    FillRule, IntRect, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform,
};

use super::legend::{self, Legend};
use super::{
    arrow, blur, callout, highlighter, label, magnifier, pen, pointer, shape, spotlight, step, text,
};
use crate::shared::picture::Crop;

/// The captured image: RGBA, straight alpha, `width * height * 4` bytes.
#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What an annotation is. Points are in image pixels.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Shape {
    Rect {
        #[serde(with = "pt")]
        a: Point,
        #[serde(with = "pt")]
        b: Point,
    },
    Ellipse {
        #[serde(with = "pt")]
        a: Point,
        #[serde(with = "pt")]
        b: Point,
    },
    /// Quadratic curve a → b pulled toward `bend` (straight when `bend` is the
    /// midpoint). `head: false` = a plain line. `text` (if any) is a label on
    /// its tail `a`.
    Arrow {
        #[serde(with = "pt")]
        a: Point,
        #[serde(with = "pt")]
        b: Point,
        #[serde(with = "pt")]
        bend: Point,
        head: bool,
        text: String,
    },
    Pen(#[serde(with = "pts")] Vec<Point>),
    Highlighter(#[serde(with = "pts")] Vec<Point>),
    /// `at` = top-left, `size` = font px, `style` = font, weight, backdrop.
    Text {
        #[serde(with = "pt")]
        at: Point,
        text: String,
        size: f32,
        style: text::Style,
        /// Set soft wrap at this width (px): where the picture cannot grow, a text breaks before
        /// the area's edge when drawn (the text itself keeps no break the user did not type).
        #[serde(default)]
        wrap: Option<f32>,
    },
    /// A numbered badge; its number is its place among the steps. Its `note`
    /// shows in the legend.
    Step {
        #[serde(with = "pt")]
        at: Point,
        radius: f32,
        note: String,
    },
    /// Text on a rounded fill of the colour; `at` = top-left, `size` = font px.
    Label {
        #[serde(with = "pt")]
        at: Point,
        text: String,
        size: f32,
    },
    /// A ring around `at` with a short arrow into it.
    Pointer {
        #[serde(with = "pt")]
        at: Point,
        radius: f32,
    },
    /// A Hide brush stroke `size` px tall; `cell` is its texture scale (image
    /// px). Its secret is the document's (`Document::seed`), one per picture.
    Blur {
        #[serde(with = "pts")]
        points: Vec<Point>,
        size: f32,
        cell: f32,
        #[serde(default = "blur::Style::legacy_default")]
        style: blur::Style,
        /// Brush (the points are its stroke), or a rectangle / ellipse between
        /// the first and last point.
        #[serde(default)]
        kind: blur::Kind,
        /// Over 0: only a ring this wide along the shape's edge.
        #[serde(default)]
        outline: f32,
        /// Progressive: where the blur is strongest, then where it is gone.
        #[serde(with = "pts", default)]
        ramp: Vec<Point>,
    },
    /// A solid box of the colour (passwords, keys: nothing left to recover).
    Redact {
        #[serde(with = "pt")]
        a: Point,
        #[serde(with = "pt")]
        b: Point,
        /// An ellipse in the box (a hidden circle stays a circle).
        #[serde(default)]
        oval: bool,
    },
    /// Everything outside the union of all spotlights is dimmed.
    Spotlight {
        #[serde(with = "pt")]
        a: Point,
        #[serde(with = "pt")]
        b: Point,
    },
    /// Bubble centred on `at` with its text, tail pointing at `tip`.
    Callout {
        #[serde(with = "pt")]
        tip: Point,
        #[serde(with = "pt")]
        at: Point,
        text: String,
        size: f32,
    },
    /// Lens over `at`: what is under it, zoomed.
    Magnifier {
        #[serde(with = "pt")]
        at: Point,
        radius: f32,
        #[serde(default = "magnifier::default_zoom")]
        zoom: f32,
        #[serde(default)]
        kind: magnifier::Kind,
    },
    /// A picture pasted in (Studio): a movable object between corners `a`
    /// and `b`; its opacity is the item colour's alpha; `lock` keeps its
    /// ratio when resized. "Merge" bakes it into the picture.
    Picture {
        #[serde(with = "pt")]
        a: Point,
        #[serde(with = "pt")]
        b: Point,
        pic: Pic,
        lock: bool,
    },
}

/// The pixels of a pasted picture, premultiplied once (shared by clones, so
/// comparing two is a pointer check).
#[derive(Clone)]
pub struct Pic(pub std::sync::Arc<(u32, u32, Vec<u8>)>);

impl Pic {
    pub fn new(width: u32, height: u32, rgba: &[u8]) -> Self {
        let mut data = rgba.to_vec();
        for px in data.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
        Pic(std::sync::Arc::new((width, height, data)))
    }
}

/// Its size only (not megabytes of pixels).
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

impl PartialEq for Pic {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl std::fmt::Debug for Pic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pic({}x{})", self.0.0, self.0.1)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub shape: Shape,
    /// RGBA. Annotations keep their colour in both themes.
    pub colour: [u8; 4],
    /// Stroke width, image px.
    pub width: f32,
    /// A white / dark ring round it so it reads on any background (shapes,
    /// arrows, lines, callouts; off by default, owner 2026-10-31).
    pub outline: bool,
    /// A rectangle or ellipse is filled with a see-through tint of its colour.
    pub fill: bool,
    /// Hidden in the Studio's Objects list: not drawn, exported or copied.
    #[serde(default)]
    pub hidden: bool,
}

impl Shape {
    /// Maps every point with `f` and every size (font, radius) by `k`.
    pub fn transform(&mut self, f: impl Fn(Point) -> Point, k: f32) {
        match self {
            Shape::Rect { a, b }
            | Shape::Ellipse { a, b }
            | Shape::Redact { a, b, .. }
            | Shape::Spotlight { a, b }
            | Shape::Picture { a, b, .. } => (*a, *b) = (f(*a), f(*b)),
            Shape::Blur {
                points,
                size,
                cell,
                outline,
                ramp,
                ..
            } => {
                points.iter_mut().for_each(|p| *p = f(*p));
                ramp.iter_mut().for_each(|p| *p = f(*p));
                *size = (*size * k).max(4.0);
                *cell = (*cell * k).max(2.0);
                *outline *= k;
            }
            Shape::Arrow { a, b, bend, .. } => (*a, *b, *bend) = (f(*a), f(*b), f(*bend)),
            Shape::Pen(points) | Shape::Highlighter(points) => {
                points.iter_mut().for_each(|p| *p = f(*p))
            }
            Shape::Text { at, size, wrap, .. } => {
                (*at, *size) = (f(*at), (*size * k).max(6.0));
                *wrap = wrap.map(|w| w * k);
            }
            Shape::Label { at, size, .. } => (*at, *size) = (f(*at), (*size * k).max(6.0)),
            Shape::Step { at, radius, .. } | Shape::Pointer { at, radius } => {
                (*at, *radius) = (f(*at), (*radius * k).max(4.0))
            }
            Shape::Callout { tip, at, size, .. } => {
                (*tip, *at, *size) = (f(*tip), f(*at), (*size * k).max(6.0))
            }
            Shape::Magnifier { at, radius, .. } => {
                (*at, *radius) = (f(*at), (*radius * k).max(8.0))
            }
        }
    }
}

impl Item {
    /// The geometric box (selection frame, resize handles): no stroke padding.
    pub fn frame(&self) -> Rectangle {
        match &self.shape {
            Shape::Rect { a, b }
            | Shape::Ellipse { a, b }
            | Shape::Redact { a, b, .. }
            | Shape::Spotlight { a, b }
            | Shape::Picture { a, b, .. } => span(&[*a, *b]),
            Shape::Blur {
                points,
                size,
                kind,
                outline,
                ramp,
                ..
            } => blur::frame(&blur::Footprint {
                points,
                size: *size,
                kind: *kind,
                outline: *outline,
                ramp,
                source: None,
                exclude: &[],
            }),
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
                wrap,
            } => Rectangle::new(*at, text::measure_styled(text, *size, *style, *wrap)),
            Shape::Label { at, text, size } => label::bubble(*at, text, *size),
            Shape::Pointer { at, radius } => pointer::frame(*at, *radius),
            Shape::Step { at, radius, .. } | Shape::Magnifier { at, radius, .. } => Rectangle::new(
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
            Shape::Blur { cell, .. } => blur::reach(*cell),
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
    /// Snip's live area (the crop) before and after, and how far every item
    /// moved with it (zero for a resize: the drawing stays on its pixels).
    Area(Rectangle, Rectangle, Vector),
    /// Every item before and after (an eraser stroke: one undo brings back all it erased).
    Items(Vec<Item>, Vec<Item>),
    /// The picture's own pixels changed (Studio's Edit text): the box that
    /// changed, before and after.
    Pixels(Box<Patch>),
    /// Several edits that undo together (applied in order).
    Seq(Vec<Edit>),
    /// The canvas grew (Studio, "Grow the canvas past the edge"): the picture
    /// as it was (the only copy kept), the new size, how far every drawing
    /// moved (zero unless it grew to the left or the top) and whether this is
    /// the undo (back to the old picture) rather than the growth.
    // Undo happens only after every later edit was undone, so the grown
    // picture is always the old one placed in a see-through canvas: it is
    // rebuilt, never stored.
    Canvas {
        old: Box<Image>,
        size: (u32, u32),
        by: Vector,
        undo: bool,
    },
}

/// A changed box of the image: `x, y, w, h` (image px), RGBA rows before and after.
#[derive(Clone, Debug)]
pub struct Patch {
    pub at: [usize; 4],
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

impl Patch {
    /// `after` written into `image`.
    fn write(&self, image: &mut Image, bytes: &[u8]) {
        let [x, y, w, h] = self.at;
        let stride = image.width as usize * 4;
        for row in 0..h {
            let to = (y + row) * stride + x * 4;
            image.rgba[to..to + w * 4].copy_from_slice(&bytes[row * w * 4..(row + 1) * w * 4]);
        }
    }
}

impl Edit {
    fn inverse(&self) -> Edit {
        match self.clone() {
            Edit::Insert(i, item) => Edit::Delete(i, item),
            Edit::Delete(i, item) => Edit::Insert(i, item),
            Edit::Change(i, before, after) => Edit::Change(i, after, before),
            Edit::Crop(before, after) => Edit::Crop(after, before),
            Edit::Area(before, after, moved) => Edit::Area(after, before, -moved),
            Edit::Items(before, after) => Edit::Items(after, before),
            Edit::Pixels(p) => Edit::Pixels(Box::new(Patch {
                at: p.at,
                before: p.after,
                after: p.before,
            })),
            Edit::Seq(all) => Edit::Seq(all.iter().rev().map(Edit::inverse).collect()),
            Edit::Canvas {
                old,
                size,
                by,
                undo,
            } => Edit::Canvas {
                old,
                size,
                by: -by,
                undo: !undo,
            },
        }
    }

    fn apply(&self, doc: &mut Document) {
        doc.extra.clear();
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
            Edit::Area(_, after, moved) => {
                doc.crop = Some(*after);
                doc.shift(*moved);
            }
            Edit::Items(_, after) => {
                doc.items = after.clone();
                doc.selected = None;
            }
            Edit::Pixels(p) => p.write(&mut doc.image, &p.after),
            Edit::Seq(all) => all.iter().for_each(|e| e.apply(doc)),
            Edit::Canvas {
                old,
                size,
                by,
                undo,
            } => {
                doc.image = match undo {
                    true => (**old).clone(),
                    false => placed(old, *size, *by),
                };
                doc.shift(*by);
            }
        }
    }
}

#[derive(Clone)]
pub struct Document {
    /// The capture.
    pub image: Image,
    /// Bottom to top.
    pub items: Vec<Item>,
    pub selected: Option<usize>,
    /// The other selected items (Studio's multi-selection; `selected` is the
    /// one last picked).
    pub extra: Vec<usize>,
    /// The part of the image that is kept (non-destructive, undoable).
    pub crop: Option<Rectangle>,
    /// The steps' notes show in a legend.
    pub legend: bool,
    /// The history, each edit with its number (`state`).
    done: Vec<(u64, Edit)>,
    undone: Vec<(u64, Edit)>,
    /// Edits numbered so far.
    made: u64,
    /// The picture's one secret (Hide's texture, never shown): OS-random per
    /// document, so every Hide in it matches and no two pictures share a texture.
    pub seed: u64,
}

impl Document {
    pub fn new(image: Image) -> Self {
        Document {
            image,
            items: Vec::new(),
            selected: None,
            extra: Vec::new(),
            crop: None,
            legend: true,
            done: Vec::new(),
            undone: Vec::new(),
            made: 0,
            seed: blur::seed(),
        }
    }

    /// The selected items' indices, ascending (only those that exist).
    pub fn selection(&self) -> Vec<usize> {
        let mut all: Vec<usize> = (self.selected.into_iter())
            .chain(self.extra.iter().copied())
            .collect();
        all.retain(|&i| i < self.items.len());
        all.sort_unstable();
        all.dedup();
        all
    }

    /// Selects exactly `all` (the last one is the primary).
    pub fn select(&mut self, all: &[usize]) {
        self.selected = all.last().copied();
        self.extra = all.iter().rev().skip(1).copied().collect();
    }

    pub fn select_one(&mut self, i: Option<usize>) {
        self.selected = i;
        self.extra.clear();
    }

    pub fn deselect(&mut self) {
        self.select_one(None);
    }

    /// The legend as it shows now (`None`: off, or no step has a note).
    pub fn legend_layout(&self) -> Option<Legend> {
        self.legend
            .then(|| legend::layout(self.view(), &self.items))
            .flatten()
    }

    /// Paints `region` of the view with `layers`, and the legend where it overlaps.
    pub fn paint(
        &self,
        layers: &[(&Item, bool)],
        region: IntRect,
        legend: Option<&Legend>,
    ) -> Pixmap {
        let ex = excludes(layers.iter().filter(|(_, v)| *v).map(|(i, _)| *i));
        self.paint_with_excludes(layers, &ex, region, legend)
    }

    /// `paint`, excluding every Hide / Redact in `excludes` from Soft's exterior rings.
    pub fn paint_with_excludes(
        &self,
        layers: &[(&Item, bool)],
        excludes: &[blur::Exclude<'_>],
        region: IntRect,
        legend: Option<&Legend>,
    ) -> Pixmap {
        let mut pm = paint_with_excludes(&self.image, layers, excludes, region, self.seed);
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

    /// `paint` on a see-through pixmap: only what `layers` paint (no legend: the base holds it).
    pub fn paint_bare(&self, layers: &[(&Item, bool)], region: IntRect) -> Pixmap {
        let ex = excludes(layers.iter().filter(|(_, v)| *v).map(|(i, _)| *i));
        paint_on(&self.image, layers, &ex, region, self.seed, true)
    }

    /// The whole view with `layers`, the legend in its corner or in a band under it.
    pub fn flatten(&self, layers: &[(&Item, bool)], legend: Option<&Legend>) -> Pixmap {
        let ex = excludes(layers.iter().filter(|(_, v)| *v).map(|(i, _)| *i));
        self.flatten_with_excludes(layers, &ex, legend)
    }

    /// `flatten`, with an explicit document-wide `excludes` list (e.g. including a Hide
    /// currently being dragged so base neighbours still exclude it).
    pub fn flatten_with_excludes(
        &self,
        layers: &[(&Item, bool)],
        excludes: &[blur::Exclude<'_>],
        legend: Option<&Legend>,
    ) -> Pixmap {
        let view = self.view();
        let pm = self.paint_with_excludes(layers, excludes, int_rect(view, view), legend);
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
        self.made += 1;
        self.done.push((self.made, edit));
        self.undone.clear();
    }

    /// Makes `rows` (whole image rows from row `top` down) the picture there,
    /// as one undoable edit of the box that differs. `false`: nothing differs.
    pub fn set_rows(&mut self, top: usize, rows: &[u8]) -> bool {
        match self.rows_patch(top, rows) {
            Some(patch) => {
                self.exec(Edit::Pixels(Box::new(patch)));
                true
            }
            None => false,
        }
    }

    /// The patch that makes `rows` the picture from row `top` (`None`: no difference).
    pub fn rows_patch(&self, top: usize, rows: &[u8]) -> Option<Patch> {
        let w = self.image.width as usize;
        let from = top * w * 4;
        let old = self.image.rgba.get(from..from + rows.len())?;
        let (mut x0, mut y0, mut x1, mut y1) = (w, usize::MAX, 0, 0);
        for (i, (a, b)) in old.chunks(4).zip(rows.chunks(4)).enumerate() {
            if a != b {
                let (x, y) = (i % w, top + i / w);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
        if x1 <= x0 {
            return None;
        }
        let at = [x0, y0, x1 - x0, y1 - y0];
        let cut = |src: &[u8], first: usize| -> Vec<u8> {
            (y0..y1)
                .flat_map(|y| {
                    let r = (y - first) * w;
                    src[(r + x0) * 4..(r + x1) * 4].iter().copied()
                })
                .collect()
        };
        Some(Patch {
            at,
            before: cut(old, top),
            after: cut(rows, top),
        })
    }

    /// Bytes the history holds for grown canvases (the old picture of each growth).
    pub fn canvas_bytes(&self) -> usize {
        fn of(e: &Edit) -> usize {
            match e {
                Edit::Canvas { old, .. } => old.rgba.len(),
                Edit::Seq(all) => all.iter().map(of).sum(),
                _ => 0,
            }
        }
        (self.done.iter().chain(&self.undone))
            .map(|(_, e)| of(e))
            .sum()
    }

    /// Which state of the history this is: the same number only for the same
    /// edits (undo then redo comes back to it; undo then a new edit never does).
    pub fn state(&self) -> u64 {
        self.done.last().map_or(0, |(n, _)| *n)
    }

    /// Where the history is: (steps done, all steps).
    pub fn steps(&self) -> (usize, usize) {
        (self.done.len(), self.done.len() + self.undone.len())
    }

    /// The newest edit done (tests).
    #[cfg(test)]
    pub fn last_edit(&self) -> Option<&(u64, Edit)> {
        self.done.last()
    }

    /// How many edits can be undone.
    pub fn depth(&self) -> usize {
        self.done.len()
    }

    /// Calls `f` with the document at each state of its history, the first
    /// state to the current one; the document ends as it was (no copy).
    pub fn replay(&mut self, mut f: impl FnMut(&Document)) {
        let (depth, selected) = (self.done.len(), self.selected);
        for _ in 0..depth {
            self.undo();
        }
        f(self);
        for _ in 0..depth {
            self.redo();
            f(self);
        }
        self.selected = selected;
    }

    pub fn undo(&mut self) -> bool {
        let Some((n, edit)) = self.done.pop() else {
            return false;
        };
        edit.inverse().apply(self);
        self.undone.push((n, edit));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((n, edit)) = self.undone.pop() else {
            return false;
        };
        edit.apply(self);
        self.done.push((n, edit));
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

    /// The visible part of the image, image px.
    pub fn view(&self) -> Rectangle {
        self.crop.unwrap_or(Rectangle::new(
            Point::ORIGIN,
            Size::new(self.image.width as f32, self.image.height as f32),
        ))
    }
}

/// The base an editor shows for a picture with no drawing: its pixels as the renderer wants
/// them (alpha premultiplied) and whether any pixel is see-through. One pass, pure: made on a
/// thread for a big picture.
pub fn base_of(image: &Image) -> (iced::widget::image::Handle, bool) {
    let mut px = image.rgba.clone();
    let mut alpha = false;
    for p in px.as_chunks_mut::<4>().0 {
        if p[3] != 255 {
            alpha = true;
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
    }
    (
        iced::widget::image::Handle::from_rgba(image.width, image.height, px),
        alpha,
    )
}

/// The flattened result (crop applied, every annotation drawn): what Copy and
/// Save get. Pure: no window, no state.
pub fn render(doc: &Document) -> Crop {
    let layers: Vec<_> = doc.items.iter().map(|item| (item, true)).collect();
    let pm = doc.flatten(&layers, doc.legend_layout().as_ref());
    // (an opaque pixel is the same premultiplied or not: no division for it)
    let mut rgba = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        match p.alpha() {
            255 => rgba.extend_from_slice(&[p.red(), p.green(), p.blue(), 255]),
            _ => {
                let c = p.demultiply();
                rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
            }
        }
    }
    Crop {
        width: pm.width(),
        height: pm.height(),
        rgba,
    }
}

/// The capture under the drawing: the view's pixels with no annotation on them
/// (what Copy text and Translate read, `actions::snip_more`).
pub fn clean(doc: &Document) -> Crop {
    let view = doc.view();
    let pm = doc.paint(&[], int_rect(view, view), None);
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

/// What the user hid (Blur, Redact, and what Spotlight dims), in image px:
/// text there must never leave the app.
pub fn hidden(doc: &Document) -> crate::features::snip::snip_more::Hidden {
    let mut out = crate::features::snip::snip_more::Hidden::default();
    for item in doc.items.iter().filter(|i| !i.hidden) {
        match item.shape {
            Shape::Blur { .. } | Shape::Redact { .. } => out.boxes.push(item.frame()),
            Shape::Spotlight { .. } => out.lit.push(item.frame()),
            _ => {}
        }
    }
    out
}

/// `hidden` in `clean`'s own px (its top-left pixel is the view's).
pub fn hidden_in_clean(doc: &Document) -> crate::features::snip::snip_more::Hidden {
    let view = doc.view();
    let origin = Point::new(view.x.floor(), view.y.floor());
    hidden(doc).shifted(Point::ORIGIN - origin)
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

/// The ellipse in `r`.
pub fn oval_path(r: Rectangle) -> Option<Path> {
    PathBuilder::from_oval(tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height)?)
}

impl Item {
    /// Footprint to exclude from Soft Hide exterior-ring sampling (`None` for
    /// non-Hide items or hidden items).
    pub(crate) fn exclude(&self) -> Option<blur::Exclude<'_>> {
        if self.hidden {
            return None;
        }
        match &self.shape {
            Shape::Blur {
                points,
                size,
                cell,
                kind,
                outline,
                ..
            } => Some(blur::Exclude::Blur {
                points,
                size: *size,
                kind: *kind,
                outline: *outline,
                feather: blur::feather(*cell),
            }),
            Shape::Redact { a, b, oval } => Some(blur::Exclude::Redact {
                a: *a,
                b: *b,
                oval: *oval,
            }),
            _ => None,
        }
    }
}

/// Every Hide and Redact footprint in `items` to exclude from Soft's exterior rings.
pub(crate) fn excludes<'a>(items: impl IntoIterator<Item = &'a Item>) -> Vec<blur::Exclude<'a>> {
    items.into_iter().filter_map(Item::exclude).collect()
}

/// `paint`, excluding every Hide / Redact in `excludes` (even ones not in `items`,
/// such as Hides above a merged picture during bake).
pub fn paint_with_excludes(
    image: &Image,
    items: &[(&Item, bool)],
    excludes: &[blur::Exclude<'_>],
    region: IntRect,
    seed: u64,
) -> Pixmap {
    paint_on(image, items, excludes, region, seed, false)
}

/// `paint`, `bare`: on a see-through pixmap instead of the picture, so it holds only what the
/// items paint (W111: the live layer of a shape never draws a second copy of the picture).
pub fn paint_on(
    image: &Image,
    items: &[(&Item, bool)],
    excludes: &[blur::Exclude<'_>],
    region: IntRect,
    seed: u64,
    bare: bool,
) -> Pixmap {
    let (w, h) = (region.width(), region.height());
    let premultiply = |px: &mut [u8]| {
        // Screen captures are opaque (one cheap look says so); premultiply anything that is not.
        if px.as_chunks::<4>().0.iter().any(|p| p[3] != 255) {
            for p in px.as_chunks_mut::<4>().0.iter_mut().filter(|p| p[3] != 255) {
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = (*c as u32 * a / 255) as u8;
                }
            }
        }
    };
    let whole = (region.x(), region.y(), w, h) == (0, 0, image.width, image.height);
    let pm = match whole.then(|| tiny_skia::IntSize::from_wh(w, h)).flatten() {
        _ if bare => Pixmap::new(w, h).expect("non-empty region"),
        // the whole picture: one copy (a new pixmap is zeroed first, 8K: a pass of 130 MB)
        Some(size) => {
            let mut data = image.rgba.clone();
            premultiply(&mut data);
            Pixmap::from_vec(data, size).expect("as many bytes as pixels")
        }
        None => {
            let mut pm = Pixmap::new(w, h).expect("non-empty region");
            let stride = image.width as usize * 4;
            let (x, y) = (region.x() as usize, region.y() as usize);
            for (row, dst) in pm.data_mut().chunks_exact_mut(w as usize * 4).enumerate() {
                let start = (y + row) * stride + x * 4;
                dst.copy_from_slice(&image.rgba[start..start + dst.len()]);
                premultiply(dst);
            }
            pm
        }
    };
    let mut layer = Layer {
        pm,
        origin: Point::new(region.x() as f32, region.y() as f32),
    };

    let holes: Vec<Rectangle> = items
        .iter()
        .filter(|(_, visible)| *visible)
        .filter_map(|(item, _)| match item.shape {
            Shape::Spotlight { a, b } if !item.hidden => Some(span(&[a, b])),
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
        if !visible || item.hidden {
            continue;
        }
        let (c, w, ring) = (item.colour, item.width, item.outline);
        match &item.shape {
            Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
                let oval = matches!(item.shape, Shape::Ellipse { .. });
                shape::paint(&mut layer, (*a, *b), oval, c, w, (ring, item.fill))
            }
            Shape::Arrow {
                a,
                b,
                bend,
                head,
                text,
            } => {
                arrow::paint(&mut layer, (*a, *b, *bend), *head, c, w, ring);
                if !text.is_empty() {
                    let size = label::arrow_size(w);
                    let at = label::centred(*a, text, size);
                    label::paint(&mut layer, at, text, size, c, ring);
                }
            }
            Shape::Pen(points) => pen::paint(&mut layer, points, c, w),
            Shape::Highlighter(points) => highlighter::paint(&mut layer, points, c, w),
            Shape::Text {
                at,
                text,
                size,
                style,
                wrap,
            } => text::paint_styled(&mut layer, *at, text, *size, c, *style, *wrap),
            Shape::Step { at, radius, .. } => step::paint(&mut layer, *at, *radius, steps, c),
            Shape::Label { at, text, size } => label::paint(&mut layer, *at, text, *size, c, true),
            Shape::Pointer { at, radius } => pointer::paint(&mut layer, *at, *radius, c, w),
            Shape::Blur {
                points,
                size,
                cell,
                style,
                kind,
                outline,
                ramp,
            } => {
                let fp = blur::Footprint {
                    points,
                    size: *size,
                    kind: *kind,
                    outline: *outline,
                    ramp,
                    source: Some(image),
                    exclude: excludes,
                };
                blur::paint(&mut layer, &fp, *cell, *style, seed)
            }
            Shape::Redact { a, b, oval } => {
                let r = span(&[*a, *b]);
                let path = match oval {
                    true => oval_path(r),
                    false => rounded(r, 0.0),
                };
                if let Some(path) = path {
                    layer.fill(&path, [c[0], c[1], c[2], 255]);
                }
            }
            Shape::Spotlight { .. } => {}
            Shape::Callout {
                tip,
                at,
                text,
                size,
            } => callout::paint(&mut layer, (*tip, *at), text, *size, c, ring),
            Shape::Magnifier {
                at,
                radius,
                zoom,
                kind,
            } => magnifier::paint(&mut layer, *at, *radius, *zoom, *kind, c, w, item.outline),
            Shape::Picture { a, b, pic, .. } => {
                paint_picture(&mut layer, span(&[*a, *b]), pic, c[3])
            }
        }
    }
    layer.pm
}

/// A pasted picture scaled into `to`, at `opacity` (0-255).
fn paint_picture(layer: &mut Layer, to: Rectangle, pic: &Pic, opacity: u8) {
    let (w, h, data) = &*pic.0;
    let Some(src) = tiny_skia::PixmapRef::from_bytes(data, *w, *h) else {
        return;
    };
    if to.width <= 0.0 || to.height <= 0.0 {
        return;
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: f32::from(opacity) / 255.0,
        quality: tiny_skia::FilterQuality::Bilinear,
        ..Default::default()
    };
    let t = Transform::from_translate(to.x - layer.origin.x, to.y - layer.origin.y)
        .pre_scale(to.width / *w as f32, to.height / *h as f32);
    layer.pm.draw_pixmap(0, 0, src, &paint, t, None);
}

/// serde for iced's `Point` (no serde feature there): `[x, y]`.
mod pt {
    use iced::Point;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(p: &Point, s: S) -> Result<S::Ok, S::Error> {
        [p.x, p.y].serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Point, D::Error> {
        let [x, y] = <[f32; 2]>::deserialize(d)?;
        Ok(Point::new(x, y))
    }
}

mod pts {
    use iced::Point;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(p: &[Point], s: S) -> Result<S::Ok, S::Error> {
        p.iter()
            .map(|p| [p.x, p.y])
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Point>, D::Error> {
        let all = Vec::<[f32; 2]>::deserialize(d)?;
        Ok(all.into_iter().map(|[x, y]| Point::new(x, y)).collect())
    }
}

/// A pasted picture is saved as its size and the premultiplied pixels in base64.
impl serde::Serialize for Pic {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use base64::Engine as _;
        let (w, h, data) = &*self.0;
        (w, h, base64::engine::general_purpose::STANDARD.encode(data)).serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for Pic {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use base64::Engine as _;
        let (w, h, text) = <(u32, u32, String)>::deserialize(d)?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)?;
        if data.len() != w as usize * h as usize * 4 {
            return Err(serde::de::Error::custom("picture size"));
        }
        Ok(Pic(std::sync::Arc::new((w, h, data))))
    }
}

/// `old` in a see-through canvas of `size`, its top-left at `at`.
fn placed(old: &Image, (w, h): (u32, u32), at: Vector) -> Image {
    let (w, h) = (w as usize, h as usize);
    let (dx, dy) = (at.x as usize, at.y as usize);
    let mut rgba = vec![0u8; w * h * 4];
    let row = old.width as usize * 4;
    for y in 0..old.height as usize {
        let to = ((y + dy) * w + dx) * 4;
        if let Some(dst) = rgba.get_mut(to..to + row) {
            dst.copy_from_slice(&old.rgba[y * row..][..row]);
        }
    }
    Image {
        width: w as u32,
        height: h as u32,
        rgba,
    }
}
