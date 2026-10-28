//! Settings' pages, one list: the sidebar, the page shown and the Labs rule
//! (a page of a Labs feature is listed only while its switch is on) all read
//! `PAGES`. A new page = one entry here + one view file.
use iced::Element;

use super::state::{Message, Section, State};
use crate::labs::{Feature, Labs};

pub mod about;
mod ai;
mod appearance;
mod custom;
mod general;
mod labs;
mod look;
mod privacy;
mod shortcuts;
mod snip;
mod song;
pub mod text_reading;
mod translate;

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
    pub use super::look::{after_row, selection_row};
}
#[cfg(test)]
pub use song::SONG_PRIVACY;

pub struct Page {
    pub id: Section,
    pub title: &'static str,
    /// A note at the right of the page's title.
    pub note: Option<&'static str>,
    /// The sidebar item's badge.
    pub badge: Option<&'static str>,
    /// Listed only while this Labs feature is on.
    pub lab: Option<Feature>,
    /// The page under its title.
    pub view: for<'a> fn(&'a State) -> Element<'a, Message>,
}

const fn page(
    id: Section,
    title: &'static str,
    view: for<'a> fn(&'a State) -> Element<'a, Message>,
) -> Page {
    Page {
        id,
        title,
        note: None,
        badge: None,
        lab: None,
        view,
    }
}

/// In the design's order.
pub const PAGES: [Page; 12] = [
    page(Section::General, "General", general::view),
    page(Section::Appearance, "Appearance", appearance::view),
    page(Section::Look, "Look & behaviour", look::view),
    page(Section::Ai, "AI", ai::view),
    page(Section::TextReading, "Text reading", text_reading::view),
    page(Section::Translate, "Translate", translate::view),
    Page {
        note: Some(shortcuts::NOTE),
        ..page(Section::Shortcuts, "Shortcuts", shortcuts::view)
    },
    Page {
        lab: Some(Feature::Snip),
        ..page(Section::Snip, "Snip", snip::view)
    },
    Page {
        lab: Some(Feature::Song),
        ..page(Section::Song, "Song search", song::view)
    },
    Page {
        note: Some(privacy::NOTE),
        ..page(Section::Privacy, "Privacy", privacy::view)
    },
    page(Section::About, "About", about::view),
    Page {
        note: Some("All off by default · Hidden when off"),
        badge: Some("Exp"),
        ..page(Section::Labs, "Labs", labs::view)
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
