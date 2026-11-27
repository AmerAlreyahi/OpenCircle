//! Settings' pages, one list: the sidebar, the page shown and the Labs rule
//! (a page of a Labs feature is listed only while its switch is on) all read
//! `PAGES`. A new page = one entry here + one view file.
use crate::shared::kit::icon::Icon;
use iced::Element;

use super::state::{Message, Section, State};
use crate::features::settings::labs::{Feature, Labs};
use crate::shared::app_language::S;

pub mod about;
pub(super) mod ai;
mod appearance;
mod custom;
mod extensions;
mod general;
pub(in crate::features::settings) use general::updates_words;
pub(crate) mod glossaries;
pub(super) mod history;
mod labs;
pub(super) mod look;
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
    /// The view lays out its own `section`s (the default); false = the shell frames its rows in one (a
    /// page not yet rebuilt into sections: Appearance, AI and Search, Translate, Song search, About).
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
        sectioned: true,
    }
}

/// In the design's order (Settings round 2): General, Appearance | Circle to Search (its Text reading
/// section included), AI and Search, Translate, Snip, Studio | Shortcuts, Privacy and data (History included)
/// | Extensions, About, Labs. Song (Labs) and the two pages whose content moved have no sidebar row.
pub const PAGES: [Page; 15] = [
    Page {
        ..page(
            Section::General,
            S::nav_general,
            Icon::Settings.glyph(),
            0,
            S::pg_general,
            general::view,
        )
    },
    Page {
        ..page(
            Section::Appearance,
            S::nav_appearance,
            Icon::Palette.glyph(),
            0,
            S::pg_appearance,
            appearance::view,
        )
    },
    page(
        Section::Look,
        S::nav_look,
        Icon::CircleArrowOut.glyph(),
        1,
        S::l_hero_desc,
        look::view,
    ),
    Page {
        ..page(
            Section::Ai,
            S::nav_ai_search,
            Icon::Chat.glyph(),
            1,
            S::pg_ai,
            ai::view,
        )
    },
    Page {
        nav: false,
        ..page(
            Section::TextReading,
            S::nav_text,
            Icon::Text.glyph(),
            1,
            S::l_hero_desc,
            text_reading::view,
        )
    },
    Page {
        ..page(
            Section::Translate,
            S::nav_translate,
            Icon::Translate.glyph(),
            1,
            S::tr_hero_desc,
            translate::view,
        )
    },
    page(
        Section::Snip,
        S::nav_snip,
        Icon::Scissors.glyph(),
        1,
        S::s_hero_desc,
        snip::view,
    ),
    page(
        Section::Studio,
        S::nav_studio,
        Icon::Sparkles.glyph(),
        1,
        S::pg_studio,
        studio::view,
    ),
    Page {
        lab: Some(Feature::Song),
        sectioned: false,
        ..page(
            Section::Song,
            S::lb_song,
            Icon::Music.glyph(),
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
            Icon::Shield.glyph(),
            2,
            S::p_note,
            history::view,
        )
    },
    page(
        Section::Shortcuts,
        S::nav_shortcuts,
        Icon::Keyboard.glyph(),
        2,
        S::k_note,
        shortcuts::view,
    ),
    page(
        Section::Privacy,
        S::nav_privacy,
        Icon::Shield.glyph(),
        2,
        S::p_note,
        privacy::view,
    ),
    Page {
        ..page(
            Section::Extensions,
            S::nav_extensions,
            Icon::Puzzle.glyph(),
            3,
            S::pg_extensions,
            extensions::view,
        )
    },
    Page {
        ..page(
            Section::About,
            S::nav_about,
            Icon::Info.glyph(),
            3,
            S::pg_about,
            about::view,
        )
    },
    Page {
        badge: Some(S::nav_exp),
        ..page(
            Section::Labs,
            S::nav_labs,
            Icon::Flask.glyph(),
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
