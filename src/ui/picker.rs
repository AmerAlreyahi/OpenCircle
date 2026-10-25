//! The compact language picker (source → target) of live translate's
//! controller and of the one-shot translation's pill: two chips; a press on
//! one opens that side's list under them, recently picked first, with a
//! search field for the rest. Built from the shared widgets only. Picks last
//! for the session (and are remembered as recent); Settings › Translate keeps
//! the defaults.
use std::cell::RefCell;

use iced::widget::{column, container, row};
use iced::{Center, Element, Fill, Length};

use super::widgets;
use crate::mt;
use crate::translate::lang::{self, AUTO, LANGUAGES};

/// Chip height and the gaps (the controller's sizes).
pub const CHIP_HEIGHT: f32 = 32.0;
pub const SPACING: f32 = 4.0;
/// One chip per side (one width for both, consistency rule).
pub const SIDE_WIDTH: f32 = 112.0;
/// Width of the closed picker: two chips and the arrow between them.
pub const WIDTH: f32 = 2.0 * SIDE_WIDTH + ARROW + 2.0 * SPACING;
const ARROW: f32 = 16.0;
/// Choices the open list shows (two columns), after filtering.
const SHOWN: usize = 8;
const COLUMNS: usize = 2;
/// What the open list adds under the chips: the search field and the rows.
pub const LIST_HEIGHT: f32 =
    SPACING + widgets::CONTROL + (SHOWN / COLUMNS) as f32 * (CHIP_HEIGHT + SPACING);
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

    /// The two chips: source → target. `height` = the chips' height.
    pub fn chips(&self, height: f32) -> Element<'_, Msg> {
        let chip = |side: Side, code: &str| {
            // "Chinese (Simplified)" -> "Chinese": one line in the chip (the
            // list and the status line tell variants apart)
            let label = match code {
                AUTO => "Detect",
                c => lang::name(c).split(" (").next().unwrap_or_default(),
            }
            .to_string();
            widgets::chip(label, Msg::Open(side))
                .width(SIDE_WIDTH)
                .height(height)
        };
        row![
            chip(Side::Source, &self.source),
            container(widgets::caption("→")).center_x(ARROW),
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
        let search = widgets::input(
            SEARCH,
            "Search languages",
            &self.query,
            Msg::Query,
            Msg::First,
        )
        .width(Fill);
        let cell = (width - SPACING * (COLUMNS - 1) as f32) / COLUMNS as f32;
        let choices = self.choices();
        let mut rows = column![].spacing(SPACING);
        for pair in choices.chunks(COLUMNS) {
            let cells = pair.iter().map(|(code, label)| {
                widgets::chip(label.clone(), Msg::Pick(code.clone()))
                    .width(cell)
                    .height(CHIP_HEIGHT)
                    .into()
            });
            rows = rows.push(row(cells).spacing(SPACING));
        }
        if choices.is_empty() {
            rows = rows.push(widgets::caption("No language matches").height(CHIP_HEIGHT));
        }
        let list = column![search, rows]
            .spacing(SPACING)
            .width(width)
            .height(Length::Fixed(LIST_HEIGHT - SPACING));
        Some(list.into())
    }
}

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

    #[test]
    fn sources_are_what_ocr_reads_and_offline_gaps_say_ai() {
        let mut p = Picker::new(AUTO, "en", true);
        p.update(Msg::Open(Side::Source));
        p.update(Msg::Query("korean".into()));
        assert!(p.choices().is_empty(), "no recognizer reads Korean yet");
        p.update(Msg::Query("".into()));
        let all = p.choices();
        assert!(all.iter().all(|(c, _)| c != AUTO), "auto is current");
        // into English: Arabic, French, Japanese have offline models, German not
        let mut label = |code: &str| {
            p.update(Msg::Query(lang::name(code).into()));
            p.choices()[0].1.clone()
        };
        assert_eq!(label("ar"), "Arabic");
        assert_eq!(label("ja"), "Japanese");
        assert_eq!(label("de"), "German · AI");
        assert!(p.update(Msg::Pick("fr".into())));
        assert_eq!(p.label(), "FR → EN");
        p.update(Msg::Open(Side::Target));
        p.update(Msg::Query("Ara".into()));
        assert_eq!(p.choices()[0].1, "Arabic · AI", "no French → Arabic model");
        // with AI nothing is marked
        let mut ai = Picker::new("fr", "en", false);
        ai.update(Msg::Open(Side::Target));
        assert!(ai.choices().iter().all(|(_, l)| !l.contains("AI")));
    }
}
