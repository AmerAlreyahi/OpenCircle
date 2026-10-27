//! The new design's shared components (`newdesing/styles/ui-kit.css`), styled
//! only from `theme::sapphire`, solid look. The setup is their first user; the
//! redesign (N-18) moves the other screens onto them. Like `widgets.rs`, the
//! only place these are styled: screens call the functions.
use iced::font::Weight;
use iced::widget::text::LineHeight;
use iced::widget::{Space, button, container, row, stack, text, text_input};
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
        .style(|_| container::Style::default().background(t().divider))
        .into()
}

/// The big call to action (the installer's Install / Launch): 44 px, radius 12,
/// `fill` (`t().accent` or `SUCCESS`) with `ON_ACCENT` text, `brightness(1.1)` on
/// hover, `scale(.99)` while pressed (drawn: a 2 px edge of the window's colour
/// takes the fill in, as .99 of 370 px is; the label stays).
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
        .style(move |_, status| {
            let pressed = status == button::Status::Pressed;
            button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => brighter(fill),
                    _ => fill,
                })),
                text_color: ON_ACCENT,
                border: Border::default()
                    .rounded(12)
                    .width(if pressed { 2 } else { 0 })
                    .color(t().bg),
                ..button::Style::default()
            }
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

/// Quiet text that acts (the installer's "Customize install ▾"): `t().text_sub`,
/// `t().text` on hover, no fill.
pub fn text_link<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    on_press: Message,
) -> Element<'a, Message> {
    button(content)
        .padding(0)
        .on_press(on_press)
        .style(|_, status| button::Style {
            text_color: match status {
                button::Status::Hovered | button::Status::Pressed => t().text,
                _ => t().text_sub,
            },
            ..button::Style::default()
        })
        .into()
}

/// A pill's width: its label (12 px / 500) and 12 px each side, at least 42 px
/// (the design's Off / On are 45 / 42 px).
fn pill_width(label: &str) -> f32 {
    (24.0 + 7.0 * label.chars().count() as f32).max(42.0)
}

/// `.oc-track` with `.oc-pills` and the gliding thumb: one choice of a few (Off /
/// On). Radius 10 track (`t().track`, divider edge, 3 px inside), 7.5 px pills;
/// the thumb (`t().elevated`) slides from `from` to `chosen` as `glide` goes 0 → 1
/// (the caller's clock: 200 ms standard-out, `motion::GLIDE`).
pub fn segmented<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
    options: &[(T, &'a str)],
    chosen: T,
    from: T,
    glide: f32,
    on: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    const GAP: f32 = 2.0;
    const PILL: f32 = 27.0; // 12 px x 1.25 and 6 px above and below
    let at = |value: T| {
        let mut x = 0.0;
        for &(v, label) in options {
            if v == value {
                return (x, pill_width(label));
            }
            x += pill_width(label) + GAP;
        }
        (0.0, 0.0)
    };
    let ((x0, w0), (x1, w1)) = (at(from), at(chosen));
    let glide = glide.clamp(0.0, 1.0);
    let (x, w) = (x0 + (x1 - x0) * glide, w0 + (w1 - w0) * glide);
    let thumb = row![
        Space::new().width(x),
        container(Space::new().width(w).height(PILL)).style(|_| container::Style {
            background: Some(t().elevated.into()),
            border: Border::default()
                .rounded(7.5)
                .width(1)
                .color(t().elevated_border),
            ..container::Style::default()
        }),
    ];
    let pills = options.iter().map(|&(value, label)| {
        let active = value == chosen;
        button(
            text(label)
                .size(12)
                .font(weight(Weight::Medium))
                .line_height(1.25)
                .align_x(Center)
                .width(Fill),
        )
        .width(pill_width(label))
        .padding([6, 0])
        .height(PILL)
        .on_press(on(value))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: (!active && hovered).then(|| t().hover.into()),
                text_color: if active || hovered {
                    t().text
                } else {
                    t().text_sub
                },
                border: Border::default().rounded(7.5),
                ..button::Style::default()
            }
        })
        .into()
    });
    let pills = row(pills).spacing(GAP).height(PILL);
    container(stack![thumb, pills])
        .padding(3)
        .style(|_| container::Style {
            background: Some(t().track.into()),
            border: Border::default().rounded(10).width(1).color(t().divider),
            ..container::Style::default()
        })
        .into()
}

/// `.oc-input`, small (the installer's folder): mono 10.5 px on `t().track`, a rim
/// edge that turns `t().accent` while focused. No `on_input` = read-only.
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
            background: t().track.into(),
            border: Border::default().rounded(10).width(1).color(match status {
                text_input::Status::Focused { .. } => t().accent,
                _ => t().rim,
            }),
            icon: t().text_sub,
            placeholder: t().text_muted,
            value: match status {
                text_input::Status::Disabled => t().text_sub,
                _ => t().text,
            },
            selection: t().accent_soft,
        });
    if let Some(on_input) = on_input {
        input = input.on_input(on_input);
    }
    input.into()
}

/// `.oc-btn`, small (the installer's "…"): `t().elevated`, radius 9, 12 px / 500.
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
                button::Status::Hovered | button::Status::Pressed => t().elevated_hover,
                _ => t().elevated,
            }
            .into(),
        ),
        text_color: match status {
            button::Status::Disabled => t().text_muted,
            _ => t().text,
        },
        border: Border::default().rounded(9),
        ..button::Style::default()
    })
    .into()
}

/// A mono chip on `t().accent_soft` (the version): 10.5 px / 500, 2x8 px, round.
pub fn chip<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(
        text(label)
            .size(10.5)
            .font(mono(Weight::Medium))
            .color(t().accent),
    )
    .padding([2, 8])
    .height(20)
    .center_y(20)
    .style(|_| container::Style {
        background: Some(t().accent_soft.into()),
        border: Border::default().rounded(999),
        ..container::Style::default()
    })
    .into()
}

/// A recessed panel (the installer's progress): radius 12, `t().track_glass`, a
/// divider edge, 14 px inside (8 px below: the progress bar's glow takes 6).
pub fn panel<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Fill)
        .padding(iced::Padding {
            top: 14.0,
            right: 14.0,
            bottom: 8.0,
            left: 14.0,
        })
        .style(|_| container::Style {
            background: Some(t().track_glass.into()),
            border: Border::default().rounded(12).width(1).color(t().divider),
            ..container::Style::default()
        })
        .into()
}

/// A 6 px progress bar, `share` (0..1) filled with `t().accent` on a `t().hover`
/// track, the fill glowing (`0 0 12px` accent: drawn as two soft rounded rings, no
/// blur on the CPU renderer). 18 px tall: the bar in its middle, the glow round it.
pub fn progress_bar<'a, Message: 'a>(share: f32) -> Element<'a, Message> {
    const GLOW: f32 = 6.0;
    let share = share.clamp(0.0, 1.0);
    let portion = |s: f32| Length::FillPortion((s * 1000.0).round() as u16);
    let glow = |grow: f32, alpha: f32| {
        let ring =
            container(Space::new().width(Fill).height(Fill)).style(move |_| container::Style {
                background: Some(faded(t().accent, alpha).into()),
                border: Border::default().rounded(999),
                ..container::Style::default()
            });
        let pad = GLOW - grow;
        row![
            container(ring)
                .padding(pad)
                .width(portion(share))
                .height(Fill),
            Space::new().width(portion(1.0 - share)),
        ]
        .height(Fill)
    };
    let fill = container(Space::new().width(Fill).height(6))
        .width(portion(share))
        .style(|_| container::Style {
            background: Some(t().accent.into()),
            border: Border::default().rounded(999),
            ..container::Style::default()
        });
    let track = container(row![
        fill,
        Space::new().width(portion(1.0 - share)).height(6)
    ])
    .width(Fill)
    .height(6)
    .style(|_| container::Style {
        background: Some(t().hover.into()),
        border: Border::default().rounded(999),
        ..container::Style::default()
    });
    let bar = container(track).padding([GLOW, 0.0]).width(Fill);
    let glows = (share > 0.0).then(|| stack![glow(GLOW, 0.10), glow(GLOW / 2.0, 0.18)]);
    let mut layers = stack![].width(Fill).height(6.0 + 2.0 * GLOW);
    if let Some(glows) = glows {
        layers = layers.push(glows);
    }
    layers.push(bar).into()
}

/// A window's close caption button (`.win-caption-btn.win-close`): 46 px wide,
/// `height` tall, the X in `t().text_sub`, red with a white X on hover.
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
                t().text_sub
            },
            ..button::Style::default()
        }
    })
    .into()
}
