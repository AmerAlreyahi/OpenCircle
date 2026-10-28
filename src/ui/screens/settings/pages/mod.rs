//! Settings' pages, one list: the sidebar, the page shown and the Labs rule
//! (a page of a Labs feature is listed only while its switch is on) all read
//! `PAGES`. A new page = one entry here + one view file.
use iced::Element;

use super::legacy;
use super::state::{Message, Section, State};
use crate::labs::{Feature, Labs};

pub mod about;
mod appearance;
mod general;
mod labs;
mod look;

#[cfg(test)]
pub use look::picks as look_picks;

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
pub const PAGES: [Page; 10] = [
    page(Section::General, "General", general::view),
    page(Section::Appearance, "Appearance", appearance::view),
    page(Section::Look, "Look & behaviour", look::view),
    page(Section::Ai, "AI", legacy::rows),
    page(Section::Translate, "Translate", legacy::rows),
    page(Section::Shortcuts, "Shortcuts", legacy::rows),
    Page {
        lab: Some(Feature::Snip),
        ..page(Section::Snip, "Snip", legacy::rows)
    },
    Page {
        lab: Some(Feature::Song),
        ..page(Section::Song, "Song search", legacy::rows)
    },
    page(Section::About, "About", about::view),
    Page {
        note: Some("All off by default · Hidden when off"),
        badge: Some("Exp"),
        ..page(Section::Labs, "Labs", labs::view)
    },
];

/// The pages the sidebar lists.
pub fn visible(labs: &Labs) -> impl Iterator<Item = &'static Page> {
    let labs = *labs;
    PAGES
        .iter()
        .filter(move |p| p.lab.is_none_or(|f| labs.has(f)))
}

/// `id`'s page.
pub fn of(id: Section) -> &'static Page {
    PAGES
        .iter()
        .find(|p| p.id == id)
        .expect("every section has a page")
}
