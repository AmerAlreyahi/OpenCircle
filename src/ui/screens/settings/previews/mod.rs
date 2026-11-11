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
use crate::app_language::{S, tr};
use crate::t;
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
    pub title: S,
    pub desc: S,
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
        title: S::l_hero,
        desc: S::l_hero_desc,
        lab: None,
        shortcut: capture,
        clips: |_| &["circle-1", "circle-2", "circle-3"],
    },
    Hero {
        page: Section::TextReading,
        key: "text_reading",
        title: S::nav_text,
        desc: S::t_hero_desc,
        lab: None,
        shortcut: copy_text,
        clips: |_| &["text"],
    },
    Hero {
        page: Section::Translate,
        key: "translate",
        title: S::nav_translate,
        desc: S::tr_hero_desc,
        lab: None,
        shortcut: translate,
        clips: |_| &["translate-page", "translate-2", "translate-3"],
    },
    Hero {
        page: Section::Ai,
        key: "ai",
        title: S::nav_ai,
        desc: S::ai_hero_desc,
        lab: None,
        shortcut: ask,
        clips: ai_clips,
    },
    Hero {
        page: Section::Snip,
        key: "snip",
        title: S::nav_snip,
        desc: S::s_hero_desc,
        lab: None,
        shortcut: snip,
        clips: |_| &["snip-blur", "snip-comment", "snip-long"],
    },
    // Live translate has no page of its own: its rows are on Translate's.
    Hero {
        page: Section::Translate,
        key: "live",
        title: S::lb_live,
        desc: S::lb_live_hero,
        lab: Some(Feature::Live),
        shortcut: live,
        clips: |_| &["live"],
    },
    Hero {
        page: Section::Song,
        key: "song",
        title: S::lb_song,
        desc: S::lb_song_hero,
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
pub const FAILED: S = S::c_preview_failed;

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
                true => Some((t!(c_downloading), None)),
                false => (player.is_some_and(|(_, p)| !p.on_disk()) && !reduced)
                    .then_some((t!(c_play_preview), Some(Message::Fetch(h.key)))),
            };
            hero(hero::Hero {
                title: tr(h.title),
                desc: tr(h.desc),
                lab: h.lab.is_some(),
                keys: key_labels((h.shortcut)(&state.draft)),
                shown,
                toggle: has_clip.then_some(Message::Preview(h.key, !shown)),
                play,
                note: state.failed.contains(&h.key).then_some(tr(FAILED)),
                frame: player.and_then(|(_, p)| p.frame().cloned()),
                width: (state.stage.clone(), state.scale, state.win_h),
                pager: player.and_then(|(_, p)| {
                    let (at, n) = p.position();
                    (n > 1).then_some((at, n, Message::Step(h.key, -1), Message::Step(h.key, 1)))
                }),
                words: hero::Words {
                    labs: t!(nav_labs),
                    no_shortcut: t!(c_no_shortcut),
                    hide: t!(c_hide_preview),
                    show: t!(c_show_preview),
                    preview: t!(c_preview),
                    previous: t!(c_prev_preview),
                    next: t!(c_next_preview),
                },
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

    /// What ships: Look, Text, Translate, AI (both providers) and Snip have
    /// clips; Live and Song stay hidden until theirs arrive. Every
    /// clip with a still is pinned for download, and only those.
    #[test]
    fn the_shipped_clips_are_what_the_pages_name() {
        let mut c = Config::default();
        let has = |k, c: &Config| !chosen(hero(k), c).is_empty();
        assert!(has("look", &c) && has("translate", &c) && has("snip", &c));
        assert!(has("ai", &c));
        c.ai.provider = Provider::Anthropic;
        assert!(has("ai", &c));
        assert!(has("text_reading", &c) && !has("live", &c) && !has("song", &c));
        assert_eq!(chosen(hero("snip"), &c).len(), 3);
        assert_eq!(chosen(hero("look"), &c).len(), 3);
        assert_eq!(
            chosen(hero("translate"), &c),
            ["translate-page", "translate-2", "translate-3"]
        );
        let stills: Vec<_> = (online::CLIPS.iter())
            .map(|f| f.name.strip_suffix(".ivf").unwrap())
            .collect();
        for name in &stills {
            assert!(player::still(name).is_some(), "{name} has no still");
        }
        assert_eq!(stills.len(), 12);
        // the web page replaced translate-4 (owner, 2026-11-10): no pin, no still
        assert!(!stills.contains(&"translate-4") && player::still("translate-4").is_none());
        for f in &online::CLIPS {
            assert!(f.size <= 1_800_000, "{} too big for a preview", f.name);
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
        // what ships: 3 photos, COFFEE, the Wikipedia page, the game screens,
        // OWID solar, the Google note, OWID tourism, the caveat
        let shipped = clip_credits(|c| player::still(c).is_some());
        assert_eq!(shipped.len(), 10);
        let page = clip_credits(|c| c == "translate-page");
        assert!(
            page.iter()
                .any(|c| c.text.contains("Plante d")
                    && c.links.iter().any(|l| l.0 == "CC BY-SA 4.0"))
        );
        let games = clip_credits(|c| c == "translate-3");
        assert!(games.iter().any(|c| c.text.contains("no real game")));
        assert!(games.iter().all(|c| !c.text.contains("YouTube")));
        for (_, url) in shipped.iter().flat_map(|c| c.links) {
            assert!(url.starts_with("https://"), "{url}");
        }
    }
}
