//! Settings' pages, one list: the sidebar, the page shown and the Labs rule
//! (a page of a Labs feature is listed only while its switch is on) all read
//! `PAGES`. A new page = one entry here + one view file.
use iced::Element;

use super::state::{Message, Section, State};
use crate::features::settings::labs::{Feature, Labs};
use crate::shared::app_language::S;

pub mod about;
mod ai;
mod appearance;
mod custom;
mod general;
mod history;
mod labs;
mod look;
mod privacy;
mod shortcuts;
pub mod snip;
mod song;
pub mod text_reading;
pub mod translate;

#[cfg(test)]
pub use appearance::picks as look_picks;
#[cfg(test)]
pub use general::START_LABEL;

/// The rows onboarding shows: the same functions as Settings' pages (one
/// source of truth for every setting's words and control).
pub mod rows {
    pub use super::ai::{key_row, keyed, preset_row, provider_row, url_row};
    pub use super::appearance::themes_row;
    pub use super::general::{start_row, update_row};
    pub use super::look::selection_row;
}
#[cfg(test)]
pub use song::SONG_PRIVACY;

pub struct Page {
    pub id: Section,
    pub title: S,
    /// A note at the right of the page's title.
    pub note: Option<S>,
    /// The sidebar item's badge.
    pub badge: Option<S>,
    /// Listed only while this Labs feature is on.
    pub lab: Option<Feature>,
    /// The page under its title.
    pub view: for<'a> fn(&'a State) -> Element<'a, Message>,
}

const fn page(id: Section, title: S, view: for<'a> fn(&'a State) -> Element<'a, Message>) -> Page {
    Page {
        id,
        title,
        note: None,
        badge: None,
        lab: None,
        view,
    }
}

/// In the design's order, except Shortcuts: after every feature page (owner, 2026-11-10).
pub const PAGES: [Page; 13] = [
    page(Section::General, S::nav_general, general::view),
    page(Section::Appearance, S::nav_appearance, appearance::view),
    page(Section::Look, S::nav_look, look::view),
    page(Section::Ai, S::nav_ai, ai::view),
    page(Section::TextReading, S::nav_text, text_reading::view),
    page(Section::Translate, S::nav_translate, translate::view),
    page(Section::Snip, S::nav_snip, snip::view),
    Page {
        lab: Some(Feature::Song),
        ..page(Section::Song, S::lb_song, song::view)
    },
    page(Section::History, S::nav_history, history::view),
    Page {
        note: Some(S::k_note),
        ..page(Section::Shortcuts, S::nav_shortcuts, shortcuts::view)
    },
    Page {
        note: Some(S::p_note),
        ..page(Section::Privacy, S::nav_privacy, privacy::view)
    },
    page(Section::About, S::nav_about, about::view),
    Page {
        note: Some(S::lb_note),
        badge: Some(S::nav_exp),
        ..page(Section::Labs, S::nav_labs, labs::view)
    },
];

/// The pages the sidebar lists (Text reading: Windows only, Linux has one reader).
pub fn visible(labs: &Labs) -> impl Iterator<Item = &'static Page> {
    let labs = *labs;
    PAGES
        .iter()
        .filter(move |p| p.lab.is_none_or(|f| labs.has(f)))
        .filter(|p| cfg!(windows) || p.id != Section::TextReading)
}

/// `id`'s page.
pub fn of(id: Section) -> &'static Page {
    PAGES
        .iter()
        .find(|p| p.id == id)
        .expect("every section has a page")
}
