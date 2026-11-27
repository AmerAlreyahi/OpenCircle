//! Settings round 2: the card that hangs from the title bar's search. Empty search: "Most used" quick
//! picks with their current values; typed: the hits. Each row = the page's icon, a title, "Page › Section",
//! an optional value chip and an arrow; the chosen one a soft accent fill; a hint line at the foot.
use iced::widget::{Space, button, column, container, opaque};
use iced::{Border, Center, Element, Fill, Padding};

use super::divider::divider;
use super::icon::glyph;
use super::text::{muted, sub};
use super::themes::c;
use super::tokens::{Role, radius, size};

/// One row.
pub struct Hit<Message> {
    pub glyph: char,
    pub title: String,
    /// Byte ranges of `title` the query matched (drawn in the accent, bold).
    pub marks: Vec<(usize, usize)>,
    pub crumb: String,
    /// The setting's current value (the quick picks').
    pub value: Option<String>,
    pub selected: bool,
    pub on_press: Message,
}

const TILE: f32 = 28.0;

fn row<'a, Message: Clone + 'a>(hit: Hit<Message>) -> Element<'a, Message> {
    let tile = container(glyph(hit.glyph, size::ICON_SMALL).color(c().text_sub))
        .center(TILE)
        .style(|_| container::Style {
            border: Border::default().rounded(8).width(1).color(c().divider),
            ..container::Style::default()
        });
    let title = hit.title;
    let words = column![
        super::text::marked(Role::ROW_TITLE, &title, &hit.marks),
        sub(Role::ROW_DESC, hit.crumb)
    ];
    let mut parts: Vec<Element<'a, Message>> = vec![
        tile.into(),
        container(words)
            .width(Fill)
            .align_x(super::dir::start())
            .into(),
    ];
    if let Some(value) = hit.value {
        parts.push(
            container(sub(Role::ROW_DESC, value))
                .padding([2.0, 8.0])
                .style(|_| container::Style {
                    background: Some(c().hover.into()),
                    border: Border::default()
                        .rounded(radius::CONTROL)
                        .width(1)
                        .color(c().divider),
                    ..container::Style::default()
                })
                .into(),
        );
    }
    parts.push(
        glyph('\u{e04d}', size::ICON_SMALL)
            .color(c().text_muted)
            .into(),
    );
    let selected = hit.selected;
    let on_press = hit.on_press;
    super::focus::press(
        button(super::dir::row(parts).spacing(10).align_y(Center))
            .width(Fill)
            .padding([6.0, 10.0])
            .style(move |_, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: (selected || hot).then(|| match selected {
                        true => c().accent_soft.into(),
                        false => c().hover.into(),
                    }),
                    border: Border::default().rounded(radius::CONTROL),
                    ..button::Style::default()
                }
            }),
        radius::CONTROL,
        Some(on_press),
    )
    .into()
}

/// The card: `head` (left label, right hint) over `hits`, `none` when there are none, `foot` (keys, count).
pub fn search_card<'a, Message: Clone + 'a>(
    head: Option<(&'a str, &'a str)>,
    hits: Vec<Hit<Message>>,
    none: &'a str,
    foot: (&'a str, String),
) -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    if let Some((left, right)) = head {
        list = list.push(
            container(super::dir::row([
                muted(Role::SECTION, left).into(),
                Space::new().width(Fill).into(),
                sub(Role::ROW_DESC, right).into(),
            ]))
            .padding(Padding::from([4.0, 10.0])),
        );
    }
    if hits.is_empty() {
        list = list.push(container(sub(Role::ROW_DESC, none)).padding([8.0, 10.0]));
    }
    for hit in hits {
        list = list.push(row(hit));
    }
    list = list.push(divider()).push(
        container(super::dir::row([
            muted(Role::CARD_DESC, foot.0).into(),
            Space::new().width(Fill).into(),
            muted(Role::CARD_DESC, foot.1).into(),
        ]))
        .padding([4.0, 10.0]),
    );
    opaque(container(list).padding(8).width(Fill).style(|_| {
        container::Style {
            background: Some(c().widget.into()),
            border: Border::default()
                .rounded(radius::DROPDOWN)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        }
    }))
}
