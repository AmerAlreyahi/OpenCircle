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
        };
        container::Style {
            background: Some(fill.into()),
            border: Border::default().rounded(radius).width(1).color(edge),
            ..container::Style::default()
        }
    })
}

/// Style E floating-widget tray (`track`, `rim` or `accent` while `held`).
pub fn tray<'a, Message: 'a>(
    held: bool,
    radius: f32,
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    surface(
        if held {
            Material::TrayHeld
        } else {
            Material::Tray
        },
        radius,
        content,
    )
}

/// Style E raised pill segment (`.gp`) inside a floating tray: `widget` fill,
/// subtle 1 px edge, pill corners, padded for controls.
pub fn segment<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    segment_card(radius::FULL, content).padding([widget_e::SEGMENT_PAD_Y, widget_e::SEGMENT_PAD_X])
}

/// Style E raised pill segment holding a direction-aware row of controls.
pub fn segment_row<'a, Message: 'a>(
    items: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    segment(
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
    tray(false, radius::DROPDOWN, segment(content))
        .padding(space::BAR_PAD)
        .width(size::PANEL)
}

/// A one-line hint over the screen ("Circle, drag or tap anything", the
/// editor's area picker, a protected capture): a style E tray pill in every theme.
pub fn hint<'a, Message: 'a>(label: impl IntoFragment<'a>) -> Container<'a, Message> {
    let line = super::text::text(Role::NOTE, label);
    tray(false, radius::FULL, line).padding(HINT_PAD)
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

    #[test]
    fn style_e_tray_and_segment_render_two_distinct_tones() {
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            let view: Container<'_, ()> = tray(
                false,
                radius::FULL,
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
