//! Draw a translation in place of an erased block: measured colour, size
//! fitted to the original line height (shrink, then wrap inside the block),
//! right-aligned for RTL (centred text stays centred), shadow or outline only
//! if the original had one. Shaping, Arabic joining and bidi: cosmic-text.

use super::erase::LineStyle;
use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap,
    fontdb,
};
use std::sync::{LazyLock, Mutex};

const LATIN: &str = "Inter";
const ARABIC: &str = "Noto Sans Arabic";

/// Bundled OFL fonts (assets/fonts, licences beside them). Variable weight
/// 400-700, subset to Latin-1 / the Arabic block with fontTools:
/// instancer (Inter opsz=14, Noto wdth=100, wght 400:700), then subset with
/// the default layout features.
static FONT_DATA: [&[u8]; 2] = [
    include_bytes!("../../assets/fonts/Inter.ttf"),
    include_bytes!("../../assets/fonts/NotoSansArabic.ttf"),
];

pub struct Fonts {
    fs: FontSystem,
    cache: SwashCache,
    /// The system's fonts are loaded too (for a script we don't bundle).
    system: bool,
}

/// What the bundled fonts draw: Latin-1, punctuation, Arabic.
fn bundled(c: char) -> bool {
    matches!(c, '\0'..='\u{00FF}' | '\u{2000}'..='\u{206F}' | '\u{0600}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// Loaded on first use only, so idle memory does not pay for it.
pub static FONTS: LazyLock<Mutex<Fonts>> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    for f in FONT_DATA {
        db.load_font_data(f.to_vec());
    }
    Mutex::new(Fonts {
        fs: FontSystem::new_with_locale_and_db("en-US".into(), db),
        cache: SwashCache::new(),
        system: false,
    })
});

/// One run of text with its own colour (accent words are separate spans).
pub type Span = (String, [u8; 3]);

/// Splits `a [[b]] c` into spans; the marked part gets `accent`.
/// An unclosed `[[` runs to the end; stray `]]` is dropped.
pub fn parse_marked(s: &str, color: [u8; 3], accent: [u8; 3]) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut marked = false;
    let mut rest = s;
    while !rest.is_empty() {
        let tag = if marked { "]]" } else { "[[" };
        let (part, next) = match rest.find(tag) {
            Some(n) => (&rest[..n], &rest[n + 2..]),
            None => (rest, ""),
        };
        let part = if marked {
            part
        } else {
            &part.replace("]]", "")
        };
        if !part.is_empty() {
            spans.push((part.to_string(), if marked { accent } else { color }));
        }
        marked = !marked;
        rest = next;
    }
    spans
}

impl Fonts {
    /// Shapes `spans` (full bidi + Arabic joining). `metrics` = font size and
    /// line height, `width` = wrap width.
    pub fn layout(
        &mut self,
        spans: &[Span],
        rtl: bool,
        metrics: Metrics,
        weight: u16,
        width: Option<f32>,
        align: Align,
    ) -> Buffer {
        // Cyrillic, CJK, Thai, ...: the system's fonts, loaded once, on the
        // first such text (Windows has them all; Linux needs them installed).
        if !self.system && spans.iter().any(|(t, _)| !t.chars().all(bundled)) {
            self.fs.db_mut().load_system_fonts();
            self.system = true;
        }
        let fs = &mut self.fs;
        let mut buf = Buffer::new(fs, metrics);
        buf.set_wrap(
            fs,
            if width.is_some() {
                Wrap::Word
            } else {
                Wrap::None
            },
        );
        buf.set_size(fs, width, None);
        let base = Attrs::new()
            .family(Family::Name(if rtl { ARABIC } else { LATIN }))
            .weight(Weight(weight));
        let rich = spans
            .iter()
            .map(|(t, c)| (t.as_str(), base.clone().color(Color::rgb(c[0], c[1], c[2]))));
        buf.set_rich_text(fs, rich, &base, Shaping::Advanced, Some(align));
        buf.shape_until_scroll(fs, false);
        buf
    }

    /// Alpha-blends the buffer into an RGBA image with its top-left at (x, y).
    /// `over` replaces every span colour (shadows/outlines).
    pub fn draw(
        &mut self,
        buf: &Buffer,
        rgba: &mut [u8],
        (iw, ih): (usize, usize),
        (x, y): (i32, i32),
        over: Option<[u8; 4]>,
    ) {
        buf.draw(
            &mut self.fs,
            &mut self.cache,
            Color::rgb(255, 255, 255),
            |gx, gy, _, _, c| {
                let [r, g, b, a] = match over {
                    Some([r, g, b, a]) => [r, g, b, (c.a() as u32 * a as u32 / 255) as u8],
                    None => [c.r(), c.g(), c.b(), c.a()],
                };
                let (px, py) = (x + gx, y + gy);
                if px < 0 || py < 0 || px >= iw as i32 || py >= ih as i32 || a == 0 {
                    return;
                }
                let i = (py as usize * iw + px as usize) * 4;
                let a = a as u32;
                for (d, s) in rgba[i..i + 3].iter_mut().zip([r, g, b]) {
                    *d = ((s as u32 * a + *d as u32 * (255 - a)) / 255) as u8;
                }
            },
        );
    }
}

/// Width and height of the laid-out text.
pub fn extent(buf: &Buffer) -> (f32, f32) {
    buf.layout_runs().fold((0.0, 0.0), |(w, _), r| {
        (f32::max(w, r.line_w), r.line_top + r.line_height)
    })
}

/// Em size of the original from its ink height and text: ink height is the
/// cap/ascender height (+ descender) of a Latin line.
/// ponytail: fixed Latin ratios (Inter-like); other scripts get 0.8.
fn source_em(text: &str, ink_h: f32) -> f32 {
    if !text.chars().any(|c| c.is_ascii_alphanumeric()) {
        return ink_h / 0.8;
    }
    let tall = text
        .chars()
        .any(|c| c.is_uppercase() || c.is_ascii_digit() || "bdfhiklt/%".contains(c));
    let desc = text.chars().any(|c| "gjpqy".contains(c));
    ink_h / (if tall { 0.73 } else { 0.53 } + if desc { 0.21 } else { 0.0 })
}

/// Where and how a block's translation goes.
pub struct Placement {
    /// Block box x, y, w, h (union of its OCR line boxes).
    pub bounds: [f32; 4],
    /// Original line texts (for the em estimate).
    pub texts: Vec<String>,
    pub styles: Vec<LineStyle>,
    pub rtl: bool,
    pub centred: bool,
}

/// Lays out `text` to fit `p.bounds` and returns the buffer, its top-left
/// and the size used.
pub fn fit(fonts: &mut Fonts, text: &str, p: &Placement) -> (Buffer, (i32, i32), f32) {
    let s0 = &p.styles[0];
    let accent = p.styles.iter().find_map(|s| s.accent).unwrap_or(s0.color);
    let spans = parse_marked(text, s0.color, accent);
    let n = p.styles.len();
    let em = source_em(&p.texts[0], (s0.ink[3] - s0.ink[1]) as f32)
        .min(p.bounds[3] / n as f32)
        .max(6.0);
    // pitch of the original lines (baseline to baseline), if it wrapped
    let pitch = (n > 1).then(|| (p.styles[n - 1].ink[3] - s0.ink[3]) as f32 / (n - 1) as f32);
    // ponytail: stroke/em threshold tuned on Inter 500 (<= 0.096) vs 700 (>= 0.110)
    let weight = if s0.stroke / em > 0.10 { 700 } else { 500 };
    let align = match (p.centred, p.rtl) {
        (true, _) => Align::Center,
        (false, true) => Align::Right,
        (false, false) => Align::Left,
    };
    let [mut bx, by, mut bw, bh] = p.bounds;
    // a short label may grow one em toward its line start (left for RTL)
    if !p.centred {
        bw += em;
        bx -= if p.rtl { em } else { 0.0 };
    }
    // same em as the original, shrink until it fits the block (wrapping at
    // the block width), floor at 40 %
    let mut size = em;
    let (buf, h) = loop {
        let lh = pitch.map_or(size * 1.3, |pt| pt.min(size * 1.6).max(size * 1.2));
        let buf = fonts.layout(
            &spans,
            p.rtl,
            Metrics::new(size, lh),
            weight,
            Some(bw),
            align,
        );
        let (w, h) = extent(&buf);
        if (w <= bw + 0.5 && h <= bh.max(lh) + 0.5) || size < em * 0.4 {
            break (buf, h);
        }
        size *= 0.95;
    };
    // the buffer is `bw` wide and aligned inside it; centre it vertically
    let top = by + (bh - h) / 2.0;
    (buf, (bx.round() as i32, top.round() as i32), size)
}

/// Draws the fitted translation (shadow or outline first, if the original had one).
pub fn draw_block(
    fonts: &mut Fonts,
    rgba: &mut [u8],
    dims: (usize, usize),
    text: &str,
    p: &Placement,
) {
    let (buf, (x, y), size) = fit(fonts, text, p);
    if let Some((c, offset)) = p.styles[0].shadow {
        let d = (size / 16.0).round().max(1.0) as i32;
        let offs: &[(i32, i32)] = if offset {
            &[(d, d)]
        } else {
            &[
                (-d, 0),
                (d, 0),
                (0, -d),
                (0, d),
                (-d, -d),
                (d, d),
                (-d, d),
                (d, -d),
            ]
        };
        let alpha = if offset { 200 } else { 255 };
        for (dx, dy) in offs {
            fonts.draw(
                &buf,
                rgba,
                dims,
                (x + dx, y + dy),
                Some([c[0], c[1], c[2], alpha]),
            );
        }
    }
    fonts.draw(&buf, rgba, dims, (x, y), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(ink: [i32; 4]) -> LineStyle {
        LineStyle {
            color: [240; 3],
            accent: Some([232, 190, 92]),
            shadow: None,
            ink,
            stroke: 3.0,
        }
    }

    /// Glyph pixels drawn into a blank image: their bounding box.
    fn ink_box(text: &str, p: &Placement, dims: (usize, usize)) -> [i32; 4] {
        let mut img = vec![0u8; dims.0 * dims.1 * 4];
        draw_block(&mut FONTS.lock().unwrap(), &mut img, dims, text, p);
        let mut e = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for (i, px) in img.chunks(4).enumerate() {
            if px[..3].iter().any(|&v| v > 0) {
                let (x, y) = ((i % dims.0) as i32, (i / dims.0) as i32);
                e = [e[0].min(x), e[1].min(y), e[2].max(x + 1), e[3].max(y + 1)];
            }
        }
        e
    }

    fn placement(bounds: [f32; 4], lines: usize, rtl: bool) -> Placement {
        let lh = bounds[3] / lines as f32;
        Placement {
            bounds,
            texts: vec!["Bring the Sunstone".into(); lines],
            styles: (0..lines)
                .map(|k| {
                    let y = (bounds[1] + lh * k as f32) as i32;
                    style([
                        bounds[0] as i32,
                        y + 4,
                        (bounds[0] + bounds[2]) as i32,
                        y + 30,
                    ])
                })
                .collect(),
            rtl,
            centred: false,
        }
    }

    #[test]
    fn rtl_is_right_aligned_to_the_block_and_fits_inside() {
        let p = placement([100.0, 50.0, 500.0, 40.0], 1, true);
        let e = ink_box("أحضر حجر الشمس", &p, (800, 200));
        assert!((e[2] - 600).abs() <= 3, "right edge {e:?}");
        assert!(e[0] > 300, "short text should hug the right edge: {e:?}");
        // a short label grows toward its start instead of shrinking to nothing
        let p = placement([500.0, 50.0, 30.0, 40.0], 1, true);
        let (_, _, size) = fit(&mut FONTS.lock().unwrap(), "مفعّلة", &p);
        assert!(size > 20.0, "short label kept readable: {size}");
        let p = placement([100.0, 50.0, 500.0, 40.0], 1, false);
        let e = ink_box("Bring it", &p, (800, 200));
        assert!(
            (e[0] - 100).abs() <= 4,
            "LTR starts at the left edge: {e:?}"
        );
    }

    #[test]
    fn long_text_shrinks_then_wraps_inside_the_block() {
        let long = "أحضر [[حجر الشمس]] إلى المعبد القديم قبل حلول الليل، ولا تبتعد عن الطريق \
                    فالغابة ليست آمنة بعد حلول الظلام أبدا";
        for (bounds, lines) in [
            ([100.0, 50.0, 400.0, 40.0], 1),
            ([100.0, 50.0, 500.0, 120.0], 3),
        ] {
            let p = placement(bounds, lines, true);
            let e = ink_box(long, &p, (800, 300));
            let (x1, y1) = (
                (bounds[0] + bounds[2]) as i32,
                (bounds[1] + bounds[3]) as i32,
            );
            // the block, plus one em (~28 px here) of growth at the RTL line start
            let x0 = bounds[0] as i32 - 28;
            assert!(
                e[0] >= x0 && e[1] >= bounds[1] as i32 && e[2] <= x1 && e[3] <= y1,
                "ink {e:?} outside block {bounds:?}"
            );
            let (buf, _, size) = fit(&mut FONTS.lock().unwrap(), long, &p);
            if lines == 3 {
                assert!(buf.layout_runs().count() > 1, "paragraph wraps");
                assert!(size > 12.0, "wrapping keeps it readable: {size}");
            }
        }
    }

    #[test]
    fn accent_markup_becomes_spans() {
        let (w, g) = ([1; 3], [2; 3]);
        assert_eq!(
            parse_marked("a [[b]] c", w, g),
            vec![("a ".into(), w), ("b".into(), g), (" c".into(), w)]
        );
        assert_eq!(parse_marked("[[x]]", w, g), vec![("x".into(), g)]);
        assert_eq!(
            parse_marked("a [[b", w, g),
            vec![("a ".into(), w), ("b".into(), g)]
        );
        assert_eq!(parse_marked("a]] b", w, g), vec![("a b".into(), w)]);
    }

    #[test]
    fn arabic_joins_and_mixed_line_runs_right_to_left() {
        let mut f = FONTS.lock().unwrap();
        let mut shape = |s: &str| {
            let b = f.layout(
                &[(s.into(), [255; 3])],
                true,
                Metrics::new(32.0, 48.0),
                500,
                None,
                Align::Right,
            );
            let r = b.layout_runs().next().unwrap();
            let g: Vec<(usize, u16, f32)> = r
                .glyphs
                .iter()
                .map(|g| (g.start, g.glyph_id, g.x))
                .collect();
            (r.rtl, g)
        };
        let (_, alone) = shape("ض");
        let (rtl, g) = shape("اضغط E للتفاعل");
        assert!(rtl, "paragraph direction");
        let at = |byte: usize| *g.iter().find(|g| g.0 == byte).unwrap();
        // joined: the dad inside a word uses a contextual form, not the isolated glyph
        assert_ne!(at("ا".len()).1, alone[0].1);
        // the Latin E comes from the Latin font (not .notdef)
        assert_ne!(at("اضغط ".len()).1, 0);
        // visual order: first word right of "E", last word left of it
        let (first, e, last) = (at(0).2, at("اضغط ".len()).2, at("اضغط E ".len()).2);
        assert!(first > e && e > last, "{first} {e} {last}");
    }
}
