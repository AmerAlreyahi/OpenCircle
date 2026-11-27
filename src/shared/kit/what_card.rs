//! "What can I do?" (R-17, the approved design "B2", option B tiles): a style E tray with
//! the logo and title, two columns of feature tiles (icon, name, one line, the user's own
//! keys as keycaps) and a footer ("Open this any time [F1] . tray menu", Don't show
//! tips). Pressing the heading drags the window.
use iced::widget::{Space, column, container, image, mouse_area};
use iced::{Border, Center, Element, Fill, Size};

use super::choice_card::Mark;
use super::dir;
use super::icon::glyph as icon_glyph;
use super::icon_button::quiet_close;
use super::key_hint::{cap, cap_width};
use super::surface::{Join, segment_card, tray};
use super::text::{self as kit_text, sub, text};
use super::themes::c;
use super::tokens::{Role, radius};

/// The card's width (the design: 540) and its parts' measures.
pub const WIDTH: f32 = 540.0;
const PAD: f32 = 6.0;
const GAP: f32 = 6.0;
const CORNER: f32 = 20.0;
const HEAD_H: f32 = 36.0;
const TILE_H: f32 = 123.0;
const FOOT_H: f32 = 32.0;
const BADGE: f32 = 30.0;

/// One feature.
#[derive(Debug, Clone)]
pub struct Tile<'a> {
    pub mark: Mark,
    pub name: &'a str,
    pub desc: &'a str,
    /// The user's own shortcut, one entry per key (none: not bound).
    pub keys: Vec<String>,
}

/// The card's own words.
#[derive(Debug, Clone, Copy)]
pub struct Words<'a> {
    pub title: &'a str,
    pub footer: &'a str,
    pub key: &'a str,
    pub tray: &'a str,
    pub dont_show: &'a str,
}

/// The card's window (logical px) for `tiles` tiles.
pub fn size(tiles: usize) -> Size {
    let rows = tiles.div_ceil(2) as f32;
    let grid = rows * TILE_H + (rows - 1.0).max(0.0) * GAP;
    Size::new(WIDTH, 2.0 * PAD + HEAD_H + GAP + grid + GAP + FOOT_H + 2.0)
}

fn badge<'a, Message: 'a>(mark: Mark) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match mark {
        Mark::Glyph(g) => icon_glyph(g, 16.0).color(c().accent).into(),
        Mark::Logo => image(super::logo::logo(16)).width(16).height(16).into(),
    };
    container(inner)
        .center(BADGE)
        .style(|_| container::Style {
            background: Some(c().accent_soft.into()),
            border: Border::default().rounded(9),
            ..container::Style::default()
        })
        .into()
}

fn keys<'a, Message: 'a>(keys: &[String]) -> Element<'a, Message> {
    // (a shortcut reads Ctrl, Shift, Space left to right in every language)
    iced::widget::row(
        keys.iter().map(|k| -> Element<'a, Message> {
            container(cap(k.as_str())).width(cap_width(k)).into()
        }),
    )
    .spacing(4)
    .into()
}

fn tile<'a, Message: 'a>(t: &Tile<'a>) -> Element<'a, Message> {
    let top = dir::row([
        badge(t.mark),
        container(
            text(Role::ROW_TITLE, t.name).font(kit_text::weight(iced::font::Weight::Semibold)),
        )
        .width(Fill)
        .align_x(dir::start())
        .into(),
    ])
    .spacing(10)
    .align_y(Center);
    let desc = container(sub(Role::ROW_DESC, t.desc))
        .width(Fill)
        .height(Fill)
        .align_x(dir::start());
    let body = column![
        top,
        desc,
        container(keys(&t.keys)).width(Fill).align_x(dir::start())
    ]
    .spacing(8)
    .width(Fill);
    segment_card(radius::DROPDOWN, body)
        .padding([12.0, 14.0])
        .width(Fill)
        .height(TILE_H)
        .into()
}

pub fn view<'a, Message: Clone + 'a>(
    tiles: &[Tile<'a>],
    words: Words<'a>,
    close: Message,
    dont_show: Message,
    drag: Message,
) -> Element<'a, Message> {
    let head = mouse_area(
        container(
            dir::row([
                image(super::logo::logo(18)).width(18).height(18).into(),
                container(text(Role::CARD_HEADING, words.title))
                    .width(Fill)
                    .align_x(dir::start())
                    .into(),
                quiet_close(false, Some(close)),
            ])
            .spacing(10)
            .align_y(Center),
        )
        .height(HEAD_H)
        .center_y(HEAD_H)
        .padding(dir::pad(iced::Padding::ZERO.left(10.0).right(2.0))),
    )
    .on_press(drag); // mouse only: drags the card
    let mut grid = column![].spacing(GAP);
    for pair in tiles.chunks(2) {
        let second: Element<'a, Message> = match pair.get(1) {
            Some(t) => tile(t),
            None => Space::new().width(Fill).into(),
        };
        grid = grid.push(dir::row([tile(&pair[0]), second]).spacing(GAP));
    }
    let foot = dir::row([
        container(
            dir::row([
                sub(Role::ROW_DESC, words.footer).into(),
                container(cap(words.key)).width(cap_width(words.key)).into(),
                sub(Role::ROW_DESC, format!("\u{b7} {}", words.tray)).into(),
            ])
            .spacing(6)
            .align_y(Center),
        )
        .width(Fill)
        .align_x(dir::start())
        .into(),
        super::button::button(
            sub(Role::ROW_DESC, words.dont_show),
            super::button::Kind::Quiet,
            Some(dont_show),
        ),
    ])
    .align_y(Center)
    .padding(dir::pad(iced::Padding::ZERO.left(12.0).right(10.0)));
    tray(
        false,
        CORNER,
        Join::None,
        column![head, grid, container(foot).height(FOOT_H).center_y(FOOT_H)].spacing(GAP),
    )
    .padding(PAD)
    .width(Fill)
    .height(Fill)
    .into()
}
