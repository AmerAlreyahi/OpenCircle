//! The toast pill (`.oc-toast`, design 11.2): an elevated pill, a dot and
//! one 12 px line, padding 14x8. Exactly `size(label)` big: its window is the
//! pill (cut to its corners, so nothing shows around it).
use iced::widget::{Space, container};
use iced::{Border, Center, Element, Fill, Size};

use super::text::{font, width};
use super::themes::c;
use super::tokens::{Role, fixed, radius, size};

/// What the dot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Done (`SUCCESS`): copied, saved.
    Ok,
    /// It failed (`DANGER`; the design's dashed edge, which the renderer can't draw).
    Error,
    /// Just so you know (the accent): the first-run notice.
    Neutral,
}

/// `ok` = done, else it failed.
impl From<bool> for Tone {
    fn from(ok: bool) -> Tone {
        if ok { Tone::Ok } else { Tone::Error }
    }
}

const PAD: [f32; 2] = [8.0, 14.0];
const GAP: f32 = 8.0;
const LINE: f32 = 16.0;

/// The pill's size for `label` (logical px, whole): its window's size.
pub fn size(label: &str) -> Size {
    let text = width(label, super::text::size(Role::NOTE), font(Role::NOTE));
    Size::new(
        (2.0 * PAD[1] + size::TOAST_DOT + GAP + text + 2.0).ceil(),
        2.0 * PAD[0] + LINE + 2.0,
    )
}

/// `with_action`'s size: the pill and its action after the label.
pub fn size_with_action(label: &str, action: &str) -> Size {
    size_with_actions(label, &[action])
}

/// `with_actions`' size: the pill and its action words after the label.
pub fn size_with_actions(label: &str, actions: &[&str]) -> Size {
    let pill = size(label);
    let more: f32 = actions
        .iter()
        .map(|a| GAP + width(a, super::text::size(Role::NOTE), font(Role::CTA)))
        .sum();
    Size::new((pill.width + more).ceil(), pill.height)
}

/// The pill with `label`, corners of `radius` (the window's cut).
pub fn toast<'a, Message: 'a>(label: &'a str, tone: Tone, radius: f32) -> Element<'a, Message> {
    made(label, tone, radius, Vec::new(), size(label))
}

/// The pill with an action after its label ("Steps cleared · Undo"): the
/// action's word in `accent`, `text` under the cursor.
pub fn with_action<'a, Message: Clone + 'a>(
    label: &'a str,
    tone: Tone,
    radius: f32,
    action: &'a str,
    on_press: Message,
) -> Element<'a, Message> {
    with_actions(label, tone, radius, vec![(action, on_press)])
}

/// The pill with several actions after its label ("Exported . Open  Show in
/// folder"), each an accent word.
pub fn with_actions<'a, Message: Clone + 'a>(
    label: &'a str,
    tone: Tone,
    radius: f32,
    actions: Vec<(&'a str, Message)>,
) -> Element<'a, Message> {
    use iced::widget::button;
    let words: Vec<&str> = actions.iter().map(|(a, _)| *a).collect();
    let at = size_with_actions(label, &words);
    let words = actions.into_iter().map(|(action, on_press)| {
        let word = button(
            super::text::text(Role::NOTE, action)
                .font(font(Role::CTA))
                .line_height(iced::Pixels(LINE)),
        )
        .padding(0)
        .style(|_, status| button::Style {
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => c().text,
                _ => c().accent,
            },
            ..button::Style::default()
        });
        super::focus::press(word, super::tokens::radius::CONTROL, Some(on_press)).into()
    });
    made(label, tone, radius, words.collect(), at)
}

fn made<'a, Message: 'a>(
    label: &'a str,
    tone: Tone,
    radius: f32,
    actions: Vec<Element<'a, Message>>,
    at: Size,
) -> Element<'a, Message> {
    let dot = match tone {
        Tone::Ok => fixed::SUCCESS,
        Tone::Error => fixed::DANGER,
        Tone::Neutral => c().accent,
    };
    let dot =
        container(Space::new().width(size::TOAST_DOT).height(size::TOAST_DOT)).style(move |_| {
            container::Style {
                background: Some(dot.into()),
                border: Border::default().rounded(size::TOAST_DOT / 2.0),
                ..container::Style::default()
            }
        });
    let line = super::text::text(Role::NOTE, label)
        .line_height(iced::Pixels(LINE))
        .wrapping(iced::widget::text::Wrapping::None);
    let lead: [Element<'a, Message>; 2] = [dot.into(), line.into()];
    let content = super::dir::row(lead.into_iter().chain(actions))
        .spacing(GAP)
        .align_y(Center);
    material(container(content).padding(PAD), radius, at)
}

/// The pill's style E tray material round `content`, exactly `at` big.
fn material<'a, Message: 'a>(
    content: container::Container<'a, Message>,
    radius: f32,
    at: Size,
) -> Element<'a, Message> {
    super::surface::tray(
        false,
        radius,
        super::surface::Join::None,
        content.center_y(Fill),
    )
    .width(at.width)
    .height(at.height)
    .into()
}

/// The two-line pill (the update's, the design's board "Update"): a round
/// accent mark (`lead`, `LEAD` px, in `accent_soft`), a 13 px / 600 title over
/// an 11.5 px line, and its buttons at the end, padding 14 / 12.
const BANNER_PAD: [f32; 2] = [14.0, 12.0];
const BANNER_GAP: f32 = 12.0;
const BANNER_H: f32 = 64.0;
/// Without buttons (a status: "Updating…").
const BANNER_LOW: f32 = 60.0;
const LINES_GAP: f32 = 2.0;
/// The round mark.
pub const LEAD: f32 = 32.0;

/// `banner`'s size for `title`, `line` and buttons `actions` px wide (0: none).
pub fn banner_size(title: &str, line: &str, actions: f32) -> Size {
    let text = width(title, super::text::size(Role::CTA), font(Role::CTA)).max(width(
        line,
        super::text::size(Role::ROW_DESC),
        font(Role::ROW_DESC),
    ));
    let end = if actions > 0.0 {
        BANNER_GAP + actions
    } else {
        0.0
    };
    Size::new(
        (BANNER_PAD[0] + LEAD + BANNER_GAP + text + end + BANNER_PAD[1] + 2.0).ceil(),
        if actions > 0.0 { BANNER_H } else { BANNER_LOW },
    )
}

pub fn banner<'a, Message: 'a>(
    lead: impl Into<Element<'a, Message>>,
    title: &'a str,
    line: &'a str,
    actions: Option<Element<'a, Message>>,
    radius: f32,
    at: Size,
) -> Element<'a, Message> {
    let lead = container(lead).center(LEAD).style(|_| container::Style {
        background: Some(c().accent_soft.into()),
        text_color: Some(c().accent),
        border: Border::default().rounded(radius::TRACK),
        ..container::Style::default()
    });
    let none = iced::widget::text::Wrapping::None;
    let lines = iced::widget::column![
        super::text::text(Role::CTA, title).wrapping(none),
        super::text::sub(Role::ROW_DESC, line).wrapping(none),
    ]
    .spacing(LINES_GAP)
    .align_x(super::dir::start())
    .width(Fill);
    let content = super::dir::row(
        [Some(lead.into()), Some(lines.into()), actions]
            .into_iter()
            .flatten(),
    )
    .spacing(BANNER_GAP)
    .align_y(Center);
    let pad = super::dir::pad(iced::Padding::ZERO.left(BANNER_PAD[0]).right(BANNER_PAD[1]));
    material(container(content).padding(pad), radius, at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::golden;

    /// The window fits the line: longer text, a wider pill; Arabic is measured
    /// as shaped (not by its character count).
    #[test]
    fn the_pill_fits_its_line() {
        let short = size("Copied");
        let long = size("Saved to C:\\Users\\me\\Pictures\\OpenCircle");
        assert!(long.width > short.width + 100.0, "{short:?} {long:?}");
        assert_eq!((short.height, long.height), (34.0, 34.0));
        let ar = size("تم النسخ");
        assert!(ar.width > 50.0 && ar.width < short.width + 40.0, "{ar:?}");
        assert_eq!(Tone::from(true), Tone::Ok);
        assert_eq!(Tone::from(false), Tone::Error);
    }

    /// W108: the "Exported" card, two actions after the label, English and Arabic, both themes.
    #[test]
    fn the_card_with_two_actions_matches_its_golden_images() {
        golden::each_theme(|theme, failed| {
            for ar in [false, true] {
                let _dir = ar.then(crate::shared::kit::testing::arabic);
                let (label, actions) = match ar {
                    false => ("Exported · steps-1.md", ["Open", "Show in folder"]),
                    true => ("تم التصدير · steps-1.md", ["فتح", "إظهار في المجلد"]),
                };
                let at = size_with_actions(label, &actions);
                let file = format!("toast-exported{}-{}", if ar { "-ar" } else { "" }, theme.id);
                let view = || {
                    let pairs = actions.iter().map(|a| (*a, ())).collect();
                    with_actions(label, Tone::Ok, 10.0, pairs)
                };
                failed.extend(golden::check("screens", &file, at, &[1.0], view));
            }
        });
    }
}
