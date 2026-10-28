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
/// (`MONO`: a Latin subset of JetBrains Mono, weights 400-600, plus ▴ ▾ ✓ →).
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
pub fn ui_font() -> Font {
    Font::with_name(ui_font_name())
}

/// The bundled Arabic face (`FONTS`), the fallback for Arabic in every font.
const ARABIC: &str = "Noto Sans Arabic";

/// Loads `FONTS` into the UI's font system (once; iced's own load of the same
/// bytes is then a no-op) and registers the Arabic face at every weight.
///
/// The text engine (cosmic-text) falls back for another script only to faces
/// of exactly the asked weight, and a variable font is registered once, at
/// its default (400). So Arabic in a 500 or 600 role (or in `MONO`) fell to
/// whatever face had some Arabic (DejaVu on Linux): unjoined, spaced letters.
/// The extra entries share the font's bytes; each is drawn at its weight
/// (the variable font's `wght` axis).
pub fn load() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        use iced::advanced::graphics::text::cosmic_text::fontdb;
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system");
        for f in FONTS {
            fonts.load_font(f.into());
        }
        let db = fonts.raw().db_mut();
        let arabic: Vec<fontdb::FaceInfo> = (db.faces())
            .filter(|f| f.families.iter().any(|(name, _)| name == ARABIC))
            .cloned()
            .collect();
        for face in arabic {
            for weight in (100..=900).step_by(100) {
                if weight != face.weight.0 {
                    db.push_face_info(fontdb::FaceInfo {
                        weight: fontdb::Weight(weight),
                        ..face.clone()
                    });
                }
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
            .unwrap_or("Inter")
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
    let mut fonts = iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system");
    let fs = fonts.raw();
    let mut buf = Buffer::new(fs, Metrics::new(size, size * 1.5));
    buf.set_size(fs, wrap, None);
    let attrs = Attrs::new().family(family).weight(W(weight));
    buf.set_text(fs, s, &attrs, Shaping::Advanced, None);
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

/// `s` in `role`: its size and font, in the theme's text colour (a section
/// head upper case and muted).
pub fn text<'a>(role: Role, s: impl IntoFragment<'a>) -> Text<'a> {
    let c = &themes::current().c;
    let (line, colour) = match role == Role::SECTION {
        true => (s.into_fragment().to_uppercase().into(), c.text_muted),
        false => (s.into_fragment(), c.text),
    };
    iced::widget::text(line)
        .size(role.size)
        .font(font(role))
        .color(colour)
}

/// The line height of `linked` text (times its size).
pub const LINKED_LINE: f32 = 1.65;

/// Text with links, in `role`: `parts` are (text, link); a link is the
/// accent, underlined, and reaches `on_link` only when clicked. `sub` = the
/// secondary text colour (a card's notes), else the main one.
pub fn linked<'a, Message: 'a>(
    parts: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    role: Role,
    sub: bool,
    on_link: impl Fn(String) -> Message + 'a,
) -> iced::Element<'a, Message> {
    use iced::widget::{rich_text, span};
    let c = &themes::current().c;
    let plain = if sub { c.text_sub } else { c.text };
    let spans: Vec<iced::widget::text::Span<'a, String>> = (parts.into_iter())
        .map(|(s, link)| match link {
            Some(url) => span(s)
                .color(c.accent)
                .underline(true)
                .link(url.to_string()),
            None => span(s).color(plain),
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
}
