//! The language picker (source → target) of the one-shot translation's pill
//! and of live translate's controller (design variant A, PICKER_SPEC.md,
//! owner 2026-11-04): two fixed chips; a press on one opens ONE list hanging
//! from that chip, `LIST` big whatever it shows (filtering never moves or
//! resizes it): a search field (typing filters at once, the typed part
//! bold), then Detect (source only), Recent (or Suggested), All languages
//! A–Z. A language that needs AI (offline, no model for the pair) gets a
//! quiet ring; while such a row is hovered or ringed, a one-line hint takes
//! the list's foot. Keys: type to filter, Up / Down / Home / End move the
//! ring, Enter picks, Tab opens the other chip's list (Esc: the App closes
//! the list first). Mirrored in a right-to-left view (`kit::dir`). From kit
//! parts only (`option_list`).
//! Picks last for the session and are remembered as recent (config).
use iced::widget::{Space, column, container, mouse_area, row};
use iced::{Center, Element, Fill, Size, Vector};

use crate::features::translate::lang::{self, AUTO, LANGUAGES};
use crate::features::translate::mt;
use crate::shared::app_language::{Lang, RLM, S, tr};
use crate::shared::kit::dir;
use crate::shared::kit::divider::divider;
use crate::shared::kit::field;
use crate::shared::kit::icon::{Icon, icon};
use crate::shared::kit::option_list::{self, Look, choice, group, keyed, needs_ai, opener};
use crate::shared::kit::scroll;
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, sub};
use crate::shared::kit::themes::c;
use crate::shared::kit::tokens::{Role, radius, size};
use crate::t;

/// A chip, and the gaps in the pill (the design's 128 x 32 chips, 24 px
/// arrow, 4 px gaps, 6 px in).
pub const CHIP: Size = Size::new(128.0, 32.0);
pub const CHIP_HEIGHT: f32 = CHIP.height;
pub const SPACING: f32 = 4.0;
const ARROW: f32 = 24.0;
const PAD: f32 = 6.0;
/// The chips and the arrow between them.
pub const WIDTH: f32 = 2.0 * CHIP.width + ARROW + 2.0 * SPACING;
/// The slot at the pill's end: a gap and the kit icon button, for Snip and for
/// the glossary.
pub const SLOT: f32 = 2.0 * (SPACING + size::ICON_BUTTON);
/// The pill's vertical padding: the icon button (34) is 2 px taller than a
/// chip, so the pill keeps its 44 px height.
const PAD_Y: f32 = (CHIP.height + 2.0 * PAD - size::ICON_BUTTON) / 2.0;
/// The pill: its size never changes (the Snip and glossary buttons' slot is always there).
pub const PILL: Size = Size::new(WIDTH + 2.0 * PAD + SLOT, CHIP.height + 2.0 * PAD);
/// The open list (fixed) and its gap under the pill.
pub const LIST: Size = Size::new(248.0, 350.0);
pub const LIST_GAP: f32 = 6.0;
/// Inside the list: its padding, the search field, the rows' room (300).
const LIST_PAD: f32 = 6.0;
const SEARCH_H: f32 = 32.0;
const BODY_H: f32 = LIST.height - 3.0 * LIST_PAD - SEARCH_H;
/// A row's and a group heading's pitch; the hint's foot (gap, line, row).
const ROW_H: f32 = 30.0;
const GROUP_H: f32 = 26.0;
const FOOT_H: f32 = 34.0;
const FOOT_GAP: f32 = 3.0;
/// A row's room at its sides; between the hint's ring and its text.
const ROW_PAD: f32 = 10.0;
const HINT_GAP: f32 = 7.0;
/// The rows' width (the scroller beside them, 2 px apart).
const ROWS_W: f32 = LIST.width - 2.0 * LIST_PAD - scroll::WIDTH - 2.0;
/// The shortest scroller.
const THUMB_MIN: f32 = 24.0;
/// Recent picks kept per side.
pub const RECENT: usize = 3;
/// The search field's widget id.
pub const SEARCH: &str = "language-search";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Source,
    Target,
}

/// A key that walks the open list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Home,
    End,
    /// The other chip's list.
    Tab,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// A side's chip: open its list (or close it if it is open).
    Open(Side),
    Query(String),
    /// Enter: the ringed row, else the first.
    First,
    Pick(String),
    Key(Nav),
    /// The pointer is on this pick (index among the rows you can pick).
    Hover(Option<usize>),
    /// The wheel, in rows (down > 0).
    Wheel(f32),
}

/// One line of the open list.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    /// A heading.
    Group(S),
    /// A language (or "auto"), and whether it is the current one.
    Choice(String, bool),
    Empty,
}

impl Item {
    fn height(&self) -> f32 {
        match self {
            Item::Group(..) => GROUP_H,
            _ => ROW_H,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    /// "auto" or a code.
    pub source: String,
    pub target: String,
    /// Translation will be offline: choices with no offline model need AI.
    offline: bool,
    open: Option<Side>,
    query: String,
    /// The keyboard's row and the pointer's (indices among the picks).
    ring: Option<usize>,
    hover: Option<usize>,
    /// The first line shown.
    top: usize,
    /// Recent picks per side, most recent first.
    recent: [Vec<String>; 2],
    /// Picks are remembered as recent (config); onboarding writes nothing before Finish.
    pub saves_recent: bool,
    /// Each language in its own name, its name in the app's language beside
    /// it in grey (onboarding's "Your language": a reader of any language
    /// finds theirs). Off: the names as today.
    pub endonyms: bool,
}

impl Picker {
    pub fn new(source: &str, target: &str, offline: bool) -> Picker {
        let saved = crate::shared::config::current().translate;
        Picker {
            source: source.into(),
            target: target.into(),
            offline,
            open: None,
            query: String::new(),
            ring: None,
            hover: None,
            top: 0,
            recent: [saved.recent_sources, saved.recent_targets],
            saves_recent: true,
            endonyms: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
        self.query.clear();
        self.reset();
    }

    fn reset(&mut self) {
        (self.ring, self.hover, self.top) = (None, None, 0);
    }

    /// True when the source or the target changed.
    pub fn update(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Open(side) if self.open == Some(side) => self.close(),
            Msg::Open(side) => {
                self.open = Some(side);
                self.query.clear();
                self.reset();
            }
            Msg::Query(q) => {
                self.query = q;
                self.reset();
                // Enter picks the first match; the ring shows which
                if !self.query.trim().is_empty() && !self.picks().is_empty() {
                    self.ring = Some(0);
                }
            }
            Msg::First => {
                let picks = self.picks();
                let ringed = self.ring.and_then(|i| picks.get(i));
                if let Some(code) = ringed.or(picks.first()).cloned() {
                    return self.update(Msg::Pick(code));
                }
            }
            Msg::Pick(code) => {
                let Some(side) = self.open else {
                    return false;
                };
                self.remember(side, &code);
                self.close();
                let slot = match side {
                    Side::Source => &mut self.source,
                    Side::Target => &mut self.target,
                };
                let changed = *slot != code;
                *slot = code;
                return changed;
            }
            Msg::Key(Nav::Tab) => {
                if let Some(side) = self.open {
                    let other = match side {
                        Side::Source => Side::Target,
                        Side::Target => Side::Source,
                    };
                    return self.update(Msg::Open(other));
                }
            }
            Msg::Key(nav) => {
                let n = self.picks().len();
                if n == 0 {
                    return false;
                }
                self.ring = Some(match (nav, self.ring) {
                    (Nav::Down, Some(i)) => (i + 1).min(n - 1),
                    (Nav::Up, Some(i)) => i.saturating_sub(1),
                    (Nav::End, _) => n - 1,
                    _ => 0,
                });
                self.follow();
            }
            Msg::Hover(i) => self.hover = i,
            Msg::Wheel(rows) => {
                let step = match rows.round() {
                    0.0 => rows.signum(),
                    r => r,
                } as isize;
                let max = self.max_top() as isize;
                self.top = (self.top as isize + step).clamp(0, max) as usize;
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

    /// The open list's lines.
    fn items(&self) -> Vec<Item> {
        let Some(side) = self.open else {
            return Vec::new();
        };
        let current = match side {
            Side::Source => self.source.as_str(),
            Side::Target => self.target.as_str(),
        };
        // A–Z (the sources: what OCR reads)
        let mut all: Vec<&'static lang::Language> = (LANGUAGES.iter())
            .filter(|l| side == Side::Target || l.ocr.is_some())
            .collect();
        all.sort_by_key(|l| crate::shared::app_language::language_name(l.code));
        let choice = |code: &str, current: bool| Item::Choice(code.to_string(), current);
        let q = self.query.trim().to_lowercase();
        if !q.is_empty() {
            let detect = side == Side::Source
                && [
                    "detect language",
                    t!(pk_detect_long).trim_start_matches(RLM),
                ]
                .iter()
                .any(|d| d.to_lowercase().contains(&q));
            let matches = all.iter().filter(|l| {
                l.code.to_lowercase().starts_with(&q)
                    || l.name.to_lowercase().contains(&q)
                    || crate::shared::app_language::language_name(l.code)
                        .to_lowercase()
                        .contains(&q)
                    || l.native.to_lowercase().contains(&q)
            });
            let mut out: Vec<Item> = (detect.then_some(AUTO).into_iter())
                .chain(matches.map(|l| l.code))
                .map(|code| choice(code, code == current))
                .collect();
            if out.is_empty() {
                out.push(Item::Empty);
            }
            return out;
        }
        let mut out = Vec::new();
        if side == Side::Source {
            out.push(choice(AUTO, current == AUTO));
        }
        // the current one first, then the recent picks (Suggested: English)
        let saved = &self.recent[side as usize];
        let suggested = side == Side::Target && saved.is_empty();
        let mut recent: Vec<&str> = Vec::new();
        let first = (current != AUTO).then_some(current).into_iter();
        let more = saved.iter().map(String::as_str);
        let english = suggested.then_some("en").into_iter();
        for code in first.chain(more).chain(english) {
            let known = all.iter().any(|l| l.code == code);
            if known && !recent.contains(&code) && recent.len() < RECENT {
                recent.push(code);
            }
        }
        if !recent.is_empty() {
            out.push(match suggested {
                true => Item::Group(S::pk_suggested),
                false => Item::Group(S::pk_recent),
            });
            out.extend(recent.iter().map(|&code| choice(code, code == current)));
        }
        out.push(Item::Group(S::pk_all));
        out.extend(all.iter().map(|l| choice(l.code, false)));
        out
    }

    /// The codes you can pick, in the list's order.
    pub fn picks(&self) -> Vec<String> {
        (self.items().into_iter())
            .filter_map(|i| match i {
                Item::Choice(code, _) => Some(code),
                _ => None,
            })
            .collect()
    }

    /// The open side's `code` needs AI: offline, with no model for the pair.
    pub fn needs_ai(&self, code: &str) -> bool {
        let (source, target) = match self.open {
            Some(Side::Source) => (code, self.target.as_str()),
            Some(Side::Target) => (self.source.as_str(), code),
            None => return false,
        };
        self.offline && source != target && !mt::available(source, target)
    }

    /// The hint at the list's foot shows: a needs-AI row is hovered or ringed.
    pub fn hint(&self) -> bool {
        let picks = self.picks();
        [self.ring, self.hover]
            .into_iter()
            .flatten()
            .any(|i| picks.get(i).is_some_and(|c| self.needs_ai(c)))
    }

    /// The rows' room: less the hint's foot while it shows.
    fn body_h(&self) -> f32 {
        BODY_H - if self.hint() { FOOT_H } else { 0.0 }
    }

    /// The last `top` that still fills the room.
    fn max_top(&self) -> usize {
        let items = self.items();
        let (room, mut used) = (self.body_h(), 0.0);
        let mut top = items.len();
        while top > 0 && used + items[top - 1].height() <= room {
            used += items[top - 1].height();
            top -= 1;
        }
        top
    }

    /// Scrolls so the ringed row shows (its heading too, scrolling up).
    fn follow(&mut self) {
        let Some(ring) = self.ring else { return };
        let items = self.items();
        let Some(at) = (items.iter().enumerate())
            .filter(|(_, i)| matches!(i, Item::Choice(..)))
            .nth(ring)
            .map(|(at, _)| at)
        else {
            return;
        };
        if at < self.top {
            self.top = at;
            if at > 0 && matches!(items[at - 1], Item::Group(..)) {
                self.top -= 1;
            }
        }
        let room = self.body_h();
        let h = |top: usize| items[top..=at].iter().map(Item::height).sum::<f32>();
        while h(self.top) > room {
            self.top += 1;
        }
    }

    /// Remembers a pick, most recent first (Detect isn't a pick to recall).
    fn remember(&mut self, side: Side, code: &str) {
        if code == AUTO || !self.saves_recent {
            return;
        }
        let recent = &mut self.recent[side as usize];
        recent.retain(|c| c != code);
        recent.insert(0, code.into());
        recent.truncate(RECENT);
        #[cfg(not(test))]
        {
            let [sources, targets] = self.recent.clone();
            std::thread::spawn(move || {
                let saved = crate::shared::config::update(|c| {
                    c.translate.recent_sources = sources;
                    c.translate.recent_targets = targets;
                });
                if let Err(e) = saved {
                    crate::log!("[opencircle] could not save the config: {e}");
                }
            });
        }
    }

    /// The name a row shows.
    fn name(&self, code: &str) -> &'static str {
        match code {
            AUTO => t!(pk_detect_long),
            c => crate::shared::app_language::language_name(c),
        }
    }

    /// The two chips: source → target (the target in the accent; the open
    /// one ringed). Mirrored right to left.
    pub fn chips(&self) -> Element<'_, Msg> {
        let chip = |side: Side, code: &str| {
            // "Chinese (Simplified)" -> "Chinese": one line in the chip
            let label = match code {
                AUTO => t!(pk_detect),
                c => crate::shared::app_language::language_name(c)
                    .split(" (")
                    .next()
                    .unwrap_or_default(),
            };
            let open = self.open == Some(side);
            let size = (CHIP.width, CHIP.height);
            opener(
                label.into(),
                side == Side::Target,
                open,
                size,
                Msg::Open(side),
            )
        };
        let glyph = if dir::rtl() {
            Icon::Back
        } else {
            Icon::Forward
        };
        let arrow = container(icon(glyph, size::ICON_SMALL).color(c().text_muted)).center_x(ARROW);
        dir::row([
            chip(Side::Source, &self.source),
            arrow.into(),
            chip(Side::Target, &self.target),
        ])
        .spacing(SPACING)
        .align_y(Center)
        .into()
    }

    /// The pill (`PILL`): the chips, then `tail` (the Snip and glossary buttons, `SLOT`
    /// wide; first when mirrored) on the widget surface, radius 18.
    pub fn pill<'a, M: Clone + 'a>(
        &'a self,
        map: fn(Msg) -> M,
        tail: Element<'a, M>,
    ) -> Element<'a, M> {
        let line = dir::row([self.chips().map(map), tail])
            .spacing(SPACING)
            .align_y(Center);
        surface(Material::Widget, radius::DRAWER, line)
            .padding([PAD_Y, PAD])
            .into()
    }

    /// Where the open list hangs, from the pill's top-left corner: the
    /// source list's start edge on the From chip's, the target list's end
    /// edge on the To chip's (mirrored right to left).
    pub fn list_at(&self) -> Option<Vector> {
        let side = self.open?;
        // (mirrored: the Snip slot is at the start side)
        let rtl = dir::rtl();
        let (start, end) = match rtl {
            true => (PAD + SLOT, PILL.width - PAD),
            false => (PAD, PILL.width - PAD - SLOT),
        };
        let x = match (side == Side::Source) != rtl {
            true => start,
            false => end - LIST.width,
        };
        Some(Vector::new(x, PILL.height + LIST_GAP))
    }

    /// The open list, `LIST` big. Nothing when closed.
    pub fn list(&self) -> Option<Element<'_, Msg>> {
        self.open?;
        let rtl = dir::rtl();
        let search = field::search(t!(pk_search), &self.query)
            .id(SEARCH)
            .on_input(Msg::Query)
            // Enter in the field (it takes the key before the list does): the ringed or first match
            .on_submit(Msg::First)
            .width(Fill);
        let items = self.items();
        let q = self.query.trim().to_lowercase();
        let room = self.body_h();
        let mut pick = (items[..self.top.min(items.len())].iter())
            .filter(|i| matches!(i, Item::Choice(..)))
            .count();
        let (mut rows, mut used) = (column![], 0.0);
        for item in items.iter().skip(self.top) {
            if used + item.height() > room {
                break;
            }
            used += item.height();
            let line: Element<'_, Msg> = match item {
                Item::Group(key) => group(tr(*key), GROUP_H),
                Item::Empty => {
                    let none = muted(Role::CONTROL, t!(pk_none));
                    let none = container(none).center_y(ROW_H).padding([0.0, ROW_PAD]);
                    match rtl {
                        true => none.align_right(Fill).into(),
                        false => none.width(Fill).into(),
                    }
                }
                Item::Choice(code, current) => {
                    let i = pick;
                    pick += 1;
                    let look = Look {
                        current: *current,
                        ringed: self.ring == Some(i),
                        hovered: self.hover == Some(i),
                        needs_ai: self.needs_ai(code),
                        aside: None,
                    };
                    let (name, look) = match (self.endonyms, lang::find(code)) {
                        (true, Some(l)) => {
                            let ours = crate::shared::app_language::language_name(l.code);
                            let aside =
                                (ours.trim_start_matches('\u{200F}') != l.native).then_some(ours);
                            (l.native, Look { aside, ..look })
                        }
                        // another UI language: the name in it, the language's own beside it
                        (false, Some(l)) if crate::shared::app_language::current() != Lang::En => {
                            let ours = self.name(code);
                            let aside =
                                (ours.trim_start_matches(RLM) != l.native).then_some(l.native);
                            (ours, Look { aside, ..look })
                        }
                        _ => (self.name(code), look),
                    };

                    let bold = (name.to_lowercase().len() == name.len() && !q.is_empty())
                        .then(|| name.to_lowercase().find(&q).map(|a| (a, a + q.len())))
                        .flatten();
                    let row = choice(name, bold, look, ROWS_W, Msg::Pick(code.clone()));
                    let row = mouse_area(row)
                        .on_enter(Msg::Hover(Some(i)))
                        .on_exit(Msg::Hover(None));
                    container(row).center_y(ROW_H).into()
                }
            };
            rows = rows.push(line);
        }
        let rows = mouse_area(container(rows).width(ROWS_W).height(room)).on_scroll(|d| {
            Msg::Wheel(match d {
                iced::mouse::ScrollDelta::Lines { y, .. } => -3.0 * y,
                iced::mouse::ScrollDelta::Pixels { y, .. } => -y / ROW_H,
            })
        });
        // the scroller, where there is more than the room shows
        let total: f32 = items.iter().map(Item::height).sum();
        let thumb: Element<'_, Msg> = match total > room {
            true => {
                let len = (room * room / total).max(THUMB_MIN);
                let before: f32 = items[..self.top].iter().map(Item::height).sum();
                let y = (before / total * room).min(room - len);
                column![Space::new().height(y), scroll::thumb(len)].into()
            }
            false => Space::new().width(scroll::WIDTH).into(),
        };
        let mut body = vec![rows.into(), Space::new().width(Fill).into(), thumb];
        if rtl {
            body.reverse();
        }
        let mut list = column![search, Space::new().height(LIST_PAD), row(body)];
        if self.hint() {
            let hint = sub(Role::META, t!(pk_needs_ai));
            let line = container(
                dir::row([needs_ai(true), hint.into()])
                    .spacing(HINT_GAP)
                    .align_y(Center),
            )
            .center_y(ROW_H)
            .padding([0.0, ROW_PAD]);
            let line = match rtl {
                true => line.align_right(Fill),
                false => line.width(Fill),
            };
            list = list.push(column![Space::new().height(FOOT_GAP), divider(), line]);
        }
        let list = option_list::frame(list)
            .padding(LIST_PAD)
            .width(LIST.width)
            .height(LIST.height);
        Some(keyed(list, |key, _shift| {
            use iced::keyboard::key::Named;
            Some(match key {
                Named::ArrowUp => Msg::Key(Nav::Up),
                Named::ArrowDown => Msg::Key(Nav::Down),
                Named::Home => Msg::Key(Nav::Home),
                Named::End => Msg::Key(Nav::End),
                Named::Tab => Msg::Key(Nav::Tab),
                Named::Enter => Msg::First,
                _ => return None,
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::shot;

    fn open(source: &str, target: &str, offline: bool, side: Side) -> Picker {
        let mut p = Picker::new(source, target, offline);
        p.update(Msg::Open(side));
        p
    }

    #[test]
    fn open_search_pick_and_recent_first() {
        let mut p = Picker::new(AUTO, "ar", false);
        assert_eq!(p.label(), "Auto → AR");
        assert!(p.picks().is_empty() && !p.is_open(), "closed: nothing");
        p.update(Msg::Open(Side::Target));
        // Suggested (no picks yet): the current one, English; then A–Z
        assert_eq!(p.picks()[..4], ["ar", "en", "ar", "zh"]);
        assert_eq!(p.items()[0], Item::Group(S::pk_suggested));
        assert_eq!(
            p.items()[1],
            Item::Choice("ar".into(), true),
            "current: checked"
        );
        // the search finds the rest by name, native name or code
        p.update(Msg::Query("jap".into()));
        assert_eq!(p.picks(), ["ja"]);
        p.update(Msg::Query("Рус".into()));
        assert_eq!(p.picks(), ["ru"]);
        p.update(Msg::Query("zh".into()));
        assert_eq!(p.picks(), ["zh", "zh-Hant"]);
        p.update(Msg::Query("klingon".into()));
        assert!(p.picks().is_empty());
        assert_eq!(p.items(), [Item::Empty], "\"No language found\"");
        let _ = p.list().expect("the list builds with no match");
        // Enter picks the first match; the list closes, the query clears
        p.update(Msg::Query("germ".into()));
        assert!(p.update(Msg::First));
        assert_eq!((p.target.as_str(), p.is_open()), ("de", false));
        // Recent: the current one, then the picks, newest first (3 at most)
        for code in ["ja", "fr", "es"] {
            p.update(Msg::Open(Side::Target));
            assert!(p.update(Msg::Pick(code.into())));
        }
        p.update(Msg::Open(Side::Target));
        assert_eq!(p.items()[0], Item::Group(S::pk_recent));
        assert_eq!(p.picks()[..RECENT], ["es", "fr", "ja"]);
        assert!(
            !p.update(Msg::Pick("es".into())),
            "the same again: no change"
        );
        // the source side keeps its own; Detect is never a recent pick
        p.update(Msg::Open(Side::Source));
        assert_eq!(p.picks()[..2], ["auto", "ar"], "Detect, then A–Z");
        assert!(p.update(Msg::Pick("ja".into())));
        p.update(Msg::Open(Side::Source));
        p.update(Msg::Pick(AUTO.into()));
        p.update(Msg::Open(Side::Source));
        assert_eq!(p.picks()[..3], ["auto", "ja", "ar"]);
        // the chip of the open side closes it
        p.update(Msg::Open(Side::Source));
        assert!(!p.is_open());
    }

    /// Up / Down / Home / End move the ring (no wrap), the list scrolls to
    /// keep it in view, Enter picks it, Tab opens the other chip's list.
    #[test]
    fn the_keyboard_walks_the_list() {
        let mut p = open(AUTO, "en", false, Side::Target);
        let n = p.picks().len();
        p.update(Msg::Key(Nav::Up));
        assert_eq!(p.ring, Some(0));
        p.update(Msg::Key(Nav::Up));
        assert_eq!(p.ring, Some(0), "no wrap");
        p.update(Msg::Key(Nav::Down));
        p.update(Msg::Key(Nav::Down));
        assert_eq!((p.ring, p.top), (Some(2), 0));
        p.update(Msg::Key(Nav::End));
        assert_eq!(p.ring, Some(n - 1));
        p.update(Msg::Key(Nav::Down));
        assert_eq!(p.ring, Some(n - 1), "no wrap");
        assert!(p.top > 0 && p.top == p.max_top(), "scrolled to the end");
        p.update(Msg::Key(Nav::Home));
        assert_eq!((p.ring, p.top), (Some(0), 0), "back up, its heading too");
        // the wheel scrolls by rows, never past the ends
        p.update(Msg::Wheel(-3.0));
        assert_eq!(p.top, 0);
        p.update(Msg::Wheel(2.0));
        assert_eq!(p.top, 2);
        p.update(Msg::Wheel(999.0));
        assert_eq!(p.top, p.max_top());
        // typing rings the first match; Down then Enter picks the second
        p.update(Msg::Query("port".into()));
        assert_eq!((p.ring, p.top), (Some(0), 0));
        p.update(Msg::Key(Nav::Down));
        assert!(p.update(Msg::First));
        assert_eq!(p.target, "pt-BR");
        // Tab: the other list
        p.update(Msg::Open(Side::Target));
        p.update(Msg::Key(Nav::Tab));
        assert_eq!(p.open, Some(Side::Source));
        p.update(Msg::Key(Nav::Tab));
        assert_eq!(p.open, Some(Side::Target));
        // the keys reach the picker through the list (the field has the focus)
        use iced::keyboard::key::Named;
        let keys: Vec<_> = [Named::ArrowDown, Named::End, Named::Enter, Named::Tab]
            .into_iter()
            .flat_map(|k| shot::named(k, false))
            .collect();
        let (msgs, _) = shot::feed(
            p.list().unwrap(),
            LIST,
            shot::Focus::Id(SEARCH),
            &keys,
            &mut shot::Clip::default(),
        );
        let msgs: Vec<_> = msgs.iter().map(|m| format!("{m:?}")).collect();
        assert_eq!(msgs, ["Key(Down)", "Key(End)", "First", "Key(Tab)"]);
        let (msgs, _) = shot::feed(
            p.list().unwrap(),
            LIST,
            shot::Focus::Id(SEARCH),
            &shot::typing("po"),
            &mut shot::Clip::default(),
        );
        let msgs: Vec<_> = msgs.iter().map(|m| format!("{m:?}")).collect();
        assert_eq!(msgs, ["Query(\"p\")", "Query(\"po\")"], "typing filters");
    }

    /// The source list's start edge sits on the From chip's, the target
    /// list's end edge on the To chip's, mirrored right to left; the list
    /// keeps its place and its size while filtering (and with the hint).
    #[test]
    fn the_list_hangs_from_its_chip_and_never_moves() {
        const MARGIN: f32 = 20.0;
        let from = PAD;
        let to_end = PAD + 2.0 * CHIP.width + ARROW + 2.0 * SPACING;
        assert_eq!(to_end, PILL.width - PAD - SLOT);
        for rtl in [false, true] {
            for side in [Side::Source, Side::Target] {
                let mut p = open(AUTO, "en", true, side);
                let _dir = dir::scope(rtl);
                let at = p.list_at().unwrap();
                assert_eq!(at.y, PILL.height + LIST_GAP);
                match (side, rtl) {
                    (Side::Source, false) => assert_eq!(at.x, from),
                    (Side::Target, false) => assert_eq!(at.x + LIST.width, to_end),
                    // mirrored: From is at the right, To at the left
                    (Side::Source, true) => assert_eq!(at.x + LIST.width, PILL.width - from),
                    (Side::Target, true) => assert_eq!(at.x, PILL.width - to_end),
                }
                // drawn: the same box whatever it shows
                let size = Size::new(LIST.width + 40.0, LIST.height + 40.0);
                let drawn = |p: &Picker| {
                    let view = container(p.list().unwrap()).padding(MARGIN);
                    let px = shot::render(view.into(), size.width as u32, size.height as u32);
                    let w = size.width as usize;
                    let ink = |x: usize, y: usize| px[(y * w + x) * 4..][..4] != px[..4];
                    let rows: Vec<usize> = (0..size.height as usize)
                        .filter(|&y| (0..w).any(|x| ink(x, y)))
                        .collect();
                    let cols: Vec<usize> = (0..w)
                        .filter(|&x| (0..size.height as usize).any(|y| ink(x, y)))
                        .collect();
                    (rows[0], rows[rows.len() - 1], cols[0], cols[cols.len() - 1])
                };
                let first = drawn(&p);
                assert_eq!(
                    (
                        first.0,
                        first.2,
                        first.1 - first.0 + 1,
                        first.3 - first.2 + 1
                    ),
                    (20, 20, LIST.height as usize, LIST.width as usize)
                );
                for q in ["p", "po", "klingon", ""] {
                    p.update(Msg::Query(q.into()));
                    assert_eq!(p.list_at(), Some(at), "{q}");
                    assert_eq!(drawn(&p), first, "{q}");
                }
                p.update(Msg::Query("turk".into()));
                assert!(p.hint() || side == Side::Source, "the hint shows");
                assert_eq!(drawn(&p), first, "with the hint");
            }
        }
    }

    #[test]
    fn sources_are_what_ocr_reads_and_offline_gaps_need_ai() {
        let mut p = open(AUTO, "en", true, Side::Source);
        p.update(Msg::Query("korean".into()));
        assert!(p.picks().is_empty(), "no recognizer reads Korean yet");
        // into English: Arabic, French, Japanese have offline models, Turkish not
        for (code, ai) in [("ar", false), ("ja", false), ("tr", true), ("en", false)] {
            assert_eq!(p.needs_ai(code), ai, "{code}");
        }
        // hovered or ringed, a needs-AI row shows the hint at the foot
        p.update(Msg::Query("".into()));
        let pt = p.picks().iter().position(|c| c == "tr").unwrap();
        assert!(!p.hint());
        p.update(Msg::Hover(Some(pt)));
        assert!(p.hint());
        let shown = shot::texts(p.list().unwrap());
        assert!(
            shown.iter().any(|t| t == "Needs AI · no local model yet"),
            "{shown:?}"
        );
        p.update(Msg::Hover(None));
        assert!(!p.hint());
        assert!(p.update(Msg::Pick("fr".into())));
        assert_eq!(p.label(), "FR → EN");
        p.update(Msg::Open(Side::Target));
        assert!(!p.needs_ai("ar"), "French → English → Arabic");
        assert!(p.needs_ai("ko"), "no model into Korean");
        // with AI nothing is marked
        let ai = open("fr", "en", false, Side::Target);
        assert!(ai.picks().iter().all(|c| !ai.needs_ai(c)));
    }
}
