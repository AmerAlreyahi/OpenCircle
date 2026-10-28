//! Appearance (`settings-window.js` "APPEARANCE"): reduced animations, the
//! 12 theme cards (a card is a theme and its mode: owner, 2026-10-28, no OS
//! mode or logo colour rows; a config that never picked one follows the OS)
//! and the active look cards (the design's 10, or the user's own: its rows
//! under the cards). No Glass row until glass exists (0.1.3).
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, State};
use super::custom;
use crate::config::{LookTheme, PRESETS};
use crate::ui::kit::card::{grid, look_card, theme_card};
use crate::ui::kit::chip::{Tone, status};
use crate::ui::kit::looks::{LOOKS, Look};
use crate::ui::kit::motion;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::themes::THEMES;
use crate::ui::theme;

/// The theme cards' columns (`sm:grid-cols-3`), the look cards' (`grid-cols-2`).
const COLUMNS: usize = 3;
const LOOK_COLUMNS: usize = 2;

const MOTION_DESC: &str = if cfg!(windows) {
    "Follows Windows' Show animations setting: off there, pages and widgets move without transitions"
} else {
    "Follows your desktop's animation setting: off there, pages and widgets move without transitions"
};

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let reduced = match motion::reduced() {
        true => ("On", Tone::Ok),
        false => ("Off", Tone::Busy),
    };
    // the chosen card: the theme showing now
    let shown = theme::kit_theme(d.theme, theme::os_dark(), &d.appearance).id;
    let cards = THEMES
        .into_iter()
        .map(|t| theme_card(t, t.id == shown, Message::ThemeCard(t.id)));
    let mut rows = column![
        setting_row::row(
            title("Reduce animations"),
            MOTION_DESC,
            status(reduced.0, reduced.1)
        ),
        setting_row::block(
            title("App and widget themes (12 palettes)"),
            "The Settings window and every floating widget; a light card turns light mode on",
            grid(COLUMNS, cards),
        ),
    ];
    rows = rows.push(setting_row::block(
        title("Active look screen borders"),
        "The screen edge and the accent while OpenCircle is open",
        grid(LOOK_COLUMNS, look_cards(state)),
    ));
    if d.look.theme == LookTheme::Custom {
        rows = rows.extend(custom::rows(state));
    }
    rows.into()
}

/// What each design look's card writes (by look id): the stored value of the
/// look shown, kept as it is when its card is picked again ("calm" too), else
/// the first preset showing it (Prism is written "aurora", Frost "mono", as
/// before schema 4). `None`: the config has no value for that look yet.
pub fn picks(state: &State) -> Vec<(&'static str, Option<LookTheme>)> {
    let now = state.draft.look.theme;
    let shows = |t: LookTheme, look: &Look| theme::kit_look(t).is_some_and(|l| l.id == look.id);
    LOOKS
        .into_iter()
        .map(|look| {
            let pick = match shows(now, look) {
                true => Some(now),
                false => (PRESETS.iter()).map(|&(t, _)| t).find(|&t| shows(t, look)),
            };
            (look.id, pick)
        })
        .collect()
}

/// The design's looks, then the user's own.
fn look_cards(state: &State) -> Vec<Element<'_, Message>> {
    let now = state.draft.look.theme;
    let mut cards: Vec<_> = (LOOKS.into_iter().zip(picks(state)))
        .map(|(look, (_, pick))| {
            let chosen = pick == Some(now);
            let on = pick.map(Message::Look);
            look_card(look.name, look.badge, look.desc, look.preview, chosen, on)
        })
        .collect();
    let (a, b) = theme::accent_of(&state.look(LookTheme::Custom));
    cards.push(look_card(
        "Custom",
        "Yours",
        "Your own colours, glow, dim, corners and edges",
        &[a, b],
        now == LookTheme::Custom,
        Some(Message::Look(LookTheme::Custom)),
    ));
    cards
}
