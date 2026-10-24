//! Text: haloed, readable on any background, editable until export. Shaped with
//! iced's own font system (one font database for the UI and annotations).
use std::sync::{LazyLock, Mutex};

use iced::advanced::graphics::text::cosmic_text::{
    Attrs, Buffer, Color, Family, Metrics, Shaping, SwashCache, Weight,
};
use iced::advanced::graphics::text::font_system;
use iced::{Point, Size, Vector};

use super::document::{Layer, halo, rounded};

static GLYPHS: LazyLock<Mutex<SwashCache>> = LazyLock::new(|| Mutex::new(SwashCache::new()));

/// Line height for a font size.
pub fn line_height(size: f32) -> f32 {
    (size * 1.25).ceil()
}

/// How a text annotation is set (the Text tool's options).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub font: Font,
    pub bold: bool,
    pub back: Back,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            font: Font::Sans,
            bold: true,
            back: Back::Outline,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    Sans,
    Serif,
    Mono,
}

impl Font {
    pub const ALL: [Font; 3] = [Font::Sans, Font::Serif, Font::Mono];
    fn family(self) -> Family<'static> {
        match self {
            Font::Sans => Family::SansSerif,
            Font::Serif => Family::Serif,
            Font::Mono => Family::Monospace,
        }
    }
}

/// What is behind the glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    None,
    /// A halo: reads on any background.
    Outline,
    /// A solid rounded box of the halo's colour.
    Box,
}

impl Back {
    pub const ALL: [Back; 3] = [Back::None, Back::Outline, Back::Box];
}

/// Labels, callouts, steps, the legend: semibold sans.
// ponytail: system fonts; switch to Family::Name("Inter") once the UI loads the
// bundled Inter into iced.
fn layout(text: &str, size: f32) -> Buffer {
    shape(text, size, Family::SansSerif, Weight::SEMIBOLD)
}

fn styled(text: &str, size: f32, style: Style) -> Buffer {
    let weight = if style.bold {
        Weight::BOLD
    } else {
        Weight::NORMAL
    };
    shape(text, size, style.font.family(), weight)
}

/// `text` shaped at `size`, unwrapped.
fn shape(text: &str, size: f32, family: Family<'_>, weight: Weight) -> Buffer {
    let mut fonts = font_system().write().expect("font system");
    let fs = fonts.raw();
    let mut buf = Buffer::new(fs, Metrics::new(size, line_height(size)));
    buf.set_size(fs, None, None);
    let attrs = Attrs::new().family(family).weight(weight);
    buf.set_text(fs, text, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(fs, false);
    buf
}

/// The box of `buf` holding `text` (an empty text still has one line, and some
/// width for the caret).
fn extent(buf: &Buffer, text: &str, size: f32) -> Size {
    let width = buf.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
    let lines = text.split('\n').count() as f32;
    Size::new(width.max(size * 0.3), lines * line_height(size))
}

/// The text's box (an empty text still has one line, and some width for the caret).
pub fn measure(text: &str, size: f32) -> Size {
    extent(&layout(text, size), text, size)
}

/// `measure` for a text set in `style`.
pub fn measure_styled(text: &str, size: f32, style: Style) -> Size {
    extent(&styled(text, size, style), text, size)
}

/// Where the caret goes after the last character, from the text's top-left.
pub fn caret(text: &str, size: f32) -> Vector {
    caret_with(text, size, |last| measure(last, size).width)
}

/// `caret` for a text set in `style`.
pub fn caret_styled(text: &str, size: f32, style: Style) -> Vector {
    caret_with(text, size, |last| measure_styled(last, size, style).width)
}

fn caret_with(text: &str, size: f32, width: impl Fn(&str) -> f32) -> Vector {
    let last = text.rsplit('\n').next().unwrap_or("");
    let lines = text.split('\n').count() as f32;
    let x = if last.is_empty() { 0.0 } else { width(last) };
    Vector::new(x, (lines - 1.0) * line_height(size))
}

/// Draws `text` with its top-left at `at`; `ring` = a halo around the glyphs.
pub fn paint(layer: &mut Layer, at: Point, text: &str, size: f32, colour: [u8; 4], ring: bool) {
    if text.is_empty() {
        return;
    }
    glyphs(layer, &layout(text, size), at, size, colour, ring);
}

/// A text annotation: `text` in `style`, top-left at `at`.
pub fn paint_styled(
    layer: &mut Layer,
    at: Point,
    text: &str,
    size: f32,
    colour: [u8; 4],
    style: Style,
) {
    if text.is_empty() {
        return;
    }
    let buf = styled(text, size, style);
    if style.back == Back::Box {
        let pad = size * 0.15;
        let body = iced::Rectangle::new(at, extent(&buf, text, size)).expand(pad);
        if let Some(shape) = rounded(body, size * 0.3) {
            let [r, g, b, _] = halo(colour);
            layer.fill(&shape, [r, g, b, 255]);
        }
    }
    glyphs(layer, &buf, at, size, colour, style.back == Back::Outline);
}

fn glyphs(layer: &mut Layer, buf: &Buffer, at: Point, size: f32, colour: [u8; 4], ring: bool) {
    let origin = (at.x - layer.origin.x, at.y - layer.origin.y);
    if ring {
        let d = (size / 14.0).max(1.0);
        for (dx, dy) in [
            (-d, 0.0),
            (d, 0.0),
            (0.0, -d),
            (0.0, d),
            (-d, -d),
            (d, d),
            (-d, d),
            (d, -d),
        ] {
            blit(layer, buf, (origin.0 + dx, origin.1 + dy), halo(colour));
        }
    }
    blit(layer, buf, origin, colour);
}

/// Alpha-blends the glyphs into the (premultiplied) pixmap at `(x, y)`.
fn blit(layer: &mut Layer, buf: &Buffer, (x, y): (f32, f32), colour: [u8; 4]) {
    let (w, h) = (layer.pm.width() as i32, layer.pm.height() as i32);
    let (x, y) = (x.round() as i32, y.round() as i32);
    let data = layer.pm.data_mut();
    let mut fonts = font_system().write().expect("font system");
    let mut glyphs = GLYPHS.lock().expect("glyph cache");
    let [r, g, b, a] = colour;
    buf.draw(
        fonts.raw(),
        &mut glyphs,
        Color::rgba(r, g, b, a),
        |gx, gy, gw, gh, c| {
            let a = c.a() as u32;
            if a == 0 {
                return;
            }
            let src = [c.r(), c.g(), c.b()].map(|v| v as u32 * a / 255);
            for py in (y + gy).max(0)..(y + gy + gh as i32).min(h) {
                for px in (x + gx).max(0)..(x + gx + gw as i32).min(w) {
                    let i = (py * w + px) as usize * 4;
                    for (k, s) in src.iter().enumerate() {
                        data[i + k] = (s + data[i + k] as u32 * (255 - a) / 255) as u8;
                    }
                    data[i + 3] = (a + data[i + 3] as u32 * (255 - a) / 255) as u8;
                }
            }
        },
    );
}
