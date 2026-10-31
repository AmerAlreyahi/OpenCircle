//! Each feature's preview at the top of its Settings page (`settings-window.js`
//! `featurePreviews`; owner, 2026-10-27: only features that ship today, a Labs
//! one only while its switch is on). One list, `HEROES`; a new preview = one
//! entry here + its clip (owner, 2026-10-28: a real recording, made by the
//! owner, `assets/previews/<clip>.webp`, docs/previews.md). A clip plays once
//! per page visit (`player`), then rests on its last frame; the clock runs
//! only meanwhile. No clip yet (owner, 2026-10-30): only the heading, no
//! Hide / Show preview and no stage; the clip's file appearing shows both.
use iced::widget::column;
use iced::{Element, Padding};

use super::state::{Message, Section, State, key_labels};
use crate::config::Config;
use crate::hotkey::QuickId;
use crate::labs::Feature;
use crate::ui::kit::hero::{self, hero};

pub mod player;

/// Under each preview (`margin-bottom: 12px`).
const BELOW: f32 = 12.0;

pub struct Hero {
    pub page: Section,
    /// Its name in `[appearance].hidden_previews`.
    pub key: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    /// Shown only while this Labs feature is on.
    pub lab: Option<Feature>,
    /// The feature's shortcut in `config` ("" = not set).
    pub shortcut: fn(&Config) -> &str,
    /// Its clip's name: `assets/previews/<clip>.webp`.
    pub clip: &'static str,
}

/// In the design's order.
pub const HEROES: [Hero; 7] = [
    Hero {
        page: Section::Look,
        key: "look",
        title: "Circle to Search",
        desc: "Circle anything and search in your browser.",
        lab: None,
        shortcut: capture,
        clip: "circle",
    },
    Hero {
        page: Section::TextReading,
        key: "text_reading",
        title: "Text reading",
        desc: "Select screen text and copy it into any app.",
        lab: None,
        shortcut: copy_text,
        clip: "text",
    },
    Hero {
        page: Section::Translate,
        key: "translate",
        title: "Translate",
        desc: "Read your screen in your language, in place.",
        lab: None,
        shortcut: translate,
        clip: "translate",
    },
    Hero {
        page: Section::Ai,
        key: "ai",
        title: "AI",
        desc: "Ask a question about what is on your screen.",
        lab: None,
        shortcut: ask,
        clip: "ai",
    },
    Hero {
        page: Section::Snip,
        key: "snip",
        title: "Snip",
        desc: "Capture, mark up and copy a picture.",
        lab: None,
        shortcut: snip,
        clip: "snip",
    },
    // Live translate has no page of its own: its rows are on Translate's.
    Hero {
        page: Section::Translate,
        key: "live",
        title: "Live translate",
        desc: "Follow game and video dialogue in your language.",
        lab: Some(Feature::Live),
        shortcut: live,
        clip: "live",
    },
    Hero {
        page: Section::Song,
        key: "song",
        title: "Song search",
        desc: "Name a song playing on your PC or microphone.",
        lab: Some(Feature::Song),
        shortcut: song,
        clip: "song",
    },
];

fn capture(c: &Config) -> &str {
    &c.shortcut
}

/// The first quick action `id` (`live`: live translate's) has.
fn quick(c: &Config, id: QuickId, live: bool) -> &str {
    (c.quick_actions.iter())
        .find(|q| q.action == id && q.live == live)
        .map_or("", |q| &q.shortcut)
}

fn copy_text(c: &Config) -> &str {
    quick(c, QuickId::CopyText, false)
}

fn translate(c: &Config) -> &str {
    quick(c, QuickId::Translate, false)
}

fn ask(c: &Config) -> &str {
    quick(c, QuickId::AiExplain, false)
}

/// The region snip (the first of the three).
fn snip(c: &Config) -> &str {
    quick(c, QuickId::Snip, false)
}

fn live(c: &Config) -> &str {
    quick(c, QuickId::Translate, true)
}

fn song(c: &Config) -> &str {
    quick(c, QuickId::SongSearch, false)
}

/// The previews on `page` (a Labs one only while its switch is on).
pub fn on(state: &State, page: Section) -> impl Iterator<Item = &'static Hero> {
    let labs = state.draft.labs;
    (HEROES.iter()).filter(move |h| h.page == page && h.lab.is_none_or(|f| labs.has(f)))
}

fn hidden(state: &State, h: &Hero) -> bool {
    (state.draft.appearance.hidden_previews.iter()).any(|k| k == h.key)
}

/// The page's shown previews that have a clip: (its `hidden_previews` name,
/// the clip).
pub fn clips(state: &State, page: Section) -> impl Iterator<Item = (&'static str, &'static [u8])> {
    (on(state, page).filter(|h| !hidden(state, h)))
        .filter_map(|h| Some((h.key, player::clip(h.clip)?)))
}

/// The page's previews, `None` when it has none.
pub fn view(state: &State) -> Option<Element<'_, Message>> {
    let heroes: Vec<_> = on(state, state.section)
        .map(|h| {
            let shown = !hidden(state, h);
            let frame = (state.players.iter()).find(|(k, _)| *k == h.key);
            let has_clip = frame.is_some() || player::clip(h.clip).is_some();
            hero(hero::Hero {
                title: h.title,
                desc: h.desc,
                lab: h.lab.is_some(),
                keys: key_labels((h.shortcut)(&state.draft)),
                shown,
                toggle: has_clip.then_some(Message::Preview(h.key, !shown)),
                replay: Message::Replay,
                frame: frame.map(|(_, p)| p.frame().clone()),
            })
        })
        .collect();
    (!heroes.is_empty()).then(|| {
        column(heroes)
            .spacing(BELOW)
            .padding(Padding::ZERO.bottom(BELOW))
            .into()
    })
}
