//! Settings round 2: the sidebar card. Pages as icon + label rows in hairline-separated groups, the open
//! one a soft accent fill with a thin accent bar at its start edge; a tail (a count, "Exp") at the row's
//! end; at the foot "Report a problem" and "Collapse" with its keys. Collapsed it is icons only
//! (`tokens::settings::NAV_THIN`), each with its name as a tooltip.
use iced::widget::{Space, button, column, container};
use iced::{Border, Center, Element, Fill, Padding};

use super::divider::divider;
use super::floating_card::floating_card;
use super::icon::glyph as icon_glyph;
use super::keycap::keycap;
use super::scroll::scroll;
use super::text::{sub, text};
use super::themes::c;
use super::tokens::settings::{NAV, NAV_THIN};
use super::tokens::{Role, radius, size};
use super::tooltip::{Why, hint};

/// One page in the list.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub label: &'a str,
    /// A lucide glyph.
    pub glyph: char,
    /// A small chip at the row's end: a count, "Exp".
    pub tail: Option<&'a str>,
    /// Rows of one group sit together; a hairline lies between groups.
    pub group: u8,
}

/// A foot item: its glyph and name, what pressing it sends, and its keys (Collapse: "Ctrl", "B").
pub struct Foot<'a, Message> {
    pub label: &'a str,
    pub glyph: char,
    pub keys: &'a [&'a str],
    pub on_press: Message,
}

/// The card's vertical padding, a hairline's room (its padding and the line), the gap between
/// rows and the rows' height: what a caller sums to size a card for its rows.
pub const CARD_PAD_V: f32 = 10.0;
pub const SEPARATOR: f32 = 13.0;
pub const ROW_GAP: f32 = 2.0;
pub const ITEM: f32 = size::NAV_ITEM;

const BAR_W: f32 = 3.0;
const BAR_H: f32 = 18.0;
const ICON_GAP: f32 = 12.0;
const PAD: f32 = 8.0;

fn item<'a, Message: Clone + 'a>(
    glyph: char,
    label: &'a str,
    tail: Option<&'a str>,
    keys: &'a [&'a str],
    active: bool,
    collapsed: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let tint = match active {
        true => c().accent,
        false => c().text_sub,
    };
    let icon = icon_glyph(glyph, size::ICON).color(tint);
    let line: Element<'a, Message> = if collapsed {
        container(icon).center(Fill).into()
    } else {
        let bar = container(Space::new())
            .width(BAR_W)
            .height(BAR_H)
            .style(move |_| container::Style {
                background: active.then(|| c().accent.into()),
                border: Border::default().rounded(BAR_W),
                ..container::Style::default()
            });
        let name = match active {
            true => text(Role::NAV, label),
            false => sub(Role::NAV, label),
        };
        let mut parts: Vec<Element<'a, Message>> = vec![
            bar.into(),
            icon.into(),
            name.into(),
            Space::new().width(Fill).into(),
        ];
        if let Some(tail) = tail {
            parts.push(super::chip::badge(tail));
        }
        // (a plain row: the keys read "Ctrl B" in every language)
        if !keys.is_empty() {
            parts.push(
                iced::widget::Row::with_children(keys.iter().map(|key| {
                    let w = 8.0 + 7.0 * key.chars().count().max(2) as f32;
                    Element::from(container(keycap(*key)).width(w))
                }))
                .spacing(2)
                .into(),
            );
        }
        super::dir::row(parts)
            .spacing(ICON_GAP)
            .height(Fill)
            .align_y(Center)
            .into()
    };
    let body = button(line)
        .width(Fill)
        .height(ITEM)
        .padding(super::dir::pad(Padding {
            left: PAD - 4.0,
            right: PAD + 2.0,
            ..Padding::ZERO
        }))
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: match (active, hot) {
                    (true, _) => Some(c().accent_soft.into()),
                    (false, true) => Some(c().hover.into()),
                    _ => None,
                },
                border: Border::default().rounded(radius::TRACK),
                ..button::Style::default()
            }
        });
    let pressed = super::focus::press(body, radius::TRACK, Some(on_press));
    let marked: Element<'a, Message> = pressed.into();
    match collapsed {
        true => hint(
            marked,
            label,
            Why::Icon,
            iced::widget::tooltip::Position::Right,
        ),
        false => marked,
    }
}

/// The card, as tall as its room: `entries` (the scrolling list), the open one `active`, `foot` under a hairline.
pub fn nav_card<'a, Message: Clone + 'a>(
    entries: &[Entry<'a>],
    active: usize,
    collapsed: bool,
    on: impl Fn(usize) -> Message,
    report: Foot<'a, Message>,
    collapse: Foot<'a, Message>,
) -> Element<'a, Message> {
    let mut list = column![].spacing(ROW_GAP);
    for (i, e) in entries.iter().enumerate() {
        if i > 0 && entries[i - 1].group != e.group {
            list = list.push(container(divider()).padding([(SEPARATOR - 1.0) / 2.0, 4.0]));
        }
        list = list.push(item(
            e.glyph,
            e.label,
            e.tail,
            &[],
            i == active,
            collapsed,
            on(i),
        ));
    }
    let foot =
        |f: Foot<'a, Message>| item(f.glyph, f.label, None, f.keys, false, collapsed, f.on_press);
    let body = column![
        scroll(list).height(Fill),
        container(divider()).padding([(SEPARATOR - 1.0) / 2.0, 4.0]),
        foot(report),
        foot(collapse),
    ]
    .spacing(ROW_GAP);
    floating_card(body)
        .padding([CARD_PAD_V, 8.0])
        .width(if collapsed { NAV_THIN } else { NAV })
        .height(Fill)
        .into()
}
