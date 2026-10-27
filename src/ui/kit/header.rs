//! A window's own header (no OS title bar): the compact one, 36 px (the
//! regular 40 px one joins with Settings).
use iced::font::Weight;
use iced::widget::{button, container, mouse_area, row, text};
use iced::{Element, Fill};

use super::icon::{Icon, icon};
use super::text::mono;
use super::themes::c;
use super::tokens::{fixed, size};

/// `title` (mono, muted) that drags the window (`on_drag` on press), and the
/// close caption button.
pub fn compact<'a, Message: Clone + 'a>(
    title: &'a str,
    on_drag: Message,
    on_close: Message,
) -> Element<'a, Message> {
    const H: f32 = size::HEADER_COMPACT;
    row![
        mouse_area(
            container(
                text(title)
                    .font(mono(Weight::Medium))
                    .size(11)
                    .color(c().text_muted)
            )
            .padding([0, 14])
            .center_y(H)
            .width(Fill)
        )
        .on_press(on_drag),
        close(H, on_close),
    ]
    .height(H)
    .into()
}

/// `.win-caption-btn.win-close`: 46 px wide, `height` tall, the X in
/// `text_sub`, red with a white X under the cursor.
fn close<'a, Message: Clone + 'a>(height: f32, on_press: Message) -> Element<'a, Message> {
    button(container(icon(Icon::Close, 14.0)).center(Fill))
        .width(size::CAPTION_BUTTON)
        .height(height)
        .padding(0)
        .on_press(on_press)
        .style(|_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hot.then_some(fixed::CLOSE_HOVER.into()),
                text_color: if hot { fixed::ON_CLOSE } else { c().text_sub },
                ..button::Style::default()
            }
        })
        .into()
}
