//! A table (the glossary editor's): one rim-edged frame (radius 14, its
//! corners clip the rows), a `track` header, rows between hairlines (`hover`
//! under the pointer), a cell's room, and the row's x. Built from parts:
//! the screen only chooses what goes in each cell.
use iced::widget::{button, column, container};
use iced::{Border, Element, Fill};

use super::divider::divider;
use super::icon::{Icon, icon};
use super::themes::c;
use super::tokens::{radius, size};

/// A row's room, and the x column's width and square.
pub const CELL: [f32; 2] = [8.0, 14.0];
pub const CROSS: f32 = 32.0;

/// The frame: `header`, then each of `rows` after a hairline.
pub fn table<'a, Message: 'a>(
    header: Element<'a, Message>,
    rows: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let head = container(header).padding(CELL).style(|_| container::Style {
        background: Some(c().track.into()),
        ..container::Style::default()
    });
    let mut all = column![head];
    for r in rows {
        all = all.push(divider()).push(r);
    }
    container(all)
        .clip(true)
        .style(|_| container::Style {
            border: Border::default()
                .rounded(radius::DROPDOWN)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}

/// One row: `content` in the cell's room, `hover` while `hot`.
pub fn row<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    hot: bool,
) -> Element<'a, Message> {
    container(content)
        .padding(CELL)
        .width(Fill)
        .style(move |_| container::Style {
            background: hot.then(|| c().hover.into()),
            ..container::Style::default()
        })
        .into()
}

/// The row's x: seen while its row is under the pointer (a keyboard focus
/// shows the kit's focus fill on it); a press sends `remove`.
pub fn cross<'a, Message: Clone + 'a>(hot: bool, remove: Message) -> Element<'a, Message> {
    let glyph = icon(Icon::Close, size::ICON_SMALL).color(match hot {
        true => c().text_sub,
        false => iced::Color::TRANSPARENT,
    });
    super::focus::press(
        button(container(glyph).center(Fill))
            .width(CROSS)
            .height(CROSS)
            .padding(0)
            .style(|_, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then(|| c().hover.into()),
                text_color: c().text,
                border: Border::default().rounded(radius::CONTROL),
                ..button::Style::default()
            }),
        radius::CONTROL,
        Some(remove),
    )
    .into()
}
