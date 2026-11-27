//! Settings round 2: a page's section, a bordered 12 px box on the page card with a small upper-case
//! label, its rows (`setting_row::plain`) ruled between, none after the last. No shadow of its own.
use iced::widget::{Space, column, container};
use iced::{Border, Element, Fill};

use super::divider::divider;
use super::text::muted;
use super::themes::c;
use super::tokens::{Role, radius};

/// The section's padding `[top and bottom, sides]`, and the label's gap under it.
const PAD: [f32; 2] = [16.0, 20.0];
const LABEL_GAP: f32 = 6.0;

/// `rows` (built by the closure: its `setting_row`s draw no rule of their own) under `label` (`None`: no label); `after` sits at the label's end ("See how it works").
pub fn section<'a, Message: 'a>(
    label: Option<&'a str>,
    after: Option<Element<'a, Message>>,
    rows: impl FnOnce() -> Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let rows = super::setting_row::ruled_by_section(rows);
    let mut list = column![];
    if label.is_some() || after.is_some() {
        let head = super::dir::row([
            muted(Role::SECTION, label.unwrap_or("")).into(),
            Space::new().width(Fill).into(),
        ])
        .push(after)
        .align_y(iced::Center);
        list = list.push(head).push(Space::new().height(LABEL_GAP));
    }
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            list = list.push(divider());
        }
        list = list.push(row);
    }
    container(list)
        .padding(PAD)
        .width(Fill)
        .style(|_| container::Style {
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// `content` (rows that rule themselves) in the section box, no label: a page not yet laid out in
/// sections.
pub fn framed<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    section(None, None, || vec![content.into()])
}
