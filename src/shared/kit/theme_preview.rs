//! Settings round 2 (Appearance, option 1): a theme drawn as a tiny window (sidebar stubs, a panel with
//! two lines and an accent button) and the cards that hold it. `mode_card` = Light / Dark / System (the
//! mode, with its radio ring); `theme_mini_card` = one colour theme of a mode (name, check when chosen).
//! System's window is light at its sidebar and dark in its panel (the design cuts it on a diagonal).
use iced::widget::{Space, button, column, container, row};
use iced::{Border, Color, Element, Fill};

use super::icon::glyph as icon_glyph;
use super::text::{sub, text};
use super::themes::{Theme, c, on};
use super::tokens::{Role, radius};

const PAD: f32 = 10.0;
const LINE: f32 = 4.0;

/// The colours of a tiny window: the frame behind it, its sidebar and its panel.
fn bar<'a, Message: 'a>(colour: Color, w: f32) -> Element<'a, Message> {
    container(Space::new().width(w).height(LINE))
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border::default().rounded(LINE),
            ..container::Style::default()
        })
        .into()
}

/// A tiny window: the sidebar and the frame from `side`, the panel from `panel`.
pub fn mini_window<'a, Message: 'a>(
    side: &'static Theme,
    panel: &'static Theme,
    h: f32,
) -> Element<'a, Message> {
    let stubs = column![
        bar(side.c.accent, 22.0),
        bar(side.c.text_muted, 30.0),
        bar(side.c.text_muted, 26.0)
    ]
    .spacing(4);
    let lines = column![bar(panel.c.text_sub, 56.0), bar(panel.c.text_muted, 76.0)].spacing(4);
    let button_ =
        container(Space::new().width(26.0).height(6.0)).style(move |_| container::Style {
            background: Some(panel.c.accent.into()),
            border: Border::default().rounded(3.0),
            ..container::Style::default()
        });
    let panel_box = container(
        column![lines, Space::new().height(Fill), button_]
            .height(Fill)
            .spacing(4),
    )
    .padding(6)
    .width(Fill)
    .height(Fill)
    .style(move |_| container::Style {
        background: Some(panel.c.widget.into()),
        border: Border::default().rounded(5.0),
        ..container::Style::default()
    });
    container(row![container(stubs).width(44.0), panel_box].spacing(4))
        .padding(5)
        .width(Fill)
        .height(h)
        .style(move |_| container::Style {
            background: Some(side.c.bg.into()),
            border: Border::default()
                .rounded(radius::CONTROL)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

fn ring<'a, Message: 'a>(chosen: bool) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match chosen {
        true => icon_glyph('\u{e070}', 11.0).color(on(c().accent)).into(),
        false => Space::new().into(),
    };
    container(inner)
        .center(18.0)
        .style(move |_| container::Style {
            background: chosen.then(|| c().accent.into()),
            border: Border::default()
                .rounded(18.0)
                .width(if chosen { 0 } else { 1 })
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}

fn frame<'a, Message: Clone + 'a>(
    body: Element<'a, Message>,
    chosen: bool,
    on_press: Message,
) -> Element<'a, Message> {
    super::focus::press(
        button(body)
            .width(Fill)
            .padding(PAD)
            .style(move |_, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(match (chosen, hot) {
                        (true, _) => c().accent_soft.into(),
                        (false, true) => c().hover.into(),
                        _ => c().card_idle.into(),
                    }),
                    border: Border::default()
                        .rounded(radius::CARD)
                        .width(if chosen { 1.5 } else { 1.0 })
                        .color(if chosen { c().accent } else { c().divider }),
                    ..button::Style::default()
                }
            }),
        radius::CARD,
        Some(on_press),
    )
    .into()
}

/// A mode card: its tiny window (`side` / `panel` themes), then the name and its line, the ring at the end.
pub fn mode_card<'a, Message: Clone + 'a>(
    (side, panel): (&'static Theme, &'static Theme),
    name: &'a str,
    desc: &'a str,
    chosen: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let words = row![
        text(Role::CARD_HEADING, name),
        container(sub(Role::ROW_DESC, desc)).width(Fill),
        ring(chosen)
    ]
    .spacing(8)
    .align_y(iced::Center);
    frame(
        column![mini_window(side, panel, 76.0), words]
            .spacing(10)
            .into(),
        chosen,
        on_press,
    )
}

/// A colour theme's card: its tiny window and its name, a check when chosen.
pub fn theme_mini_card<'a, Message: Clone + 'a>(
    theme: &'static Theme,
    name: &'a str,
    chosen: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let mark: Element<'a, Message> = match chosen {
        true => icon_glyph('\u{e070}', 12.0).color(c().accent).into(),
        false => Space::new().into(),
    };
    let words =
        row![container(text(Role::CARD_TITLE, name)).width(Fill), mark].align_y(iced::Center);
    frame(
        column![mini_window(theme, theme, 52.0), words]
            .spacing(8)
            .into(),
        chosen,
        on_press,
    )
}
