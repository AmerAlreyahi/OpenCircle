//! What translate in place shows over the frame (the design's "Translate
//! screen" states), from kit parts only: the language pill (source → target,
//! the status), and the result card for its prompts, errors and the word card.
use iced::widget::{column, container, opaque, pin, row, stack};
use iced::{Center, Element, Fill, Point, Rectangle, Size};

use super::{Card, Msg, Phase, Session};
use crate::actions::ask::Status;
use crate::actions::{self};
use crate::ui::ai_chat::{actions, card, note};
use crate::ui::kit::button::{Kind, button, label as button_label};
use crate::ui::kit::markdown;
use crate::ui::kit::panel_frame::Header;
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{error, text as label};
use crate::ui::kit::tokens::{Role, radius};
use crate::ui::picker;
use crate::ui::screens::model_download::{self, Model, OCR, SCRIPTS, Stage, TRANSLATION};

/// The pill's distance to the top of the screen, and the status tag's to
/// the pill (PICKER_SPEC.md).
pub const PILL_TOP: f32 = 14.0;
const TAG_GAP: f32 = 8.0;
/// Inside the status tag, and between its text and its button.
const TAG_PAD: [f32; 2] = [3.0, 10.0];
const TAG_SPACING: f32 = 8.0;
/// Between a card's blocks.
const STACK: f32 = 10.0;
/// The note under the pill (`surface::hint`'s padding).
const NOTE_PAD: [f32; 2] = [8.0, 14.0];

/// A problem's first sentence: the rest is how-to the "Set up AI" action covers.
fn gist(message: &str) -> &str {
    let end = ([". ", " ("].iter().filter_map(|m| message.find(m)))
        .min()
        .unwrap_or(message.len());
    message[..end].trim_end_matches('.')
}

impl Session {
    /// The layer over the frame: status and the language picker, and the
    /// floating result card (at `at`, like every result card) for prompts,
    /// errors and the word card. `hover`: on the card's resize edge.
    pub fn view<'a>(&'a self, screen: Size, at: Rectangle, hover: bool) -> Element<'a, Msg> {
        let card =
            |body: Element<'a, Msg>, footer: Option<Element<'a, Msg>>, title: Option<&'a str>| {
                let title = title.unwrap_or(actions::translate::ACTION.label);
                let header = Header {
                    glyph: actions::translate::ACTION.icon,
                    title,
                    chip: None,
                    menu: false,
                    held: false,
                    logo: None,
                };
                card(header, body, footer, at, screen, hover, Msg::Card)
            };
        // the pill stays beside the card
        let with_pill = |card: Element<'a, Msg>| -> Element<'a, Msg> {
            stack![
                card,
                self.pill(Rectangle::with_size(screen), None, None, None)
            ]
            .into()
        };
        let whole = Rectangle::with_size(screen);
        // the download card (`model_download`): asks, its progress, why it failed
        let download = |model: &Model, bytes, stage, provider| {
            let on = model_download::On {
                download: Msg::Download,
                dismiss: Some(Msg::Close),
                provider,
            };
            let card = model_download::card(model, bytes, stage, on);
            let pill = Rectangle::new(pill_at(whole), picker::PILL);
            stack![
                model_download::floating(card, screen, pill),
                self.pill(whole, None, None, None)
            ]
            .into()
        };
        // what is (being) fetched: the pair's models, else OCR's
        let fetched = |stage| match self.offline {
            Some(pair) => download(&TRANSLATION, pair.full_size(), stage, None),
            None => match &self.readers {
                // only the readers that aren't installed
                Some((bytes, names)) => {
                    let does = format!("Read {names} text");
                    download(
                        &Model {
                            does: &does,
                            ..SCRIPTS
                        },
                        *bytes,
                        stage,
                        None,
                    )
                }
                None => download(&OCR, crate::ocr::models::full_size(), stage, None),
            },
        };
        match &self.phase {
            Phase::Consent(None) => fetched(Stage::Ask),
            Phase::Consent(Some((_, bytes))) => {
                download(&TRANSLATION, *bytes, Stage::Ask, Some(Msg::OpenSettings))
            }
            Phase::Downloading(done) => fetched(Stage::at(*done)),
            Phase::DownloadFailed(why) => fetched(Stage::Failed(why)),
            Phase::Translating => self.pill(whole, Some("Translating…"), None, None),
            // Already in the target, no offline model, an AI error: one soft line
            // under the pill, never a card, and the pill stays put (owner,
            // 2026-11-04). The picker stays: another pair may work offline.
            Phase::Failed(problem) => {
                let mut line = row![note(gist(&problem.message))]
                    .spacing(TAG_SPACING)
                    .align_y(Center);
                if problem.setup {
                    let set_up = button_label("Set up AI");
                    line = line.push(button(set_up, Kind::Quiet, Some(Msg::OpenSettings)));
                }
                let below = surface(Material::Widget, radius::FULL, line).padding(NOTE_PAD);
                self.pill(whole, None, None, Some(below.into()))
            }
            // lines kept in their own language: a quiet offer in the pill
            Phase::Shown(shown) if self.card.is_none() && shown.offer.is_some() => {
                self.offer(whole, shown.offer.as_ref().expect("just checked"))
            }
            Phase::Shown(_) => match &self.card {
                Some(word) => {
                    let (body, footer) = card_view(word);
                    with_pill(card(body, Some(footer), Some(&word.title)))
                }
                None => self.pill(whole, None, None, None),
            },
        }
    }

    /// The pill with the offer beside it: "Spanish text found" and its Get.
    fn offer<'a>(&'a self, region: Rectangle, o: &'a super::Offer) -> Element<'a, Msg> {
        let get = button(button_label(&o.get), Kind::Elevated, Some(Msg::GetHeld));
        self.pill(region, Some(&o.found), Some(get), None)
    }

    /// The pill (`picker::PILL`, fixed) at the top centre of `region` (the
    /// screen, or what a card leaves free); `status` (and `then`, a button) a
    /// quiet tag beside it on its end side; `below` a note centred under it;
    /// the open language list hanging from its chip. Nothing here changes
    /// the pill's place or size.
    fn pill<'a>(
        &'a self,
        region: Rectangle,
        status: Option<&'a str>,
        then: Option<Element<'a, Msg>>,
        below: Option<Element<'a, Msg>>,
    ) -> Element<'a, Msg> {
        let rtl = self.picker.rtl;
        let at = pill_at(region);
        let mut layer = stack![pin(opaque(self.picker.pill().map(Msg::Picker))).position(at)];
        if status.is_some() || then.is_some() {
            let line = row![].push(status.map(note)).push(then);
            let tag = surface(
                Material::Widget,
                radius::FULL,
                line.spacing(TAG_SPACING).align_y(Center),
            )
            .padding(TAG_PAD);
            let tag = container(opaque(tag)).center_y(picker::PILL.height);
            let tag: Element<'a, Msg> = match rtl {
                true => {
                    let room = (at.x - region.x - TAG_GAP).max(0.0);
                    pin(tag.align_right(room))
                        .position(Point::new(region.x, at.y))
                        .into()
                }
                false => pin(tag)
                    .position(Point::new(at.x + picker::PILL.width + TAG_GAP, at.y))
                    .into(),
            };
            layer = layer.push(tag);
        }
        if let Some(below) = below {
            let y = at.y + picker::PILL.height + STACK;
            let below = container(opaque(below)).center_x(region.width);
            layer = layer.push(pin(below).position(Point::new(region.x, y)));
        }
        if let (Some(list), Some(offset)) = (self.picker.list(), self.picker.list_at()) {
            layer = layer.push(pin(opaque(list.map(Msg::Picker))).position(at + offset));
        }
        layer.into()
    }
}

/// Where the pill's top-left corner is in `region`: centred, `PILL_TOP` down.
pub fn pill_at(region: Rectangle) -> Point {
    let x = region.x + ((region.width - picker::PILL.width) / 2.0).round();
    Point::new(x, region.y + PILL_TOP)
}

/// `at` (a result card) kept off the pill: where it would reach the pill's
/// rect (a side card on a screen too narrow to pass it), its top moves under
/// the pill, its bottom staying; never shorter than `card::MIN`.
pub fn clear_of_pill(at: Rectangle, screen: Size) -> Rectangle {
    let pill = Rectangle::new(pill_at(Rectangle::with_size(screen)), picker::PILL);
    let below = pill.y + pill.height + STACK;
    if at.x >= pill.x + pill.width || at.x + at.width <= pill.x || at.y >= below {
        return at;
    }
    let bottom = at.y + at.height;
    let top = below.min(bottom - crate::ui::kit::panel_frame::MIN.height.min(at.height));
    Rectangle::new(Point::new(at.x, top), Size::new(at.width, bottom - top))
}

/// The word card (the design's "Word explanation", titled "translation ·
/// original"): its body (the translation, the AI's explanation) and actions
/// (Explain = E, Ask AI = A).
fn card_view(card: &Card) -> (Element<'_, Msg>, Element<'_, Msg>) {
    // Mixed lines ("«name» ...") would each pick their own side: a
    // right-to-left target's lines are laid at the right as a block. (A
    // `Fill` text aligned right drew nothing in the card.)
    let side = |line: iced::widget::Text<'static>| -> Element<'_, Msg> {
        match card.rtl {
            true => container(line).align_right(Fill).into(),
            false => line.width(Fill).into(),
        }
    };
    let mut body = column![
        note(card.block.original.clone()).width(Fill),
        side(label(Role::CHAT, card.block.translation.clone())),
    ]
    .spacing(STACK);
    if let Some(turn) = card.chat.turns.last() {
        body = body.push(
            match turn.answer.is_empty() && card.chat.status == Status::Streaming {
                true => note("Thinking…").width(Fill).into(),
                // Markdown, as the chat shows it (lines shape by their own direction)
                false => markdown::markdown(card.answer.items()).map(Msg::Answer),
            },
        );
    }
    if let Status::Failed(e) = &card.chat.status {
        body = body.push(error(Role::NOTE, e.clone()));
    }
    let ask = &actions::ask::ACTION;
    let buttons = actions(("Explain", Msg::Explain), [(ask.label, Msg::Ask)]);
    (body.into(), buttons)
}
