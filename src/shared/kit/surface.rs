//! `surface(Material, radius, content)`: what every panel and window body sits
//! on, solid (glass joins with the GPU renderer). Floating widgets use style E
//! two-tone trays (`Material::Tray` / `tray`) holding raised `widget` segments
//! (`segment`, `segment_row`, `segment_card`).
use iced::widget::{Container, container, text::IntoFragment};
use iced::{Border, Center, Color, Element};

use super::themes::{self, c};
use super::tokens::{Role, radius, widget_e};

/// A hint's padding (the toast pill's, design 11.2).
const HINT_PAD: [f32; 2] = [8.0, 14.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// A window's body (`--app-bg-solid`, rim edge).
    Window,
    /// A recessed panel laid over a window (`--app-track-glass`, divider edge).
    Recessed,
    /// A floating widget or card (`.oc-widget`: `--app-widget-solid`, rim edge).
    Widget,
    /// A tile on a window (the setup's logo emblem): `hover`, rim edge.
    Tile,
    /// A floating widget while it is held and dragged (`.oc-dragging`): the
    /// widget with an `accent` edge.
    Held,
    /// The Studio's dark stage (`canvas_dark`, divider edge).
    Stage,
    /// Style E floating-widget recessed outer tray (`track`, rim edge).
    Tray,
    /// A quiet chip on a window (the Studio rail's group tiles): the faint `track_glass` tint, no edge.
    Chip,
    /// Style E floating-widget tray while dragged (`track`, `accent` edge).
    TrayHeld,
}

/// Subtle edge around a raised segment on a recessed tray (no CSS shadow on the CPU renderer).
pub(crate) fn segment_edge() -> Color {
    match themes::current().light {
        true => c().elevated_border,
        false => c().divider,
    }
}

/// `content` on `material` with corners of `radius` and a 1 px edge. The
/// caller sizes and pads it.
pub fn surface<'a, Message: 'a>(
    material: Material,
    radius: f32,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    corners(material, [radius; 4], content)
}

/// `surface` with a radius per corner: top-left, top-right, bottom-right, bottom-left.
fn corners<'a, Message: 'a>(
    material: Material,
    corners: [f32; 4],
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    let radius = iced::border::Radius {
        top_left: corners[0],
        top_right: corners[1],
        bottom_right: corners[2],
        bottom_left: corners[3],
    };
    container(content).style(move |_| {
        let (fill, edge) = match material {
            Material::Window => (c().bg, c().rim),
            Material::Recessed => (c().track_glass, c().divider),
            Material::Stage => (c().canvas_dark, c().divider),
            Material::Widget => (c().widget, c().rim),
            Material::Tile => (c().hover, c().rim),
            Material::Held => (c().widget, c().accent),
            Material::Tray => (c().track, c().rim),
            Material::TrayHeld => (c().track, c().accent),
            Material::Chip => (c().track_glass, Color::TRANSPARENT),
        };
        container::Style {
            background: Some(fill.into()),
            border: Border::default().rounded(radius).width(1).color(edge),
            ..container::Style::default()
        }
    })
}

/// Which edge of a tray another tray is attached to (the Snip options row and its toolbar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    None,
    /// A tray sits against this one's bottom edge.
    Below,
    /// A tray sits against this one's top edge.
    Above,
}

impl Join {
    /// The edge the neighbour sees: the opposite one.
    pub fn facing(self) -> Join {
        match self {
            Join::Below => Join::Above,
            Join::Above => Join::Below,
            Join::None => Join::None,
        }
    }
}

/// The corner radius on a joined edge: small, so two trays read as one piece.
pub const JOIN_RADIUS: f32 = 8.0;

/// Style E floating-widget tray (`track`, `rim` or `accent` while `held`); its `join` edge is
/// nearly square, to sit flush against its neighbour.
pub fn tray<'a, Message: 'a>(
    held: bool,
    radius: f32,
    join: Join,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    let j = JOIN_RADIUS.min(radius);
    let c = match join {
        Join::None => [radius; 4],
        Join::Below => [radius, radius, j, j],
        Join::Above => [j, j, radius, radius],
    };
    corners(
        if held {
            Material::TrayHeld
        } else {
            Material::Tray
        },
        c,
        content,
    )
}

/// Style E raised pill segment (`.gp`) inside a floating tray: `widget` fill,
/// subtle 1 px edge, pill corners, padded for controls.
pub fn segment<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    segment_pad(widget_e::SEGMENT_PAD_Y, content)
}

/// `segment` with its own vertical padding (the Snip toolbar's and the translate pill's
/// segments are shorter than the bar's: `widget_e::SEGMENT_PAD_Y_*`).
pub fn segment_pad<'a, Message: 'a>(
    pad_y: f32,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    segment_card(radius::FULL, content).padding([pad_y, widget_e::SEGMENT_PAD_X])
}

/// Style E raised pill segment holding a direction-aware row of controls.
pub fn segment_row<'a, Message: 'a>(
    items: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    segment_row_pad(widget_e::SEGMENT_PAD_Y, items)
}

/// `segment_row` with its own vertical padding (see `segment_pad`).
pub fn segment_row_pad<'a, Message: 'a>(
    pad_y: f32,
    items: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    segment_pad(
        pad_y,
        super::dir::row(items)
            .spacing(widget_e::SEGMENT_GAP)
            .align_y(Center),
    )
    .into()
}

/// Style E raised card inside a floating tray (AI chat body, card content,
/// update card body): `widget` fill with a subtle 1 px edge.
pub fn segment_card<'a, Message: 'a>(
    corner: f32,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    container(content).style(move |_| container::Style {
        background: Some(c().widget.into()),
        border: Border::default()
            .rounded(corner)
            .width(1)
            .color(segment_edge()),
        ..container::Style::default()
    })
}

/// A toolbar's panel (Snip's Undo panel): `content` on a style E tray with
/// `MENU_RADIUS` corners, `size::PANEL` wide.
pub fn panel<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    use super::tokens::{size, space};
    tray(false, radius::DROPDOWN, Join::None, segment(content))
        .padding(space::BAR_PAD)
        .width(size::PANEL)
}

/// A one-line hint over the screen ("Circle, drag or tap anything", the
/// editor's area picker, a protected capture): a style E tray pill in every theme.
pub fn hint<'a, Message: 'a>(label: impl IntoFragment<'a>) -> Container<'a, Message> {
    let line = super::text::text(Role::NOTE, label);
    tray(false, radius::FULL, Join::None, line).padding(HINT_PAD)
}

/// Style E live pin frame (`.pinw`): `content` with a `widget_e::PIN_BORDER` ring
/// in `color` (the Active Look's) drawn over its edges, rounded to `corner`
/// (the window itself is cut to the same corner, `windows::floating`).
pub fn pin_frame<'a, Message: 'a>(
    corner: f32,
    color: iced::Color,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let ring = container(iced::widget::Space::new())
        .width(iced::Fill)
        .height(iced::Fill)
        .style(move |_| container::Style {
            border: Border::default()
                .rounded(corner)
                .width(widget_e::PIN_BORDER)
                .color(color),
            ..container::Style::default()
        });
    iced::widget::stack![
        container(content).width(iced::Fill).height(iced::Fill),
        ring
    ]
    .width(iced::Fill)
    .height(iced::Fill)
    .into()
}

/// A tooltip's pill (`tooltip::hint`, `tooltip::cut`): the solid `widget` pill
/// every window's tooltip keeps (Settings, the Studio), not the style E tray.
pub(crate) fn tooltip_pill<'a, Message: 'a>(
    label: impl IntoFragment<'a>,
) -> Container<'a, Message> {
    let line = super::text::text(Role::NOTE, label);
    surface(Material::Widget, radius::FULL, line).padding(HINT_PAD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::shot;
    use crate::shared::kit::themes::{DAYLIGHT, SAPPHIRE};

    /// Owner 2026-11-27: a tray's joined edge has small corners (it sits flush on its neighbour),
    /// the other edge stays round.
    #[test]
    fn a_joined_tray_squares_the_corners_on_the_joined_edge() {
        let corner_filled = |join: Join, top: bool| {
            let v: Element<'_, ()> = tray(
                false,
                26.0,
                join,
                iced::widget::Space::new().width(120).height(52),
            )
            .into();
            let px = shot::render(v, 120, 52);
            let y = if top { 3 } else { 48 };
            // a pixel just inside the left edge: tray fill when the corner is small, background when round
            let i = ((y * 120 + 3) * 4) as usize;
            px[i..i + 3] != themes::c().bg.into_rgba8()[..3]
        };
        assert!(corner_filled(Join::Below, false) && !corner_filled(Join::Below, true));
        assert!(corner_filled(Join::Above, true) && !corner_filled(Join::Above, false));
        assert!(!corner_filled(Join::None, true) && !corner_filled(Join::None, false));
    }

    #[test]
    fn style_e_tray_and_segment_render_two_distinct_tones() {
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let view: Container<'_, ()> = tray(
                false,
                radius::FULL,
                Join::None,
                segment_row([super::super::text::text(Role::CONTROL, "Action").into()]),
            )
            .padding([widget_e::TRAY_PAD_Y, widget_e::TRAY_PAD_X]);
            let px = shot::render(view.into(), 140, 48);
            let has = |col: Color| {
                let target = col.into_rgba8();
                px.as_chunks::<4>().0.iter().any(|p| {
                    p[0].abs_diff(target[0]) <= 3
                        && p[1].abs_diff(target[1]) <= 3
                        && p[2].abs_diff(target[2]) <= 3
                })
            };
            assert!(has(theme.c.track), "{}: tray track missing", theme.id);
            assert!(has(theme.c.widget), "{}: segment widget missing", theme.id);
        }
        themes::set(&SAPPHIRE);
    }
}
