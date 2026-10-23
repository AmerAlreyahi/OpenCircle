//! Shared floating components. Every floating widget is built here, styled only
//! from `theme.rs`; screens never style a container or button themselves.
//!
//! Glass later: blur the frozen frame once per overlay and paint the blurred crop
//! behind each surface (no nested/live blur).
use iced::widget::{Button, Container, Tooltip, button, container, row, space, text, tooltip};
use iced::{Background, Border, Color, Element, Font, Shadow, Vector, gradient};

use super::theme;

/// The icon font (Lucide subset, see `assets/lucide/`); load it once at start.
pub const ICON_FONT_BYTES: &[u8] = include_bytes!("../../assets/lucide/lucide-subset.ttf");
const ICON_FONT: Font = Font::with_name("lucide");

/// Every button is this tall (DESIGN.md: 44 px controls).
pub const CONTROL: f32 = 44.0;
/// Text buttons share one width (consistency rule), so bars have a known size.
pub const TEXT_BUTTON_WIDTH: f32 = 136.0;
/// Inner padding of a `surface`.
pub const SURFACE_PADDING: f32 = 6.0;

/// Fully rounded glass pill with one line of text (overlay hint, tooltips).
pub fn pill<'a, Message: 'a>(label: &'a str) -> Container<'a, Message> {
    let palette = theme::current();
    container(text(label).size(14).color(palette.text))
        .padding([8, 18])
        .style(|_| glass(999.0))
}

/// Glass panel (20 px radius): action bar, toolbars, popovers, side panels.
pub fn surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content)
        .padding(SURFACE_PADDING)
        .style(|_| glass(20.0))
}

/// Icon + label + key hint. `primary` = accent gradient (the default action),
/// otherwise quiet (transparent until hovered).
pub fn text_button<'a, Message: Clone + 'a>(
    icon: char,
    label: &'a str,
    key_hint: String,
    primary: bool,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::current();
    let (fg, hint) = if primary {
        (theme::ON_ACCENT, theme::ON_ACCENT)
    } else {
        (palette.text, palette.muted)
    };
    let content = row![
        text(icon).font(ICON_FONT).size(16).color(fg),
        text(label).size(14).color(fg),
        space::horizontal(),
        text(key_hint).size(11).color(hint),
    ]
    .spacing(8)
    .height(iced::Fill)
    .align_y(iced::Center);
    button(content)
        .width(TEXT_BUTTON_WIDTH)
        .height(CONTROL)
        .padding([0, 12])
        .on_press(on_press)
        .style(move |_, status| {
            let background = match (primary, status) {
                (true, _) => Some(accent(status)),
                (false, button::Status::Hovered) => Some(palette.hover.into()),
                (false, button::Status::Pressed) => Some(palette.pressed.into()),
                (false, _) => None,
            };
            button::Style {
                background,
                text_color: fg,
                border: Border::default().rounded(14),
                ..button::Style::default()
            }
        })
}

/// Shows `tip` in a pill beside `content` on hover; also the control's description.
pub fn with_tip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    tip: &'a str,
    side: tooltip::Position,
) -> Tooltip<'a, Message> {
    tooltip(content, pill(tip), side).gap(10)
}

/// The accent gradient; hover/press brighten it a little.
fn accent(status: button::Status) -> Background {
    let lift = match status {
        button::Status::Hovered => 0.08,
        button::Status::Pressed => 0.14,
        _ => 0.0,
    };
    let mix = |c: Color| Color {
        r: c.r + (1.0 - c.r) * lift,
        g: c.g + (1.0 - c.g) * lift,
        b: c.b + (1.0 - c.b) * lift,
        a: c.a,
    };
    gradient::Linear::new(std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, mix(theme::ACCENT_START))
        .add_stop(1.0, mix(theme::ACCENT_END))
        .into()
}

/// The one glass style: panel colour, 1 px border, soft shadow.
fn glass(radius: f32) -> container::Style {
    let palette = theme::current();
    container::Style {
        background: Some(palette.panel.into()),
        border: Border {
            color: palette.border,
            width: 1.0,
            radius: radius.into(),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}
