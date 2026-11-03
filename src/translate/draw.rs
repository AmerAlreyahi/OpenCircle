//! Draw a translation in place of an erased block: measured colour, size
//! fitted to the original line height (shrink, then wrap inside the block),
//! right-aligned for RTL (centred text stays centred), shadow or outline only
//! if the original had one. Shaping, Arabic joining and bidi: cosmic-text.

use super::erase::LineStyle;
use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap,
    fontdb,
};

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

impl Fonts {
    /// A font system of only the bundled fonts (the system's are added on the
    /// first text that needs them). One per render, dropped with it: the
    /// fonts, the system face list and the glyph cache are not kept while
    /// idle (#145). Cheap: faces are parsed on use.
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        for f in FONT_DATA {
            db.load_font_data(f.to_vec());
        }
        // Each variable font registers once, at 400; cosmic-text matches the
        // exact weight first, so with the system's fonts loaded Arabic at 600
        // went to Segoe UI (Windows). Every weight names the bundled face (drawn
        // at it on the `wght` axis), as `ui::kit::text::load` does.
        for face in db.faces().cloned().collect::<Vec<_>>() {
            for weight in (100..=900).step_by(100) {
                if weight != face.weight.0 {
                    db.push_face_info(fontdb::FaceInfo {
                        weight: fontdb::Weight(weight),
                        ..face.clone()
                    });
                }
            }
        }
        Fonts {
            fs: FontSystem::new_with_locale_and_db("en-US".into(), db),
            cache: SwashCache::new(),
            system: false,
        }
    }
}

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

/// `s` cut into runs of Arabic-script text (true) and other letters, in
/// order. Spaces, digits and punctuation stay in the run they're in (both
/// fonts have them): moved to Inter in an Arabic line they lost their
/// right-to-left order and mirroring ("( مع" drawn as "مع(").
fn runs(s: &str) -> impl Iterator<Item = (&str, bool)> {
    use crate::translate::lang::Script;
    let kind = |c: char| match c {
        c if Script::Arabic.has(c) => Some(true),
        c if c.is_alphabetic() => Some(false),
        _ => None,
    };
    let mut rest = s;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let own = rest.chars().find_map(kind).unwrap_or(false);
        let end = (rest.char_indices())
            .find(|&(_, c)| kind(c).is_some_and(|k| k != own))
            .map_or(rest.len(), |(i, _)| i);
        let (run, tail) = rest.split_at(end);
        rest = tail;
        Some((run, own))
    })
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
        // Each letter names its own bundled font: Arabic in Noto, the rest in
        // Inter. Left to cosmic-text's fallback, Latin in an Arabic line went
        // to the first system face of the exact weight once the system's
        // fonts were loaded: on Windows the symbol font Marlett (a kept French
        // sentence drawn as rows of ticks and boxes).
        let rich = spans.iter().flat_map(|(t, c)| {
            let attrs = base.clone().color(Color::rgb(c[0], c[1], c[2]));
            runs(t).map(move |(r, arabic)| {
                (
                    r,
                    attrs
                        .clone()
                        .family(Family::Name(if arabic { ARABIC } else { LATIN })),
                )
            })
        });
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
        dims: (usize, usize),
        at: (i32, i32),
        over: Option<[u8; 4]>,
    ) {
        self.paint(buf, rgba, dims, at, &|_, c| match over {
            Some([r, g, b, a]) => [r, g, b, (c[3] as u32 * a as u32 / 255) as u8],
            None => c,
        });
    }

    /// `draw` with each glyph pixel's colour from `shade(y in the buffer,
    /// its colour)`.
    pub fn paint(
        &mut self,
        buf: &Buffer,
        rgba: &mut [u8],
        (iw, ih): (usize, usize),
        (x, y): (i32, i32),
        shade: &dyn Fn(i32, [u8; 4]) -> [u8; 4],
    ) {
        buf.draw(
            &mut self.fs,
            &mut self.cache,
            Color::rgb(255, 255, 255),
            |gx, gy, _, _, c| {
                let [r, g, b, a] = shade(gy, [c.r(), c.g(), c.b(), c.a()]);
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
/// box without the padding), not far under it (a misread ink:
/// 85 % of what the box says), nor
/// much above what its dense band says (the
/// x-height of mixed-case Latin, ~0.53 em): an icon or a stray mark in the
/// window cannot blow it up.
fn original_em(p: &Placement) -> f32 {
    let s0 = &p.styles[0];
    let n = p.styles.len() as f32;
    let text = &p.texts[0];
    // (capitals have no dense band: their bars make a few rows the
    // busiest, the round letters' middles count under half of those)
    let lower = text.chars().any(|c| c.is_ascii_lowercase());
    let band = if lower { s0.band / 0.53 } else { 0.0 };
    let ink = source_em(text, (s0.ink[3] - s0.ink[1]) as f32);
    // nor far under what the OCR box says: an ink the erase misread (a
    // sliver of an outlined subtitle) must not shrink a 60 px line to 8 px
    let ocr_h = p.line_h / 1.3;
    let from_box = source_em(text, ocr_h).min(ocr_h);
    // (the band under-reads big wide letters, a subtitle's: their x-height
    // rows hold only stems, under half the busiest row's ink; small UI text
    // keeps the tight cap, an icon beside it doubles the ink)
    let slack = if ocr_h >= 48.0 { 1.6 } else { 1.15 };
    let em = ink
        .min(if band > 0.0 { band * slack } else { ink })
        .min(ocr_h);
    // (a loose OCR box around small UI text is about twice its ink or more:
    // only big text, subtitles and captions, gets the floor)
    let em = if from_box >= 24.0 && em < 0.35 * from_box {
        0.85 * from_box
    } else {
        em
    };
    em.min(p.bounds[3] / n).max(6.0)
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

/// The box the fitted glyphs span (x, y, w, h): the runs' horizontal
/// extent, their line boxes vertically.
pub fn ink_rect((buf, (x, y), _): &Fit) -> [f32; 4] {
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for run in buf.layout_runs() {
        for g in run.glyphs {
            x0 = x0.min(g.x);
            x1 = x1.max(g.x + g.w);
        }
        y0 = y0.min(run.line_top);
        y1 = y1.max(run.line_top + run.line_height);
    }
    if x0 > x1 {
        return [0.0; 4];
    }
    [*x as f32 + x0, *y as f32 + y0, x1 - x0, y1 - y0]
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
    match p.styles[0].fade {
        // the fill fades over each line's cap height, as the original did
        Some((top, bottom)) => {
            let lines: Vec<(f32, f32)> =
                buf.layout_runs().map(|r| (r.line_top, r.line_y)).collect();
            fonts.paint(&buf, rgba, dims, (x, y), &|gy, c| {
                let gy = gy as f32;
                let base = (lines.iter().rev())
                    .find(|l| l.0 <= gy)
                    .or(lines.first())
                    .map_or(gy, |l| l.1);
                let t = ((gy - (base - 0.73 * size)) / (0.73 * size)).clamp(0.0, 1.0);
                let mix = |k: usize| (top[k] as f32 + (bottom[k] as f32 - top[k] as f32) * t) as u8;
                [mix(0), mix(1), mix(2), c[3]]
            });
        }
        None => fonts.draw(&buf, rgba, dims, (x, y), None),
    }
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
            fade: None,
            field: None,
            tail: None,
        }
    }

    /// Glyph pixels drawn into a blank image: their bounding box.
    fn ink_box(text: &str, p: &Placement, dims: (usize, usize)) -> [i32; 4] {
        let mut img = vec![0u8; dims.0 * dims.1 * 4];
        let mut fonts = Fonts::new();
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
        let mut fonts = Fonts::new();
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
        let (buf, _, size) = fit(&mut Fonts::new(), long, &p).unwrap();
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

    /// fr->ar on the Win10 VM: a French sentence kept in an Arabic paragraph
    /// was drawn in Marlett (ticks, boxes) once a "⁰" had loaded the
    /// system's fonts. Every letter now comes from a bundled font, at any
    /// weight, with the system's fonts loaded (this machine's weight-500
    /// faces took the Latin before).
    #[test]
    fn mixed_arabic_and_latin_never_fall_back_to_a_system_font() {
        let mut f = Fonts::new();
        let text = "النظام الشمسي ⁰ Il est composé de cette étoile (1,99 × 10³⁰ kg) هو النظام";
        for (rtl, weight) in [(true, 400), (true, 500), (true, 600), (false, 500)] {
            let b = f.layout(
                &[(text.into(), [255; 3])],
                rtl,
                Metrics::new(16.0, 22.0),
                weight,
                Some(400.0),
                Align::Right,
            );
            assert!(f.system, "the test loads the system's fonts");
            for run in b.layout_runs() {
                for g in run.glyphs {
                    let c = &run.text[g.start..g.end];
                    if !c.chars().all(bundled) {
                        continue;
                    }
                    let face = f.fs.db().face(g.font_id).expect("a face");
                    let family = &face.families[0].0;
                    assert!(
                        family == LATIN || family == ARABIC,
                        "{c:?} at weight {weight} drawn in {family}"
                    );
                    assert_ne!(g.glyph_id, 0, "{c:?} is .notdef");
                }
            }
        }
        let r: Vec<_> = runs("ب ج E (x) د.").collect();
        assert_eq!(r, [("ب ج ", true), ("E (x) ", false), ("د.", true)]);
    }

    #[test]
    fn arabic_joins_and_mixed_line_runs_right_to_left() {
        let mut f = Fonts::new();
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
