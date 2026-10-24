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
/// cap/ascender height (+ descender) of a Latin line; brackets reach both.
/// ponytail: fixed Latin ratios (Inter-like); other scripts get 0.8.
fn source_em(text: &str, ink_h: f32) -> f32 {
    if !text.chars().any(|c| c.is_ascii_alphanumeric()) {
        return ink_h / 0.8;
    }
    let tall = text
        .chars()
        .any(|c| c.is_uppercase() || c.is_ascii_digit() || "bdfhklt/%()[]{}|".contains(c));
    let desc = text.chars().any(|c| "gjpqy()[]{}|,;".contains(c));
    ink_h / (if tall { 0.73 } else { 0.53 } + if desc { 0.21 } else { 0.0 })
}

/// Where and how a block's translation goes.
pub struct Placement {
    /// The room it may use, x, y, w, h: the block's box (union of its OCR
    /// line boxes), widened into free space by `render`.
    pub bounds: [f32; 4],
    /// The first line's box height (padded OCR box): caps the em.
    pub line_h: f32,
    /// Original line texts (for the em estimate).
    pub texts: Vec<String>,
    pub styles: Vec<LineStyle>,
    pub rtl: bool,
    pub centred: bool,
}

/// The laid-out translation: buffer, top-left, font size.
pub type Fit = (Buffer, (i32, i32), f32);

/// Smallest redraw, relative to the original size: below it, text is not
/// legible as a replacement, so the original stays.
const MIN_SCALE: f32 = 0.7;

/// The original's size (em), from its ink, never above its OCR box (the line
/// box without the padding) nor much above what its dense band says (the
/// x-height of mixed-case Latin, ~0.53 em, else the cap height): an icon or
/// a stray mark in the window cannot blow it up.
fn original_em(p: &Placement) -> f32 {
    let s0 = &p.styles[0];
    let n = p.styles.len() as f32;
    let text = &p.texts[0];
    let band = s0.band
        / if text.chars().any(|c| c.is_ascii_lowercase()) {
            0.53
        } else {
            0.73
        };
    let ink = source_em(text, (s0.ink[3] - s0.ink[1]) as f32);
    ink.min(if band > 0.0 { band * 1.15 } else { ink })
        .min(p.line_h / 1.3)
        .min(p.bounds[3] / n)
        .max(6.0)
}

/// Font weight from stroke width per em (Inter: 400 about 0.08, 500 about
/// 0.095, 700 from 0.11), capped at semibold (600); a link (saturated, dark
/// text on a light page) is normal.
fn weight(s: &LineStyle, em: f32) -> u16 {
    let c = s.color;
    let sat = c.iter().max().unwrap() - c.iter().min().unwrap();
    let dark = (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32) < 140.0;
    match s.stroke / em {
        _ if sat > 60 && dark => 400,
        r if r > 0.115 => 600,
        r if r > 0.09 => 500,
        _ => 400,
    }
}

/// Lays out `text` to fit `p.bounds`: the original size at most, shrunk
/// (wrapping at the room's width, inside the block's height) down to
/// [`MIN_SCALE`] of it. `None` = it does not fit legibly.
pub fn fit(fonts: &mut Fonts, text: &str, p: &Placement) -> Option<Fit> {
    let s0 = &p.styles[0];
    // a paragraph's colour is most of its lines' (one line of links does
    // not turn it blue; on a tie, the first line's)
    let near = |a: [u8; 3], b: [u8; 3]| (0..3).all(|k| a[k].abs_diff(b[k]) <= 40);
    let color = (p.styles.iter().rev().map(|s| s.color))
        .max_by_key(|&c| p.styles.iter().filter(|s| near(s.color, c)).count())
        .unwrap_or(s0.color);
    let accent = p.styles.iter().find_map(|s| s.accent).unwrap_or(color);
    let spans = parse_marked(text, color, accent);
    let n = p.styles.len();
    let em = original_em(p);
    // pitch of the original lines (baseline to baseline), if it wrapped
    let pitch = (n > 1).then(|| (p.styles[n - 1].ink[3] - s0.ink[3]) as f32 / (n - 1) as f32);
    let weight = weight(s0, em);
    let align = match (p.centred, p.rtl) {
        (true, _) => Align::Center,
        (false, true) => Align::Right,
        (false, false) => Align::Left,
    };
    let [bx, by, bw, bh] = p.bounds;
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
        if w <= bw + 0.5 && h <= bh.max(lh) + 0.5 {
            break (buf, h);
        }
        size *= 0.95;
        if size < em * MIN_SCALE {
            return None;
        }
    };
    // the buffer is `bw` wide and aligned inside it; centre it vertically
    let top = by + (bh - h) / 2.0;
    Some((buf, (bx.round() as i32, top.round() as i32), size))
}

/// Draws the fitted translation (shadow or outline first, if the original had one).
pub fn draw_block(
    fonts: &mut Fonts,
    rgba: &mut [u8],
    dims: (usize, usize),
    p: &Placement,
    (buf, (x, y), size): Fit,
) {
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
            residual: 0.0,
            band: 0.0,
            text_x: [i32::MIN, i32::MAX],
        }
    }

    /// Glyph pixels drawn into a blank image: their bounding box.
    fn ink_box(text: &str, p: &Placement, dims: (usize, usize)) -> [i32; 4] {
        let mut img = vec![0u8; dims.0 * dims.1 * 4];
        let mut fonts = FONTS.lock().unwrap();
        let fit = fit(&mut fonts, text, p).expect("fits");
        draw_block(&mut fonts, &mut img, dims, p, fit);
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
            line_h: lh,
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
        let p = placement([100.0, 50.0, 500.0, 40.0], 1, false);
        let e = ink_box("Bring it", &p, (800, 200));
        assert!(
            (e[0] - 100).abs() <= 4,
            "LTR starts at the left edge: {e:?}"
        );
    }

    /// Never bigger than the original, never under 70 % of it: a translation
    /// that does not fit legibly is not drawn (`render` keeps the original).
    #[test]
    fn size_stays_between_70_and_100_percent_of_the_original() {
        let mut fonts = FONTS.lock().unwrap();
        // "Bring the Sunstone": ink 26 px with capitals and a descender = 27.7 px em
        let p = placement([100.0, 50.0, 900.0, 40.0], 1, true);
        let (_, _, size) = fit(&mut fonts, "خروج", &p).unwrap();
        assert!(
            (size - 27.7).abs() < 0.5,
            "a short word keeps the size: {size}"
        );
        // a stray mark made the ink 3x taller: the OCR box caps the em
        let mut tall = placement([100.0, 50.0, 900.0, 40.0], 1, true);
        tall.styles[0].ink[1] -= 52;
        let (_, _, size) = fit(&mut fonts, "خروج", &tall).unwrap();
        assert!(size <= 40.0 / 1.3 + 0.1, "capped: {size}");
        // too long for one line at 70 %: nothing
        let p = placement([500.0, 50.0, 30.0, 40.0], 1, true);
        assert!(fit(&mut fonts, "مفعّلة", &p).is_none());
        let long = "أحضر [[حجر الشمس]] إلى المعبد القديم قبل حلول الليل، ولا تبتعد عن الطريق \
                    فالغابة ليست آمنة بعد حلول الظلام أبدا";
        let p = placement([100.0, 50.0, 400.0, 40.0], 1, true);
        assert!(fit(&mut fonts, long, &p).is_none());
    }

    #[test]
    fn long_text_wraps_inside_the_block() {
        let long = "أحضر [[حجر الشمس]] إلى المعبد القديم قبل حلول الليل، ولا تبتعد عن الطريق \
                    فالغابة ليست آمنة بعد حلول الظلام أبدا";
        let bounds = [100.0, 50.0, 500.0, 120.0];
        let p = placement(bounds, 3, true);
        let e = ink_box(long, &p, (800, 300));
        let (x1, y1) = (
            (bounds[0] + bounds[2]) as i32,
            (bounds[1] + bounds[3]) as i32,
        );
        assert!(
            e[0] >= bounds[0] as i32 && e[1] >= bounds[1] as i32 && e[2] <= x1 && e[3] <= y1,
            "ink {e:?} outside block {bounds:?}"
        );
        let (buf, _, size) = fit(&mut FONTS.lock().unwrap(), long, &p).unwrap();
        assert!(buf.layout_runs().count() > 1, "paragraph wraps");
        assert!(
            size >= 27.7 * MIN_SCALE,
            "wrapping keeps it readable: {size}"
        );
    }

    /// Weight from stroke per em, capped at semibold; links are normal.
    #[test]
    fn weight_is_capped_and_links_are_normal() {
        let s = |stroke: f32, color: [u8; 3]| LineStyle {
            stroke,
            color,
            ..style([0, 0, 1, 1])
        };
        assert_eq!(weight(&s(2.0, [240; 3]), 25.0), 400);
        assert_eq!(weight(&s(2.5, [240; 3]), 25.0), 500);
        assert_eq!(
            weight(&s(6.0, [240; 3]), 25.0),
            600,
            "bold caps at semibold"
        );
        assert_eq!(weight(&s(6.0, [51, 102, 204]), 25.0), 400, "a link");
        assert_eq!(weight(&s(6.0, [232, 190, 92]), 25.0), 600, "a gold name");
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
