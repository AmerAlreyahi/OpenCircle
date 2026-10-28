//! A feature's preview at the top of its Settings page (`settings-window.js`
//! `featureHero`, `.oc-feature-*`): the feature's name and one line, its
//! shortcut as keycaps, Hide / Show preview, and the stage: a "5 s preview"
//! chip, Replay, and the clip's frame (owner, 2026-10-28: real recordings,
//! played by the caller), or a quiet "Preview" placeholder until the clip
//! exists.
use iced::widget::image::Handle;
use iced::widget::{Image, Space, column, container, row, text};
use iced::{Border, Center, ContentFit, Element, Fill};

use super::button::{Kind, button, label};
use super::icon::{Icon, icon};
use super::keycap::keycap;
use super::text::sub;
use super::themes::c;
use super::tokens::{Role, radius, size, space};

/// The clip's place on the stage: 200 px tall, the clip fitted inside (a
/// 1200x680 clip shows at about 353x200, sharp up to 300 % scaling).
pub const MEDIA_H: f32 = 200.0;
/// The card's inside (`padding: 10px`) and the stage's (`8px`).
const PAD: f32 = 10.0;
const STAGE_PAD: f32 = 8.0;
/// The chip's type (9 px).
const SMALL: f32 = 9.0;
/// A keycap's sides round its label.
const CAP_PAD: f32 = 16.0;

/// What a preview shows.
pub struct Hero<'a, Message> {
    pub title: &'a str,
    pub desc: &'a str,
    /// A Labs feature: "Labs" after the name.
    pub lab: bool,
    /// The feature's shortcut, one label per key (empty = not set).
    pub keys: Vec<String>,
    /// The stage is shown (else only the heading and Show preview).
    pub shown: bool,
    pub toggle: Message,
    pub replay: Message,
    /// The clip's frame now (`None`: no clip, the placeholder).
    pub frame: Option<Handle>,
}

pub fn hero<'a, Message: Clone + 'a>(h: Hero<'a, Message>) -> Element<'a, Message> {
    let mut name = row![super::text::text(Role::PAGE_TITLE, h.title)]
        .spacing(space::GAP)
        .align_y(iced::Bottom);
    if h.lab {
        name = name.push(sub(Role::CARD_DESC, "Labs"));
    }
    let keys: Element<'a, Message> = match h.keys.is_empty() {
        true => sub(Role::CARD_DESC, "Shortcut not set").into(),
        false => row(h.keys.into_iter().map(|k| {
            let w = super::text::width(&k, Role::KEYCAP.size, super::text::font(Role::KEYCAP));
            container(keycap(k)).width(w + CAP_PAD).into()
        }))
        .spacing(space::TRACK_PAD)
        .into(),
    };
    let collapse = match h.shown {
        true => "Hide preview",
        false => "Show preview",
    };
    // one compact line, shown or hidden: the name and line left; Hide / Show
    // preview, then the keycaps, right
    let heading = row![
        column![name, sub(Role::CARD_DESC, h.desc)]
            .spacing(space::TRACK_GAP)
            .width(Fill),
        button(label(collapse), Kind::Elevated, Some(h.toggle)),
        keys,
    ]
    .spacing(space::GAP * 2.0)
    .align_y(Center);
    let mut body = column![heading].spacing(STAGE_PAD);
    if h.shown {
        body = body.push(stage(h.frame, h.replay));
    }
    container(body)
        .padding(PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().card_idle.into()),
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

fn stage<'a, Message: Clone + 'a>(frame: Option<Handle>, replay: Message) -> Element<'a, Message> {
    let chip = container(
        text("5 s preview")
            .size(SMALL)
            .font(super::text::font(Role::CARD_DESC))
            .color(c().accent),
    )
    .padding([2, 8])
    .style(|_| container::Style {
        background: Some(c().accent_soft.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    });
    let again = row![icon(Icon::Replay, size::ICON_SMALL), label("Replay")]
        .spacing(space::GAP)
        .align_y(Center);
    // Replay only with a clip to play again (the placeholder has none)
    let replay = frame
        .is_some()
        .then(|| button(again, Kind::Elevated, Some(replay)));
    let top = row![chip, Space::new().width(Fill)]
        .push(replay)
        .align_y(Center);
    let media: Element<'a, Message> = match frame {
        Some(frame) => Image::new(frame)
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Contain)
            .border_radius(radius::MENU)
            .into(),
        None => sub(Role::CARD_DESC, "Preview").into(),
    };
    container(
        column![
            top,
            container(media)
                .center_x(Fill)
                .center_y(MEDIA_H)
                .height(MEDIA_H)
        ]
        .spacing(space::TRACK_PAD),
    )
    .padding(STAGE_PAD)
    .width(Fill)
    .clip(true)
    .style(|_| container::Style {
        background: Some(c().track_glass.into()),
        border: Border::default()
            .rounded(radius::CARD)
            .width(1)
            .color(c().divider),
        ..container::Style::default()
    })
    .into()
}
