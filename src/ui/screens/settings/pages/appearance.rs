//! Appearance (`settings-window.js` "APPEARANCE"): the OS mode, the logo's
//! colour, reduced animations, the 12 theme cards (one pick per mode) and the
//! active look cards (the design's 10, or the user's own: its rows under the
//! cards). No Glass row until glass exists (0.1.3).
use iced::Element;
use iced::widget::column;

use super::super::legacy::custom_rows;
use super::super::state::{Message, Pick, State};
use crate::config::{Logo, LookTheme, PRESETS, Theme};
use crate::ui::kit::card::{grid, look_card, theme_card};
use crate::ui::kit::chip::{Tone, status};
use crate::ui::kit::divider::divider;
use crate::ui::kit::looks::{LOOKS, Look};
use crate::ui::kit::motion;
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::themes::THEMES;
use crate::ui::theme;

/// The theme cards' columns (`sm:grid-cols-3`), the look cards' (`grid-cols-2`).
const COLUMNS: usize = 3;
const LOOK_COLUMNS: usize = 2;

const MODE_DESC: &str = if cfg!(windows) {
    "System follows your Windows light or dark mode"
} else {
    "System follows your desktop's light or dark mode"
};

const MOTION_DESC: &str = if cfg!(windows) {
    "Follows Windows' Show animations setting: off there, pages and widgets move without transitions"
} else {
    "Follows your desktop's animation setting: off there, pages and widgets move without transitions"
};

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let (from, glide) = state.thumb(d.theme, |p| match p {
        Pick::Mode(t) => Some(t),
        _ => None,
    });
    let modes = [
        (Theme::System, "System"),
        (Theme::Light, "Light"),
        (Theme::Dark, "Dark"),
    ];
    let logo = d.appearance.logo;
    let (logo_from, logo_glide) = state.thumb(logo, |p| match p {
        Pick::Logo(l) => Some(l),
        _ => None,
    });
    let logos = [(Logo::Theme, "Theme"), (Logo::Purple, "Purple")];
    let reduced = match motion::reduced() {
        true => ("On", Tone::Ok),
        false => ("Off", Tone::Busy),
    };
    // a card is chosen when it is its mode's pick (System shows one of the two)
    let picked = |id: &str| id == d.appearance.dark || id == d.appearance.light;
    let cards = THEMES
        .into_iter()
        .map(|t| theme_card(t, picked(t.id), Message::ThemeCard(t.id)));
    let mut rows = column![
        setting_row::row(
            title("OS mode"),
            MODE_DESC,
            segmented(&modes, d.theme, from, glide, Message::Theme),
        ),
        setting_row::row(
            title("Logo colour"),
            "Theme recolours the logo with your theme; Purple keeps the original icon",
            segmented(&logos, logo, logo_from, logo_glide, Message::Logo),
        ),
        setting_row::row(
            title("Reduce animations"),
            MOTION_DESC,
            status(reduced.0, reduced.1)
        ),
        setting_row::block(
            title("App and widget themes (12 palettes)"),
            "One for dark mode, one for light: the Settings window and every floating widget",
            grid(COLUMNS, cards),
        ),
    ];
    rows = rows.push(setting_row::block(
        title("Active look screen borders"),
        "The screen edge and the accent while OpenCircle is open",
        grid(LOOK_COLUMNS, look_cards(state)),
    ));
    if d.look.theme == LookTheme::Custom {
        for r in custom_rows(state) {
            rows = rows.push(r).push(divider());
        }
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
