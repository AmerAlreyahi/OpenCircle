//! The Steps strip's parts (STEPS_BOARD_SPEC, owner approved 2026-11-02):
//! `step(picture, number, look, on_delete)` = a numbered thumbnail: a 96x60
//! `track` well, radius 6, the capture fitted inside (never cropped or
//! upscaled), an 18 px `accent` badge with a 2 px `widget` ring 3 px in from
//! the top-start corner; under the cursor, lifted or focused a 1 px `accent`
//! ring (focused: an `accent_soft` well), and under the cursor the delete x
//! at the top-end corner.
//! `close(on_press)` = the strip's and a step's small x (20 px, `muted`;
//! `hover` fill under the cursor), one look for both.
use iced::widget::{button, container, image, pin, stack, text};
use iced::{Border, Center, ContentFit, Element, Fill, Size};

use super::icon::{FONT, Icon};
use super::text::font;
use super::themes::c;
use super::tokens::{Role, radius};

/// A thumbnail's well.
pub const WELL: Size = Size::new(96.0, 60.0);
/// The strip's padding and the gap between its parts.
pub const PAD: f32 = 4.0;
/// The strip's top row (grip, x).
pub const TOP: f32 = 22.0;
/// The badge's side.
pub const BADGE: f32 = 18.0;
/// The small x's side.
pub const X: f32 = 20.0;
/// The well's corners.
const CORNER: f32 = 6.0;
/// The badge's and the x's distance from the well's corner.
const INSET: f32 = 3.0;
/// The badge's ring.
const RING: f32 = 2.0;
/// The badge's number.
const NUMBER: f32 = 9.4;

/// How a thumbnail shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    /// Under the cursor: the ring and the delete x.
    pub hover: bool,
    /// Being dragged to another place: the ring.
    pub lifted: bool,
    /// The keyboard is on it: the ring on an `accent_soft` well.
    pub focused: bool,
    /// Right to left: the badge at the top-right, the x at the top-left.
    pub rtl: bool,
}

pub fn step<'a, Message: Clone + 'a>(
    picture: image::Handle,
    number: usize,
    look: Look,
    on_delete: Option<Message>,
) -> Element<'a, Message> {
    let ring = look.hover || look.lifted || look.focused;
    let well = container(image(picture).content_fit(ContentFit::ScaleDown))
        .center(Fill)
        .width(WELL.width)
        .height(WELL.height)
        .style(move |_| container::Style {
            background: Some(
                match look.focused {
                    true => c().accent_soft,
                    false => c().track,
                }
                .into(),
            ),
            border: Border::default()
                .rounded(CORNER)
                .width(if ring { 1 } else { 0 })
                .color(c().accent),
            ..container::Style::default()
        });
    let badge = container(
        text(number.to_string())
            .size(NUMBER)
            .font(font(Role::BADGE))
            .color(super::themes::on(c().accent)),
    )
    .center(BADGE)
    .style(|_| container::Style {
        background: Some(c().accent.into()),
        border: Border::default()
            .rounded(BADGE / 2.0)
            .width(RING)
            .color(c().widget),
        ..container::Style::default()
    });
    let corner = |x: f32, side: f32| match look.rtl {
        true => WELL.width - INSET - side - x,
        false => INSET + x,
    };
    let mut layers = stack![well, pin(badge).x(corner(0.0, BADGE)).y(INSET)];
    if let Some(delete) = on_delete.filter(|_| look.hover) {
        let end = WELL.width - 2.0 * INSET - X;
        layers = layers.push(pin(close(delete)).x(corner(end, X)).y(INSET));
    }
    layers.width(WELL.width).height(WELL.height).into()
}

/// The small x: a 20 px circle, a 12 px x in `text_muted` (`text` on `hover`
/// under the cursor), on the widget surface with a `rim` edge (it reads over
/// a thumbnail too).
pub fn close<'a, Message: Clone + 'a>(on_press: Message) -> Element<'a, Message> {
    use button::Status;
    super::focus::press(
        button(
            container(text(Icon::Close.glyph()).font(FONT).size(12))
                .center(Fill)
                .align_y(Center),
        )
        .width(X)
        .height(X)
        .padding(0)
        .style(|_, status| {
            let hot = matches!(status, Status::Hovered | Status::Pressed);
            button::Style {
                background: Some(if hot { c().hover } else { c().widget }.into()),
                text_color: if hot { c().text } else { c().text_muted },
                border: Border::default()
                    .rounded(radius::FULL)
                    .width(1)
                    .color(c().rim),
                ..button::Style::default()
            }
        }),
        radius::FULL,
        Some(on_press),
    )
    .into()
}
