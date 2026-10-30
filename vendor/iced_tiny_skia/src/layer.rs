use crate::Primitive;
use crate::core::renderer::Quad;
use crate::core::{
    self, Background, Color, Point, Rectangle, Svg, Transformation,
};
use crate::graphics::damage;
use crate::graphics::layer;
use crate::graphics::text::{Editor, Paragraph, Text};
use crate::graphics::{self, Image};

use std::sync::Arc;

pub type Stack = layer::Stack<Layer>;

#[derive(Debug, Clone)]
pub struct Layer {
    pub bounds: Rectangle,
    pub quads: Vec<(Quad, Background)>,
    pub primitives: Vec<Item<Primitive>>,
    pub images: Vec<Image>,
    pub text: Vec<Item<Text>>,
}

impl Layer {
    pub fn draw_quad(
        &mut self,
        mut quad: Quad,
        background: Background,
        transformation: Transformation,
    ) {
        quad.bounds = quad.bounds * transformation;
        self.quads.push((quad, background));
    }

    pub fn draw_paragraph(
        &mut self,
        paragraph: &Paragraph,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let paragraph = Text::Paragraph {
            paragraph: paragraph.downgrade(),
            position,
            color,
            clip_bounds,
            transformation,
        };

        self.text.push(Item::Live(paragraph));
    }

    pub fn draw_editor(
        &mut self,
        editor: &Editor,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let editor = Text::Editor {
            editor: editor.downgrade(),
            position,
            color,
            clip_bounds,
            transformation,
        };

        self.text.push(Item::Live(editor));
    }

    pub fn draw_text(
        &mut self,
        text: core::Text,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let text = Text::Cached {
            content: text.content,
            bounds: Rectangle::new(position, text.bounds) * transformation,
            color,
            size: text.size * transformation.scale_factor(),
            line_height: text.line_height.to_absolute(text.size)
                * transformation.scale_factor(),
            font: text.font,
            align_x: text.align_x,
            align_y: text.align_y,
            shaping: text.shaping,
            clip_bounds: clip_bounds * transformation,
        };

        self.text.push(Item::Live(text));
    }

    pub fn draw_text_raw(
        &mut self,
        raw: graphics::text::Raw,
        transformation: Transformation,
    ) {
        let raw = Text::Raw {
            raw,
            transformation,
        };

        self.text.push(Item::Live(raw));
    }

    pub fn draw_text_group(
        &mut self,
        text: Vec<Text>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.text
            .push(Item::Group(text, clip_bounds, transformation));
    }

    pub fn draw_text_cache(
        &mut self,
        text: Arc<[Text]>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.text
            .push(Item::Cached(text, clip_bounds, transformation));
    }

    pub fn draw_image(&mut self, image: Image, transformation: Transformation) {
        match image {
            Image::Raster {
                image,
                bounds,
                clip_bounds,
            } => {
                self.draw_raster(image, bounds, clip_bounds, transformation);
            }
            Image::Vector {
                svg,
                bounds,
                clip_bounds,
            } => {
                self.draw_svg(svg, bounds, clip_bounds, transformation);
            }
        }
    }

    pub fn draw_raster(
        &mut self,
        image: core::Image,
        bounds: Rectangle,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let image = Image::Raster {
            image: core::Image {
                border_radius: image.border_radius
                    * transformation.scale_factor(),
                ..image
            },
            bounds: bounds * transformation,
            clip_bounds: clip_bounds * transformation,
        };

        self.images.push(image);
    }

    pub fn draw_svg(
        &mut self,
        svg: Svg,
        bounds: Rectangle,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        let svg = Image::Vector {
            svg,
            bounds: bounds * transformation,
            clip_bounds: clip_bounds * transformation,
        };

        self.images.push(svg);
    }

    pub fn draw_primitive_group(
        &mut self,
        primitives: Vec<Primitive>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.primitives.push(Item::Group(
            primitives,
            clip_bounds * transformation,
            transformation,
        ));
    }

    pub fn draw_primitive_cache(
        &mut self,
        primitives: Arc<[Primitive]>,
        clip_bounds: Rectangle,
        transformation: Transformation,
    ) {
        self.primitives.push(Item::Cached(
            primitives,
            clip_bounds * transformation,
            transformation,
        ));
    }

    /// OpenCircle patch (#23): the damage between two frames' layer stacks.
    /// Upstream counts a layer that came or went (a widget translated into
    /// its own layer while it slides, a popup) as its whole bounds, which are
    /// the window's: one bar sliding in redrew the full screen. Here such a
    /// layer counts only where it draws.
    pub fn frame_damage(previous: &[Self], current: &[Self]) -> Vec<Rectangle> {
        damage::diff(
            previous,
            current,
            |layer| {
                use graphics::Layer as _;
                Self::damage(&Self::with_bounds(layer.bounds), layer)
            },
            Self::damage,
        )
    }

    pub fn damage(previous: &Self, current: &Self) -> Vec<Rectangle> {
        // OpenCircle patch: a layer that moved or changed size (a stack's
        // upper child: snip's live area canvas, resized or moved with its
        // area) redraws what changed in it and the strips only one of the two
        // bounds covers (upstream: both bounds whole, the area's inside every
        // frame of a resize).
        let (moved, clip) = match (previous.bounds, current.bounds) {
            (a, b) if a == b => (Vec::new(), b),
            (a, b) if finite(a) && finite(b) => (apart(a, b), a.union(&b)),
            (a, b) => return vec![a, b],
        };

        let mut damage = damage::list(
            &previous.quads,
            &current.quads,
            |(quad, _)| {
                quad.bounds
                    .expand(1.0)
                    .intersection(&clip)
                    .into_iter()
                    .collect()
            },
            |(quad_a, background_a), (quad_b, background_b)| {
                quad_a == quad_b && background_a == background_b
            },
        );

        let text = damage::diff(
            &previous.text,
            &current.text,
            |item| {
                item.as_slice()
                    .iter()
                    .filter_map(drawn_bounds)
                    .map(|bounds| bounds * item.transformation())
                    .collect()
            },
            |text_a, text_b| {
                damage::list(
                    text_a.as_slice(),
                    text_b.as_slice(),
                    |text| {
                        drawn_bounds(text)
                            .into_iter()
                            .map(|bounds| bounds * text_a.transformation())
                            .collect()
                    },
                    |text_a, text_b| text_a == text_b,
                )
            },
        );

        // OpenCircle patch (#89): where each primitive draws, a stroke's
        // width and anti-aliasing included (upstream: its path's bounds),
        // a cached canvas's too (upstream: the whole canvas, so any change
        // to the overlay's full-window canvas redrew the window)
        let drawn = |item: &Item<Primitive>| match item {
            Item::Live(primitive) => drawn_primitive(primitive),
            Item::Group(_, group_bounds, transformation)
            | Item::Cached(_, group_bounds, transformation) => item
                .as_slice()
                .iter()
                .flat_map(drawn_primitive)
                .map(|bounds| bounds * *transformation)
                .filter_map(|bounds| bounds.intersection(group_bounds))
                .collect(),
        };
        let primitives = damage::diff(
            &previous.primitives,
            &current.primitives,
            drawn,
            |primitive_a, primitive_b| match (primitive_a, primitive_b) {
                (
                    Item::Cached(cache_a, bounds_a, transformation_a),
                    Item::Cached(cache_b, bounds_b, transformation_b),
                ) if Arc::ptr_eq(cache_a, cache_b)
                    && bounds_a == bounds_b
                    && transformation_a == transformation_b =>
                {
                    Vec::new()
                }
                // OpenCircle patch (#89): a canvas drawn anew the same as
                // before (the zoomed frame's black under its picture, a few
                // shapes) is unchanged (upstream: always redrawn)
                (Item::Group(a, bounds_a, t_a), Item::Group(b, bounds_b, t_b))
                    if bounds_a == bounds_b && t_a == t_b =>
                {
                    match a == b {
                        true => Vec::new(),
                        // OpenCircle patch: fills of rectangles whose
                        // rectangles moved (the dim's tiles round a box being
                        // moved or resized) redraw only the strips that
                        // changed, not each tile whole
                        false => boxes_moved(a, b)
                            .map(|moved| {
                                (moved.into_iter())
                                    .map(|r| r * *t_a)
                                    .filter_map(|r| r.intersection(bounds_a))
                                    .collect()
                            })
                            .unwrap_or_else(|| {
                                drawn(primitive_a).into_iter().chain(drawn(primitive_b)).collect()
                            }),
                    }
                }
                _ => drawn(primitive_a).into_iter().chain(drawn(primitive_b)).collect(),
            },
        );

        let images = damage::list(
            &previous.images,
            &current.images,
            |image| vec![image.bounds().expand(1.0)],
            same_image,
        );

        damage.extend(text);
        damage.extend(primitives);
        damage.extend(images);
        damage.extend(moved);
        damage
    }
}

/// OpenCircle patch: where two drawings of as many primitives differ: only
/// the primitives that changed (upstream: all of both), and of a fill of
/// rectangles that kept its paint and rule only what each changed rectangle
/// covers before or after but not both, a rectangle that came or went whole,
/// +1 px for anti-aliasing (a fill of rectangles changes only there, whatever
/// the rule: the dim's tiles round a box being moved or resized). `None`:
/// primitives came or went (then each is redrawn whole).
fn boxes_moved(a: &[Primitive], b: &[Primitive]) -> Option<Vec<Rectangle>> {
    if a.len() != b.len() {
        return None;
    }
    let mut out = Vec::new();
    for (pa, pb) in a.iter().zip(b).filter(|(pa, pb)| pa != pb) {
        let boxes = match (pa, pb) {
            (
                Primitive::Fill { path: path_a, paint: paint_a, rule: rule_a },
                Primitive::Fill { path: path_b, paint: paint_b, rule: rule_b },
            ) if paint_a == paint_b && rule_a == rule_b => rects(path_a).zip(rects(path_b)),
            _ => None,
        };
        let Some((ra, rb)) = boxes else {
            out.extend(drawn_primitive(pa));
            out.extend(drawn_primitive(pb));
            continue;
        };
        let n = ra.len().min(rb.len());
        for (x, y) in ra.iter().zip(&rb).filter(|(x, y)| x != y) {
            out.extend(apart(*x, *y).into_iter().map(|r| r.expand(1.0)));
        }
        out.extend(ra[n..].iter().chain(&rb[n..]).map(|r| r.expand(1.0)));
    }
    Some(out)
}

/// OpenCircle patch: `path` as the axis-aligned rectangles it is made of
/// (each a closed run of 4 straight sides), else `None`.
fn rects(path: &tiny_skia::Path) -> Option<Vec<Rectangle>> {
    use tiny_skia::PathSegment;
    let mut out = Vec::new();
    let mut corners: Vec<tiny_skia::Point> = Vec::new();
    let mut close = |corners: &mut Vec<tiny_skia::Point>| -> Option<()> {
        if corners.is_empty() {
            return Some(());
        }
        if corners.len() == 5 && corners[4] == corners[0] {
            let _ = corners.pop();
        }
        if corners.len() != 4 {
            return None;
        }
        let sides = (0..4).all(|i| {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            (p.x == q.x) != (p.y == q.y)
        });
        if !sides {
            return None;
        }
        let (x0, x1) = corners.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        let (y0, y1) = corners.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        out.push(Rectangle::new(Point::new(x0, y0), core::Size::new(x1 - x0, y1 - y0)));
        corners.clear();
        Some(())
    };
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                close(&mut corners)?;
                corners.push(p);
            }
            PathSegment::LineTo(p) => corners.push(p),
            PathSegment::Close => close(&mut corners)?,
            PathSegment::QuadTo(..) | PathSegment::CubicTo(..) => return None,
        }
    }
    close(&mut corners)?;
    Some(out)
}

/// OpenCircle patch: `r` is a real rectangle (not a layer's infinite bounds).
fn finite(r: Rectangle) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
}

/// OpenCircle patch: what `a` or `b` covers but not both, as rectangles.
fn apart(a: Rectangle, b: Rectangle) -> Vec<Rectangle> {
    let Some(both) = a.intersection(&b) else {
        return vec![a, b];
    };
    let mut out = Vec::new();
    for r in [a, b] {
        let (x1, y1) = (r.x + r.width, r.y + r.height);
        let (bx1, by1) = (both.x + both.width, both.y + both.height);
        let mut add = |x0: f32, y0: f32, x1: f32, y1: f32| {
            if x1 > x0 && y1 > y0 {
                out.push(Rectangle::new(Point::new(x0, y0), core::Size::new(x1 - x0, y1 - y0)));
            }
        };
        add(r.x, r.y, x1, both.y); // above
        add(r.x, by1, x1, y1); // below
        add(r.x, both.y, both.x, by1); // left
        add(bx1, both.y, x1, by1); // right
    }
    out
}

/// OpenCircle patch (#89): where `primitive` draws: its path's bounds, a
/// pixel more for anti-aliasing, and a stroke's reach past its path (half
/// its width, a mitred corner up to its miter limit times that). A stroke
/// draws only along its path: the bounds of its runs of segments
/// (`stroke_runs`), not of all of it (a selection box's outline covered the
/// whole box, so any change to it redrew the box's inside too).
fn drawn_primitive(primitive: &Primitive) -> Vec<Rectangle> {
    match primitive {
        Primitive::Fill { .. } => vec![primitive.visible_bounds().expand(1.0)],
        Primitive::Stroke { path, stroke, .. } => {
            // (only a mitred join reaches past half its width)
            let miter = match stroke.line_join {
                tiny_skia::LineJoin::Miter | tiny_skia::LineJoin::MiterClip => {
                    stroke.miter_limit.max(1.0)
                }
                tiny_skia::LineJoin::Round | tiny_skia::LineJoin::Bevel => 1.0,
            };
            let reach = stroke.width / 2.0 * miter;
            stroke_runs(path)
                .into_iter()
                .map(|run| run.expand(reach + 1.0))
                .collect()
        }
    }
}

/// The bounds of `path`'s segments (a curve's control points included, which
/// bound it), runs of them in path order as one while that wastes at most
/// `damage::group`'s area: a box's sides apart, a curve's neighbours as one.
fn stroke_runs(path: &tiny_skia::Path) -> Vec<Rectangle> {
    use tiny_skia::PathSegment;
    const WASTE: f32 = 20_000.0;
    let bounds = |points: &[tiny_skia::Point]| {
        let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
        let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in points {
            (x0, y0) = (x0.min(p.x), y0.min(p.y));
            (x1, y1) = (x1.max(p.x), y1.max(p.y));
        }
        Rectangle::new(Point::new(x0, y0), core::Size::new(x1 - x0, y1 - y0))
    };
    let mut runs: Vec<Rectangle> = Vec::new();
    let mut start = tiny_skia::Point::zero();
    let mut at = start;
    for segment in path.segments() {
        let seg = match segment {
            PathSegment::MoveTo(p) => {
                start = p;
                bounds(&[p]) // a lone point still draws its caps
            }
            PathSegment::LineTo(p) => bounds(&[at, p]),
            PathSegment::QuadTo(c, p) => bounds(&[at, c, p]),
            PathSegment::CubicTo(c, d, p) => bounds(&[at, c, d, p]),
            PathSegment::Close => bounds(&[at, start]),
        };
        at = match segment {
            PathSegment::MoveTo(p)
            | PathSegment::LineTo(p)
            | PathSegment::QuadTo(_, p)
            | PathSegment::CubicTo(_, _, p) => p,
            PathSegment::Close => start,
        };
        let union = runs.last().map(|run| (run.union(&seg), run.area()));
        match union {
            Some((union, run)) if union.area() - run - seg.area() <= WASTE => {
                *runs.last_mut().expect("a run") = union;
            }
            _ => runs.push(seg),
        }
    }
    runs
}

/// OpenCircle patch (#89): `a` and `b` are the same picture drawn the same
/// way. Upstream `Image::eq` compares an RGBA handle's pixels byte by byte (a
/// 4K frozen screen: 33 MB, on each frame the overlay redraws); handles with
/// the same id hold the same pixels.
pub(crate) fn same_image(a: &Image, b: &Image) -> bool {
    match (a, b) {
        (
            Image::Raster { image: a, bounds: ab, clip_bounds: ac },
            Image::Raster { image: b, bounds: bb, clip_bounds: bc },
        ) => same_look(a, b) && ab == bb && ac == bc,
        _ => a == b,
    }
}

/// OpenCircle patch (#89): `a` and `b` draw the same picture the same way,
/// anywhere (`same_image`).
pub(crate) fn same_look(a: &core::Image, b: &core::Image) -> bool {
    a.handle.id() == b.handle.id()
        && a.filter_method == b.filter_method
        && a.rotation == b.rotation
        && a.border_radius == b.border_radius
        && a.opacity == b.opacity
        && a.snap == b.snap
}

/// OpenCircle patch (#53): where a text is really drawn. Upstream
/// `Text::visible_bounds` puts a cached text's box at its anchor point, but
/// `draw_cached` draws the text around that point by its alignment (a
/// vertically centred text sits half a line higher), so when the text changed
/// the part above the anchor kept the old glyphs on screen (ghost text in
/// every `pick_list`). A quarter of the font size wider on every side: glyphs
/// overhang their box (a "J" reaches left of its origin, accents above);
/// a paragraph's too (below).
fn drawn_bounds(text: &Text) -> Option<Rectangle> {
    use crate::core::{alignment::Vertical, text::Alignment};
    let Text::Cached {
        content,
        bounds,
        clip_bounds,
        size,
        line_height,
        align_x,
        align_y,
        ..
    } = text
    else {
        // A paragraph's `min_bounds` is its advance box: a glyph overhangs it
        // too (the "J" of a dropdown's value at 150 % left a stale column), by
        // up to a quarter of the line height.
        return match text {
            Text::Paragraph {
                position,
                paragraph,
                clip_bounds,
                transformation,
                ..
            } => Rectangle::new(*position, paragraph.min_bounds)
                .expand(paragraph.min_bounds.height / 4.0)
                .intersection(clip_bounds)
                .map(|bounds| bounds * *transformation),
            _ => text.visible_bounds(),
        };
    };
    // OpenCircle patch: a canvas text's box is its max width (often infinite)
    // and an infinite height, so its damage reached the window's corner from
    // its anchor (snip's loupe label redrew half the screen on every pointer
    // move): at most 1.5 em per character, a line per line (or per character
    // when it may wrap).
    // ponytail: an advance estimate, not a measure; measure the paragraph if a
    // glyph wider than 1.5 em ever leaves a stale column.
    let chars = content.chars().count() as f32;
    let lines = match bounds.width.is_finite() {
        true => chars.max(1.0),
        false => content.lines().count().max(1) as f32,
    };
    let bounds = Rectangle {
        width: bounds.width.min(chars * 1.5 * f32::from(*size)),
        height: bounds.height.min(lines * f32::from(*line_height)),
        ..*bounds
    };
    let bounds = &bounds;
    let x = match align_x {
        Alignment::Center => bounds.x - bounds.width / 2.0,
        Alignment::Right => bounds.x - bounds.width,
        Alignment::Default | Alignment::Left | Alignment::Justified => {
            bounds.x
        }
    };
    let y = match align_y {
        Vertical::Top => bounds.y,
        Vertical::Center => bounds.y - bounds.height / 2.0,
        Vertical::Bottom => bounds.y - bounds.height,
    };
    Rectangle::new(Point::new(x, y), bounds.size())
        .expand(f32::from(*size) / 4.0)
        .intersection(clip_bounds)
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            bounds: Rectangle::INFINITE,
            quads: Vec::new(),
            primitives: Vec::new(),
            text: Vec::new(),
            images: Vec::new(),
        }
    }
}

impl graphics::Layer for Layer {
    fn with_bounds(bounds: Rectangle) -> Self {
        Self {
            bounds,
            ..Self::default()
        }
    }

    fn bounds(&self) -> Rectangle {
        self.bounds
    }

    fn flush(&mut self) {}

    fn resize(&mut self, bounds: Rectangle) {
        self.bounds = bounds;
    }

    fn reset(&mut self) {
        self.bounds = Rectangle::INFINITE;

        self.quads.clear();
        self.primitives.clear();
        self.text.clear();
        self.images.clear();
    }

    fn start(&self) -> usize {
        if !self.quads.is_empty() {
            return 1;
        }

        if !self.primitives.is_empty() {
            return 2;
        }

        if !self.images.is_empty() {
            return 3;
        }

        if !self.text.is_empty() {
            return 4;
        }

        usize::MAX
    }

    fn end(&self) -> usize {
        if !self.text.is_empty() {
            return 4;
        }

        if !self.images.is_empty() {
            return 3;
        }

        if !self.primitives.is_empty() {
            return 2;
        }

        if !self.quads.is_empty() {
            return 1;
        }

        0
    }

    fn merge(&mut self, layer: &mut Self) {
        self.quads.append(&mut layer.quads);
        self.primitives.append(&mut layer.primitives);
        self.text.append(&mut layer.text);
        self.images.append(&mut layer.images);
    }
}

#[derive(Debug, Clone)]
pub enum Item<T> {
    Live(T),
    Group(Vec<T>, Rectangle, Transformation),
    Cached(Arc<[T]>, Rectangle, Transformation),
}

impl<T> Item<T> {
    pub fn transformation(&self) -> Transformation {
        match self {
            Item::Live(_) => Transformation::IDENTITY,
            Item::Group(_, _, transformation)
            | Item::Cached(_, _, transformation) => *transformation,
        }
    }

    pub fn clip_bounds(&self) -> Rectangle {
        match self {
            Item::Live(_) => Rectangle::INFINITE,
            Item::Group(_, clip_bounds, _)
            | Item::Cached(_, clip_bounds, _) => *clip_bounds,
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match self {
            Item::Live(item) => std::slice::from_ref(item),
            Item::Group(group, _, _) => group.as_slice(),
            Item::Cached(cache, _, _) => cache,
        }
    }
}
