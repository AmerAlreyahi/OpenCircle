//! A window's side panel (Studio's Library and Properties, board 2): its own
//! 40 px header (a name and the button that hides it), a hairline, and a body
//! that scrolls, on the `sidebar` fill. Also the parts its lists are made of:
//! a collapsible section header (30 px), a list row (a thumbnail row or an
//! object row, hover shade, `accent_soft` while selected) and the menu bar's
//! opener (28 px).
use iced::widget::{button, column, container, text};
use iced::{Border, Center, Element, Fill, Length, Padding};

use super::icon::{FONT, Icon};
use super::icon_button::{self, State};
use super::themes::c;
use super::tokens::{Role, size, space};
use super::tooltip::{Why, hint};
use super::{divider, scroll};

pub const SECTION_H: f32 = 30.0;
const OPENER_H: f32 = 28.0;
const OPENER_R: f32 = 8.0;
const PAD: f32 = 12.0;

/// The panel: `title`, the hide button (`glyph`, `tip`), a hairline, `body`.
pub fn panel<'a, Message: Clone + 'a>(
    width: f32,
    title: &'a str,
    glyph: char,
    hide: Message,
    tip: &'a str,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let toggle = hint(
        icon_button::square(glyph, State::default(), Some(hide)),
        tip,
        Why::Icon,
        iced::widget::tooltip::Position::Bottom,
    );
    let head = container(
        crate::dir_row![super::text::text(Role::CONTROL, title).width(Fill), toggle]
            .align_y(Center),
    )
    .padding(Padding {
        left: PAD,
        right: 8.0,
        ..Padding::ZERO
    })
    .height(size::HEADER)
    .center_y(size::HEADER);
    container(column![
        head,
        divider::divider(),
        scroll::scroll(body).height(Fill)
    ])
    .width(Length::Fixed(width))
    .height(Fill)
    .style(|_| container::Style {
        background: Some(c().sidebar.into()),
        ..container::Style::default()
    })
    .into()
}

/// A section's header: a chevron (down open, forward folded), the name in
/// small caps, a count at the end.
pub fn section_head<'a, Message: Clone + 'a>(
    words: &'a str,
    open: bool,
    count: Option<usize>,
    toggle: Message,
) -> Element<'a, Message> {
    let glyph = if open {
        Icon::ChevronDown
    } else {
        Icon::Forward
    };
    let face = text(glyph.glyph()).font(FONT);
    let chevron = face.size(12).color(c().text_muted);
    let mut line = crate::dir_row![
        chevron,
        super::text::muted(Role::SECTION, words).width(Fill)
    ]
    .spacing(space::GAP)
    .align_y(Center);
    if let Some(n) = count {
        line = line.push(super::text::muted(Role::KEYCAP, n.to_string()));
    }
    let b = button(line)
        .width(Fill)
        .height(SECTION_H)
        .padding([0.0, PAD])
        .style(|_, status| button::Style {
            background: matches!(status, button::Status::Hovered).then(|| c().hover.into()),
            text_color: c().text_muted,
            ..button::Style::default()
        });
    super::focus::press(b, 0.0, Some(toggle)).into()
}

/// A list row `height` tall around `content`: shaded under the pointer, tinted
/// while `selected`; pressable when `press` is given.
pub fn row<'a, Message: Clone + 'a>(
    content: Element<'a, Message>,
    height: f32,
    selected: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    let b = button(container(content).center_y(Fill))
        .width(Fill)
        .height(height)
        .padding([0.0, PAD])
        .style(move |_, status| button::Style {
            background: match () {
                _ if selected => Some(c().accent_soft.into()),
                _ if matches!(status, button::Status::Hovered) => Some(c().hover.into()),
                _ => None,
            },
            text_color: c().text,
            border: Border::default(),
            ..button::Style::default()
        });
    super::focus::press(b, 0.0, press).into()
}

/// A row's lead glyph (14 px): `text_sub`, the accent while selected.
pub fn lead<'a, Message: 'a>(glyph: char, selected: bool) -> Element<'a, Message> {
    text(glyph)
        .font(FONT)
        .size(14)
        .color(if selected { c().accent } else { c().text_sub })
        .into()
}

/// A menu bar's opener: 28 high, radius 8, raised while its menu is `open`.
pub fn menu_opener<'a, Message: Clone + 'a>(
    words: &'a str,
    open: bool,
    press: Message,
) -> Element<'a, Message> {
    let label = super::text::text(Role::NAV, words);
    let b = button(container(label).center_y(Fill).padding([0.0, 11.0]))
        .height(OPENER_H)
        .padding(0)
        .style(move |_, status| {
            let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let (fill, edge) = match () {
                _ if open => (Some(c().elevated), c().elevated_border),
                _ if hot => (Some(c().hover), iced::Color::TRANSPARENT),
                _ => (None, iced::Color::TRANSPARENT),
            };
            button::Style {
                background: fill.map(Into::into),
                text_color: if open || hot { c().text } else { c().text_sub },
                border: Border::default().rounded(OPENER_R).width(1).color(edge),
                ..button::Style::default()
            }
        });
    super::focus::press(b, OPENER_R, Some(press)).into()
}
