//! Settings round 2 (B2): a comparable choice as cards, each saying what it does before the pick. Two
//! options sit side by side (icon at the start, words after it); three or four stand as columns (icon over
//! the words). The chosen card: soft accent fill, accent edge, a filled accent icon tile and a check; the
//! others a quiet card with an empty ring. One part for every such choice (Default action, Selection,
//! Search opens, Engine, After a capture, Answers from ...).
use iced::widget::{Space, button, column, container};
use iced::{Border, Element, Fill};

use super::icon::glyph as icon_glyph;
use super::text::{sub, text};
use super::themes::{c, on};
use super::tokens::{Role, radius, space};

/// What marks a card: a lucide glyph, or the OpenCircle logo (the one AI marker).
#[derive(Debug, Clone, Copy)]
pub enum Mark {
    Glyph(char),
    Logo,
}

/// One option.
#[derive(Debug, Clone, Copy)]
pub struct Choice<'a> {
    pub mark: Mark,
    pub title: &'a str,
    pub desc: &'a str,
}

const TILE: f32 = 32.0;
const RING: f32 = 20.0;
const PAD: f32 = 14.0;
/// Least heights: side by side, standing.
const WIDE_H: f32 = 78.0;
const TALL_H: f32 = 132.0;

fn tile<'a, Message: 'a>(mark: Mark, chosen: bool) -> Element<'a, Message> {
    let tint = match chosen {
        true => on(c().accent),
        false => c().text,
    };
    let inner: Element<'a, Message> = match mark {
        Mark::Glyph(g) => icon_glyph(g, 16.0).color(tint).into(),
        Mark::Logo => iced::widget::image(super::logo::logo(16))
            .width(16)
            .height(16)
            .into(),
    };
    container(inner)
        .center(TILE)
        .style(move |_| container::Style {
            background: Some(
                match chosen {
                    true => c().accent,
                    false => c().widget,
                }
                .into(),
            ),
            border: Border::default()
                .rounded(radius::CONTROL)
                .width(if chosen { 0 } else { 1 })
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

fn ring<'a, Message: 'a>(chosen: bool) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match chosen {
        true => icon_glyph('\u{e070}', 12.0).color(on(c().accent)).into(),
        false => Space::new().into(),
    };
    container(inner)
        .center(RING)
        .style(move |_| container::Style {
            background: chosen.then(|| c().accent.into()),
            border: Border::default()
                .rounded(RING)
                .width(if chosen { 0 } else { 1 })
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}

fn card<'a, Message: Clone + 'a>(
    option: Choice<'a>,
    chosen: bool,
    tall: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let words = column![
        text(Role::CARD_HEADING, option.title).size(super::text::size(Role::ROW_TITLE) + 1.0),
        sub(Role::ROW_DESC, option.desc)
    ]
    .spacing(2)
    .align_x(super::dir::start());
    let body: Element<'a, Message> = match tall {
        true => column![
            super::dir::row([
                tile(option.mark, chosen),
                Space::new().width(Fill).into(),
                ring(chosen)
            ]),
            Space::new().height(space::GAP + 2.0),
            words
        ]
        .into(),
        false => super::dir::row([
            tile(option.mark, chosen),
            container(words).width(Fill).into(),
            ring(chosen),
        ])
        .spacing(12)
        .align_y(iced::Alignment::Start)
        .into(),
    };
    let min = if tall { TALL_H } else { WIDE_H };
    super::focus::press(
        button(column![body, Space::new().height(Fill)].height(min - 2.0 * PAD))
            .width(Fill)
            .height(min)
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

/// `options` as cards in one row, `chosen` the picked one; `pick(i)` is what a press sends. A fifth card
/// or more wraps in rows of four.
pub fn choice_cards<'a, Message: Clone + 'a>(
    options: &[Choice<'a>],
    chosen: usize,
    pick: impl Fn(usize) -> Message,
) -> Element<'a, Message> {
    let tall = options.len() > 2;
    let cards = options
        .iter()
        .enumerate()
        .map(|(i, o)| card(*o, i == chosen, tall, pick(i)));
    super::card::grid(options.len().clamp(1, 4), cards)
}
