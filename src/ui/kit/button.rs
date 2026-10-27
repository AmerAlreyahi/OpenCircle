//! `button(content, Kind, on_press)`: every text button, one look per kind.
//! States change colour only (the layout is the same in every state, so a
//! hover redraws the button alone). No `on_press` = disabled.
//!
//! Sizes join with their first user: today the filled kinds are the design's
//! call to action (44 px across, radius 12), `Elevated` the small `.oc-btn`
//! (4x8, radius 9), `Quiet` bare text.
use iced::font::Weight;
use iced::widget::{button as iced_button, container, row, text};
use iced::{Background, Border, Center, Color, Element, Fill};

use super::text::{mono, weight};
use super::themes::c;
use super::tokens::{fixed, radius, size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `.oc-btn`: `--ui-elevated-bg`, lighter under the cursor.
    Elevated,
    /// The call to action in the theme's accent.
    Accent,
    /// The call to action in `SUCCESS` (done: Launch).
    Success,
    /// The call to action in `DANGER` (Uninstall).
    Danger,
    /// Text that acts: `text_sub`, `text` under the cursor, no fill.
    Quiet,
}

impl Kind {
    #[cfg(test)]
    pub const ALL: [Kind; 5] = [
        Kind::Elevated,
        Kind::Accent,
        Kind::Success,
        Kind::Danger,
        Kind::Quiet,
    ];

    /// A call to action's fill.
    fn fill(self) -> Option<Color> {
        match self {
            Kind::Accent => Some(c().accent),
            Kind::Success => Some(fixed::SUCCESS),
            Kind::Danger => Some(fixed::DANGER),
            Kind::Elevated | Kind::Quiet => None,
        }
    }
}

pub fn button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    kind: Kind,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use iced_button::Status;
    let hot = |status| matches!(status, Status::Hovered | Status::Pressed);
    let enabled = on_press.is_some();
    let b = match kind.fill() {
        // `brightness(1.1)` on hover; `scale(.99)` while pressed, drawn as a 2 px
        // edge of the window's colour taking the fill in (.99 of 370 px)
        Some(fill) => iced_button(container(content).center(Fill))
            .width(Fill)
            .height(size::CTA_H)
            .padding(0)
            .style(move |_, status| iced_button::Style {
                // dimmed when off (the design has none); a status not yet
                // known (before the first redraw) counts as at rest
                background: Some(Background::Color(match status {
                    _ if !enabled => faded(fill, 0.45),
                    Status::Hovered | Status::Pressed => brighter(fill),
                    _ => fill,
                })),
                text_color: fixed::ON_ACCENT,
                border: Border::default()
                    .rounded(radius::CARD)
                    .width(if status == Status::Pressed { 2 } else { 0 })
                    .color(c().bg),
                ..iced_button::Style::default()
            }),
        None if kind == Kind::Elevated => {
            iced_button(content)
                .padding([4.0, 8.0])
                .style(move |_, status| iced_button::Style {
                    background: Some(
                        match hot(status) {
                            true => c().elevated_hover,
                            false => c().elevated,
                        }
                        .into(),
                    ),
                    text_color: match status {
                        Status::Disabled => c().text_muted,
                        _ => c().text,
                    },
                    border: Border::default().rounded(radius::CONTROL),
                    ..iced_button::Style::default()
                })
        }
        None => iced_button(content)
            .padding(0)
            .style(move |_, status| iced_button::Style {
                text_color: match status {
                    _ if !enabled => c().text_muted,
                    Status::Hovered | Status::Pressed => c().text,
                    _ => c().text_sub,
                },
                ..iced_button::Style::default()
            }),
    };
    b.on_press_maybe(on_press).into()
}

/// A call to action's label: 13 px / 600, and an optional mono note at 75 %
/// (" · 19 MB").
pub fn cta_label<'a, Message: 'a>(label: &'a str, note: Option<String>) -> Element<'a, Message> {
    let mut line = row![
        text(label)
            .size(13)
            .font(weight(Weight::Semibold))
            .color(fixed::ON_ACCENT)
    ]
    .spacing(8)
    .align_y(Center);
    if let Some(note) = note {
        line = line.push(
            text(note)
                .size(11)
                .font(mono(Weight::Normal))
                .color(faded(fixed::ON_ACCENT, 0.75)),
        );
    }
    line.into()
}

/// `.oc-btn`'s label: 12 px / 500 (the button gives the colour).
pub fn label<'a>(label: &'a str) -> iced::widget::Text<'a> {
    use super::tokens::Role;
    text(label)
        .size(Role::CONTROL.size)
        .font(super::text::font(Role::CONTROL))
        .line_height(1.5)
}

/// `c` as CSS `brightness(1.1)` draws it.
fn brighter(c: Color) -> Color {
    let up = |v: f32| (v * 1.1).min(1.0);
    Color {
        r: up(c.r),
        g: up(c.g),
        b: up(c.b),
        a: c.a,
    }
}

/// `c` at `alpha` of its opacity.
pub(super) fn faded(c: Color, alpha: f32) -> Color {
    Color {
        a: c.a * alpha,
        ..c
    }
}
