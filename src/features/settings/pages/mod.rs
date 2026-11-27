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
mod extensions;
mod general;
pub(crate) mod glossaries;
mod history;
mod labs;
mod look;
mod privacy;
mod shortcuts;
pub mod snip;
mod song;
mod studio;
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
}
#[cfg(test)]
pub use song::SONG_PRIVACY;

pub struct Page {
    pub id: Section,
    pub title: S,
    /// The sidebar item's badge.
    pub badge: Option<S>,
    /// Listed only while this Labs feature is on.
    pub lab: Option<Feature>,
    /// The page under its title.
    pub view: for<'a> fn(&'a State) -> Element<'a, Message>,
    /// Settings round 2: the sidebar icon (also the page header's tile), the group (a hairline
    /// between groups) and the ONE line under the title. A page with no sidebar row (`nav` false:
    /// its content lives on another page) is still found by `of`.
    pub glyph: char,
    pub group: u8,
    pub line: S,
    pub nav: bool,
    /// The view lays out its own `section`s; false = the shell frames its rows in one (a page not yet
    /// rebuilt into sections).
    pub sectioned: bool,
}

const fn page(
    id: Section,
    title: S,
    glyph: char,
    group: u8,
    line: S,
    view: for<'a> fn(&'a State) -> Element<'a, Message>,
) -> Page {
    Page {
        id,
        title,
        badge: None,
        lab: None,
        view,
        glyph,
        group,
        line,
        nav: true,
        sectioned: false,
    }
}

/// In the design's order (Settings round 2): General, Appearance | Circle to Search (its Text reading
/// section included), AI and Search, Translate, Snip, Studio | Shortcuts, Privacy and data (History included)
/// | Extensions, About, Labs. Song (Labs) and the two pages whose content moved have no sidebar row.
pub const PAGES: [Page; 15] = [
    Page {
        sectioned: true,
        ..page(
            Section::General,
            S::nav_general,
            '\u{e157}',
            0,
            S::pg_general,
            general::view,
        )
    },
    page(
        Section::Appearance,
        S::nav_appearance,
        '\u{e1dc}',
        0,
        S::pg_appearance,
        appearance::view,
    ),
    page(
        Section::Look,
        S::nav_look,
        '\u{e3fe}',
        1,
        S::l_hero_desc,
        look::view,
    ),
    page(
        Section::Ai,
        S::nav_ai_search,
        '\u{e119}',
        1,
        S::pg_ai,
        ai::view,
    ),
    Page {
        nav: false,
        ..page(
            Section::TextReading,
            S::nav_text,
            '\u{e197}',
            1,
            S::l_hero_desc,
            text_reading::view,
        )
    },
    page(
        Section::Translate,
        S::nav_translate,
        '\u{e103}',
        1,
        S::tr_hero_desc,
        translate::view,
    ),
    page(
        Section::Snip,
        S::nav_snip,
        '\u{e151}',
        1,
        S::s_hero_desc,
        snip::view,
    ),
    page(
        Section::Studio,
        S::nav_studio,
        '\u{e416}',
        1,
        S::pg_studio,
        studio::view,
    ),
    Page {
        lab: Some(Feature::Song),
        ..page(
            Section::Song,
            S::lb_song,
            '\u{e125}',
            1,
            S::lb_song_d,
            song::view,
        )
    },
    Page {
        nav: false,
        ..page(
            Section::History,
            S::nav_history,
            '\u{e15b}',
            2,
            S::p_note,
            history::view,
        )
    },
    page(
        Section::Shortcuts,
        S::nav_shortcuts,
        '\u{e283}',
        2,
        S::k_note,
        shortcuts::view,
    ),
    page(
        Section::Privacy,
        S::nav_privacy,
        '\u{e15b}',
        2,
        S::p_note,
        privacy::view,
    ),
    Page {
        sectioned: true,
        ..page(
            Section::Extensions,
            S::nav_extensions,
            '\u{e29b}',
            3,
            S::pg_extensions,
            extensions::view,
        )
    },
    page(
        Section::About,
        S::nav_about,
        '\u{e0fe}',
        3,
        S::pg_about,
        about::view,
    ),
    Page {
        badge: Some(S::nav_exp),
        ..page(
            Section::Labs,
            S::nav_labs,
            '\u{e0da}',
            3,
            S::lb_note,
            labs::view,
        )
    },
];

/// The pages the sidebar lists (Text reading: Windows only, Linux has one reader).
pub fn visible(labs: &Labs) -> impl Iterator<Item = &'static Page> {
    let labs = *labs;
    PAGES
        .iter()
        .filter(move |p| p.lab.is_none_or(|f| labs.has(f)))
        .filter(|p| p.nav)
}

/// `id`'s page.
pub fn of(id: Section) -> &'static Page {
    PAGES
        .iter()
        .find(|p| p.id == id)
        .expect("every section has a page")
}
