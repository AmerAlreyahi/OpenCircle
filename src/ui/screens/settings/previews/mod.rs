//! Each feature's preview at the top of its Settings page (`settings-window.js`
//! `featurePreviews`; owner, 2026-10-27: only features that ship today, a Labs
//! one only while its switch is on). One list, `HEROES`; a new preview = one
//! entry here + its clip (owner, 2026-10-28: a real recording, made by the
//! owner; docs/previews.md). The app bundles a still per clip
//! (`assets/previews/stills/<clip>.webp`); "Play preview" downloads the page's
//! 1080p clips once (`online`, owner 2026-11-04), then they play in turn and
//! loop while the page is open (`player`), also at later visits, from disk.
//! No still yet (owner, 2026-10-30): only the heading, no Hide / Show preview
//! and no stage; the still's file appearing shows both.
use iced::widget::column;
use iced::{Element, Padding};

use super::state::{Message, Section, State, key_labels};
use crate::config::{Config, Provider};
use crate::hotkey::QuickId;
use crate::labs::Feature;
use crate::ui::kit::hero::{self, hero};

mod av1;
pub mod online;
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
    /// Its clips' names for this config, in play order (all in turn, in a
    /// loop); a name with no still is absent.
    pub clips: fn(&Config) -> &'static [&'static str],
}

/// The AI page: the in-app chat's clip with an AI provider, the browser's
/// without one (it switches with the setting).
fn ai_clips(c: &Config) -> &'static [&'static str] {
    match c.ai.provider {
        Provider::Anthropic | Provider::OpenaiCompatible => &["ai-chat"],
        _ => &["ai-browser"],
    }
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
        clips: |_| &["circle-1", "circle-2", "circle-3"],
    },
    Hero {
        page: Section::TextReading,
        key: "text_reading",
        title: "Text reading",
        desc: "Select screen text and copy it into any app.",
        lab: None,
        shortcut: copy_text,
        clips: |_| &["text"],
    },
    Hero {
        page: Section::Translate,
        key: "translate",
        title: "Translate",
        desc: "Read your screen in your language, in place.",
        lab: None,
        shortcut: translate,
        clips: |_| &["translate"],
    },
    Hero {
        page: Section::Ai,
        key: "ai",
        title: "Ask AI",
        desc: "Ask a question about what is on your screen.",
        lab: None,
        shortcut: ask,
        clips: ai_clips,
    },
    Hero {
        page: Section::Snip,
        key: "snip",
        title: "Snip",
        desc: "Capture, mark up and copy a picture.",
        lab: None,
        shortcut: snip,
        clips: |_| &["snip-blur", "snip-comment", "snip-long"],
    },
    // Live translate has no page of its own: its rows are on Translate's.
    Hero {
        page: Section::Translate,
        key: "live",
        title: "Live translate",
        desc: "Follow game and video dialogue in your language.",
        lab: Some(Feature::Live),
        shortcut: live,
        clips: |_| &["live"],
    },
    Hero {
        page: Section::Song,
        key: "song",
        title: "Song search",
        desc: "Name a song playing on your PC or microphone.",
        lab: Some(Feature::Song),
        shortcut: song,
        clips: |_| &["song"],
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

/// The clips `h` plays now, in play order: those with a still (empty = none,
/// so no preview).
pub fn chosen(h: &Hero, config: &Config) -> Vec<&'static str> {
    chosen_from(h, config, |n| player::still(n).is_some())
}

fn chosen_from(h: &Hero, config: &Config, has: impl Fn(&str) -> bool) -> Vec<&'static str> {
    (h.clips)(config)
        .iter()
        .copied()
        .filter(|n| has(n))
        .collect()
}

/// The page's shown previews that have a clip: (its `hidden_previews` name,
/// its clips).
pub fn clips(
    state: &State,
    page: Section,
) -> impl Iterator<Item = (&'static str, Vec<&'static str>)> {
    (on(state, page).filter(|h| !hidden(state, h)))
        .map(|h| (h.key, chosen(h, &state.draft)))
        .filter(|(_, c)| !c.is_empty())
}

/// The calm line under a stage whose download failed.
pub const FAILED: &str = "Couldn't download the preview. Check your connection and try again.";

/// The page's previews, `None` when it has none.
pub fn view(state: &State) -> Option<Element<'_, Message>> {
    let reduced = crate::ui::kit::motion::reduced();
    let heroes: Vec<_> = on(state, state.section)
        .map(|h| {
            let shown = !hidden(state, h);
            let player = (state.players.iter()).find(|(k, _)| *k == h.key);
            let has_clip = player.is_some() || !chosen(h, &state.draft).is_empty();
            // the clips aren't on disk yet: the click that downloads them
            let play = match state.fetching.contains(&h.key) {
                true => Some(("Downloading…", None)),
                false => (player.is_some_and(|(_, p)| !p.on_disk()) && !reduced)
                    .then_some(("Play preview", Some(Message::Fetch(h.key)))),
            };
            hero(hero::Hero {
                title: h.title,
                desc: h.desc,
                lab: h.lab.is_some(),
                keys: key_labels((h.shortcut)(&state.draft)),
                shown,
                toggle: has_clip.then_some(Message::Preview(h.key, !shown)),
                play,
                note: state.failed.contains(&h.key).then_some(FAILED),
                frame: player.and_then(|(_, p)| p.frame().cloned()),
                width: (state.stage.clone(), state.scale, state.win_h),
                pager: player.and_then(|(_, p)| {
                    let (at, n) = p.position();
                    (n > 1).then_some((at, n, Message::Step(h.key, -1), Message::Step(h.key, 1)))
                }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::screens::settings::pages::about::{CLIP_CREDITS, clip_credits};

    fn hero(key: &str) -> &'static Hero {
        HEROES.iter().find(|h| h.key == key).unwrap()
    }

    /// A fake bundle of stills.
    fn bundle(have: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |n| have.contains(&n)
    }

    #[test]
    fn a_page_without_files_has_no_clips_and_a_missing_one_is_skipped() {
        let c = Config::default();
        assert!(chosen_from(hero("text_reading"), &c, bundle(&[])).is_empty());
        assert!(chosen_from(hero("live"), &c, bundle(&["circle-1"])).is_empty());
        // Snip with only its long capture bundled: just that one
        let got = chosen_from(hero("snip"), &c, bundle(&["snip-long"]));
        assert_eq!(got, ["snip-long"]);
    }

    /// Every page plays all its clips, in order, in a loop (owner,
    /// 2026-11-04: no more one per opening).
    #[test]
    fn a_page_plays_every_clip_in_order() {
        let c = Config::default();
        let b = bundle(&[
            "circle-3",
            "circle-1",
            "circle-2",
            "snip-long",
            "snip-blur",
            "snip-comment",
        ]);
        let look = chosen_from(hero("look"), &c, &b);
        assert_eq!(look, ["circle-1", "circle-2", "circle-3"]);
        let snip = chosen_from(hero("snip"), &c, &b);
        assert_eq!(snip, ["snip-blur", "snip-comment", "snip-long"]);
    }

    /// AI: the chat clip with a provider, the browser's without.
    #[test]
    fn ai_picks_its_clip_by_provider() {
        let b = bundle(&["ai-chat", "ai-browser"]);
        let mut c = Config::default();
        assert_eq!(chosen_from(hero("ai"), &c, &b), ["ai-browser"]);
        for p in [Provider::Anthropic, Provider::OpenaiCompatible] {
            c.ai.provider = p;
            assert_eq!(chosen_from(hero("ai"), &c, &b), ["ai-chat"]);
        }
    }

    /// What ships: Look, Text, AI (both providers) and Snip have clips;
    /// Translate is held for a re-recording (owner, 2026-11-03: the take showed
    /// translate bugs); Live and Song stay hidden until theirs arrive. Every
    /// clip with a still is pinned for download, and only those.
    #[test]
    fn the_shipped_clips_are_what_the_pages_name() {
        let mut c = Config::default();
        let has = |k, c: &Config| !chosen(hero(k), c).is_empty();
        assert!(has("look", &c) && !has("translate", &c) && has("snip", &c));
        assert!(has("ai", &c));
        c.ai.provider = Provider::Anthropic;
        assert!(has("ai", &c));
        assert!(has("text_reading", &c) && !has("live", &c) && !has("song", &c));
        assert_eq!(chosen(hero("snip"), &c).len(), 3);
        assert_eq!(chosen(hero("look"), &c).len(), 3);
        let stills: Vec<_> = (online::CLIPS.iter())
            .map(|f| f.name.strip_suffix(".ivf").unwrap())
            .collect();
        for name in &stills {
            assert!(player::still(name).is_some(), "{name} has no still");
        }
        assert_eq!(stills.len(), 9);
        for f in &online::CLIPS {
            assert!(f.size <= 1_600_000, "{} too big for a preview", f.name);
            assert!(f.sha256.len() == 64 && f.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    /// About lists the credits of exactly the bundled clips.
    #[test]
    fn about_credits_follow_the_bundled_clips() {
        assert!(clip_credits(|_| false).is_empty());
        assert_eq!(clip_credits(|_| true).len(), CLIP_CREDITS.len());
        let demo_snips = clip_credits(|c| c == "snip-blur" || c == "snip-comment");
        assert!(demo_snips.is_empty(), "the demo snips need no credit");
        let ai = clip_credits(|c| c == "ai-chat");
        assert!(ai.iter().any(|c| c.text.contains("IRENA (2025)")));
        assert!(
            ai.iter().all(|c| !c.text.contains("Google")),
            "no Google note"
        );
        let browser = clip_credits(|c| c == "ai-browser");
        assert!(
            browser
                .iter()
                .any(|c| c.text.contains("not affiliated with"))
        );
        // COFFEE: bundled, credited under the Unsplash License
        assert!(player::still("text").is_some());
        assert!(
            CLIP_CREDITS
                .iter()
                .any(|c| c.text.contains("Unsplash License"))
        );
        // what ships: 3 photos, COFFEE, OWID solar, Google note, OWID tourism,
        // the caveat (Wikipedia comes back with the translate clip)
        let shipped = clip_credits(|c| player::still(c).is_some());
        assert_eq!(shipped.len(), 8);
        assert!(shipped.iter().all(|c| !c.text.contains("Matti Blume")));
        let all = clip_credits(|_| true);
        let sa = (all.iter().find(|c| c.text.contains("Matti Blume"))).unwrap();
        assert!(sa.text.contains("MIT licence"));
        assert!(sa.links.iter().any(|l| l.0 == "Licence text"));
        for (_, url) in shipped.iter().flat_map(|c| c.links) {
            assert!(url.starts_with("https://"), "{url}");
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/LICENSES/CC-BY-SA-4.0.txt");
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.starts_with("Attribution-ShareAlike 4.0 International"));
    }
}
