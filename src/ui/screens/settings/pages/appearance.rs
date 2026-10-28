//! Appearance (`settings-window.js` "APPEARANCE"): the OS mode, the logo's
//! colour, reduced animations, and the 12 theme cards (one pick per mode).
//! No Glass row until glass exists (0.1.3).
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, Pick, State};
use crate::config::{Logo, Theme};
use crate::ui::kit::card::{grid, theme_card};
use crate::ui::kit::chip::{Tone, status};
use crate::ui::kit::motion;
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::themes::THEMES;

/// The theme cards' columns (`sm:grid-cols-3`).
const COLUMNS: usize = 3;

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
    column![
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
    ]
    .into()
}
