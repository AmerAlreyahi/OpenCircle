//! `.oc-sidebar`: a window's page list, 168 px wide on `sidebar` with a
//! divider at its right. Items (`.oc-nav-item`, 36 px, 12.5 px / 500) are
//! `text_sub`, `text` on `hover` under the cursor; the active one is `text`
//! with a 5 px accent dot, over ONE elevated highlight (`.oc-nav-glide`) that
//! glides from item `from` to `active` as `glide` goes 0 → 1 (the caller's
//! clock: `motion::GLIDE`, standard). A footer line under a hairline.
use iced::font::Weight;
use iced::widget::{Space, button, column, container, row, stack, text};
use iced::{Border, Center, Element, Fill, Padding};

use super::button::faded;
use super::chip::badge;
use super::divider::divider;
use super::scroll::scroll;
use super::themes::c;
use super::tokens::{Role, radius, size, space};

/// One page in the list: its name and an optional badge ("Exp").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item<'a> {
    pub label: &'a str,
    pub badge: Option<&'a str>,
}

/// Between two items (`space-y-0.5`).
const GAP: f32 = 2.0;
/// The active item's dot and an item's side padding.
const DOT: f32 = 5.0;
const ITEM_PAD: f32 = 12.0;
/// The footer: 10.5 px at half strength, 8 px under its hairline and at its sides.
const FOOTER: f32 = 10.5;
const FOOTER_PAD: f32 = 8.0;

/// Where item `i` starts, down the list.
fn top(i: usize) -> f32 {
    i as f32 * (size::NAV_ITEM + GAP)
}

pub fn sidebar<'a, Message: Clone + 'a>(
    items: &[Item<'a>],
    active: usize,
    from: usize,
    glide: f32,
    on: impl Fn(usize) -> Message,
    footer: [&'a str; 2],
) -> Element<'a, Message> {
    let glide = glide.clamp(0.0, 1.0);
    let y = top(from) + (top(active) - top(from)) * glide;
    let highlight = column![
        Space::new().height(y),
        container(Space::new().width(Fill).height(size::NAV_ITEM)).style(|_| container::Style {
            background: Some(c().elevated.into()),
            border: Border::default()
                .rounded(radius::CONTROL)
                .width(1)
                .color(c().elevated_border),
            ..container::Style::default()
        }),
    ];
    let list = column(
        items
            .iter()
            .enumerate()
            .map(|(i, item)| nav_item(*item, i == active, on(i))),
    )
    .spacing(GAP);
    let nav = scroll(stack![highlight, list]).height(Fill);
    let small = |s: &'a str| {
        text(s)
            .size(FOOTER)
            .font(super::text::weight(Weight::Normal))
            .color(faded(c().text, 0.5))
    };
    let foot = column![
        divider(),
        row![small(footer[0]), Space::new().width(Fill), small(footer[1])]
            .padding(Padding::from([0.0, FOOTER_PAD]).top(FOOTER_PAD)),
    ];
    let body = container(column![nav, foot].spacing(GAP))
        .padding(space::SIDEBAR_PAD)
        .width(size::SIDEBAR - 1.0)
        .height(Fill)
        .style(|_| container::Style::default().background(c().sidebar));
    let edge = container(Space::new().width(1).height(Fill))
        .style(|_| container::Style::default().background(c().divider));
    row![body, edge].height(Fill).into()
}

fn nav_item<'a, Message: Clone + 'a>(
    item: Item<'a>,
    active: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let label = text(item.label)
        .size(Role::NAV.size)
        .font(super::text::font(Role::NAV));
    let dot = container(Space::new().width(DOT).height(DOT)).style(move |_| container::Style {
        // the active item's; the others keep its room (the badge stays put)
        background: active.then(|| faded(c().accent, 0.9).into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    });
    // `justify-between`: the label left, the dot right, a badge between them
    let mut line = row![label, Space::new().width(Fill)]
        .height(Fill)
        .align_y(Center);
    if let Some(b) = item.badge {
        line = line.push(badge(b)).push(Space::new().width(Fill));
    }
    let line = line.push(dot);
    button(line)
        .width(Fill)
        .height(size::NAV_ITEM)
        .padding([0.0, ITEM_PAD])
        .on_press(on_press)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: (hot && !active).then(|| c().hover.into()),
                text_color: if active || hot {
                    c().text
                } else {
                    c().text_sub
                },
                border: Border::default().rounded(radius::CONTROL),
                ..button::Style::default()
            }
        })
        .into()
}
