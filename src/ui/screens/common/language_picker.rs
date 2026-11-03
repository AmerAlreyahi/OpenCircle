//! The compact language picker (source → target) of live translate's
//! controller and of the one-shot translation's pill: two chips; a press on
//! one opens that side's list under them, recently picked first, with a
//! search field for the rest. From kit parts only (`kit::option_list`: the
//! openers and the rows; the tray menu reuses it). Picks last for the session
//! (and are remembered as recent); Settings › Translate keeps the defaults.
use std::cell::RefCell;

use iced::widget::{Space, column, container, row};
use iced::{Center, Element, Fill, Length};

use crate::mt;
use crate::translate::lang::{self, AUTO, LANGUAGES};
use crate::ui::kit::field;
use crate::ui::kit::option_list::{opener, option};
use crate::ui::kit::text::sub;
use crate::ui::kit::tokens::Role;

/// Chip height and the gaps (the controller's sizes).
pub const CHIP_HEIGHT: f32 = 32.0;
pub const SPACING: f32 = 4.0;
/// One chip per side (one width for both, consistency rule).
pub const SIDE_WIDTH: f32 = 112.0;
/// Width of the closed picker: two chips and the arrow between them.
pub const WIDTH: f32 = 2.0 * SIDE_WIDTH + ARROW + 2.0 * SPACING;
const ARROW: f32 = 16.0;
/// The search field's height (`kit::field::input`: 12 px on a 1.5 line, 6 px
/// in, its edge).
const SEARCH_H: f32 = 32.0;
/// Choices the open list shows (two columns), after filtering.
const SHOWN: usize = 8;
const COLUMNS: usize = 2;
/// One line of the note explaining the "AI" badge (`AI_ONLY`: 12 px on a
/// 1.5 line), and the list width it surely fits on one line in (Inter: 487 px
/// of ink, test `the_ai_note_fits_inside_the_list`; Segoe UI runs wider);
/// narrower (the translate pill, the live controller) it wraps to two.
const NOTE_LINE: f32 = 18.0;
const NOTE_ONE_LINE: f32 = 520.0;
/// What the open list adds under the chips: the search field, the rows and
/// a gap; offline also the badge's note (`Picker::list_height`).
pub const LIST_HEIGHT: f32 =
    SPACING + SEARCH_H + (SHOWN / COLUMNS) as f32 * (CHIP_HEIGHT + SPACING) + SPACING;
/// The search field's widget id.
const SEARCH: &str = "language-search";

thread_local! {
    /// Picked in this run, most recent first, per side (memory only; the
    /// pickers live on the UI thread).
    static RECENT: RefCell<[Vec<String>; 2]> = RefCell::default();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Source,
    Target,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// A side's chip: open its list (or close it if it is open).
    Open(Side),
    Query(String),
    /// Enter in the search field: the first match.
    First,
    Pick(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    /// "auto" or a code.
    pub source: String,
    pub target: String,
    /// Translation will be offline: choices with no offline model say "AI".
    offline: bool,
    open: Option<Side>,
    query: String,
}

impl Picker {
    pub fn new(source: &str, target: &str, offline: bool) -> Picker {
        Picker {
            source: source.into(),
            target: target.into(),
            offline,
            open: None,
            query: String::new(),
        }
    }

    /// What the open list adds under the chips: offline with room for the
    /// badge's note (kept while it is empty: the list never jumps while
    /// typing); with AI nothing is marked.
    pub fn list_height(&self, width: f32) -> f32 {
        LIST_HEIGHT + if self.offline { note_h(width) } else { 0.0 }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
        self.query.clear();
    }

    /// True when the source or the target changed.
    pub fn update(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Open(side) if self.open == Some(side) => self.close(),
            Msg::Open(side) => {
                self.open = Some(side);
                self.query.clear();
            }
            Msg::Query(q) => self.query = q,
            Msg::First => {
                if let Some((code, _)) = self.choices().first() {
                    return self.update(Msg::Pick(code.clone()));
                }
            }
            Msg::Pick(code) => {
                let Some(side) = self.open else {
                    return false;
                };
                remember(side, &code);
                self.close();
                let slot = match side {
                    Side::Source => &mut self.source,
                    Side::Target => &mut self.target,
                };
                let changed = *slot != code;
                *slot = code;
                return changed;
            }
        }
        false
    }

    /// "Auto → AR", for status lines.
    #[cfg(test)]
    pub fn label(&self) -> String {
        let code = |c: &str| match c {
            AUTO => "Auto".to_string(),
            c => c.to_uppercase(),
        };
        format!("{} → {}", code(&self.source), code(&self.target))
    }

    /// The open side's choices, (code, label): recently picked first, then
    /// the table's order; never the current one; matching the search.
    pub fn choices(&self) -> Vec<(String, String)> {
        let Some(side) = self.open else {
            return Vec::new();
        };
        let (current, all): (&str, Vec<&str>) = match side {
            Side::Source => {
                let readable = LANGUAGES.iter().filter(|l| l.ocr.is_some());
                (
                    &self.source,
                    [AUTO].into_iter().chain(readable.map(|l| l.code)).collect(),
                )
            }
            Side::Target => (&self.target, LANGUAGES.iter().map(|l| l.code).collect()),
        };
        let recent = RECENT.with_borrow(|r| r[side as usize].clone());
        let q = self.query.trim().to_lowercase();
        let matches = |code: &str| {
            let l = lang::find(code);
            q.is_empty()
                || code.to_lowercase().starts_with(&q)
                || lang::name(code).to_lowercase().contains(&q)
                || l.is_some_and(|l| l.native.to_lowercase().contains(&q))
        };
        let mut out: Vec<String> = Vec::new();
        for code in (recent.iter().map(String::as_str)).chain(all.iter().copied()) {
            let known = all.contains(&code);
            if known && code != current && matches(code) && !out.iter().any(|c| c == code) {
                out.push(code.to_string());
            }
        }
        out.truncate(SHOWN);
        out.into_iter()
            .map(|code| {
                let label = self.choice_label(side, &code);
                (code, label)
            })
            .collect()
    }

    /// The language's name; " · AI" when offline translation can't do it
    /// (it would need AI: the user is told before anything is sent).
    fn choice_label(&self, side: Side, code: &str) -> String {
        let name = match code {
            AUTO => "Detect language",
            c => lang::name(c),
        };
        let (source, target) = match side {
            Side::Source => (code, self.target.as_str()),
            Side::Target => (self.source.as_str(), code),
        };
        match self.offline && !mt::available(source, target) {
            true => format!("{name} · AI"),
            false => name.to_string(),
        }
    }

    /// The two openers: source → target (the target in the accent).
    /// `height` = their height.
    pub fn chips(&self, height: f32) -> Element<'_, Msg> {
        let chip = |side: Side, code: &str| {
            // "Chinese (Simplified)" -> "Chinese": one line in the chip (the
            // list and the status line tell variants apart)
            let label = match code {
                AUTO => "Detect",
                c => lang::name(c).split(" (").next().unwrap_or_default(),
            }
            .to_string();
            let open = Msg::Open(side);
            opener(label, side == Side::Target, SIDE_WIDTH, height, open)
        };
        row![
            chip(Side::Source, &self.source),
            container(sub(Role::NOTE, "→")).center_x(ARROW),
            chip(Side::Target, &self.target),
        ]
        .spacing(SPACING)
        .align_y(Center)
        .into()
    }

    /// The open list: the search field, then the choices in two columns,
    /// `width` wide. Nothing when closed.
    pub fn list(&self, width: f32) -> Option<Element<'_, Msg>> {
        self.open?;
        let search = field::input("Search languages", &self.query, false, false)
            .id(SEARCH)
            .on_input(Msg::Query)
            .on_submit(Msg::First)
            .width(Fill);
        let cell = (width - SPACING * (COLUMNS - 1) as f32) / COLUMNS as f32;
        let choices = self.choices();
        let mut rows = column![].spacing(SPACING);
        for pair in choices.chunks(COLUMNS) {
            let cells = pair.iter().map(|(code, label)| {
                // "German · AI": the name, and a badge for what needs AI
                let (name, badge) = match label.strip_suffix(" · AI") {
                    Some(name) => (name, Some("AI")),
                    None => (label.as_str(), None),
                };
                let pick = Msg::Pick(code.clone());
                option(name.to_string(), badge, false, (cell, CHIP_HEIGHT), pick)
            });
            rows = rows.push(row(cells).spacing(SPACING));
        }
        if choices.is_empty() {
            let none = sub(Role::NOTE, "No language matches");
            rows = rows.push(container(none).center_y(CHIP_HEIGHT));
        }
        // what the badge means, while one shows (owner, 2026-10-30)
        let needs_ai = choices.iter().any(|(_, l)| l.ends_with(" · AI"));
        let note = needs_ai.then(|| sub(Role::NOTE, AI_ONLY));
        let list = column![search, rows, Space::new().height(Fill)]
            .push(note)
            .spacing(SPACING)
            .width(width)
            .height(Length::Fixed(self.list_height(width) - SPACING));
        Some(list.into())
    }
}

/// The note's height in a list `width` wide (one line or two).
fn note_h(width: f32) -> f32 {
    match width >= NOTE_ONE_LINE {
        true => NOTE_LINE,
        false => 2.0 * NOTE_LINE,
    }
}

/// Under the list when an "AI" badge shows.
pub const AI_ONLY: &str =
    "AI: no offline model for this language yet. Offline support is coming in a future update.";

/// Remembers a pick, most recent first.
fn remember(side: Side, code: &str) {
    RECENT.with_borrow_mut(|r| {
        let recent = &mut r[side as usize];
        recent.retain(|c| c != code);
        recent.insert(0, code.into());
        recent.truncate(SHOWN);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(p: &Picker) -> Vec<String> {
        p.choices().into_iter().map(|(c, _)| c).collect()
    }

    #[test]
    fn open_search_pick_and_recent_first() {
        let mut p = Picker::new(AUTO, "ar", false);
        assert_eq!(p.label(), "Auto → AR");
        assert!(p.choices().is_empty() && !p.is_open(), "closed: nothing");
        p.update(Msg::Open(Side::Target));
        let first = codes(&p);
        assert_eq!(first.len(), SHOWN);
        assert!(!first.contains(&"ar".into()), "never the current one");
        assert_eq!(first[0], "en", "the table's order after the recent ones");
        // the search finds the rest by name, native name or code
        p.update(Msg::Query("jap".into()));
        assert_eq!(codes(&p), ["ja"]);
        p.update(Msg::Query("Рус".into()));
        assert_eq!(codes(&p), ["ru"]);
        p.update(Msg::Query("zh".into()));
        assert_eq!(codes(&p), ["zh", "zh-Hant"]);
        p.update(Msg::Query("klingon".into()));
        assert!(p.choices().is_empty());
        let _ = p.list(368.0).expect("the list builds with no match");
        // Enter picks the first match; the list closes, the query clears
        p.update(Msg::Query("germ".into()));
        assert!(p.update(Msg::First));
        assert_eq!((p.target.as_str(), p.is_open()), ("de", false));
        assert_eq!(p.label(), "Auto → DE");
        // the pick comes first next time; picking the same again is no change
        p.update(Msg::Open(Side::Target));
        assert!(p.update(Msg::Pick("ja".into())));
        p.update(Msg::Open(Side::Target));
        assert_eq!(
            codes(&p)[..3],
            ["de", "ar", "en"],
            "recent first (ja is current)"
        );
        assert!(
            !p.update(Msg::Pick("ja".into())),
            "the same again: no change"
        );
        // the chip of the open side closes it
        p.update(Msg::Open(Side::Source));
        p.update(Msg::Open(Side::Source));
        assert!(!p.is_open());
    }

    /// The badge's note fits the list: it wraps to two lines at the
    /// translate pill's width, and the list kept room for one (the second
    /// was cut at the panel's bottom edge, owner's take 2026-11-03).
    #[test]
    fn the_ai_note_fits_inside_the_list() {
        use crate::ui::kit::testing::{golden, shot};
        use iced::Size;
        // the note alone, with room to spare: its ink's height and width
        let ink = |width: f32| {
            let size = Size::new(width, 80.0);
            let note = || {
                let note: iced::widget::Container<Msg> =
                    container(sub(Role::NOTE, AI_ONLY)).width(Fill).height(Fill);
                let m = crate::ui::kit::surface::Material::Widget;
                crate::ui::kit::surface::surface(m, 0.0, note).into()
            };
            let (_, px) = shot::redrawn(|i| (i == 0).then(note), size, 1.0);
            let w = width as usize;
            let p = |x: usize, y: usize| &px[(y * w + x) * 4..(y * w + x + 1) * 4];
            let at = |x: usize, y: usize| p(x, y) != p(w - 14, y);
            let rows = (0..70).filter(|&y| (8..w - 16).any(|x| at(x, y)));
            let cols = (8..w - 16).filter(|&x| (0..70).any(|y| at(x, y)));
            (rows.max().unwrap_or(0) + 1, cols.max().unwrap_or(0) + 1)
        };
        let (h, w) = ink(800.0);
        assert!(
            h <= NOTE_LINE as usize && w <= NOTE_ONE_LINE as usize,
            "one line: {w}x{h}"
        );
        for width in [360.0, 420.0, NOTE_ONE_LINE] {
            let (h, _) = ink(width);
            assert!(
                h <= note_h(width) as usize,
                "{width}: needs {h}, keeps {}",
                note_h(width)
            );
        }
        let width = 360.0;
        // the open list with the note, as the pill shows it
        let mut p = Picker::new(AUTO, "en", true);
        p.update(Msg::Open(Side::Target));
        assert!(p.choices().iter().any(|(_, l)| l.ends_with(" · AI")));
        let size = Size::new(width, p.list_height(width));
        let view = || {
            let list = p.list(width).unwrap();
            crate::ui::kit::surface::surface(crate::ui::kit::surface::Material::Widget, 0.0, list)
                .into()
        };
        let failed = golden::check("screens", "language-picker-ai-note", size, &[1.0], view);
        assert!(failed.is_empty(), "{failed:?}");
    }

    #[test]
    fn sources_are_what_ocr_reads_and_offline_gaps_say_ai() {
        let mut p = Picker::new(AUTO, "en", true);
        p.update(Msg::Open(Side::Source));
        p.update(Msg::Query("korean".into()));
        assert!(p.choices().is_empty(), "no recognizer reads Korean yet");
        p.update(Msg::Query("".into()));
        let all = p.choices();
        assert!(all.iter().all(|(c, _)| c != AUTO), "auto is current");
        // into English: Arabic, French, Japanese have offline models, Portuguese not
        let mut label = |code: &str| {
            p.update(Msg::Query(lang::name(code).into()));
            p.choices()[0].1.clone()
        };
        assert_eq!(label("ar"), "Arabic");
        assert_eq!(label("ja"), "Japanese");
        assert_eq!(label("pt"), "Portuguese · AI");
        assert!(p.update(Msg::Pick("fr".into())));
        assert_eq!(p.label(), "FR → EN");
        p.update(Msg::Open(Side::Target));
        p.update(Msg::Query("Ara".into()));
        assert_eq!(p.choices()[0].1, "Arabic", "French → English → Arabic");
        p.update(Msg::Query("Kor".into()));
        assert_eq!(p.choices()[0].1, "Korean · AI", "no model into Korean");
        let shown = crate::ui::kit::testing::shot::texts(p.list(368.0).unwrap());
        assert!(
            shown.iter().any(|t| t == AI_ONLY),
            "the badge explained: {shown:?}"
        );
        // with AI nothing is marked
        let mut ai = Picker::new("fr", "en", false);
        ai.update(Msg::Open(Side::Target));
        assert!(ai.choices().iter().all(|(_, l)| !l.contains("AI")));
    }
}
