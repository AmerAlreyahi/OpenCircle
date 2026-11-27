//! The recording widgets' parts (R1 bar, S2 thumbnail; style E, approved
//! design `widgets.py` "rec" group): the pill, the red dot, the timer, the Stop
//! button, the device button, the size and fps readout, level and waveform
//! bars, the thumbnail frame with its duration chip and actions, the audio
//! chip. Screens only compose them; nothing here reads the config or a clock.
use iced::font::Weight;
use iced::widget::{Space, button, column, container, image, row, stack};
use iced::{Border, Center, Color, ContentFit, Element, Fill, Length};

use super::button::faded;
use super::dir;
use super::focus;
use super::icon::{FONT, Icon, icon};
use super::surface::{Material, segment_edge, surface};
use super::text::{self, cut, font};
use super::themes::{c, current};
use super::tokens::{Role, fixed, radius, size};

/// The recording red (`--rec`) and the paused amber (`--amber`) of the design.
pub fn red() -> Color {
    match current().light {
        true => Color::from_rgb8(0xdc, 0x26, 0x26),
        false => Color::from_rgb8(0xf8, 0x71, 0x71),
    }
}

pub fn amber() -> Color {
    match current().light {
        true => Color::from_rgb8(0xb4, 0x53, 0x09),
        false => Color::from_rgb8(0xfb, 0xbf, 0x24),
    }
}

/// The bar's height and the buttons' side (`.rec` 52, `.ib` 34).
pub const BAR_H: f32 = 52.0;
pub const SIDE: f32 = 34.0;
/// The gap between the bar and what hangs under it.
pub const GAP: f32 = 8.0;

const fn mono(size: f32, weight: Weight) -> Role {
    Role {
        size,
        weight,
        mono: true,
    }
}

fn hot(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}

/// The bar: a pill `width` wide of `items` (in the reading direction), 4 px apart.
pub fn bar<'a, M: 'a>(corners: f32, width: f32, items: Vec<Element<'a, M>>) -> Element<'a, M> {
    let content = dir::row(items)
        .spacing(4)
        .align_y(Center)
        .padding([0.0, 10.0]);
    surface(Material::Widget, corners, content)
        .width(width)
        .height(BAR_H)
        .center_y(BAR_H)
        .into()
}

/// The bar with `under` hung below it (the device list, a card), centred.
pub fn hung<'a, M: 'a>(bar: Element<'a, M>, under: Option<Element<'a, M>>) -> Element<'a, M> {
    let mut stack = column![bar].spacing(GAP).align_x(Center);
    if let Some(under) = under {
        stack = stack.push(under);
    }
    container(stack).width(Fill).height(Fill).into()
}

/// A card under the bar, `width` wide.
pub fn card_under<'a, M: 'a>(width: f32, content: Element<'a, M>) -> Element<'a, M> {
    surface(Material::Widget, radius::DROPDOWN, content)
        .padding(12.0)
        .width(width)
        .into()
}

/// A group heading of the device list.
pub fn heading<'a, M: 'a>(words: &'a str) -> Element<'a, M> {
    container(super::text::muted(Role::SECTION, words))
        .padding([6.0, 10.0])
        .into()
}

/// The live dot: 10 px with a 4 px halo (amber while paused).
pub fn dot<'a, M: 'a>(paused: bool) -> Element<'a, M> {
    let tone = if paused { amber() } else { red() };
    let inner = container(Space::new())
        .width(10)
        .height(10)
        .style(move |_| container::Style {
            background: Some(tone.into()),
            border: Border::default().rounded(5),
            ..container::Style::default()
        });
    container(inner)
        .center(18)
        .style(move |_| container::Style {
            background: Some(faded(tone, 0.22).into()),
            border: Border::default().rounded(9),
            ..container::Style::default()
        })
        .into()
}

/// `00:12` in mono, 56 px wide so the bar does not breathe as it counts.
pub fn timer<'a, M: 'a>(words: &str) -> Element<'a, M> {
    container(text::text(mono(14.0, Weight::Semibold), words.to_string()))
        .width(56)
        .padding([0.0, 6.0])
        .into()
}

/// The red Stop button (`.stopb`): a 14 px square on 34 px, 10 px corners.
pub fn stop<'a, M: Clone + 'a>(on_press: Option<M>) -> Element<'a, M> {
    let face = container(icon(Icon::Stop, 14.0).color(Color::WHITE)).center(Fill);
    focus::press(
        button(face)
            .width(SIDE)
            .height(SIDE)
            .padding(0)
            .style(|_, status| button::Style {
                background: Some(
                    red()
                        .scale_alpha(if hot(status) { 0.86 } else { 1.0 })
                        .into(),
                ),
                text_color: Color::WHITE,
                border: Border::default().rounded(radius::TRACK),
                ..button::Style::default()
            }),
        radius::TRACK,
        on_press,
    )
    .into()
}

/// The "Record" call to action of the ready bar: red, with its white dot.
pub fn record_button<'a, M: Clone + 'a>(label: &'a str, on_press: M) -> Element<'a, M> {
    let dot = container(Space::new())
        .width(8)
        .height(8)
        .style(|_| container::Style {
            background: Some(Color::WHITE.into()),
            border: Border::default().rounded(4),
            ..container::Style::default()
        });
    let face = container(
        row![dot, text::text(Role::CONTROL, label).color(Color::WHITE)]
            .spacing(7)
            .align_y(Center),
    )
    .center_y(Fill);
    focus::press(
        button(face)
            .height(SIDE)
            .padding([0.0, 14.0])
            .style(|_, status| button::Style {
                background: Some(
                    red()
                        .scale_alpha(if hot(status) { 0.86 } else { 1.0 })
                        .into(),
                ),
                text_color: Color::WHITE,
                border: Border::default().rounded(radius::FULL),
                ..button::Style::default()
            }),
        radius::FULL,
        Some(on_press),
    )
    .into()
}

/// "Paused" (`.stag`): outlined in amber.
pub fn paused_tag<'a, M: 'a>(label: &'a str) -> Element<'a, M> {
    container(text::text(
        Role {
            size: 10.5,
            ..Role::CONTROL
        },
        label,
    ))
    .height(22)
    .padding([0.0, 8.0])
    .center_y(22)
    .style(|_| container::Style {
        text_color: Some(amber()),
        border: Border::default().rounded(11).width(1).color(amber()),
        ..container::Style::default()
    })
    .into()
}

/// The device button (`.dev`): the mic (or the muted mic, danger-toned), its
/// name, a chevron; the open one carries the `hover` tint.
pub fn device<'a, M: Clone + 'a>(
    muted: bool,
    label: &str,
    open: bool,
    on_press: M,
) -> Element<'a, M> {
    let tone = |sub: Color| if muted { fixed::DANGER } else { sub };
    let glyph = if muted { Icon::MicOff } else { Icon::Mic };
    let shown = cut(label, 12.0, font(Role::CONTROL), 120.0);
    let face = row![
        icon(glyph, size::ICON_SMALL).color(tone(c().text_sub)),
        text::text(Role::CONTROL, shown).color(tone(c().text)),
        icon(Icon::ChevronDown, size::ICON_SMALL).color(c().text_sub),
    ]
    .spacing(7)
    .align_y(Center);
    focus::press(
        button(face)
            .height(SIDE)
            .padding([0.0, 10.0])
            .style(move |_, status| button::Style {
                background: (open || hot(status)).then(|| c().hover.into()),
                text_color: c().text,
                border: Border::default().rounded(radius::TRACK),
                ..button::Style::default()
            }),
        radius::TRACK,
        Some(on_press),
    )
    .into()
}

/// `1920×1080 • 30 fps` (`.meta`): mono, always left to right.
pub fn meta<'a, M: 'a>(width: u32, height: u32, fps: String) -> Element<'a, M> {
    let part = |s: String| {
        text::text(mono(11.0, Weight::Medium), s)
            .color(c().text_sub)
            .wrapping(iced::widget::text::Wrapping::None)
    };
    let sep = container(Space::new())
        .width(3)
        .height(3)
        .style(|_| container::Style {
            background: Some(c().text_muted.into()),
            border: Border::default().rounded(2),
            ..container::Style::default()
        });
    row![part(format!("{width}×{height}")), sep, part(fps)]
        .spacing(8)
        .align_y(Center)
        .padding([0.0, 10.0])
        .into()
}

/// Level bars (`.lvl`): heights 0..1 of 16 px, accent.
pub fn levels<'a, M: 'a>(heights: &[f32]) -> Element<'a, M> {
    let bars = heights.iter().map(|h| {
        container(Space::new())
            .width(3)
            .height((h.clamp(0.0, 1.0) * 14.0).max(2.0))
            .style(|_| container::Style {
                background: Some(c().accent.into()),
                border: Border::default().rounded(2),
                ..container::Style::default()
            })
            .into()
    });
    container(row(bars).spacing(2).align_y(Center))
        .padding([0.0, 10.0])
        .center_y(16)
        .into()
}

/// A waveform (`.wave`): heights 0..1 of 28 px; the first `played` bars full, the rest faint.
pub fn wave<'a, M: 'a>(heights: &[f32], played: usize) -> Element<'a, M> {
    let bars = heights.iter().enumerate().map(|(i, h)| {
        let alpha = if i < played { 0.85 } else { 0.28 };
        container(Space::new())
            .width(3)
            .height((h.clamp(0.0, 1.0) * 28.0).max(3.0))
            .style(move |_| container::Style {
                background: Some(faded(c().accent, alpha).into()),
                border: Border::default().rounded(2),
                ..container::Style::default()
            })
            .into()
    });
    container(row(bars).spacing(2).align_y(Center))
        .width(Fill)
        .center_y(28)
        .clip(true)
        .into()
}

/// A small mono readout (the chip's time).
pub fn time<'a, M: 'a>(words: String) -> Element<'a, M> {
    text::text(mono(11.0, Weight::Normal), words)
        .color(c().text_sub)
        .into()
}

/// The duration chip on a thumbnail (`.dur`): dark glass, white mono 10.5.
pub fn duration<'a, M: 'a>(glyph: Icon, words: &str) -> Element<'a, M> {
    container(
        row![
            icon(glyph, 12.0).color(Color::WHITE),
            text::text(mono(10.5, Weight::Semibold), words.to_string()).color(Color::WHITE),
        ]
        .spacing(5)
        .align_y(Center),
    )
    .height(22)
    .padding([0.0, 8.0])
    .center_y(22)
    .style(|_| container::Style {
        background: Some(Color::from_rgba8(8, 12, 24, 0.72).into()),
        border: Border::default().rounded(11),
        ..container::Style::default()
    })
    .into()
}

/// A thumbnail's action (`.acts2 span`): 30 px, `widget` fill; the primary one in the accent.
pub fn action<'a, M: Clone + 'a>(glyph: char, primary: bool, on_press: M) -> Element<'a, M> {
    let face = container(
        iced::widget::text(glyph)
            .font(FONT)
            .size(size::ICON_SMALL + 2.0),
    )
    .center(Fill);
    focus::press(
        button(face)
            .width(30)
            .height(30)
            .padding(0)
            .style(move |_, status| {
                let (fill, ink) = match (primary, hot(status)) {
                    (true, _) => (c().accent, Color::WHITE),
                    (false, true) => (c().elevated_hover, c().text),
                    (false, false) => (c().widget, c().text),
                };
                button::Style {
                    background: Some(fill.into()),
                    text_color: ink,
                    border: Border::default()
                        .rounded(radius::TRACK)
                        .width(1)
                        .color(segment_edge()),
                    ..button::Style::default()
                }
            }),
        radius::TRACK,
        Some(on_press),
    )
    .into()
}

/// The actions in a row (in the reading direction), 6 px apart.
pub fn actions<'a, M: 'a>(items: Vec<Element<'a, M>>) -> Element<'a, M> {
    dir::row(items).spacing(6).into()
}

/// The video thumbnail (`.thumb`): the picture (or a flat tile), the duration
/// chip at the start of the foot, `actions` at the end of the top (on hover),
/// and the timer line along the foot (`left`: 0..1 of it).
pub fn thumb<'a, M: 'a>(
    corners: f32,
    poster: Option<&image::Handle>,
    chip: Element<'a, M>,
    actions: Option<Element<'a, M>>,
    left: f32,
) -> Element<'a, M> {
    let picture: Element<'a, M> = match poster {
        Some(handle) => image(handle.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover)
            .border_radius(corners)
            .into(),
        None => container(Space::new())
            .width(Fill)
            .height(Fill)
            .style(move |_| container::Style {
                background: Some(c().track.into()),
                border: Border::default().rounded(corners),
                ..container::Style::default()
            })
            .into(),
    };
    let chip = container(chip)
        .padding(8.0)
        .align_bottom(Fill)
        .align_left(Fill);
    let top: Element<'a, M> = match actions {
        Some(a) => container(a).padding(8.0).align_right(Fill).into(),
        None => Space::new().into(),
    };
    let rest = Color::from_rgba(1.0, 1.0, 1.0, 0.25);
    let line = container(
        container(Space::new())
            .width(Length::FillPortion(
                (left.clamp(0.0, 1.0) * 1000.0) as u16 + 1,
            ))
            .height(3)
            .style(|_| container::Style::default().background(Color::WHITE)),
    )
    .width(Fill)
    .height(3)
    .style(move |_| container::Style::default().background(rest));
    let foot = column![Space::new().height(Fill), line];
    surface(Material::Widget, corners, stack![picture, chip, top, foot])
        .width(Fill)
        .height(Fill)
        .into()
}

/// The audio chip (`.achip`): the mark tile, the waveform, then `end` (the time, or the actions).
pub fn chip<'a, M: 'a>(corners: f32, wave: Element<'a, M>, end: Element<'a, M>) -> Element<'a, M> {
    let tile = container(icon(Icon::Waveform, 18.0).color(c().accent))
        .center(32)
        .style(|_| container::Style {
            background: Some(c().accent_soft.into()),
            border: Border::default().rounded(10),
            ..container::Style::default()
        });
    let line = dir::row(vec![tile.into(), wave, end])
        .spacing(10)
        .align_y(Center);
    surface(Material::Widget, corners, line)
        .padding([0.0, 10.0])
        .width(Fill)
        .height(Fill)
        .center_y(Fill)
        .into()
}

/// A vertical hairline of the bar's groups (22 px).
pub fn group_line<'a, M: 'a>() -> Element<'a, M> {
    super::divider::vertical(Length::Fixed(22.0))
}
