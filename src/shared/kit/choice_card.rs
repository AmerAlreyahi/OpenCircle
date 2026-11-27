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
use super::tokens::{Role, radius, settings};

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

const RING: f32 = 20.0;

fn tile<'a, Message: 'a>(mark: Mark, chosen: bool) -> Element<'a, Message> {
    let tint = match chosen {
        true => on(c().accent),
        false => c().text,
    };
    let inner: Element<'a, Message> = match mark {
        Mark::Glyph(g) => icon_glyph(g, settings::CHOICE_ICON).color(tint).into(),
        Mark::Logo => iced::widget::image(super::logo::logo(18))
            .width(18)
            .height(18)
            .into(),
    };
    container(inner)
        .center(settings::CHOICE_TILE)
        .style(move |_| container::Style {
            background: Some(
                match chosen {
                    true => c().accent,
                    false => c().widget,
                }
                .into(),
            ),
            border: Border::default()
                .rounded(radius::TRACK)
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
        text(Role::CHOICE_TITLE, option.title),
        sub(Role::CHOICE_DESC, option.desc).line_height(1.5)
    ]
    .spacing(if tall { 4 } else { 3 })
    .align_x(super::dir::start());
    let body: Element<'a, Message> = match tall {
        true => column![
            super::dir::row([
                tile(option.mark, chosen),
                Space::new().width(Fill).into(),
                ring(chosen)
            ]),
            Space::new().height(settings::CARD_GAP),
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
    let min = if tall {
        settings::CHOICE_TALL_H
    } else {
        settings::CHOICE_WIDE_H
    };
    super::focus::press(
        button(column![body, Space::new().height(Fill)].height(min - 2.0 * settings::CHOICE_PAD))
            .width(Fill)
            .height(min)
            .padding(settings::CHOICE_PAD)
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
    super::card::grid_spaced(options.len().clamp(1, 4), settings::CARD_GAP, cards)
}
