//! "What can I do?" (R-17, design B2 option B): every core feature with the key the user
//! really has for it (read from the config when the card opens, never hard-coded), as tiles
//! from `kit::what_card`. Opened by F1 in OpenCircle's own windows, by the tray's
//! "What can I do?" row and by Settings > General > Tips; Esc or the x closes it.
use iced::{Element, Size};

use crate::shared::app_language::{S, tr};
use crate::shared::config::{Config, Target};
use crate::shared::kit::choice_card::Mark;
use crate::shared::kit::icon::Icon;
use crate::shared::kit::tokens::radius;
use crate::shared::kit::what_card::{self, Tile, Words};
use crate::shared::platform::tray::{Entry, quick_shortcut};

/// The corners the window is cut to (`.wc`: 20).
pub const RADIUS: f32 = radius::DRAWER + 2.0;

#[derive(Debug, Clone)]
pub enum Msg {
    Close,
    /// "Don't show tips": every tip off; the card stays.
    DontShow,
    /// The heading was pressed: the window follows the pointer.
    Drag,
}

/// The features and their keys, as the config has them now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    tiles: Vec<(Mark2, S, S, Vec<String>)>,
}

/// `Mark` is not `Eq`; the card keeps which mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark2 {
    Logo,
    Glyph(Icon),
}

/// "Ctrl+Shift+Space" as its keys.
fn keys_of(shortcut: Option<String>) -> Vec<String> {
    (shortcut.iter())
        .flat_map(|s| s.split('+'))
        .map(str::to_string)
        .collect()
}

impl Card {
    pub fn new(cfg: &Config) -> Card {
        let quick = |id: &str| quick_shortcut(cfg, id, false, Some(Target::Circle));
        let tile = |mark, name, desc, key: Option<String>| (mark, name, desc, keys_of(key));
        Card {
            tiles: vec![
                tile(
                    Mark2::Glyph(Icon::Focus),
                    S::wc_circle,
                    S::wc_circle_d,
                    Entry::Circle.shortcut(cfg),
                ),
                tile(
                    Mark2::Glyph(Icon::Translate),
                    S::wc_translate,
                    S::wc_translate_d,
                    Entry::Translate.shortcut(cfg),
                ),
                tile(Mark2::Logo, S::wc_ai, S::wc_ai_d, quick("ai_explain")),
                tile(
                    Mark2::Glyph(Icon::Text),
                    S::wc_text,
                    S::wc_text_d,
                    quick("copy_text"),
                ),
                tile(
                    Mark2::Glyph(Icon::Scissors),
                    S::wc_snip,
                    S::wc_snip_d,
                    Entry::Snip.shortcut(cfg),
                ),
                // recording has no quick action in the config yet: its keys are the design's
                // (a quick action named "record" shows the user's own, when one is added)
                tile(
                    Mark2::Glyph(Icon::Video),
                    S::wc_record,
                    S::wc_record_d,
                    quick("record").or_else(|| Some("Alt+Shift+R".into())),
                ),
            ],
        }
    }

    /// The window (logical px).
    pub fn size(&self) -> Size {
        what_card::size(self.tiles.len())
    }

    /// The keys shown for each feature, by its name key (tests).
    #[cfg(test)]
    pub fn keys(&self, name: S) -> Vec<String> {
        (self.tiles.iter())
            .find(|t| t.1 == name)
            .map(|t| t.3.clone())
            .unwrap_or_default()
    }
}

pub fn view(card: &Card) -> Element<'_, Msg> {
    let tiles: Vec<Tile<'_>> = (card.tiles.iter())
        .map(|(mark, name, desc, keys)| Tile {
            mark: match mark {
                Mark2::Logo => Mark::Logo,
                Mark2::Glyph(i) => Mark::Glyph(i.glyph()),
            },
            name: tr(*name),
            desc: tr(*desc),
            keys: keys.clone(),
        })
        .collect();
    let words = Words {
        title: tr(S::wc_title),
        footer: tr(S::wc_footer),
        key: "F1",
        tray: tr(S::wc_tray),
        dont_show: tr(S::tip_dont_show),
    };
    what_card::view(&tiles, words, Msg::Close, Msg::DontShow, Msg::Drag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::config::Config;
    use crate::shared::kit::testing::{golden, shot};

    /// R17-AC7: the keys on the tiles are the user's own: a changed shortcut shows, an
    /// unbound action shows none (never a hard-coded default).
    #[test]
    fn the_keys_come_from_the_users_config() {
        let defaults = Card::new(&Config::default());
        assert_eq!(defaults.keys(S::wc_snip), ["Alt", "A"]);
        assert_eq!(defaults.keys(S::wc_circle), ["Ctrl", "Shift", "Space"]);
        assert_eq!(defaults.keys(S::wc_ai), ["Ctrl", "Shift", "E"]);
        assert_eq!(defaults.keys(S::wc_record), ["Alt", "Shift", "R"]);
        let mut cfg = Config {
            shortcut: "Ctrl+Alt+Q".into(),
            ..Config::default()
        };
        for q in &mut cfg.quick_actions {
            match q.action.as_str() {
                "snip" => q.shortcut = "Ctrl+Alt+S".into(),
                "copy_text" => q.shortcut.clear(),
                _ => {}
            }
        }
        let mine = Card::new(&cfg);
        assert_eq!(mine.keys(S::wc_circle), ["Ctrl", "Alt", "Q"]);
        assert_eq!(mine.keys(S::wc_snip), ["Ctrl", "Alt", "S"]);
        assert!(mine.keys(S::wc_text).is_empty(), "unbound: no keys");
        // and the drawn card says so
        let texts = shot::texts(view(&mine));
        assert!(texts.iter().any(|t| t == "Q"), "{texts:?}");
        assert!(!texts.iter().any(|t| t == "X"), "{texts:?}");
    }

    /// The card's six words and its footer, in both languages.
    #[test]
    fn the_card_names_every_feature() {
        let card = Card::new(&Config::default());
        let texts = shot::texts(view(&card));
        for want in [
            "What can I do?",
            "Circle to Search",
            "Translate",
            "Ask AI",
            "Copy text",
            "Snip",
            "Record",
            "F1",
            "Don’t show tips",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let _ar = crate::shared::kit::testing::arabic();
        let texts = shot::texts(view(&card));
        assert!(texts.iter().any(|t| t.contains("ماذا يمكنني")), "{texts:?}");
    }

    /// Light and dark, English and Arabic, 1x and 2x.
    #[test]
    fn the_card_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            let card = Card::new(&Config::default());
            let file = format!("what-card-{}", theme.id);
            failed.extend(golden::check(
                "screens",
                &file,
                card.size(),
                &[1.0, 2.0],
                || view(&card),
            ));
            let _ar = crate::shared::kit::testing::arabic();
            let file = format!("what-card-ar-{}", theme.id);
            failed.extend(golden::check("screens", &file, card.size(), &[1.0], || {
                view(&card)
            }));
        });
    }
}
