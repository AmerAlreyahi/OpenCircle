//! A one-time tip (R-17, the approved design "B1"): a style E tray with two raised
//! segments, the message (the OpenCircle logo, words, a keycap or a bold lead) and its
//! actions (Got it, Don't show tips, a quiet x). It sits against the widget it hints at.
//! In a right-to-left view the segments and everything in them run right to left.
use iced::widget::{container, image};
use iced::{Center, Element, Size};

use super::button::{Kind, button};
use super::dir;
use super::icon_button::quiet_close;
use super::key_hint::{cap, cap_width};
use super::surface::{Join, segment_card, tray};
use super::text::{self as kit_text, font, sub, text};
use super::tokens::{Role, radius, widget_e};

/// The tray's height (the design: 44, rim included) and its raised segments' (32).
pub const HEIGHT: f32 = 44.0;
const SEGMENT_H: f32 = 32.0;
/// The tray's corners (`.tipw`: 20).
const CORNER: f32 = 20.0;
const PAD: f32 = widget_e::TRAY_PAD_X;
const GAP: f32 = 6.0;
/// The logo, and the gap after it and between the message's runs.
const LOGO: f32 = 15.0;
const ITEM_GAP: f32 = 5.0;
/// The message segment's padding (start, end); a text action's (`.ql`: 26 high, 8 each side).
const MSG_PAD: [f32; 2] = [8.0, 12.0];
const ACTION_H: f32 = 26.0;
const ACTION_PAD: f32 = 8.0;
const ROLE: Role = Role::NAV;
const ACTION: Role = Role::LABEL;

/// A run of the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run<'a> {
    Plain(&'a str),
    /// A bold lead word ("Original").
    Strong(&'a str),
    /// A key, as a keycap.
    Key(&'a str),
}

/// The action words.
#[derive(Debug, Clone, Copy)]
pub struct Words<'a> {
    pub got_it: &'a str,
    pub dont_show: &'a str,
}

fn run_width(run: &Run<'_>) -> f32 {
    match run {
        Run::Plain(s) => kit_text::width(s, kit_text::size(ROLE), font(ROLE)),
        Run::Strong(s) => kit_text::width(
            s,
            kit_text::size(ROLE),
            kit_text::weight(iced::font::Weight::Semibold),
        ),
        Run::Key(k) => cap_width(k),
    }
}

fn action_width(label: &str) -> f32 {
    kit_text::width(label, kit_text::size(ACTION), font(ACTION)) + 2.0 * ACTION_PAD
}

/// The segments' widths: the message, the actions.
fn segments(runs: &[Run<'_>], words: Words<'_>) -> (f32, f32) {
    let items = 1 + runs.len();
    let message = MSG_PAD[0]
        + MSG_PAD[1]
        + LOGO
        + runs.iter().map(run_width).sum::<f32>()
        + items as f32 * ITEM_GAP;
    let actions = 2.0 * widget_e::SEGMENT_PAD_X
        + action_width(words.got_it)
        + action_width(words.dont_show)
        + widget_e::QUIET_X
        + 2.0 * widget_e::SEGMENT_GAP;
    (message, actions)
}

/// How big the tip is (logical px).
pub fn size(runs: &[Run<'_>], words: Words<'_>) -> Size {
    let (m, a) = segments(runs, words);
    Size::new((2.0 * PAD + m + GAP + a).ceil(), HEIGHT)
}

pub fn view<'a, Message: Clone + 'a>(
    runs: &[Run<'a>],
    words: Words<'a>,
    got_it: Message,
    dont_show: Message,
    close: Message,
) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = vec![
        image(super::logo::logo(LOGO as u32))
            .width(LOGO)
            .height(LOGO)
            .into(),
    ];
    items.extend(runs.iter().map(|run| -> Element<'a, Message> {
        match run {
            Run::Plain(s) => text(ROLE, *s).into(),
            Run::Strong(s) => text(ROLE, *s)
                .font(kit_text::weight(iced::font::Weight::Semibold))
                .into(),
            Run::Key(k) => container(cap(k)).height(24).center_y(24).into(),
        }
    }));
    let message = segment_card(
        radius::FULL,
        container(dir::row(items).spacing(ITEM_GAP).align_y(Center)).padding(dir::pad(
            iced::Padding::ZERO.left(MSG_PAD[0]).right(MSG_PAD[1]),
        )),
    )
    .height(SEGMENT_H)
    .center_y(SEGMENT_H);
    let word = |label: &'a str, accent: bool, msg: Message| -> Element<'a, Message> {
        let label = match accent {
            true => {
                kit_text::accent(ACTION, label).font(kit_text::weight(iced::font::Weight::Semibold))
            }
            false => sub(ACTION, label),
        };
        button(
            container(label)
                .height(ACTION_H)
                .center_y(ACTION_H)
                .padding([0.0, ACTION_PAD]),
            Kind::Quiet,
            Some(msg),
        )
    };
    let actions = segment_card(
        radius::FULL,
        dir::row([
            word(words.got_it, true, got_it),
            word(words.dont_show, false, dont_show),
            quiet_close(false, Some(close)),
        ])
        .spacing(widget_e::SEGMENT_GAP)
        .padding(iced::Padding::new(widget_e::SEGMENT_PAD_X).top(0).bottom(0))
        .align_y(Center),
    )
    .height(SEGMENT_H)
    .center_y(SEGMENT_H);
    tray(
        false,
        CORNER,
        Join::None,
        dir::row([message.into(), actions.into()])
            .spacing(GAP)
            .align_y(Center),
    )
    .padding(PAD)
    .height(HEIGHT)
    .center_y(HEIGHT)
    .into()
}
