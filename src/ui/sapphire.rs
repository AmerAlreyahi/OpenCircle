//! The new design's shared components (`newdesing/styles/ui-kit.css`), styled
//! only from `theme::sapphire`, solid look. The setup is their first user; the
//! redesign (N-18) moves the other screens onto them. Like `widgets.rs`, the
//! only place these are styled: screens call the functions.
use iced::font::Weight;
use iced::widget::text::LineHeight;
use iced::widget::{Space, button, container, row, text, text_input};
use iced::{Background, Border, Center, Color, Element, Fill, Font, Length};

use super::theme::sapphire::*;
use super::widgets::{MONO, ui_font};

/// The UI font at a weight (the design's 500 / 600).
pub fn weight(weight: Weight) -> Font {
    Font {
        weight,
        ..ui_font()
    }
}

pub fn mono(weight: Weight) -> Font {
    Font { weight, ..MONO }
}

/// `c` at `alpha` of its opacity.
fn faded(c: Color, alpha: f32) -> Color {
    Color {
        a: c.a * alpha,
        ..c
    }
}

/// A 1 px `--app-divider` hairline across.
pub fn divider<'a, Message: 'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .style(|_| container::Style::default().background(DIVIDER))
        .into()
}

/// The big call to action (the installer's Install / Launch): 44 px, radius 12,
/// `fill` (`ACCENT` or `SUCCESS`) with `ON_ACCENT` text, `brightness(1.1)` on hover.
pub fn cta<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    fill: Color,
    on_press: Message,
) -> Element<'a, Message> {
    button(container(content).center(Fill))
        .width(Fill)
        .height(44)
        .padding(0)
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => brighter(fill),
                _ => fill,
            })),
            text_color: ON_ACCENT,
            border: Border::default().rounded(12),
            ..button::Style::default()
        })
        .into()
}

/// A CTA's label: 13 px / 600, and an optional mono note at 75 % (" · 19 MB").
pub fn cta_label<'a, Message: 'a>(label: &'a str, note: Option<String>) -> Element<'a, Message> {
    let mut line = row![
        text(label)
            .size(13)
            .font(weight(Weight::Semibold))
            .color(ON_ACCENT)
    ]
    .spacing(8)
    .align_y(Center);
    if let Some(note) = note {
        line = line.push(
            text(note)
                .size(11)
                .font(mono(Weight::Normal))
                .color(faded(ON_ACCENT, 0.75)),
        );
    }
    line.into()
}

/// Quiet text that acts (the installer's "Customize install ▾"): `TEXT_SUB`,
/// `TEXT` on hover, no fill.
pub fn text_link<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    on_press: Message,
) -> Element<'a, Message> {
    button(content)
        .padding(0)
        .on_press(on_press)
        .style(|_, status| button::Style {
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => TEXT,
                _ => TEXT_SUB,
            },
            ..button::Style::default()
        })
        .into()
}

/// `.oc-track` with `.oc-pill`s: one choice of a few (Off / On). Radius 10 track
/// (`TRACK`, divider edge, 3 px inside), 7.5 px pills, the chosen one `ELEVATED`.
pub fn segmented<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
    options: &[(T, &'a str)],
    chosen: T,
    on: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let pills = options.iter().map(|&(value, label)| {
        let active = value == chosen;
        button(
            text(label)
                .size(12)
                .font(weight(Weight::Medium))
                .line_height(1.25),
        )
        .padding([5, 12])
        .on_press(on(value))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: match (active, hovered) {
                    (true, _) => Some(ELEVATED.into()),
                    (false, true) => Some(HOVER.into()),
                    _ => None,
                },
                text_color: if active || hovered { TEXT } else { TEXT_SUB },
                border: Border::default()
                    .rounded(7.5)
                    .width(1)
                    .color(Color::TRANSPARENT),
                ..button::Style::default()
            }
        })
        .into()
    });
    container(row(pills).spacing(2))
        .padding(3)
        .style(|_| container::Style {
            background: Some(TRACK.into()),
            border: Border::default().rounded(10).width(1).color(DIVIDER),
            ..container::Style::default()
        })
        .into()
}

/// `.oc-input`, small (the installer's folder): mono 10.5 px on `TRACK`, a rim
/// edge that turns `ACCENT` while focused. No `on_input` = read-only.
pub fn field<'a, Message: Clone + 'a>(
    value: &str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
) -> Element<'a, Message> {
    let mut input = text_input("", value)
        .font(mono(Weight::Normal))
        .size(10.5)
        .line_height(LineHeight::Absolute(16.into()))
        .padding([4, 8])
        .on_submit(on_submit)
        .style(|_, status| text_input::Style {
            background: TRACK.into(),
            border: Border::default().rounded(10).width(1).color(match status {
                text_input::Status::Focused { .. } => ACCENT,
                _ => RIM,
            }),
            icon: TEXT_SUB,
            placeholder: TEXT_MUTED,
            value: match status {
                text_input::Status::Disabled => TEXT_SUB,
                _ => TEXT,
            },
            selection: ACCENT_SOFT,
        });
    if let Some(on_input) = on_input {
        input = input.on_input(on_input);
    }
    input.into()
}

/// `.oc-btn`, small (the installer's "…"): `ELEVATED`, radius 9, 12 px / 500.
/// No `on_press` = dimmed.
pub fn small_button<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(12)
            .font(weight(Weight::Medium))
            .line_height(1.5),
    )
    .padding([4, 8])
    .on_press_maybe(on_press)
    .style(|_, status| button::Style {
        background: Some(
            match status {
                button::Status::Hovered | button::Status::Pressed => ELEVATED_HOVER,
                _ => ELEVATED,
            }
            .into(),
        ),
        text_color: match status {
            button::Status::Disabled => TEXT_MUTED,
            _ => TEXT,
        },
        border: Border::default().rounded(9),
        ..button::Style::default()
    })
    .into()
}

/// A mono chip on `ACCENT_SOFT` (the version): 10.5 px / 500, 2x8 px, round.
pub fn chip<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(
        text(label)
            .size(10.5)
            .font(mono(Weight::Medium))
            .color(ACCENT),
    )
    .padding([2, 8])
    .height(20)
    .center_y(20)
    .style(|_| container::Style {
        background: Some(ACCENT_SOFT.into()),
        border: Border::default().rounded(999),
        ..container::Style::default()
    })
    .into()
}

/// A recessed panel (the installer's progress): radius 12, `TRACK_GLASS`, a
/// divider edge, 14 px inside.
pub fn panel<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Fill)
        .padding(14)
        .style(|_| container::Style {
            background: Some(TRACK_GLASS.into()),
            border: Border::default().rounded(12).width(1).color(DIVIDER),
            ..container::Style::default()
        })
        .into()
}

/// A 6 px progress bar, `share` (0..1) filled with `ACCENT` on a `HOVER` track.
pub fn progress_bar<'a, Message: 'a>(share: f32) -> Element<'a, Message> {
    let share = share.clamp(0.0, 1.0);
    let filled = container(Space::new().width(Fill).height(6))
        .width(Length::FillPortion((share * 1000.0).round() as u16))
        .style(|_| container::Style {
            background: Some(ACCENT.into()),
            border: Border::default().rounded(999),
            ..container::Style::default()
        });
    let rest = Space::new()
        .width(Length::FillPortion(((1.0 - share) * 1000.0).round() as u16))
        .height(6);
    container(row![filled, rest])
        .width(Fill)
        .height(6)
        .style(|_| container::Style {
            background: Some(HOVER.into()),
            border: Border::default().rounded(999),
            ..container::Style::default()
        })
        .into()
}

/// A window's close caption button (`.win-caption-btn.win-close`): 46 px wide,
/// `height` tall, the X in `TEXT_SUB`, red with a white X on hover.
pub fn close_button<'a, Message: Clone + 'a>(
    height: f32,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        container(
            text(super::widgets::CLOSE)
                .font(super::widgets::ICON_FONT)
                .size(14),
        )
        .center(Fill),
    )
    .width(46)
    .height(height)
    .padding(0)
    .on_press(on_press)
    .style(|_, status| {
        let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hot.then_some(super::theme::CLOSE_HOVER.into()),
            text_color: if hot {
                super::theme::ON_CLOSE_HOVER
            } else {
                TEXT_SUB
            },
            ..button::Style::default()
        }
    })
    .into()
}
