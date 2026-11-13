//! Appearance (`settings-window.js` "APPEARANCE"): reduced animations, the
//! 12 theme cards (a card is a theme and its mode: owner, 2026-10-28, no OS
//! mode or logo colour rows; a config that never picked one follows the OS)
//! and the active look cards (the design's 10, or the user's own: its rows
//! under the cards). No Glass row until glass exists.
use crate::shared::app_language::{S, tr};
use crate::t;
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, State};
use super::custom;
use crate::shared::config::{LookTheme, PRESETS};
use crate::shared::kit::card::{grid, look_card, theme_card};
use crate::shared::kit::chip::{Tone, status};
use crate::shared::kit::looks::{LOOKS, Look};
use crate::shared::kit::motion;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::themes::THEMES;
use crate::shared::theme;

/// The theme cards' columns (`sm:grid-cols-3`), the look cards' (`grid-cols-2`).
const COLUMNS: usize = 3;
const LOOK_COLUMNS: usize = 2;

const MOTION_DESC: S = if cfg!(windows) {
    S::a_reduce_win
} else {
    S::a_reduce_linux
};

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let reduced = match motion::reduced() {
        true => (t!(c_on), Tone::Ok),
        false => (t!(a_reduce_off), Tone::Busy),
    };
    let mut rows = column![
        setting_row::row(
            title(t!(a_reduce)),
            tr(MOTION_DESC),
            status(reduced.0, reduced.1)
        ),
        themes_row(state),
    ];
    rows = rows.push(setting_row::block(
        title(t!(a_looks)),
        t!(a_looks_desc),
        grid(LOOK_COLUMNS, look_cards(state)),
    ));
    if d.look.theme == LookTheme::Custom {
        rows = rows.extend(custom::rows(state));
    }
    rows.into()
}

/// The 12 theme cards (Settings, and onboarding's Look step).
pub fn themes_row(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    // the chosen card: the theme showing now
    let shown = theme::kit_theme(d.theme, theme::os_dark(), &d.appearance).id;
    let cards = THEMES.into_iter().map(|t| {
        let words = theme_words(t.id).map_or((t.name, t.desc), |(n, d)| (tr(n), tr(d)));
        theme_card(t, words, t.id == shown, Message::ThemeCard(t.id))
    });
    setting_row::block(title(t!(a_themes)), t!(a_themes_desc), grid(COLUMNS, cards))
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
            let (name, badge, desc) = look_words(look.id)
                .map_or((look.name, look.badge, look.desc), |(n, b, d)| {
                    (tr(n), tr(b), tr(d))
                });
            look_card(name, badge, desc, look.preview, chosen, on)
        })
        .collect();
    let (a, b) = theme::accent_of(&state.look(LookTheme::Custom));
    cards.push(look_card(
        t!(lk_custom),
        t!(lk_custom_b),
        t!(lk_custom_d),
        &[a, b],
        now == LookTheme::Custom,
        Some(Message::Look(LookTheme::Custom)),
    ));
    cards
}

/// A theme card's name and line in the app's language (`None`: a theme the
/// table doesn't have keeps its English words).
fn theme_words(id: &str) -> Option<(S, S)> {
    Some(match id {
        "sapphire" => (S::th_sapphire, S::th_sapphire_d),
        "obsidian" => (S::th_obsidian, S::th_obsidian_d),
        "mica" => (S::th_mica, S::th_mica_d),
        "titanium" => (S::th_titanium, S::th_titanium_d),
        "amethyst" => (S::th_amethyst, S::th_amethyst_d),
        "emerald" => (S::th_emerald, S::th_emerald_d),
        "rose" => (S::th_rose, S::th_rose_d),
        "graphite" => (S::th_graphite, S::th_graphite_d),
        "midnight" => (S::th_midnight, S::th_midnight_d),
        "frost" => (S::th_frost, S::th_frost_d),
        "daylight" => (S::th_daylight, S::th_daylight_d),
        "sandstone" => (S::th_sandstone, S::th_sandstone_d),
        _ => return None,
    })
}

/// A look card's name, badge and line in the app's language (the hidden
/// looks a config may still have keep their English words).
fn look_words(id: &str) -> Option<(S, S, S)> {
    Some(match id {
        "sapphire_glow" => (S::lk_glow, S::lk_glow_b, S::lk_glow_d),
        "spectrum" => (S::lk_spectrum, S::lk_spectrum_b, S::lk_spectrum_d),
        "aurora" => (S::lk_aurora, S::lk_aurora_b, S::lk_aurora_d),
        "horizon" => (S::lk_horizon, S::lk_horizon_b, S::lk_horizon_d),
        "sunset" => (S::lk_sunset, S::lk_sunset_b, S::lk_sunset_d),
        "ocean" => (S::lk_ocean, S::lk_ocean_b, S::lk_ocean_d),
        "neon" => (S::lk_neon, S::lk_neon_b, S::lk_neon_d),
        "solar" => (S::lk_solar, S::lk_solar_b, S::lk_solar_d),
        "hud" => (S::lk_hud, S::lk_hud_b, S::lk_hud_d),
        _ => return None,
    })
}
