//! Snip's More menu: Copy text, Ask AI, Translate on the picked area (design
//! `SNIP_MORE_SPEC.md`, 0.1.7). The pure decisions live here, the menu and the
//! App wiring elsewhere:
//! - which picture each row gets (Ask AI: the annotated one; Copy text and
//!   Translate: the clean capture under the drawing, with every Blur, Redact
//!   and Spotlight-dimmed part masked: a hidden secret never reaches the
//!   clipboard or the translator);
//! - the key caps (the user's own quick-action shortcut for the same action);
//! - what is disabled, and why.
//!
//! The work itself is the one main mode's bar runs (`actions::text`,
//! `actions::ask`, `actions::translate`): nothing is implemented twice.
use iced::Rectangle;

use super::Crop;
use crate::config::Config;
use crate::ocr::TextBlock;

/// The three rows, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    CopyText,
    AskAi,
    Translate,
}

impl Row {
    pub const ALL: [Row; 3] = [Row::CopyText, Row::AskAi, Row::Translate];

    /// The quick action whose shortcut the row shows (and answers while Snip
    /// has an area): `QuickId` names.
    pub fn quick(self) -> &'static str {
        match self {
            Row::CopyText => "copy_text",
            Row::AskAi => "ai_explain",
            Row::Translate => "translate",
        }
    }

    /// The row's name (Latin, Arabic); Translate reads "Show original" while
    /// the translation is on.
    pub fn label(self, translated: bool) -> (&'static str, &'static str) {
        match self {
            Row::CopyText => ("Copy text", "نسخ النص"),
            Row::AskAi => ("Ask AI", "اسأل الذكاء الاصطناعي"),
            Row::Translate if translated => ("Show original", "إظهار الأصل"),
            Row::Translate => ("Translate", "ترجمة"),
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Row::CopyText => super::text::ACTION.icon,
            Row::Translate => super::translate::ACTION.icon,
            // the OpenCircle logo is drawn instead (never sparkles)
            Row::AskAi => super::ask::ACTION.icon,
        }
    }
}

/// How reading the area's text stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Read {
    /// Not read yet (More has not opened for this area).
    #[default]
    Idle,
    /// Reading: the rows show the spinner where the key caps go.
    Reading,
    /// Read: there is text.
    Text,
    /// Read: none.
    NoText,
    /// The reader's models are not here (the download card asks on the click).
    NoModels,
}

/// What the host tells the menu.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Info {
    pub read: Read,
    /// One key cap per key of each row's shortcut (`Row::ALL` order); empty =
    /// no shortcut bound.
    pub caps: [Vec<String>; 3],
    /// Ask AI opens the browser (no AI key): the row's external-link mark.
    pub browser: bool,
    /// The translation is on the picture ("Show original").
    pub translated: bool,
}

/// Why a row is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    NoText,
    /// A stitched page is taller than the screen: nothing to draw in place.
    LongCapture,
}

impl Why {
    /// The menu foot's words (Latin, Arabic).
    pub fn words(self) -> (&'static str, &'static str) {
        match self {
            Why::NoText => ("No text found in this area", "لا يوجد نص في هذه المنطقة"),
            Why::LongCapture => (
                "Translate works on one screen at a time",
                "الترجمة تعمل على شاشة واحدة في كل مرة",
            ),
        }
    }
}

/// Why `row` cannot run now (`long`: a long capture is running). Copy text and
/// Ask AI first finish a long capture; Translate cannot. Showing the original
/// again always works.
pub fn why(row: Row, info: &Info, long: bool) -> Option<Why> {
    let no_text = info.read == Read::NoText;
    match row {
        Row::AskAi => None,
        Row::CopyText if no_text => Some(Why::NoText),
        Row::CopyText => None,
        Row::Translate if info.translated => None,
        Row::Translate if long => Some(Why::LongCapture),
        Row::Translate if no_text => Some(Why::NoText),
        Row::Translate => None,
    }
}

/// The menu foot: the first row's reason, else Ask AI's browser note.
pub fn foot(info: &Info, long: bool) -> Option<(&'static str, &'static str)> {
    if let Some(w) = Row::ALL.iter().find_map(|&r| why(r, info, long)) {
        return Some(w.words());
    }
    info.browser.then_some((
        "Opens in your browser with this picture",
        "يفتح في متصفحك مع هذه الصورة",
    ))
}

/// A shortcut ("Ctrl+Shift+X") as one cap per key; nothing for an unbound one.
pub fn caps(shortcut: &str) -> Vec<String> {
    (shortcut.split('+').map(str::trim))
        .filter(|k| !k.is_empty())
        .map(str::to_string)
        .collect()
}

/// Each row's caps from the user's own quick actions (never hard-coded).
pub fn caps_from(cfg: &Config) -> [Vec<String>; 3] {
    Row::ALL.map(|row| {
        let id = row.quick();
        (cfg.quick_actions.iter())
            // Translate's key is the one-shot one, not the live toggle
            .find(|q| q.action == id && !q.live)
            .map(|q| caps(&q.shortcut))
            .unwrap_or_default()
    })
}

/// The row a quick action's shortcut answers while Snip has a picked area.
pub fn row_of(qa: &crate::config::QuickAction) -> Option<Row> {
    if qa.live {
        return None;
    }
    Row::ALL.into_iter().find(|r| qa.action == r.quick())
}

/// The parts of the picture the user hid, in the picture's own px (the crop's):
/// text there must not leave the app.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hidden {
    /// Blur and Redact: these boxes.
    pub boxes: Vec<Rectangle>,
    /// Spotlights: everything outside the union of these boxes (none = no
    /// spotlight, nothing dimmed).
    pub lit: Vec<Rectangle>,
}

impl Hidden {
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.lit.is_empty()
    }

    /// The same parts moved by `by`.
    pub fn shifted(&self, by: iced::Vector) -> Hidden {
        let go = |r: &Rectangle| Rectangle::new(r.position() + by, r.size());
        Hidden {
            boxes: self.boxes.iter().map(go).collect(),
            lit: self.lit.iter().map(go).collect(),
        }
    }

    /// The pixel at (`x`, `y`) is hidden.
    pub fn covers(&self, x: f32, y: f32) -> bool {
        let inside =
            |r: &Rectangle| (r.x..r.x + r.width).contains(&x) && (r.y..r.y + r.height).contains(&y);
        self.boxes.iter().any(inside) || (!self.lit.is_empty() && !self.lit.iter().any(inside))
    }
}

/// `crop` with every hidden pixel painted out (white): what Copy text and
/// Translate read, so a blurred secret has nothing left to recognise.
pub fn mask(crop: &mut Crop, hidden: &Hidden) {
    if hidden.is_empty() {
        return;
    }
    let w = crop.width as usize;
    let (pixels, _) = crop.rgba.as_chunks_mut::<4>();
    for (i, px) in pixels.iter_mut().enumerate() {
        let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
        if hidden.covers(x, y) {
            *px = [255, 255, 255, 255];
        }
    }
}

/// After a translation was pasted into `dst` (RGBA, `width` px wide), the
/// hidden pixels of `area` (x0, y0, x1, y1) get `src`'s back: the paint-out
/// that kept them from the translator is not left on the picture.
pub fn keep_hidden(dst: &mut [u8], src: &[u8], width: usize, area: [usize; 4], hidden: &Hidden) {
    if hidden.is_empty() {
        return;
    }
    let [x0, y0, x1, y1] = area;
    for y in y0..y1 {
        for x in x0..x1 {
            if hidden.covers(x as f32 + 0.5, y as f32 + 0.5) {
                let at = (y * width + x) * 4;
                dst[at..at + 4].copy_from_slice(&src[at..at + 4]);
            }
        }
    }
}

/// The lines of `blocks` that are not under a hidden part (a second guard
/// after `mask`, which already painted the hidden pixels out: a line whose
/// middle is hidden is dropped; one only touching it keeps its readable part).
pub fn visible(blocks: Vec<TextBlock>, hidden: &Hidden) -> Vec<TextBlock> {
    blocks
        .into_iter()
        .filter(|b| {
            let [x, y, w, h] = b.bounds;
            !hidden.covers(x + w / 2.0, y + h / 2.0)
        })
        .collect()
}

/// The text of the lines (`text::join_lines`: reading order, rows joined).
pub fn text_of(blocks: &[TextBlock]) -> String {
    super::text::join_lines(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Size};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    fn block(text: &str, bounds: [f32; 4]) -> TextBlock {
        TextBlock {
            text: text.into(),
            bounds,
            confidence: 1.0,
        }
    }

    /// A blurred or redacted line never reaches the clipboard: its pixels are
    /// painted out before the read, and a line the reader still found there is
    /// dropped.
    #[test]
    fn hidden_text_never_leaves() {
        let hidden = Hidden {
            boxes: vec![rect(0.0, 10.0, 100.0, 10.0)],
            lit: vec![],
        };
        let mut crop = Crop {
            width: 100,
            height: 30,
            rgba: vec![0; 100 * 30 * 4],
        };
        mask(&mut crop, &hidden);
        let at = |x: usize, y: usize| crop.rgba[(y * 100 + x) * 4];
        assert_eq!((at(50, 15), at(50, 5), at(50, 25)), (255, 0, 0));
        let blocks = vec![
            block("public", [0.0, 0.0, 100.0, 9.0]),
            block("secret", [0.0, 10.0, 100.0, 10.0]),
            block("half secret", [0.0, 12.0, 100.0, 10.0]),
            // only touching the hidden part: what is readable of it stays
            block("next line", [0.0, 18.0, 100.0, 10.0]),
        ];
        let kept = visible(blocks, &hidden);
        assert_eq!(text_of(&kept), "public\nnext line");
    }

    /// Spotlight dims everything outside its boxes: that part is hidden too.
    #[test]
    fn what_a_spotlight_dims_is_hidden() {
        let hidden = Hidden {
            boxes: vec![],
            lit: vec![rect(0.0, 0.0, 50.0, 10.0), rect(60.0, 0.0, 40.0, 10.0)],
        };
        assert!(!hidden.covers(10.0, 5.0) && !hidden.covers(70.0, 5.0));
        assert!(hidden.covers(55.0, 5.0) && hidden.covers(10.0, 20.0));
        let kept = visible(
            vec![
                block("lit", [0.0, 0.0, 40.0, 8.0]),
                block("dim", [0.0, 20.0, 40.0, 8.0]),
            ],
            &hidden,
        );
        assert_eq!(kept.len(), 1);
        assert!(Hidden::default().is_empty() && !Hidden::default().covers(1.0, 1.0));
    }

    #[test]
    fn keys_come_from_the_users_own_quick_actions() {
        let cfg = Config::default();
        let k = caps_from(&cfg);
        assert_eq!(k[0], ["Ctrl", "Shift", "X"]);
        assert_eq!(k[1], ["Ctrl", "Shift", "E"]);
        // the one-shot translate key, not the live one (Ctrl+Shift+L)
        assert_eq!(k[2], ["Ctrl", "Shift", "T"]);
        // changed in Settings: the row follows; unbound: no caps
        let mut mine = Config::default();
        for q in &mut mine.quick_actions {
            match (q.action.as_str(), q.live) {
                ("copy_text", _) => q.shortcut = "Alt+Q".into(),
                ("ai_explain", _) => q.shortcut.clear(),
                _ => {}
            }
        }
        let k = caps_from(&mine);
        assert_eq!(
            (k[0].clone(), k[1].clone()),
            (vec!["Alt".into(), "Q".into()], vec![])
        );
        let live = mine.quick_actions.iter().find(|q| q.live).unwrap();
        assert_eq!(row_of(live), None, "the live toggle is not Translate's row");
        let copy = mine.quick_actions.iter().find(|q| q.action == "copy_text");
        assert_eq!(row_of(copy.unwrap()), Some(Row::CopyText));
    }

    #[test]
    fn disabled_rows_say_why() {
        let mut info = Info::default();
        for row in Row::ALL {
            assert_eq!(why(row, &info, false), None, "{row:?}: not read yet");
        }
        info.read = Read::NoText;
        assert_eq!(why(Row::CopyText, &info, false), Some(Why::NoText));
        assert_eq!(why(Row::Translate, &info, false), Some(Why::NoText));
        assert_eq!(why(Row::AskAi, &info, false), None, "Ask AI still works");
        assert_eq!(foot(&info, false), Some(Why::NoText.words()));
        // long capture: Copy text and Ask AI finish it first; Translate is off
        info.read = Read::Text;
        assert_eq!(why(Row::CopyText, &info, true), None);
        assert_eq!(why(Row::AskAi, &info, true), None);
        assert_eq!(why(Row::Translate, &info, true), Some(Why::LongCapture));
        assert_eq!(
            foot(&info, true).unwrap().0,
            "Translate works on one screen at a time"
        );
        // "Show original" always works
        info.translated = true;
        assert_eq!(why(Row::Translate, &info, true), None);
        assert_eq!(Row::Translate.label(true).0, "Show original");
        // no AI key: the browser note
        let browser = Info {
            browser: true,
            ..Info::default()
        };
        assert_eq!(
            foot(&browser, false).unwrap().0,
            "Opens in your browser with this picture"
        );
        assert_eq!(foot(&Info::default(), false), None);
    }
}
