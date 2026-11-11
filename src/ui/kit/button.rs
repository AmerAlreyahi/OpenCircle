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
    /// Quiet text that removes (History's Clear all): `DANGER` under the cursor.
    Remove,
}

impl Kind {
    #[cfg(test)]
    pub const ALL: [Kind; 6] = [
        Kind::Elevated,
        Kind::Accent,
        Kind::Success,
        Kind::Danger,
        Kind::Quiet,
        Kind::Remove,
    ];

    /// A call to action's fill.
    fn fill(self) -> Option<Color> {
        match self {
            Kind::Accent => Some(c().accent),
            Kind::Success => Some(fixed::SUCCESS),
            Kind::Danger => Some(fixed::DANGER),
            Kind::Elevated | Kind::Quiet | Kind::Remove => None,
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
                text_color: super::themes::on(fill),
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
                    // a status not yet known (before the first redraw) is at rest
                    text_color: match enabled {
                        false => c().text_muted,
                        true => c().text,
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
                    Status::Hovered | Status::Pressed if kind == Kind::Remove => fixed::DANGER,
                    Status::Hovered | Status::Pressed => c().text,
                    _ => c().text_sub,
                },
                ..iced_button::Style::default()
            }),
    };
    b.on_press_maybe(on_press).into()
}

/// `.oc-btn` across its share of a row (`grid` buttons, 8x13 px): `content`
/// centred.
pub fn fill<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use iced_button::Status;
    let enabled = on_press.is_some();
    iced_button(container(content).center_x(Fill))
        .width(Fill)
        .padding([8.0, 13.0])
        .on_press_maybe(on_press)
        .style(move |_, status| iced_button::Style {
            background: Some(
                match status {
                    Status::Hovered | Status::Pressed => c().elevated_hover,
                    _ => c().elevated,
                }
                .into(),
            ),
            text_color: match enabled {
                false => c().text_muted,
                true => c().text,
            },
            border: Border::default().rounded(radius::CONTROL),
            ..iced_button::Style::default()
        })
        .into()
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
        .size(super::text::size(Role::CONTROL))
        .font(super::text::font(Role::CONTROL))
        .line_height(1.5)
}

/// `.oc-btn` at its own size (6x13 px, radius 9, `label`'s 12 px / 500):
/// `elevated` with its edge, lighter under the cursor; `accent` (a card's
/// primary action, `.oc-model-primary`) the accent with the window's colour
/// as text, 600, brighter under the cursor. A 2 px inset edge while pressed;
/// no `on_press` = dimmed (`opacity .45`), same size and place.
pub fn regular<'a, Message: Clone + 'a>(
    label: &'a str,
    accent: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use super::tokens::Role;
    use iced_button::Status;
    let live = on_press.is_some();
    let font = match accent {
        true => weight(Weight::Semibold),
        false => super::text::font(Role::CONTROL),
    };
    iced_button(
        text(label)
            .size(super::text::size(Role::CONTROL))
            .font(font),
    )
    .padding([6.0, 13.0])
    .on_press_maybe(on_press)
    .style(move |_, status| {
        let hot = live && matches!(status, Status::Hovered | Status::Pressed);
        let (fill, ink, edge) = match accent {
            true => (c().accent, c().bg, c().accent),
            false => (c().elevated, c().text, c().elevated_border),
        };
        let fill = match () {
            _ if hot && accent => brighter(fill),
            _ if hot => c().elevated_hover,
            _ => fill,
        };
        let dim = |v: Color| if live { v } else { faded(v, 0.45) };
        let (width, edge) = match () {
            _ if live && status == Status::Pressed => (2.0, c().widget),
            _ => (1.0, edge),
        };
        iced_button::Style {
            background: Some(dim(fill).into()),
            text_color: dim(ink),
            border: Border::default()
                .rounded(radius::CONTROL)
                .width(width)
                .color(dim(edge)),
            ..iced_button::Style::default()
        }
    })
    .into()
}

/// An on / off pill (the composer's Include screen, `#aiIncludeScreenToggle`):
/// `.oc-btn`'s look at 4x10 px, radius 8, a 6 px dot (`accent` while `on`,
/// `text_muted` while off) and the 11 px label; lighter under the cursor, a 2
/// px inset edge while pressed.
pub fn switch<'a, Message: Clone + 'a>(
    label: &'a str,
    on: bool,
    on_press: Message,
) -> Element<'a, Message> {
    use super::tokens::{Role, space};
    use iced_button::Status;
    let dot = super::chip::dot(if on { c().accent } else { c().text_muted });
    let line = crate::dir_row![
        dot,
        text(label)
            .size(super::text::size(Role::META))
            .font(super::text::font(Role::META))
    ]
    .spacing(space::GAP)
    .align_y(Center);
    iced_button(line)
        .padding([4.0, 10.0])
        .on_press(on_press)
        .style(|_, status| {
            let (fill, width, edge) = match status {
                Status::Pressed => (c().elevated_hover, 2.0, c().widget),
                Status::Hovered => (c().elevated_hover, 1.0, c().elevated_border),
                _ => (c().elevated, 1.0, c().elevated_border),
            };
            iced_button::Style {
                background: Some(fill.into()),
                text_color: c().text,
                border: Border::default()
                    .rounded(radius::MENU)
                    .width(width)
                    .color(edge),
                ..iced_button::Style::default()
            }
        })
        .into()
}

/// An on / off switch (Snip's toolbar panels: Outline, Fill, Bold, Line
/// help): a 32x18 pill, `accent` with its knob right while on, `track` with a
/// `divider` edge and the knob left while off; the knob 12 px, white (`fixed::HANDLE`)
/// on / `text_sub` off. No `on_press` = dimmed (an option that does not
/// apply, e.g. Fill for a line). Lighter under the cursor.
pub fn toggle<'a, Message: Clone + 'a>(
    on: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use iced::alignment::Horizontal;
    use iced_button::Status;
    let live = on_press.is_some();
    let knob = container(iced::widget::Space::new())
        .width(TOGGLE_KNOB)
        .height(TOGGLE_KNOB)
        .style(move |_| container::Style {
            background: Some(
                match on {
                    true => fixed::HANDLE,
                    false => c().text_sub,
                }
                .into(),
            ),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        });
    // right to left: "on" is the knob at the left
    let side = if on != super::dir::rtl() {
        Horizontal::Right
    } else {
        Horizontal::Left
    };
    iced_button(container(knob).align_x(side).center_y(Fill).width(Fill))
        .width(TOGGLE.width)
        .height(TOGGLE.height)
        .padding(3)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hot = live && matches!(status, Status::Hovered | Status::Pressed);
            let fill = match (on, hot) {
                (true, false) => c().accent,
                (true, true) => brighter(c().accent),
                (false, false) => c().track,
                (false, true) => c().hover,
            };
            let alpha = if live { 1.0 } else { 0.4 };
            iced_button::Style {
                background: Some(faded(fill, alpha).into()),
                border: Border::default()
                    .rounded(radius::FULL)
                    .width(if on { 0 } else { 1 })
                    .color(faded(c().divider, alpha)),
                ..iced_button::Style::default()
            }
        })
        .into()
}

/// `toggle`'s size and its knob's.
pub const TOGGLE: iced::Size = iced::Size::new(32.0, 18.0);
const TOGGLE_KNOB: f32 = 12.0;

/// A round button in the accent with nothing on it, `side` across (a control
/// minimised to a dot: the live controller); brighter under the cursor, a
/// 2 px inset edge of the widget colour while pressed.
pub fn dot<'a, Message: Clone + 'a>(side: f32, on_press: Message) -> Element<'a, Message> {
    use iced_button::Status;
    iced_button(iced::widget::Space::new())
        .width(side)
        .height(side)
        .padding(0)
        .on_press(on_press)
        .style(|_, status| iced_button::Style {
            background: Some(
                match status {
                    Status::Hovered | Status::Pressed => brighter(c().accent),
                    _ => c().accent,
                }
                .into(),
            ),
            border: Border::default()
                .rounded(radius::FULL)
                .width(if status == Status::Pressed { 2 } else { 0 })
                .color(c().widget),
            ..iced_button::Style::default()
        })
        .into()
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

/// A compact pill's height (`.oc-btn !py-1.5 !px-2.5 !rounded-xl`).
pub const COMPACT_H: f32 = 30.0;

/// The compact pill (the bar's Search in browser, Copy text, Translate): a
/// 14 px glyph and an 11.5 px label (`label: None` = the glyph alone, narrow
/// screens), centred in `width` x `COMPACT_H`, radius 12. `.oc-btn`'s look:
/// `elevated` with its edge, lighter under the cursor, a 2 px inset edge while
/// pressed. `active` (the current choice) is `accent_soft` with `accent` text;
/// Tab's focus an `accent` edge; no `on_press` = dimmed, same size and place.
pub fn compact<'a, Message: Clone + 'a>(
    glyph: char,
    label: Option<&'a str>,
    width: f32,
    (active, focused): (bool, bool),
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use super::tokens::{Role, space};
    use iced_button::Status;
    let live = on_press.is_some();
    let content = row![text(glyph).font(super::icon::FONT).size(size::ICON_SMALL)]
        .push(label.map(|l| {
            text(l)
                .size(super::text::size(Role::BAR_PILL))
                .font(super::text::font(Role::BAR_PILL))
        }))
        .spacing(space::GAP)
        .align_y(Center);
    iced_button(container(content).center(Fill))
        .width(width)
        .height(COMPACT_H)
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hot = live && matches!(status, Status::Hovered | Status::Pressed);
            let fill = match () {
                _ if active && live => c().accent_soft,
                _ if hot => c().elevated_hover,
                _ => c().elevated,
            };
            let text_color = match () {
                _ if !live => c().text_muted,
                _ if active => c().accent,
                _ => c().text,
            };
            let (width, edge) = match () {
                _ if live && status == Status::Pressed => (2.0, c().widget),
                _ if focused => (1.0, c().accent),
                _ => (1.0, c().elevated_border),
            };
            iced_button::Style {
                background: Some(fill.into()),
                text_color,
                border: Border::default()
                    .rounded(radius::CARD)
                    .width(width)
                    .color(edge),
                ..iced_button::Style::default()
            }
        })
        .into()
}

/// A toolbar's action (Snip's Copy and Save, icon-only, owner 2026-11-02): a
/// 16 px glyph, `size::ICON_BUTTON` square, radius 9. `Accent` = the primary
/// (accent fill, `themes::on` glyph), else `Elevated` (`elevated`, its edge).
/// Tab's `focused`: an `accent_soft` fill (Elevated) or a 3 px `accent_soft`
/// ring (Accent); never a white ring.
pub fn tool<'a, Message: Clone + 'a>(
    glyph: char,
    kind: Kind,
    focused: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let content = text(glyph).font(super::icon::FONT).size(size::ICON);
    tooled(content, size::ICON_BUTTON, kind, focused, on_press)
}

/// `tool` with a label after its glyph, `width` wide (the Steps strip's Copy
/// all, Snip's "Add step N" in Copy's place). No `on_press` = dimmed (.45).
pub fn labelled<'a, Message: Clone + 'a>(
    glyph: char,
    label: impl text::IntoFragment<'a>,
    width: f32,
    kind: Kind,
    focused: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use super::tokens::{Role, space};
    let content = row![
        text(glyph).font(super::icon::FONT).size(size::ICON_SMALL),
        text(label)
            .size(super::text::size(Role::CONTROL))
            .font(super::text::font(Role::CTA))
            .wrapping(text::Wrapping::None),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    tooled(content, width, kind, focused, on_press)
}

/// The width `labelled` needs for `label` (its glyph, the gap, 14 px each side).
pub fn labelled_width(label: &str) -> f32 {
    use super::tokens::{Role, space};
    const SIDE: f32 = 14.0;
    let words = super::text::width(
        label,
        super::text::size(Role::CONTROL),
        super::text::font(Role::CTA),
    );
    (2.0 * SIDE + size::ICON_SMALL + space::GAP + words).ceil()
}

/// A section that opens in place (the What's new card's Fixes): a chevron
/// (`ChevronDown` closed, `ChevronUp` open), `title` (13 px / 600) and `count`
/// (muted), across the row on `hover`, radius 9, 36 px tall.
pub fn fold<'a, Message: Clone + 'a>(
    title: &'a str,
    count: usize,
    open: bool,
    on_press: Message,
) -> Element<'a, Message> {
    use super::icon::{Icon, icon};
    use super::tokens::Role;
    use iced_button::Status;
    let chevron = match open {
        true => Icon::ChevronUp,
        false => Icon::ChevronDown,
    };
    let content = row![
        icon(chevron, size::ICON_SMALL).color(c().text_sub),
        super::text::text(Role::CTA, title),
        super::text::muted(Role::ROW_DESC, count.to_string()),
    ]
    .spacing(8)
    .align_y(Center);
    iced_button(container(content).center_y(Fill))
        .width(Fill)
        .height(FOLD_H)
        .padding([0.0, 10.0])
        .on_press(on_press)
        .style(|_, status| iced_button::Style {
            background: Some(
                match status {
                    Status::Hovered | Status::Pressed => c().elevated_hover,
                    _ => c().hover,
                }
                .into(),
            ),
            text_color: c().text,
            border: Border::default().rounded(radius::CONTROL),
            ..iced_button::Style::default()
        })
        .into()
}

/// `fold`'s height.
pub const FOLD_H: f32 = 36.0;

fn tooled<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    width: f32,
    kind: Kind,
    focused: bool,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    use iced_button::Status;
    let live = on_press.is_some();
    iced_button(container(content).center(Fill))
        .width(width)
        .height(size::ICON_BUTTON)
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let hot = live && matches!(status, Status::Hovered | Status::Pressed);
            let accent = kind == Kind::Accent;
            let fill = match () {
                _ if accent && !live => faded(c().accent, 0.45),
                _ if accent && hot => brighter(c().accent),
                _ if accent => c().accent,
                _ if focused => c().accent_soft,
                _ if hot => c().elevated_hover,
                _ => c().elevated,
            };
            let text_color = match () {
                _ if accent && !live => faded(super::themes::on(c().accent), 0.45),
                _ if !live => c().text_muted,
                _ if accent => super::themes::on(c().accent),
                _ => c().text,
            };
            let (width, edge) = match () {
                _ if live && status == Status::Pressed => (2.0, c().widget),
                _ if accent && focused => (3.0, c().accent_soft),
                _ if accent => (0.0, c().accent),
                _ => (1.0, c().elevated_border),
            };
            iced_button::Style {
                background: Some(fill.into()),
                text_color,
                border: Border::default()
                    .rounded(radius::CONTROL)
                    .width(width)
                    .color(edge),
                ..iced_button::Style::default()
            }
        })
        .into()
}
