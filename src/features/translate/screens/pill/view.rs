//! What translate in place shows over the frame (the design's "Translate
//! screen" states), from kit parts only: the language pill (source → target,
//! the status), and the result card for its prompts, errors and the word card.
use iced::widget::{column, container, mouse_area, opaque, pin, row, stack};
use iced::{Center, Element, Fill, Point, Rectangle, Size};

use super::glossary::{AddForm, AddMsg};
use super::{Card, Msg, Phase, Session};
use crate::features::ask_ai::Fix;
use crate::features::ask_ai::ask::Status;
use crate::features::ask_ai::screens::ai_chat::{actions, ai_error, card, note, provider_name};
use crate::features::text_reading::screens::model_download::{
    self, Does, Model, OCR, SCRIPTS, Stage, TRANSLATION,
};
use crate::features::translate::screens::language_picker as picker;
use crate::shared::app_language::{S, fmt, tr};
use crate::shared::kit::button::{Kind, button, label as button_label, regular};
use crate::shared::kit::dir;
use crate::shared::kit::icon_button::{State, icon_button};
use crate::shared::kit::markdown;
use crate::shared::kit::panel_frame::{self as frame, Header};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, text as label};
use crate::shared::kit::themes::c;
use crate::shared::kit::tokens::{Role, radius};
use crate::shared::kit::tooltip::{Why, hint};
use crate::t;

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
        // the pill and its lists speak the app's language (the word card's
        // own text follows the translation's language: `Card::rtl`)
        let _dir = dir::scope(crate::shared::app_language::rtl());
        let card =
            |body: Element<'a, Msg>, footer: Option<Element<'a, Msg>>, title: Option<&'a str>| {
                let title = title.unwrap_or(tr(crate::features::translate::action::ACTION.label));
                let header = Header {
                    glyph: crate::features::translate::action::ACTION.icon,
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
                    let does = fmt(S::md_read_names, &[names]);
                    download(
                        &Model {
                            does: Does::Text(&does),
                            ..SCRIPTS
                        },
                        *bytes,
                        stage,
                        None,
                    )
                }
                None => download(
                    &OCR,
                    crate::features::text_reading::models::full_size(),
                    stage,
                    None,
                ),
            },
        };
        match &self.phase {
            Phase::Consent(None) => fetched(Stage::Ask),
            Phase::Consent(Some((_, bytes))) => {
                download(&TRANSLATION, *bytes, Stage::Ask, Some(Msg::OpenSettings))
            }
            Phase::Downloading(done) => fetched(Stage::at(*done)),
            Phase::DownloadFailed(why) => fetched(Stage::Failed(why)),
            Phase::Translating => self.pill(whole, Some(t!(tp_translating)), None, None),
            // Already in the target, no offline model, an AI error: one soft line
            // under the pill, never a card, and the pill stays put (owner,
            // 2026-11-04). The picker stays: another pair may work offline.
            Phase::Failed(problem) => {
                let mut line = row![note(gist(&problem.message))]
                    .spacing(TAG_SPACING)
                    .align_y(Center);
                if problem.setup {
                    let set_up = button_label(t!(tp_set_up_ai));
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
                    let (body, footer) = card_view(word, at.width - frame::PADDING * 2.0);
                    with_pill(card(body, Some(footer), Some(&word.title)))
                }
                // (owned text: the tag's element slot)
                None => {
                    let update = self.update_line().map(|l| note(l).into());
                    self.pill(whole, self.tag.as_deref(), update, None)
                }
            },
        }
    }

    /// The model update's quiet tag (`update_models`): its size and share,
    /// or once that it stopped; else nothing.
    fn update_line(&self) -> Option<String> {
        match self.updating {
            Some((bytes, share)) => Some(fmt(
                S::tp_updating,
                &[
                    &format!("{:.0}%", share * 100.0),
                    &format!("{} MB", model_download::mb(bytes)),
                ],
            )),
            None => self.update_paused.then(|| t!(tp_update_paused).to_string()),
        }
    }

    /// The pill's Snip button (its slot is always there, so the pill never
    /// changes): live once something translated is on screen (`seen`: what
    /// Snip would take), dimmed before.
    fn snip_button(&self) -> Element<'_, Msg> {
        let on = self.seen().map(|_| Msg::Snip);
        let button = icon_button(
            crate::features::snip::action::ACTION.icon,
            State::default(),
            on,
        );
        let why = match self.snip_keys.is_empty() {
            true => Why::Icon,
            false => Why::Keys(&self.snip_keys),
        };
        hint(
            button,
            tr(crate::features::snip::action::ACTION.label),
            why,
            iced::widget::tooltip::Position::Bottom,
        )
    }

    /// The pill's buttons after the chips: Snip, then the glossary.
    fn tail(&self) -> Element<'_, Msg> {
        dir::row([self.snip_button(), self.glossary.button()])
            .spacing(picker::SPACING)
            .into()
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
        let rtl = dir::rtl();
        let at = pill_at(region);
        let mut layer =
            stack![pin(opaque(self.picker.pill(Msg::Picker, self.tail()))).position(at)];
        if status.is_some() || then.is_some() {
            let line = dir::row([status.map(|s| note(s).into()), then].into_iter().flatten());
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
        if let (Some(list), Some(offset)) = (self.glossary.list(), self.glossary.at()) {
            layer = layer.push(pin(opaque(list)).position(at + offset));
        }
        layer.into()
    }
}

/// Where the pill's top-left corner is in `region`: centred, `PILL_TOP` down.
pub fn pill_at(region: Rectangle) -> Point {
    let x = region.x + ((region.width - picker::PILL.width) / 2.0).round();
    Point::new(x, region.y + PILL_TOP)
}

/// The pill's rect on a whole `screen` (where it always sits).
pub fn pill_rect(screen: Size) -> Rectangle {
    Rectangle::new(pill_at(Rectangle::with_size(screen)), picker::PILL)
}

/// `at` (a result card) kept off the pill: where it would reach the pill's
/// rect (a side card on a screen too narrow to pass it), its top moves under
/// the pill, its bottom staying; never shorter than `card::MIN`.
pub fn clear_of_pill(at: Rectangle, screen: Size) -> Rectangle {
    let pill = pill_rect(screen);
    let below = pill.y + pill.height + STACK;
    if at.x >= pill.x + pill.width || at.x + at.width <= pill.x || at.y >= below {
        return at;
    }
    let bottom = at.y + at.height;
    let top = below.min(bottom - crate::shared::kit::panel_frame::MIN.height.min(at.height));
    Rectangle::new(Point::new(at.x, top), Size::new(at.width, bottom - top))
}

/// The Add to glossary form in the word card: the term, Keep as is | Always
/// as [field], and In [glossary] (a name field when there is none yet).
fn add_form(form: &AddForm) -> Element<'_, Msg> {
    use crate::shared::kit::dropdown::dropdown;
    use crate::shared::kit::field::input;
    let term = input(
        t!(gl_add_card),
        &form.term,
        false,
        form.term.trim().is_empty(),
    )
    .on_input(|s| Msg::Add(AddMsg::Term(s)))
    .on_submit(Msg::Add(AddMsg::Save))
    .width(Fill);
    let how = dir::row([
        regular(t!(gl_keep), form.keep, Some(Msg::Add(AddMsg::Keep(true)))),
        regular(
            t!(gl_always),
            !form.keep,
            Some(Msg::Add(AddMsg::Keep(false))),
        ),
        input("", &form.always, false, false)
            .on_input(|s| Msg::Add(AddMsg::Always(s)))
            .on_submit(Msg::Add(AddMsg::Save))
            .width(Fill)
            .into(),
    ])
    .spacing(TAG_SPACING)
    .align_y(Center);
    let into: Element<'_, Msg> = match form.picks.is_empty() {
        true => input(t!(gl_name), &form.name, false, false)
            .on_input(|s| Msg::Add(AddMsg::Name(s)))
            .width(Fill)
            .into(),
        false => {
            let chosen = form.picks.iter().find(|p| p.0 == form.into).cloned();
            dropdown(&form.picks, chosen, "", |p| Msg::Add(AddMsg::Into(p)))
        }
    };
    let line = dir::row([muted(Role::NOTE, t!(gl_in_glossary)).into(), into])
        .spacing(TAG_SPACING)
        .align_y(Center);
    column![term, how, line].spacing(STACK).into()
}

/// The original text of the word card; the names a glossary could keep
/// (`glossary::names`) wear the kit's dotted line and a click opens the
/// Add to glossary form.
fn original(text: &str) -> Element<'_, Msg> {
    let words: Vec<&str> = text.split_whitespace().collect();
    // (the names are marked on a game screen only)
    let marked = match crate::features::translate::glossary::screen::is_game() {
        true => super::glossary::names(&words),
        false => vec![false; words.len()],
    };
    if !marked.contains(&true) {
        return note(text.to_string()).width(Fill).into();
    }
    let mut line = iced::widget::Row::new();
    for (i, w) in words.iter().enumerate() {
        let last = i + 1 == words.len();
        let tail = if last { "" } else { " " };
        if marked[i] {
            let name = crate::shared::kit::dotted::dotted(note(w.to_string()).color(c().text));
            let word = w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            line = line.push(mouse_area(name).on_press(Msg::AddFor(word)));
            line = line.push(note(tail));
        } else {
            line = line.push(note(format!("{w}{tail}")));
        }
    }
    line.wrap().into()
}

/// The word card (the design's "Word explanation", titled "translation ·
/// original"): its body (the translation, the AI's explanation) and actions
/// (Explain = E, Ask AI = A).
fn card_view(card: &Card, width: f32) -> (Element<'_, Msg>, Element<'_, Msg>) {
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
        original(&card.block.original),
        side(label(Role::CHAT, card.block.translation.clone())),
    ]
    .spacing(STACK);
    if let Some(turn) = card.chat.turns.last() {
        body = body.push(
            match turn.answer.is_empty() && card.chat.status == Status::Streaming {
                true => note(t!(tp_thinking)).width(Fill).into(),
                // Markdown, as the chat shows it (lines shape by their own direction)
                false => markdown::markdown(card.answer.items(), t!(c_copy)).map(Msg::Answer),
            },
        );
    }
    if let Status::Failed(e) = &card.chat.status {
        let on = |fix| match fix {
            Fix::Retry => Msg::Explain,
            Fix::Model | Fix::Key | Fix::Settings => Msg::OpenSettings,
        };
        let provider = provider_name(&crate::shared::config::current().ai);
        body = body.push(ai_error(
            e,
            &provider,
            width,
            card.details,
            Msg::Details,
            on,
        ));
    }
    if let Some(form) = &card.add {
        let buttons = actions(
            (t!(c_save), Msg::Add(AddMsg::Save)),
            [(t!(c_cancel), Msg::Add(AddMsg::Cancel))],
        );
        return (body.push(add_form(form)).into(), buttons);
    }
    let ask = &crate::features::ask_ai::ask::ACTION;
    let mut others = vec![(tr(ask.label), Msg::Ask)];
    // (Add to glossary: a game screen only)
    if crate::features::translate::glossary::screen::is_game() {
        others.push((t!(gl_add_card), Msg::AddStart));
    }
    let buttons = actions((t!(tp_explain), Msg::Explain), others);
    (body.into(), buttons)
}
