//! A window's own header (no OS title bar): the regular one, 40 px (Settings:
//! logo, title, a status slot, minimise / maximise / close), and the compact
//! one, 36 px (the setup: a mono title and close).
use iced::font::Weight;
use iced::widget::{Space, button, container, image, mouse_area, row, text};
use iced::{Center, Element, Fill};

use super::icon::{Icon, icon};
use super::text::{mono, weight};
use super::themes::c;
use super::tokens::{fixed, size};

/// What a regular header reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caption {
    /// Pressed on the free area: the window follows the pointer (or, the second
    /// press of a double-click, maximises).
    Drag,
    Minimise,
    /// Maximise, or restore when maximised.
    Maximise,
    Close,
}

/// The Settings window's header (`h-10 pl-3.5`): the 16 px `logo` and `title`
/// (12 px / 600) drag the window; `status` (a header pill) sits 8 px left of
/// the caption buttons.
pub fn regular<'a, Message: Clone + 'a>(
    logo: image::Handle,
    title: &'a str,
    status: impl Into<Element<'a, Message>>,
    maximised: bool,
    on: impl Fn(Caption) -> Message,
) -> Element<'a, Message> {
    const H: f32 = size::HEADER;
    const PAD: f32 = 14.0;
    const LOGO_GAP: f32 = 10.0;
    const STATUS_GAP: f32 = 8.0;
    const GLYPH: f32 = 12.0;
    let name = row![
        image(logo).width(size::ICON).height(size::ICON),
        text(title)
            .size(12)
            .font(weight(Weight::Semibold))
            .color(c().text),
    ]
    .spacing(LOGO_GAP)
    .align_y(Center);
    let restore = if maximised {
        Icon::Restore
    } else {
        Icon::Maximise
    };
    row![
        mouse_area(container(name).padding([0.0, PAD]).center_y(H).width(Fill))
            .on_press(on(Caption::Drag)),
        status.into(),
        Space::new().width(STATUS_GAP),
        caption(Icon::Minimise, GLYPH, H, false, on(Caption::Minimise)),
        caption(restore, GLYPH, H, false, on(Caption::Maximise)),
        caption(Icon::Close, GLYPH, H, true, on(Caption::Close)),
    ]
    .height(H)
    .align_y(Center)
    .into()
}

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
        caption(Icon::Close, 14.0, H, true, on_close),
    ]
    .height(H)
    .into()
}

/// `.win-caption-btn`: 46 px wide, `height` tall, the glyph in `text_sub`;
/// under the cursor `text` on `hover`, the close one red with a white glyph.
fn caption<'a, Message: Clone + 'a>(
    glyph: Icon,
    px: f32,
    height: f32,
    close: bool,
    on_press: Message,
) -> Element<'a, Message> {
    button(container(icon(glyph, px)).center(Fill))
        .width(size::CAPTION_BUTTON)
        .height(height)
        .padding(0)
        .on_press(on_press)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let (background, text_color) = match (hot, close) {
                (true, true) => (Some(fixed::CLOSE_HOVER.into()), fixed::ON_CLOSE),
                (true, false) => (Some(c().hover.into()), c().text),
                (false, _) => (None, c().text_sub),
            };
            button::Style {
                background,
                text_color,
                ..button::Style::default()
            }
        })
        .into()
}
