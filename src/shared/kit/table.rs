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
/// shows the kit's focus fill and the x); a press sends `remove`.
pub fn cross<'a, Message: Clone + 'a>(hot: bool, remove: Message) -> Element<'a, Message> {
    let glyph = icon(Icon::Close, size::ICON_SMALL).color(match hot {
        true => c().text_sub,
        false => iced::Color::TRANSPARENT,
    });
    let focused = (Icon::Close.glyph(), size::ICON_SMALL, c().text_sub);
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
    .glyph(focused.0, focused.1, focused.2)
    .into()
}

#[cfg(test)]
mod tests {
    use iced::Size;

    use super::super::testing::shot;

    /// A row's x is hidden until hover, but a keyboard-focused one shows its
    /// glyph, not only the focus fill.
    #[test]
    fn a_focused_x_shows_its_glyph() {
        let size = Size::new(48.0, 48.0);
        let view = || {
            iced::widget::container(super::cross(false, ()))
                .center(iced::Fill)
                .into()
        };
        let bright = |px: &[u8]| {
            // the middle 14 x 14 px: pixels of a light glyph on the dark theme
            (17..31)
                .flat_map(|y| (17..31).map(move |x| (y * 48 + x) * 4))
                .filter(|&i| px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32 > 300)
                .count()
        };
        assert_eq!(bright(&shot::render(view(), 48, 48)), 0, "hidden at rest");
        let focused = shot::after_focus(view, size);
        assert!(bright(&focused) >= 6, "{} glyph pixels", bright(&focused));
    }
}
