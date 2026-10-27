//! The setup's components, now thin calls into the kit (`ui::kit`); the setup
//! moves onto the kit directly and this file goes (UI plan step 36).
use iced::{Element, Fill, Padding};

use super::kit::button::{self, Kind};
use super::kit::surface::{Material, surface};
use super::kit::tokens::radius;
use super::kit::{
    chip as kit_chip, divider as kit_divider, field as kit_field, progress, segmented,
};

pub use super::kit::header::compact as header;
pub use super::kit::text::{mono, weight};

pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    kit_divider::divider()
}

/// The big call to action (Install / Launch / Uninstall).
pub fn cta<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    kind: Kind,
    on_press: Message,
) -> Element<'a, Message> {
    button::button(content, kind, Some(on_press))
}

pub use button::cta_label;

/// Quiet text that acts ("Customize install ▾").
pub fn text_link<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    on_press: Message,
) -> Element<'a, Message> {
    button::button(content, Kind::Quiet, Some(on_press))
}

/// Off / On between two labels, the thumb gliding from `from` (`glide` 0 → 1).
pub fn segmented<'a, Message: Clone + 'a>(
    labels: [&'a str; 2],
    chosen: bool,
    from: bool,
    glide: f32,
    on: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    segmented::boolean(labels, chosen, from, glide, on)
}

pub use kit_field::field;

/// `.oc-btn`, small (the installer's "…"). No `on_press` = dimmed.
pub fn small_button<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    button::button(button::label(label), Kind::Elevated, on_press)
}

pub use kit_chip::chip;

/// A recessed panel (the installer's progress): 14 px inside (8 px below: the
/// progress bar's glow takes 6).
pub fn panel<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    surface(Material::Recessed, radius::CARD, content)
        .width(Fill)
        .padding(Padding {
            top: 14.0,
            right: 14.0,
            bottom: 8.0,
            left: 14.0,
        })
        .into()
}

pub use progress::bar as progress_bar;

/// The window's body: `content` on the window's colour inside its rim.
pub fn window<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    surface(Material::Window, radius::WINDOW, content)
        .padding(1)
        .width(Fill)
        .height(Fill)
        .into()
}
