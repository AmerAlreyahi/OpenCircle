//! The calm error card (an AI provider's error): a small icon in the danger
//! tone, a short plain headline, the cause small and muted (at most two lines;
//! "Show details" for the rest), then one fix button and, where it helps,
//! Retry. Only the icon is red; the text keeps the normal tokens.
use iced::widget::{column, container};
use iced::{Border, Center, Element, Fill};

use super::button::{Kind, button, label, regular};
use super::icon::{Icon, icon};
use super::text;
use super::themes::c;
use super::tokens::{Role, fixed, radius, size};

const PAD: f32 = 12.0;
/// Between the icon and the text.
const ICON_GAP: f32 = 10.0;
/// Between the headline, the detail and the buttons.
const STACK: f32 = 6.0;
/// Between the fix button and Retry.
const ACTIONS_GAP: f32 = 14.0;
/// The detail's line height (times its size).
const LINE: f32 = 1.5;
/// The detail shows this many lines until it is opened.
pub const LINES: usize = 2;

/// What the card says and offers.
pub struct ErrorCard<M> {
    pub headline: String,
    /// What to do about it (F12): shown whole under the headline. With a hint
    /// the card is the one failure card: `detail` (the system's own text) is
    /// folded behind "Show details" next to the buttons. Empty: the AI chat's
    /// card, whose detail shows two lines until opened.
    pub hint: String,
    pub detail: String,
    /// The card's width (px), to clamp the detail to `LINES`.
    pub width: f32,
    /// The whole detail shows ("Show details" was clicked).
    pub open: bool,
    pub on_details: M,
    /// The kind's fix button (its label and message).
    pub fix: Option<(&'static str, M)>,
    /// A quiet Retry beside the fix (`None` when the fix is Retry itself).
    pub retry: Option<M>,
    /// The words on the card, in the app's language.
    pub words: Words,
}

/// "Show details", "Hide details" and "Retry".
#[derive(Debug, Clone, Copy)]
pub struct Words {
    pub show: &'static str,
    pub hide: &'static str,
    pub retry: &'static str,
}

/// The detail cut to `LINES` lines with "…" at `width` px, or `None` when it fits.
pub fn clamp(detail: &str, width: f32) -> Option<String> {
    let (size, font) = (super::text::size(Role::NOTE), text::font(Role::NOTE));
    let fits = |s: &str| text::lines(s, size, font, width) <= LINES;
    if fits(detail) {
        return None;
    }
    let chars: Vec<char> = detail.chars().collect();
    let cut = |n: usize| format!("{}…", chars[..n].iter().collect::<String>().trim_end());
    // the longest start that fits, by halving (shaping is the cost)
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        match fits(&cut(mid)) {
            true => lo = mid,
            false => hi = mid - 1,
        }
    }
    Some(cut(lo))
}

/// The detail's width inside a card `width` px wide.
fn detail_width(width: f32) -> f32 {
    (width - 2.0 * PAD - size::ICON - ICON_GAP).max(1.0)
}

/// The provider's own message, as received: its lines start where its own
/// content does (an English message stays left to right in an Arabic card).
fn detail_text<'a, M: 'a>(detail: String) -> Element<'a, M> {
    let start = super::dir::content_start(&detail);
    text::own(Role::NOTE, detail)
        .color(c().text_muted)
        .line_height(LINE)
        .width(Fill)
        .align_x(start)
        .into()
}

pub fn error_card<'a, M: Clone + 'a>(card: ErrorCard<M>) -> Element<'a, M> {
    let headline = text::text(Role::ROW_TITLE, card.headline);
    let mut body = column![headline]
        .spacing(STACK)
        .width(Fill)
        .align_x(super::dir::start());
    let folded = !card.hint.is_empty();
    let word = match card.open {
        true => card.words.hide,
        false => card.words.show,
    };
    let mut toggle = None;
    if folded {
        body = body.push(detail_text(card.hint));
        if !card.detail.is_empty() {
            toggle = Some(button(label(word), Kind::Quiet, Some(card.on_details)));
        }
    } else if !card.detail.is_empty() {
        let short = (!card.open)
            .then(|| clamp(&card.detail, detail_width(card.width)))
            .flatten();
        let long = card.open || short.is_some();
        let shown: Element<'a, M> = match short {
            Some(short) => detail_text(short),
            None => detail_text(card.detail.clone()),
        };
        body = body.push(shown);
        if long {
            body = body.push(button(label(word), Kind::Quiet, Some(card.on_details)));
        }
    }
    let mut actions = super::dir::row([]).spacing(ACTIONS_GAP).align_y(Center);
    if let Some((name, msg)) = card.fix {
        actions = actions.push(regular(name, false, Some(msg)));
    }
    if let Some(retry) = card.retry {
        actions = actions.push(button(label(card.words.retry), Kind::Quiet, Some(retry)));
    }
    if let Some(toggle) = toggle {
        actions = actions.push(toggle);
    }
    body = body.push(container(actions).padding([4, 0]));
    if folded && card.open && !card.detail.is_empty() {
        body = body.push(
            container(
                text::own(Role::NOTE, card.detail)
                    .font(super::text::mono(iced::font::Weight::Normal))
                    .color(c().text_muted)
                    .width(Fill),
            )
            .padding([6, 8])
            .width(Fill)
            .style(|_| container::Style {
                background: Some(c().track.into()),
                border: Border::default().rounded(radius::TRACK),
                ..container::Style::default()
            }),
        );
    }
    let mark = container(icon(Icon::Alert, size::ICON).color(fixed::DANGER)).padding([1, 0]);
    container(crate::dir_row![mark, body].spacing(ICON_GAP))
        .padding(PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().card_idle.into()),
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_detail_shows_two_lines_until_opened() {
        let long = "HTTP 404: This model models/old-model is no longer available. Please update \
                    your code to use a newer model for the latest features and improvements.";
        let short = clamp(long, 300.0).expect("longer than two lines at 300 px");
        assert!(short.ends_with('…') && short.len() < long.len(), "{short}");
        let (size, font) = (super::text::size(Role::NOTE), text::font(Role::NOTE));
        assert_eq!(text::lines(&short, size, font, 300.0), LINES);
        assert_eq!(clamp("HTTP 503", 300.0), None, "fits: shown whole");
        // its card: the cut detail and Show details, then (opened) the whole one
        let card = |open| {
            let card = error_card(ErrorCard {
                headline: "This model isn't available".into(),
                hint: String::new(),
                detail: long.into(),
                width: 300.0 + 2.0 * PAD + size::ICON + ICON_GAP,
                open,
                on_details: (),
                fix: Some(("Choose another model", ())),
                retry: Some(()),
                words: Words {
                    show: "Show details",
                    hide: "Hide details",
                    retry: "Retry",
                },
            });
            super::super::testing::shot::texts(card)
        };
        let closed = card(false);
        assert!(
            closed.contains(&short) && closed.iter().any(|t| t == "Show details"),
            "{closed:?}"
        );
        assert!(
            closed.iter().any(|t| t == "Choose another model")
                && closed.iter().any(|t| t == "Retry")
        );
        let open = card(true);
        assert!(
            open.iter().any(|t| t == long) && open.iter().any(|t| t == "Hide details"),
            "{open:?}"
        );
    }
}

#[cfg(test)]
mod f12_tests {
    use super::*;

    /// F12-1: with a hint the card is the one failure card: headline, what to
    /// do, the fix button; the system's own text stays folded until Show details.
    #[test]
    fn f12_the_system_text_is_folded_behind_show_details() {
        let card = |open| {
            let card = error_card(ErrorCard {
                headline: "Download could not finish".into(),
                hint: "Try again in a minute.".into(),
                detail: "response 1 (cancelled)".into(),
                width: 400.0,
                open,
                on_details: (),
                fix: Some(("Try again", ())),
                retry: None,
                words: Words {
                    show: "Show details",
                    hide: "Hide details",
                    retry: "Retry",
                },
            });
            super::super::testing::shot::texts(card)
        };
        let has = |t: &[String], s: &str| t.iter().any(|x| x == s);
        let closed = card(false);
        for want in [
            "Download could not finish",
            "Try again in a minute.",
            "Try again",
            "Show details",
        ] {
            assert!(has(&closed, want), "{want}: {closed:?}");
        }
        assert!(!has(&closed, "response 1 (cancelled)"), "{closed:?}");
        let open = card(true);
        assert!(has(&open, "response 1 (cancelled)"), "{open:?}");
        assert!(has(&open, "Hide details"), "{open:?}");
    }
}
