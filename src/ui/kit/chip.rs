//! A chip: a short label on a tint. `chip` (mono on `accent_soft`), `tag`
//! (Labs' "Experimental"), `badge` (the sidebar's "Exp") and `status` (a
//! header pill with a dot: Settings' auto-save), `pill` (a round label).
use iced::font::Weight;
use iced::widget::{Space, container, row, text};
use iced::{Border, Center, Color, Element};

use super::button::faded;
use super::text::{mono, weight};
use super::themes::c;
use super::tokens::{fixed, radius, size, space};

/// The design's `rounded` (4 px): a tag's and a badge's corners.
const SMALL: f32 = 4.0;

/// Mono on `accent_soft` (the setup's version): 10.5 px / 500, 2x8 px, 20 px tall.
pub fn chip<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(
        text(label)
            .size(10.5)
            .font(mono(Weight::Medium))
            .color(c().accent),
    )
    .padding([2, 8])
    .height(20)
    .center_y(20)
    .style(|_| container::Style {
        background: Some(c().accent_soft.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    })
    .into()
}

/// 10 px `text_sub` on `hover` with a divider edge, 2x6 px (a Labs row's
/// "Experimental").
pub fn tag<'a, Message: 'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(10).font(weight(Weight::Normal)))
        .padding([2, 6])
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            text_color: Some(c().text_sub),
            border: Border::default().rounded(SMALL).width(1).color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// 9.5 px `text_muted` on `hover`, 2x6 px, no edge (a sidebar item's "Exp").
pub fn badge<'a, Message: 'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(9.5).font(weight(Weight::Normal)))
        .padding([2, 6])
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            text_color: Some(c().text_muted),
            border: Border::default().rounded(SMALL),
            ..container::Style::default()
        })
        .into()
}

/// A round pill: 10.5 px `text_sub` (`mono`: 11 px mono) on `hover` with a
/// divider edge, 2x8 px; `width` = fixed, the label centred (the About page's
/// "MIT", its licence pills).
pub fn pill<'a, Message: 'a>(
    label: &'a str,
    mono: bool,
    width: Option<f32>,
) -> Element<'a, Message> {
    let line = match mono {
        true => text(label).size(11).font(super::text::mono(Weight::Normal)),
        false => text(label).size(10.5).font(weight(Weight::Normal)),
    };
    let mut pill = container(line).padding([2, 8]);
    if let Some(w) = width {
        pill = pill.center_x(w);
    }
    pill.style(|_| container::Style {
        background: Some(c().hover.into()),
        text_color: Some(c().text_sub),
        border: Border::default()
            .rounded(radius::FULL)
            .width(1)
            .color(c().divider),
        ..container::Style::default()
    })
    .into()
}

/// What a status pill's dot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Done, all is well (`SUCCESS`).
    Ok,
    /// On its way (`accent`).
    Busy,
    /// Failed, or held back (`DANGER`).
    Error,
}

/// A header pill (`#autoSaveStatusPill`): a 6 px dot of `tone` and an 11 px
/// line at 85 %, on `hover` with a divider edge, round, 2x10 px.
pub fn status<'a, Message: 'a>(label: impl Into<String>, tone: Tone) -> Element<'a, Message> {
    let dot: Color = match tone {
        Tone::Ok => fixed::SUCCESS,
        Tone::Busy => c().accent,
        Tone::Error => fixed::DANGER,
    };
    marked(self::dot(dot), label)
}

/// The status pill with the OpenCircle `logo` (14 px) for its dot: an AI
/// marker ("Thought for 1.2 s"); never a generic AI glyph.
pub fn logo<'a, Message: 'a>(
    label: impl Into<String>,
    logo: iced::widget::image::Handle,
) -> Element<'a, Message> {
    let mark = iced::widget::image(logo)
        .width(size::ICON_SMALL)
        .height(size::ICON_SMALL);
    marked(mark, label)
}

fn marked<'a, Message: 'a>(
    mark: impl Into<Element<'a, Message>>,
    label: impl Into<String>,
) -> Element<'a, Message> {
    let line = text(label.into())
        .size(11)
        .font(weight(Weight::Normal))
        .color(faded(c().text, 0.85));
    container(row![mark.into(), line].spacing(space::GAP).align_y(Center))
        .padding([2, 10])
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// A 6 px round dot of `colour` (a status: the setup's "for everyone").
pub fn dot<'a, Message: 'a>(colour: Color) -> Element<'a, Message> {
    container(Space::new().width(size::DOT).height(size::DOT))
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        })
        .into()
}

/// The done badge: a mono 600 ✓ on a 20 px `SUCCESS` disc (the setup's
/// emblem once installed).
pub fn done<'a, Message: 'a>() -> Element<'a, Message> {
    const D: f32 = 20.0;
    container(
        text("✓")
            .font(mono(Weight::Semibold))
            .size(11)
            .color(fixed::ON_ACCENT),
    )
    .center(D)
    .style(|_| container::Style {
        background: Some(fixed::SUCCESS.into()),
        border: Border::default().rounded(D / 2.0),
        ..container::Style::default()
    })
    .into()
}

/// The composer's attachment line (`#aiAttachedScreenChip`): a mono `mark`
/// ("UI", 9 px / 600 `accent` on a 20x16 `accent_soft` tile, radius 4), the
/// 11 px / 500 `label` and the mono `detail` in `text_muted` ("420×260 px"),
/// across its row on `hover`, 6x10 px, radius 12.
pub fn attached<'a, Message: 'a>(
    mark: &'a str,
    label: &'a str,
    detail: String,
) -> Element<'a, Message> {
    const TILE: (f32, f32) = (20.0, 16.0);
    let tile = container(text(mark).size(9).font(mono(Weight::Semibold)))
        .center_x(TILE.0)
        .center_y(TILE.1)
        .style(|_| container::Style {
            background: Some(c().accent_soft.into()),
            text_color: Some(c().accent),
            border: Border::default().rounded(SMALL),
            ..container::Style::default()
        });
    let line = row![
        tile,
        text(label)
            .size(11)
            .font(weight(Weight::Medium))
            .color(c().text),
        text(detail)
            .size(11)
            .font(mono(Weight::Normal))
            .color(c().text_muted),
    ]
    .spacing(8)
    .align_y(Center);
    container(line)
        .padding([6, 10])
        .width(iced::Fill)
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            border: Border::default().rounded(radius::CARD),
            ..container::Style::default()
        })
        .into()
}

/// An emblem: `icon` (20 px, `accent`) centred on a 42 px `accent_soft`
/// tile, radius 13 (the model download card's heading).
pub fn emblem<'a, Message: 'a>(icon: super::icon::Icon) -> Element<'a, Message> {
    const SIZE: f32 = 42.0;
    const GLYPH: f32 = 20.0;
    const CORNER: f32 = 13.0;
    container(super::icon::icon(icon, GLYPH).color(c().accent))
        .center(SIZE)
        .style(|_| container::Style {
            background: Some(c().accent_soft.into()),
            border: Border::default().rounded(CORNER),
            ..container::Style::default()
        })
        .into()
}

/// A chip that acts (a QR code's "QR code: …" and "Open link · host"): a
/// 12 px line in `text` on `hover`, a divider edge, round, 6x12 px; lighter
/// under the cursor.
pub fn action<'a, Message: Clone + 'a>(label: String, on_press: Message) -> Element<'a, Message> {
    use iced::widget::button;
    button(text(label).size(12).font(weight(Weight::Normal)))
        .padding([6, 12])
        .on_press(on_press)
        .style(|_, status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => c().elevated_hover,
                    _ => c().hover,
                }
                .into(),
            ),
            text_color: c().text,
            border: Border::default()
                .rounded(radius::FULL)
                .width(1)
                .color(c().divider),
            ..button::Style::default()
        })
        .into()
}

/// A chip floating over the screen on its own (a selection's Select text or
/// link): ONE pill on the widget surface, `height` tall, never a pill inside
/// another frame.
pub fn floating<'a, Message: Clone + 'a>(
    label: String,
    on_press: Message,
    height: f32,
    pad: f32,
) -> Element<'a, Message> {
    use iced::widget::button;
    button(container(text(label).size(12).font(weight(Weight::Normal))).center_y(iced::Fill))
        .height(height)
        .padding([0.0, pad])
        .on_press(on_press)
        .style(|_, status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => c().elevated_hover,
                    _ => c().widget,
                }
                .into(),
            ),
            text_color: c().text,
            border: Border::default()
                .rounded(radius::FULL)
                .width(1)
                .color(c().rim),
            ..button::Style::default()
        })
        .into()
}

/// The editing toolbar's Drag chip (`SnipDragHandle`: `.oc-btn`, 63x30, radius
/// 12): the grip dots in `accent` and "Drag". A press starts the OS drag of the
/// image (a button reacts on release, too late); the edge turns `accent`
/// under the cursor.
pub fn drag_out<'a, Message: Clone + 'a>(on_down: Message) -> Element<'a, Message> {
    use iced::widget::{hover, mouse_area};
    let look = |edge: Color, ink: Color| {
        let dots = text(super::icon::Icon::Grip.glyph())
            .font(super::icon::FONT)
            .size(14)
            .color(c().accent);
        let label = text("Drag")
            .size(11)
            .font(weight(Weight::Medium))
            .color(ink);
        container(row![dots, label].spacing(space::GAP).align_y(Center))
            .center_x(63)
            .center_y(30)
            .style(move |_| container::Style {
                background: Some(c().elevated.into()),
                border: Border::default().rounded(12).width(1).color(edge),
                ..container::Style::default()
            })
    };
    mouse_area(hover(
        look(c().elevated_border, c().text_sub),
        look(c().accent, c().text),
    ))
    .on_press(on_down)
    .interaction(iced::mouse::Interaction::Grab)
    .into()
}

/// A Text backdrop's sample for an option button: an "A" plain, ringed, or on
/// a fill (the button's colours).
pub fn sample<'a, Message: 'a>(ring: bool, fill: bool) -> Element<'a, Message> {
    let ink = if fill { c().widget } else { c().text_sub };
    container(text("A").size(14).color(ink))
        .padding([0, 4])
        .style(move |_| container::Style {
            background: fill.then(|| c().text_sub.into()),
            border: Border::default()
                .rounded(radius::KEYCAP)
                .width(if ring { 1.5 } else { 0.0 })
                .color(c().text_sub),
            ..container::Style::default()
        })
        .into()
}
