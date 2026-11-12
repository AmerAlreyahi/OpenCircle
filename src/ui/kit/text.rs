//! The UI's fonts and text styles: the design's font stack, its mono font,
//! and `text(Role, s)`, a line of text in a type role (`tokens::Role`) in the
//! theme's text colour.
use iced::Font;
use iced::font::Weight;
use iced::widget::text::{IntoFragment, Text};

use super::themes;
use super::tokens::Role;

/// Every font the UI registers at start (`ui::run`; the headless renders too):
/// the icons, Inter (the UI font where Segoe UI is missing), Noto Sans
/// Arabic (Arabic where the UI font has none) and the design's mono font
/// (`MONO`: a Latin subset of JetBrains Mono, weights 400-600, plus ▴ ▾ ✓ → Δ).
/// OFL licences in `assets/fonts/`.
pub const FONTS: [&[u8]; 4] = [
    super::icon::FONT_BYTES,
    include_bytes!("../../../assets/fonts/Inter.ttf"),
    include_bytes!("../../../assets/fonts/NotoSansArabic.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono.ttf"),
];

/// The design's `.mono` text (file names, versions, sizes, paths).
pub const MONO: Font = Font::with_name("JetBrains Mono");

/// The UI font, the design's stack: Segoe UI Variable Text, else Segoe UI
/// (Windows), else the bundled Inter. Picked once, from the fonts on this PC.
///
/// In a right-to-left view (`kit::dir`) it is always the bundled Inter: Arabic
/// then comes from the bundled Noto Sans Arabic on every OS, so Windows 10
/// (Segoe UI has Arabic of its own) looks as Windows 11 (Segoe UI Variable
/// has none) and as the goldens do (ARABIC_UI_SPEC.md decision 6).
pub fn ui_font() -> Font {
    Font::with_name(match super::dir::rtl() {
        true => BUNDLED,
        false => ui_font_name(),
    })
}

/// The bundled UI face (`FONTS`).
const BUNDLED: &str = "Inter";

/// The bundled Arabic face (`FONTS`), the fallback for Arabic in every font.
const ARABIC: &str = "Noto Sans Arabic";

/// The bundled variable faces (`wght` 400-700; mono 400-600).
const VARIABLE: [&str; 3] = [BUNDLED, ARABIC, "JetBrains Mono"];

/// Registers each `VARIABLE` face at every weight.
///
/// The text engine (cosmic-text) takes a named family's face only at exactly
/// the asked weight, else the first face of ANY family at that weight that
/// has the characters, and a variable font is registered once, at its
/// default (400). So a 500/600/700 Inter (labels, bold markdown, the Snip
/// text tool) was drawn by the system's face of that weight (Ubuntu Sans
/// SemiBold, DejaVu Sans Bold, Segoe UI Bold), and Arabic in a 500 role by
/// whatever face had some Arabic: unjoined, spaced letters. The extra entries
/// share the font's bytes; each is drawn at its weight (the `wght` axis).
/// A character none of them has still falls to the system's faces.
fn every_weight(db: &mut iced::advanced::graphics::text::cosmic_text::fontdb::Database) {
    use iced::advanced::graphics::text::cosmic_text::fontdb;
    let ours: Vec<fontdb::FaceInfo> = (db.faces())
        .filter(|f| matches!(f.source, fontdb::Source::Binary(_)))
        .filter(|f| {
            f.families
                .iter()
                .any(|(name, _)| VARIABLE.contains(&name.as_str()))
        })
        .cloned()
        .collect();
    for face in ours {
        for weight in (100..=900).step_by(100) {
            if weight != face.weight.0 {
                db.push_face_info(fontdb::FaceInfo {
                    weight: fontdb::Weight(weight),
                    ..face.clone()
                });
            }
        }
    }
}

/// Loads `FONTS` into the UI's font system (once; iced's own load of the same
/// bytes is then a no-op) and registers the bundled faces at every weight
/// (`every_weight`).
pub fn load() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        #[cfg(all(test, not(windows)))]
        use iced::advanced::graphics::text::cosmic_text::fontdb;
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system");
        for f in FONTS {
            fonts.load_font(f.into());
        }
        let db = fonts.raw().db_mut();
        every_weight(db);
        // Tests draw with the bundled faces ONLY (goldens must not depend on
        // the machine's fonts: a character they lack is the same "missing
        // glyph" box on every machine, not whatever face this one falls back
        // to). Windows keeps its own fonts: its goldens are not compared
        // (`golden::enabled`) and the UI font there is Segoe UI.
        #[cfg(all(test, not(windows)))]
        {
            let system: Vec<_> = (db.faces())
                .filter(|f| !matches!(f.source, fontdb::Source::Binary(_)))
                .map(|f| f.id)
                .collect();
            for id in system {
                db.remove_face(id);
            }
        }
    });
}

pub fn ui_font_name() -> &'static str {
    static FONT: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        load();
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system");
        let db = fonts.raw().db();
        let found = |name: &&str| {
            db.faces()
                .any(|f| f.families.iter().any(|(family, _)| family == name))
        };
        ["Segoe UI Variable Text", "Segoe UI"]
            .into_iter()
            .find(found)
            .unwrap_or(BUNDLED)
    })
}

/// The UI font at a weight (the design's 500 / 600).
pub fn weight(weight: Weight) -> Font {
    Font {
        weight,
        ..ui_font()
    }
}

/// The mono font at a weight.
pub fn mono(weight: Weight) -> Font {
    Font { weight, ..MONO }
}

/// How wide `s` is on one line at `size` px in `font`.
pub fn width(s: &str, size: f32, font: Font) -> f32 {
    laid_out(s, size, font, None).1
}

/// `s` cut with "…" to fit `room` px on one line at `size` px in `font` (the
/// design's `truncate`); as it is when it fits.
pub fn cut(s: &str, size: f32, font: Font, room: f32) -> String {
    if width(s, size, font) <= room {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let fits = |n: usize| {
        let t: String = chars[..n].iter().collect();
        width(&format!("{}…", t.trim_end()), size, font) <= room
    };
    // the longest start that fits, by halving (shaping is the cost)
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid) { lo = mid } else { hi = mid - 1 }
    }
    let t: String = chars[..lo].iter().collect();
    format!("{}…", t.trim_end())
}

/// `s` cut in the middle with "…" to fit `room` px (an id keeps its end:
/// ":free", a version); as it is when it fits.
pub fn cut_middle(s: &str, size: f32, font: Font, room: f32) -> String {
    if width(s, size, font) <= room {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let with = |k: usize| {
        let tail = k / 3;
        let head: String = chars[..k - tail].iter().collect();
        let tail: String = chars[chars.len() - tail..].iter().collect();
        format!("{}…{}", head.trim_end(), tail.trim_start())
    };
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&with(mid), size, font) <= room {
            lo = mid
        } else {
            hi = mid - 1
        }
    }
    with(lo)
}

/// The first of `texts` (longest first) that fits `room` px on one line at
/// `size` in the UI font, else the last: a placeholder is never cut off.
pub fn fitting<'a>(texts: &[&'a str], size: f32, room: f32) -> &'a str {
    let fits = |t: &&str| width(t, size, ui_font()) <= room;
    texts
        .iter()
        .copied()
        .find(fits)
        .unwrap_or(texts[texts.len() - 1])
}

/// How many lines `s` takes at `size` px in `font`, wrapped at `wrap` px.
pub fn lines(s: &str, size: f32, font: Font, wrap: f32) -> usize {
    laid_out(s, size, font, Some(wrap)).0
}

/// `s` laid out (wrapped at `wrap`, else on one line): its line count and widest line.
fn laid_out(s: &str, size: f32, font: Font, wrap: Option<f32>) -> (usize, f32) {
    use iced::advanced::graphics::text::cosmic_text::{
        Attrs, Buffer, Family, Metrics, Shaping, Weight as W,
    };
    let ui = ui_font_name(); // (loads the fonts, takes the same lock: before ours)
    let family = match font.family {
        iced::font::Family::Name(name) => Family::Name(name),
        _ => Family::Name(ui),
    };
    let weight = match font.weight {
        Weight::Thin => 100,
        Weight::ExtraLight => 200,
        Weight::Light => 300,
        Weight::Normal => 400,
        Weight::Medium => 500,
        Weight::Semibold => 600,
        Weight::Bold => 700,
        Weight::ExtraBold => 800,
        Weight::Black => 900,
    };
    // measured as drawn (`text`): with the UI's base direction mark
    let mono = font.family == MONO.family;
    let based = based(s.into(), mono);
    let s = based.as_ref();
    let mut fonts = iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system");
    let fs = fonts.raw();
    let mut buf = Buffer::new(fs, Metrics::new(size, size * 1.5));
    buf.set_size(fs, wrap, None);
    let attrs = Attrs::new().family(family).weight(W(weight));
    // shaped as the text widget shapes it (iced's `Shaping::Auto`): ASCII
    // unkerned, anything else advanced; kerned Latin measured narrower than
    // drawn, and a pill's label wrapped
    let shaping = match s.is_ascii() {
        true => Shaping::Basic,
        false => Shaping::Advanced,
    };
    buf.set_text(fs, s, &attrs, shaping, None);
    buf.shape_until_scroll(fs, false);
    (buf.layout_runs()).fold((0, 0.0), |(n, w), r| (n + 1, f32::max(w, r.line_w)))
}

/// The font of `role`.
pub fn font(role: Role) -> Font {
    match role.mono {
        true => mono(role.weight),
        false => weight(role.weight),
    }
}

/// A RIGHT-TO-LEFT MARK: the first strong character of a run, so the text
/// engine gives it the right-to-left base direction (a paragraph takes the
/// direction of its first strong letter: a Latin-first Arabic sentence was an
/// LTR paragraph, laid out and aligned from the left). Zero width.
const RLM: char = '\u{200F}';

/// `s` with the UI's base direction when the view is right to left (`RLM`);
/// as it is left to right, in the mono font (key caps, paths, versions stay
/// Latin), when empty and when it starts with a direction mark already (every
/// string-table Arabic string does).
fn based<'a>(s: std::borrow::Cow<'a, str>, mono: bool) -> std::borrow::Cow<'a, str> {
    match super::dir::rtl() && !mono && !s.is_empty() && !s.starts_with(RLM) {
        true => format!("{RLM}{s}").into(),
        false => s,
    }
}

/// `s` in `role`: its size and font, in the theme's text colour (a section
/// head upper case and muted). In a right-to-left UI the run's base
/// direction is the UI's, whatever its first letter is (`based`); text with
/// a direction of its own is `own`.
pub fn text<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    run(role, s, true)
}

/// `s` in `role` with the direction of its OWN content (the first strong
/// letter): a chat message, a provider's message as received.
pub fn own<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    run(role, s, false)
}

fn run<'a>(role: Role, s: impl IntoFragment<'a>, base: bool) -> Text<'a> {
    let c = &themes::current().c;
    let (line, colour) = match role == Role::SECTION {
        true => (s.into_fragment().to_uppercase().into(), c.text_muted),
        false => (s.into_fragment(), c.text),
    };
    let line = match base {
        true => based(line, role.mono),
        false => line,
    };
    iced::widget::text(line)
        .size(size(role))
        .font(font(role))
        .color(colour)
}

/// The size `role` is drawn at: 1 px larger in Arabic (not the mono roles:
/// paths, versions and key names stay Latin).
pub fn size(role: Role) -> f32 {
    match role.mono {
        true => role.size,
        false => super::dir::size(role.size),
    }
}

/// `s` in `role`, in the secondary text colour (a description, a note).
pub fn sub<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    text(role, s).color(themes::current().c.text_sub)
}

/// A wrapped paragraph in a box as wide as its room (a card's lines): right
/// to left it fills the room and starts at its right edge, where a column
/// that is not start-aligned would leave it at the left. Left to right it is
/// as it was. (The lines themselves wrap inside the room: a line with a Latin
/// name in Arabic was laid out wider than its room by the text engine until
/// its vendored fix, `vendor/cosmic-text`.)
pub fn paragraph(line: Text<'_>) -> Text<'_> {
    match super::dir::rtl() {
        true => line
            .width(iced::Fill)
            .align_x(iced::widget::text::Alignment::Right),
        false => line,
    }
}

/// `s` in `role`, in the muted text colour (a footer's aside).
pub fn muted<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    text(role, s).color(themes::current().c.text_muted)
}

/// `s` in `role`, in the theme's accent (a handle, a link-like name).
pub fn accent<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    text(role, s).color(themes::current().c.accent)
}

/// `s` in `role`, in `DANGER` (what is wrong with a value).
pub fn error<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    text(role, s).color(super::tokens::fixed::DANGER)
}

/// A line of text is this many times its font size tall wherever its box
/// clips it (text fields): Arabic with every vowel mark stays inside
/// (Noto Sans Arabic inks about 1.6 times its size).
pub const LINE: f32 = 1.7;

/// The line height of `styled` text (times its size).
pub const LINKED_LINE: f32 = 1.65;

/// How a run of `styled` text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// The secondary text colour.
    Plain,
    /// The main text colour, 600 (a bold label).
    Strong,
    /// A key or name (inline code): mono on `hover`, radius 4.
    Key,
}

/// Notes text (the What's new card): runs of `Ink`, links in the accent that
/// reach `on_link` only when clicked, in `role`, wrapped.
pub fn styled<'a, Message: 'a>(
    parts: impl IntoIterator<Item = (&'a str, Ink, Option<&'a str>)>,
    role: Role,
    on_link: impl Fn(String) -> Message + 'a,
) -> iced::Element<'a, Message> {
    use iced::widget::{rich_text, span};
    let c = &themes::current().c;
    let spans: Vec<iced::widget::text::Span<'a, String>> = (parts.into_iter())
        .map(|(s, ink, link)| {
            let run = match ink {
                Ink::Plain => span(s).color(c.text_sub),
                Ink::Strong => span(s).color(c.text).font(weight(Weight::Semibold)),
                Ink::Key => span(s)
                    .color(c.text)
                    .font(mono(Weight::Medium))
                    .background(c.hover)
                    .border(iced::Border::default().rounded(4.0))
                    .padding([0.0, 3.0]),
            };
            match link {
                Some(url) => run.color(c.accent).underline(true).link(url.to_string()),
                None => run,
            }
        })
        .collect();
    rich_text(spans)
        .size(role.size)
        .font(font(role))
        .line_height(LINKED_LINE)
        .on_link_click(on_link)
        .into()
}

#[cfg(test)]
mod tests {
    use iced::advanced::graphics::text::cosmic_text::{
        Attrs, Buffer, Family, Metrics, Shaping, Weight as W,
    };

    /// The ink of `sample` in `family` at `size` px laid out in one `LINE` line:
    /// (top, bottom) in px from the line's top.
    fn ink(sample: &str, family: &str, size: f32) -> (f32, f32) {
        use iced::advanced::graphics::text::cosmic_text::{FontSystem, SwashCache, fontdb};
        let mut db = fontdb::Database::new();
        for f in super::FONTS {
            db.load_font_data(f.to_vec());
        }
        let mut fs = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let mut buf = Buffer::new(&mut fs, Metrics::new(size, size * super::LINE));
        let attrs = Attrs::new().family(Family::Name(family));
        buf.set_text(&mut fs, sample, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(&mut fs, false);
        let mut cache = SwashCache::new();
        let (mut top, mut bottom) = (f32::MAX, f32::MIN);
        for run in buf.layout_runs() {
            for glyph in run.glyphs {
                let p = glyph.physical((0.0, 0.0), 1.0);
                let Some(img) = cache.get_image(&mut fs, p.cache_key) else {
                    continue;
                };
                if img.placement.height == 0 {
                    continue;
                }
                let y = run.line_y + p.y as f32 - img.placement.top as f32;
                top = top.min(y);
                bottom = bottom.max(y + img.placement.height as f32);
            }
        }
        (top, bottom)
    }

    /// Windows' own fonts are in the font system: the UI font is Segoe UI, as
    /// on every Windows PC (a test that planted SystemRoot left them out, and
    /// every screen was drawn without them).
    #[cfg(windows)]
    #[test]
    fn the_ui_font_on_windows_is_segoe_ui() {
        assert_ne!(super::ui_font_name(), "Inter", "Windows' fonts not loaded");
    }

    /// A field's line holds the tallest Latin and Arabic text (accents, Arabic
    /// marks above and below): a text field clips to its line (#53).
    #[test]
    fn a_line_holds_latin_and_arabic_marks() {
        let line = 14.0 * super::LINE;
        for (sample, family) in [
            ("ÅÉÎÕÛ jgpqy ÇŞ", "Inter"),
            ("إِنَّ الْعِلْمَ نُورٌ ضِّ", "Noto Sans Arabic"),
        ] {
            let (top, bottom) = ink(sample, family, 14.0);
            assert!(
                top >= 0.0 && bottom <= line,
                "{family}: ink {top}..{bottom} outside the {line} px line"
            );
        }
    }

    /// Arabic is drawn by the bundled Arabic face at every weight a role uses,
    /// in the UI font and in `MONO` (it fell to another face at 500 and in
    /// mono: unjoined, spaced letters).
    #[test]
    fn arabic_comes_from_the_arabic_face_at_every_weight() {
        const WORDS: &str = "بحث في المتصفح";
        let ui = super::ui_font_name();
        let mut wrong = Vec::new();
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .unwrap();
        let fs = fonts.raw();
        for family in [ui, "JetBrains Mono"] {
            for weight in [400, 500, 600, 700] {
                let mut buf = Buffer::new(fs, Metrics::new(12.0, 18.0));
                let attrs = Attrs::new().family(Family::Name(family)).weight(W(weight));
                buf.set_text(fs, WORDS, &attrs, Shaping::Advanced, None);
                buf.shape_until_scroll(fs, false);
                let glyphs: Vec<_> = (buf.layout_runs())
                    .flat_map(|r| r.glyphs.iter())
                    .filter(|g| !WORDS[g.start..g.end].trim().is_empty())
                    .map(|g| g.font_id)
                    .collect();
                assert!(!glyphs.is_empty());
                for id in glyphs {
                    let name = fs.db().face(id).unwrap().families[0].0.clone();
                    // Windows' Segoe UI has its own shaped Arabic; the bug was a
                    // fallback without Arabic shaping (DejaVu on Linux).
                    if name != super::ARABIC && name != "Segoe UI" {
                        wrong.push(format!("{family} at {weight}: {name}"));
                    }
                }
            }
        }
        drop(fonts);
        wrong.dedup();
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    /// The mono readouts' signs (the ruler's `·` and `Δ`) are in the bundled
    /// subset: a sign it lacks falls to whatever face the machine has, and is
    /// drawn another width there (CI's goldens differed from a dev machine's).
    #[test]
    fn mono_text_signs_come_from_the_mono_face() {
        const SIGNS: &str = "500 px · Δx 300 · Δy 400 · 333 at 150 % ▴▾✓→";
        super::load();
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .unwrap();
        let fs = fonts.raw();
        let mut buf = Buffer::new(fs, Metrics::new(12.0, 18.0));
        let attrs = Attrs::new().family(Family::Name("JetBrains Mono"));
        buf.set_text(fs, SIGNS, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(fs, false);
        let other: Vec<_> = (buf.layout_runs())
            .flat_map(|r| r.glyphs.iter())
            .filter(|g| fs.db().face(g.font_id).unwrap().families[0].0 != "JetBrains Mono")
            .map(|g| &SIGNS[g.start..g.end])
            .collect();
        drop(fonts);
        assert!(other.is_empty(), "not in the mono face: {other:?}");
    }

    /// A 500/600/700 Inter is drawn by Inter, even where the machine has
    /// another face of exactly that weight (Linux's DejaVu Sans Bold, Ubuntu
    /// Sans SemiBold): bold markdown, the Snip text tool and the hover chip's
    /// title were drawn by the system's face (or by none at all).
    #[test]
    fn every_weight_is_drawn_by_the_bundled_face_beside_a_system_face() {
        use iced::advanced::graphics::text::cosmic_text::{FontSystem, fontdb};
        let mut db = fontdb::Database::new();
        for f in super::FONTS {
            db.load_font_data(f.to_vec());
        }
        // a "system" Latin face at each weight a role uses, named as one the
        // engine's script fallback asks for first (Inter's bytes: its glyphs
        // cover the same text)
        let inter = (db.faces())
            .find(|f| f.families[0].0 == "Inter")
            .unwrap()
            .clone();
        for weight in [500, 600, 700] {
            db.push_face_info(fontdb::FaceInfo {
                families: vec![("DejaVu Sans".into(), fontdb::Language::English_UnitedStates)],
                weight: fontdb::Weight(weight),
                ..inter.clone()
            });
        }
        super::every_weight(&mut db);
        let mut fs = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let mut wrong = Vec::new();
        for (family, weight) in [("Inter", 500), ("Inter", 600), ("Inter", 700)] {
            let mut buf = Buffer::new(&mut fs, Metrics::new(12.0, 18.0));
            let attrs = Attrs::new().family(Family::Name(family)).weight(W(weight));
            buf.set_text(&mut fs, "Key Action", &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(&mut fs, false);
            for g in buf.layout_runs().flat_map(|r| r.glyphs.iter()) {
                let face = fs.db().face(g.font_id).unwrap().families[0].0.clone();
                if face != family || g.font_weight.0 != weight {
                    wrong.push(format!("{family} {weight}: {face} {}", g.font_weight.0));
                }
            }
        }
        wrong.dedup();
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    /// In a right-to-left UI a run takes the UI's base direction whatever its
    /// first letter is (a Latin-first Arabic sentence, a Latin-only label), the
    /// Latin words inside stay in order; the exceptions stay as they are.
    #[test]
    fn a_right_to_left_ui_gives_every_run_its_base_direction() {
        use super::{RLM, based, own, text, width};
        use crate::ui::kit::testing::shot;
        use crate::ui::kit::tokens::Role;
        use crate::ui::kit::{dir, text::ui_font};
        let size = iced::Size::new(400.0, 200.0);
        let sentence = "Google Lens \u{623}\u{648} \u{628}\u{62d}\u{62b} Google";
        let off = |view: iced::Element<'static, ()>, rtl| shot::off_base_direction(view, size, rtl);
        {
            let _ar = dir::scope(true);
            // Latin-first sentence, a Latin-only label: right-to-left paragraphs
            assert!(off(text(Role::NOTE, sentence).into(), true).is_empty());
            assert!(off(text(Role::CONTROL, "Latin Label").into(), true).is_empty());
            // the Latin run keeps its order: its words are not reversed
            let marked = based(sentence.into(), false);
            assert!(marked.trim_start_matches(RLM).starts_with("Google Lens"));
            // exceptions: mono (key caps, paths), a mark already there, own direction, empty
            assert_eq!(based("Ctrl+C".into(), true), "Ctrl+C");
            assert_eq!(based("\u{200F}abc".into(), false), "\u{200F}abc");
            assert_eq!(based("".into(), false), "");
            assert!(off(text(Role::KEYCAP, "Ctrl+C").into(), true).is_empty());
            assert_eq!(
                off(own(Role::NOTE, "Hello world").into(), true),
                ["Hello world"]
            );
            // measured as drawn
            assert!(width("Latin Label", 13.0, ui_font()) > 0.0);
        }
        // left to right: untouched
        assert_eq!(based("Latin".into(), false), "Latin");
        assert!(off(text(Role::NOTE, sentence).into(), false).is_empty());
    }
}
