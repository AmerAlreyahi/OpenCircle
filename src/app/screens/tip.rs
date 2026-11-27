//! A one-time tip (R-17, design B1): the words of each `Tip` as `kit::tip_hint` runs. The
//! app draws it against the widget it hints at (`app::tips`).
use iced::{Element, Size};

use crate::shared::app_language::{S, tr};
use crate::shared::kit::tip_hint::{self, Run, Words};
use crate::shared::tips::Tip;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    GotIt,
    DontShow,
    Close,
}

fn runs(tip: Tip) -> Vec<Run<'static>> {
    match tip {
        Tip::SnipMove => vec![
            Run::Plain(tr(S::tip_snip_a)),
            Run::Key("Ctrl"),
            Run::Plain(tr(S::tip_snip_b)),
        ],
        Tip::Original => vec![
            Run::Strong(tr(S::tip_orig_a)),
            Run::Plain(tr(S::tip_orig_b)),
        ],
        Tip::Bar => vec![
            Run::Plain(tr(S::tip_bar_a)),
            Run::Key("F1"),
            Run::Plain(tr(S::tip_bar_b)),
        ],
    }
}

fn words() -> Words<'static> {
    Words {
        got_it: tr(S::tip_got_it),
        dont_show: tr(S::tip_dont_show),
    }
}

/// The tip's size (logical px).
pub fn size(tip: Tip) -> Size {
    tip_hint::size(&runs(tip), words())
}

pub fn view(tip: Tip) -> Element<'static, Msg> {
    tip_hint::view(&runs(tip), words(), Msg::GotIt, Msg::DontShow, Msg::Close)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::{golden, shot};

    /// The three drawn tips say their words, with the key as its own cap.
    #[test]
    fn each_tip_says_its_words() {
        let texts = shot::texts(view(Tip::SnipMove));
        for want in [
            "Hold",
            "Ctrl",
            "and drag to move a shape",
            "Got it",
            "Don’t show tips",
        ] {
            assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
        }
        let texts = shot::texts(view(Tip::Bar));
        assert!(texts.iter().any(|t| t == "F1"), "{texts:?}");
        let texts = shot::texts(view(Tip::Original));
        assert!(texts.iter().any(|t| t == "Original"), "{texts:?}");
    }

    /// Each tip, light and dark, English and Arabic.
    #[test]
    fn every_tip_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            for (tip, name) in [
                (Tip::SnipMove, "snip"),
                (Tip::Original, "translate"),
                (Tip::Bar, "bar"),
            ] {
                let file = format!("tip-{name}-{}", theme.id);
                failed.extend(golden::check(
                    "screens",
                    &file,
                    size(tip),
                    &[1.0, 2.0],
                    || view(tip),
                ));
                let _ar = crate::shared::kit::testing::arabic();
                let file = format!("tip-{name}-ar-{}", theme.id);
                failed.extend(golden::check("screens", &file, size(tip), &[1.0], || {
                    view(tip)
                }));
            }
        });
    }
}
