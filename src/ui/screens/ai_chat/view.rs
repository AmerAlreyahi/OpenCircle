//! What the result cards show (the design's app-states cards: Recognised
//! text, Browser hand-off, Ask AI without a key), composed from kit parts
//! only, in the one result card frame (`kit::panel_frame::chat_frame`).
use std::sync::Arc;

use crate::app_language::{S, fmt, tr};
use crate::t;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, markdown as md, stack};
use iced::{Center, Element, Fill, Rectangle, Size};

use super::{AiChat, LOG, Msg, PICKS, State, Tab, classifier, picked};
use crate::actions;
use crate::actions::ask::{Chat, PRESETS, Setup, Status, Turn};
use crate::ai::providers::{self, Preset};
use crate::ai::{AiError, Fix};
use crate::config::{self, StaysOpen};
use crate::ui::keys;
use crate::ui::kit::bubble::bubble;
use crate::ui::kit::button::{Kind, button, cta_label, label};
use crate::ui::kit::chip::Tone;
use crate::ui::kit::dir;
use crate::ui::kit::dropdown::dropdown;
use crate::ui::kit::error_card::{ErrorCard, Words, error_card};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::markdown;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame::{self as frame, Event, Header};
use crate::ui::kit::scroll::scroll;
use crate::ui::kit::segmented::{self, segmented};
use crate::ui::kit::spinner;
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::text_area::text_area;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::tooltip::{Why, hint};
use crate::ui::kit::{chip, field};
use crate::ui::screens::model_list;

/// A card's body and its footer.
type Part<'a> = (Element<'a, Msg>, Option<Element<'a, Msg>>);

/// The AI chat's title (the design's header).
const CHAT_TITLE: &str = "OpenCircle AI";
/// Between a chat's turns (`space-y-4`).
const TURN_GAP: f32 = 16.0;
/// Between a card's blocks (`.as-note` margins, the textarea's top).
const STACK: f32 = 10.0;
/// Between the actions of a row (`.as-actions` gap 8; a call to action
/// and quiet text, as the update card).
const ACTIONS_GAP: f32 = 16.0;
/// Between chips (`gap-2`).
const CHIPS_GAP: f32 = 8.0;

/// The result card for `header` at `at` on a `screen`: the kit's frame, with
/// no close button (Esc, or a click outside it). `hover`: the pointer is on
/// its resize edge.
#[allow(clippy::too_many_arguments)]
pub fn card<'a, M: Clone + 'a>(
    header: Header<'a>,
    body: impl Into<Element<'a, M>>,
    footer: Option<Element<'a, M>>,
    at: Rectangle,
    screen: Size,
    hover: bool,
    on: impl Fn(Event) -> M,
) -> Element<'a, M> {
    frame::chat_frame(header, None, body, footer, at, screen, hover, on)
}

/// A small quiet text button (a message's Copy, Edit, Regenerate).
fn quiet<'a>(name: &'a str, msg: Msg) -> Element<'a, Msg> {
    button(label(name), Kind::Quiet, Some(msg))
}

/// A card's note (`.as-note`): 12 px secondary text.
pub fn note<'a>(line: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    sub(Role::NOTE, line).line_height(1.65)
}

/// The call to action and the other choices of a card (`.as-actions`): the
/// first fills the row in the accent, the rest are quiet text.
pub fn actions<'a, M: Clone + 'a>(
    main: (&'a str, M),
    others: impl IntoIterator<Item = (&'a str, M)>,
) -> Element<'a, M> {
    let mut line = dir::row([button(cta_label(main.0, None), Kind::Accent, Some(main.1))])
        .spacing(ACTIONS_GAP)
        .align_y(Center);
    for (name, msg) in others {
        line = line.push(button(label(name), Kind::Quiet, Some(msg)));
    }
    line.into()
}

impl AiChat {
    /// The panel as the floating result card at `at` on a `screen`;
    /// `hover`: the pointer is on its resize edge.
    pub fn view(&self, at: Rectangle, screen: Size, hover: bool) -> Element<'_, Msg> {
        let (glyph, title, (body, footer)) = match &self.state {
            State::Ask {
                setup,
                chat,
                answers,
                include,
                ..
            } => (
                actions::ask::ACTION.icon,
                CHAT_TITLE,
                self.ask_view(setup.as_ref(), chat, answers, *include, at.width),
            ),
        };
        let chip = match &self.state {
            State::Ask {
                setup: Some(Ok(setup)),
                ..
            } if setup.browser() => Some("Google AI Mode"),
            State::Ask {
                setup: Some(Ok(setup)),
                ..
            } => Some(setup.cfg.model.as_str()),
            _ => None,
        };
        let chatting = matches!(self.state, State::Ask { .. });
        let header = Header {
            glyph,
            title,
            chip,
            logo: chatting.then(|| crate::ui::kit::logo::logo(16)),
            menu: chatting,
            held: self.held,
        };
        if chatting {
            // the header's end: a new chat and the pin (no close button: Esc
            // or a click outside closes an unpinned chat; a pinned one, its pin)
            let new = button(label(t!(chat_new)), Kind::Elevated, Some(Msg::NewChat));
            let new = hint(new, t!(chat_new_tip), Why::Icon, Position::Bottom);
            let state = icon_button::State {
                active: self.pinned,
                ..Default::default()
            };
            // pinned: "pin-off" on the filled (active) button, so it reads at a glance
            let (glyph, tip) = match self.pinned {
                true => (Icon::Unpin, t!(chat_unpin)),
                false => (Icon::Pin, t!(chat_pin)),
            };
            let pin = icon_button(glyph.glyph(), state, Some(Msg::Pin));
            let pin = hint(pin, tip, Why::Icon, Position::Bottom);
            let ends = dir::row([new, pin]).spacing(space::GAP).align_y(Center);
            let card = frame::chat_frame(
                header,
                Some(ends.into()),
                body,
                footer,
                at,
                screen,
                hover,
                Msg::Card,
            );
            return match self.drawer {
                Some(tab) => {
                    let inside = self.drawer_view(tab, frame::drawer_room(at));
                    stack![card, frame::drawer(inside, at)]
                        .width(Fill)
                        .height(Fill)
                        .into()
                }
                None => card,
            };
        }
        card(header, body, footer, at, screen, hover, Msg::Card)
    }

    #[allow(clippy::too_many_arguments)]
    fn ask_view<'a>(
        &'a self,
        setup: Option<&'a Result<Arc<Setup>, String>>,
        chat: &'a Chat,
        answers: &'a [(usize, md::Content)],
        include: bool,
        width: f32,
    ) -> Part<'a> {
        let Some(Ok(ready)) = setup else {
            let line: Element<'a, Msg> = match setup {
                Some(Err(steps)) => scroll(note(steps.as_str())).height(Fill).into(),
                _ => note(t!(chat_ready)).into(),
            };
            return (line, None);
        };

        let browser = matches!(setup, Some(Ok(s)) if s.browser());
        let streaming = chat.status == Status::Streaming;
        let mut log = column![].spacing(TURN_GAP).width(Fill);
        for (i, turn) in chat.turns.iter().enumerate() {
            let last = i + 1 == chat.turns.len();
            log = log.push(self.question(i, turn, !streaming && !browser));
            let mut answer = column![].spacing(STACK).width(Fill);
            if let Some(thought) = turn.thought {
                let line = fmt(S::chat_thought, &[&format!("{:.1}", thought.as_secs_f32())]);
                answer = answer.push(chip::logo(line, crate::ui::kit::logo::logo(14)));
            }
            let busy = last && streaming;
            if let (false, Some((_, parsed))) = (turn.answer.is_empty(), answers.get(i)) {
                answer =
                    answer.push(markdown::markdown(parsed.items(), t!(c_copy)).map(Msg::Answer));
            } else if busy {
                answer = answer.push(match browser {
                    true => note(t!(chat_opening)).into(),
                    false => spinner::thinking(
                        crate::ui::kit::logo::logo(16),
                        self.spin.map_or(0.0, |(_, turn)| turn),
                        motion::reduced(),
                        t!(tp_thinking),
                    ),
                });
            }
            if !turn.answer.is_empty() && !busy && !browser {
                let mut tools =
                    dir::row([quiet(t!(c_copy), Msg::CopyTurn(i, true))]).spacing(ACTIONS_GAP);
                if last {
                    tools = tools.push(quiet(t!(chat_regenerate), Msg::Regenerate));
                }
                answer = answer.push(tools);
            }
            // nothing answered (a failed turn): no empty block, so no second gap
            if turn.thought.is_some() || !turn.answer.is_empty() || busy {
                log = log.push(answer);
            }
        }
        // Questions about the picture itself: Lens takes the crop (browser provider).
        if browser && chat.status == Status::Done {
            let lens = button(label(t!(chat_lens)), Kind::Quiet, Some(Msg::Search));
            log = log.push(lens);
        }
        match &chat.status {
            Status::Failed(e) => {
                let width = width - 2.0 * frame::PADDING;
                let provider = provider_name(&ready.cfg);
                log = log.push(ai_error(
                    e,
                    &provider,
                    width,
                    self.details,
                    Msg::Details,
                    fix,
                ));
            }
            Status::Cancelled => log = log.push(note(t!(chat_stopped))),
            _ => {}
        }
        if chat.turns.is_empty() {
            let prompts = PRESETS
                .iter()
                .enumerate()
                .map(|(i, p)| chip::action(tr(p.label).to_string(), Msg::Preset(i)));
            log = log.push(
                column![
                    note(t!(chat_intro)),
                    iced::widget::row(prompts).spacing(CHIPS_GAP).wrap()
                ]
                .spacing(STACK),
            );
        }
        log = match &self.note {
            Some(Ok(done)) => log.push(note(tr(done.0))),
            Some(Err(e)) => log.push(error(Role::NOTE, tr(e.0))),
            None => log,
        };
        let placeholder = placeholder(chat);
        let input: Element<'a, Msg> = match browser {
            true => self.follow_up(placeholder, streaming, None, None),
            false => {
                let attached = include && chat.takes(&ready.png);
                self.composer(placeholder, include, attached, streaming)
            }
        };
        let body = crate::ui::kit::scroll::quiet(log).id(LOG).height(Fill);
        (body.into(), Some(input))
    }

    /// The user's turn `i`: the selection's chip when it went along, the
    /// bubble, and Copy / Edit (`editable`: not while an answer streams).
    fn question<'a>(&'a self, i: usize, turn: &'a Turn, editable: bool) -> Element<'a, Msg> {
        let mut side = column![].spacing(STACK).align_x(dir::end()).width(Fill);
        if turn.screen {
            side = side.push(chip::status(
                match turn.shot {
                    Some((w, h)) => fmt(S::chat_region, &[&format!("{w}×{h} px")]),
                    None => t!(chat_no_shot).to_string(),
                },
                Tone::Busy,
            ));
        }
        side = side.push(bubble(&turn.question));
        let mut tools = dir::row([quiet(t!(c_copy), Msg::CopyTurn(i, false))]).spacing(ACTIONS_GAP);
        if editable {
            tools = tools.push(quiet(t!(c_edit), Msg::EditTurn(i)));
        }
        side.push(tools).into()
    }

    /// The chat's composer (the design's, solid): the attached screen's chip
    /// while a picture goes with the next message (`attached`), the field
    /// (more lines with Shift / Ctrl+Enter), then Include screen and Send
    /// (Stop while answering). Web search's Auto / On / Off joins it with AI online.
    fn composer<'a>(
        &'a self,
        placeholder: &'a str,
        include: bool,
        attached: bool,
        streaming: bool,
    ) -> Element<'a, Msg> {
        let size = format!("{}×{} px", self.crop.width, self.crop.height);
        let top = attached.then(|| chip::attached("UI", t!(chat_attached), size));
        let switch = crate::ui::kit::button::switch(t!(chat_include), include, Msg::IncludeScreen);
        self.follow_up(placeholder, streaming, top, Some(switch))
    }

    /// The composer box: `top` over the field, `extra` at the start of its
    /// controls row (the Include screen switch) and its send (or, while
    /// streaming, stop) button at the end.
    fn follow_up<'a>(
        &'a self,
        placeholder: &'a str,
        streaming: bool,
        top: Option<Element<'a, Msg>>,
        extra: Option<Element<'a, Msg>>,
    ) -> Element<'a, Msg> {
        use crate::ui::kit::button::{COMPACT_H, compact};
        let round =
            |icon: Icon, msg| compact(icon.glyph(), None, COMPACT_H, (false, false), Some(msg));
        let send = match streaming {
            true => hint(
                round(Icon::Stop, Msg::Stop),
                t!(chat_stop),
                Why::Icon,
                Position::Top,
            ),
            false => hint(
                round(Icon::Send, Msg::Submit),
                t!(chat_send),
                Why::Keys("Enter"),
                Position::Top,
            ),
        };
        let controls = dir::row([])
            .push(extra)
            .push(iced::widget::space::horizontal())
            .push(send)
            .spacing(space::GAP)
            .align_y(Center);
        let composer = text_area(
            super::INPUT,
            placeholder,
            &self.input,
            Msg::Edit,
            Msg::Submit,
            top,
            controls.into(),
            Msg::FocusInput,
        );
        keys::guard(composer, Msg::Key)
    }

    /// The drawer (the design's "Workspace"), `width` inside: back, its title
    /// and the tabs, then the tab.
    fn drawer_view(&self, tab: Tab, width: f32) -> Element<'_, Msg> {
        let back = icon_button(
            Icon::Back.glyph(),
            icon_button::State::default(),
            Some(Msg::Drawer(None)),
        );
        let back = hint(back, t!(chat_back), Why::Icon, Position::Bottom);
        let tabs = segmented::inline(
            &[
                (Tab::History, t!(nav_history)),
                (Tab::Settings, t!(chat_tab_settings)),
            ],
            tab,
            tab,
            1.0,
            |t| Msg::Drawer(Some(t)),
        );
        let top = crate::dir_row![
            back,
            container(crate::ui::kit::text::text(
                Role::CARD_HEADING,
                t!(chat_workspace)
            ))
            .width(Fill)
            .align_x(dir::start()),
            tabs
        ]
        .spacing(space::GAP)
        .align_y(Center);
        let parts = match tab {
            Tab::History => self.history_view(width),
            Tab::Settings => vec![scroll(self.settings_view()).height(Fill).into()],
        };
        column![top].extend(parts).spacing(STACK).into()
    }

    /// This PC's saved chats, favourites then newest first: the search field
    /// and a new chat, a card per chat (star, open, delete), and the footer.
    fn history_view(&self, width: f32) -> Vec<Element<'_, Msg>> {
        let search = field::input(t!(chat_search), &self.find, false, false)
            .align_x(dir::start())
            .on_input(Msg::Find)
            .width(Fill);
        let new = button(label(t!(chat_new)), Kind::Elevated, Some(Msg::NewChat));
        let top = crate::dir_row![search, new]
            .spacing(space::GAP)
            .align_y(Center);
        if !config::current().chat.keep_history {
            let off = note(t!(chat_hist_off));
            return vec![top.into(), off.into()];
        }
        let open = match &self.state {
            State::Ask { chat, .. } => chat.id.clone(),
        };
        let now = crate::store::chats::now();
        let mut cards = column![].spacing(space::GAP).width(Fill);
        for entry in &self.listed {
            cards = cards.push(crate::ui::kit::card::chat(crate::ui::kit::card::Chat {
                title: &entry.title,
                when: ago(entry.updated, now),
                crop: entry.crop.map(|(w, h)| format!("{w}×{h} px")),
                starred: entry.star,
                selected: open.as_deref() == Some(entry.id.as_str()),
                width,
                open: Msg::OpenSaved(entry.id.clone()),
                star: Msg::Star(entry.id.clone(), !entry.star),
                delete: Msg::DeleteSaved(entry.id.clone()),
                delete_label: t!(chat_del),
            }));
        }
        let list: Element<'_, Msg> = match self.listed.is_empty() {
            true if self.find.trim().is_empty() => note(t!(chat_none)).into(),
            true => container(note(t!(chat_no_match))).center_x(Fill).into(),
            false => scroll(cards).height(Fill).into(),
        };
        let clear = button(label(t!(chat_clear)), Kind::Remove, Some(Msg::ClearHistory));
        let footer = crate::dir_row![
            container(crate::ui::kit::text::muted(Role::CARD_DESC, t!(chat_local)))
                .width(Fill)
                .align_x(dir::start()),
            clear
        ]
        .align_y(Center);
        let list = container(list).height(Fill);
        vec![top.into(), list.into(), footer.into()]
    }

    /// "AI chat stays open", the quick provider / model picker, and the way to
    /// the full AI settings.
    fn settings_view(&self) -> Element<'_, Msg> {
        let cfg = config::current();
        let stays = segmented(
            &[
                (StaysOpen::FollowUp, t!(ai_stays_follow)),
                (StaysOpen::Always, t!(ai_stays_always)),
            ],
            cfg.chat.stays_open,
            cfg.chat.stays_open,
            1.0,
            Msg::StaysOpen,
        );
        let provider = dropdown(&PICKS[..], picked(&cfg.ai), t!(ai_prov_choose), Msg::Pick);
        let mut part = column![
            sub(Role::LABEL, t!(ai_stays)),
            stays,
            sub(Role::LABEL, t!(ai_prov)),
            provider,
        ]
        .spacing(STACK)
        .width(Fill)
        .align_x(dir::start());
        if cfg.ai.provider != config::Provider::Browser {
            let on = model_list::On {
                opened: Msg::ModelsOpened,
                query: Msg::ModelQuery,
                first: Msg::ModelFirst,
                pick: Msg::ModelPick,
                load: Msg::LoadModels,
            };
            let example = Preset::of(&cfg.ai)
                .filter(|p| !p.model.is_empty())
                .map_or(t!(ai_model_type).to_string(), |p| {
                    fmt(S::ai_model_eg, &[&p.model])
                });
            let model = self.models.view(&cfg.ai.model, &example, on);
            part = part.push(sub(Role::LABEL, t!(ai_model))).push(model);
            if classifier(&cfg.ai.model) {
                part = part.push(error(Role::NOTE, t!(chat_classifier)));
            }
        }
        part.push(crate::ui::kit::button::fill(
            label(t!(chat_open_ai)),
            Some(Msg::OpenAiSettings),
        ))
        .into()
    }
}

/// Where an error card's fix goes: the drawer's model picker, Settings > Ask
/// AI (the key, the setup), or the question again.
pub(super) fn fix(fix: Fix) -> Msg {
    match fix {
        Fix::Model => Msg::ChooseModel,
        Fix::Key | Fix::Settings => Msg::OpenAiSettings,
        Fix::Retry => Msg::Regenerate,
    }
}

/// The provider's name as Settings shows it ("" when the file names none).
pub fn provider_name(cfg: &config::AiConfig) -> String {
    match providers::Preset::of(cfg) {
        Some(p) if !p.custom() => p.to_string(),
        _ => String::new(),
    }
}

/// An AI error as the kit's calm card (`kit::error_card`): its headline,
/// the provider's own message and the kind's fix (`on`), with Retry beside
/// a fix that is not Retry itself. `width`: the card's.
pub fn ai_error<'a, M: Clone + 'a>(
    e: &'a AiError,
    provider: &str,
    width: f32,
    open: bool,
    on_details: M,
    on: impl Fn(Fix) -> M,
) -> Element<'a, M> {
    let fix = e.fix();
    let retry = matches!(fix, Some(f) if f != Fix::Retry);
    error_card(ErrorCard {
        headline: e.headline(provider),
        detail: e.detail(),
        width,
        open,
        on_details,
        fix: fix.map(|f| (f.label(), on(f))),
        retry: retry.then(|| on(Fix::Retry)),
        words: Words {
            show: t!(err_details),
            hide: t!(err_hide),
            retry: t!(fix_retry),
        },
    })
}

/// The composer's placeholder: a first message, then a follow-up.
pub(super) fn placeholder(chat: &Chat) -> &'static str {
    match chat.turns.is_empty() {
        true => t!(chat_ph_first),
        false => t!(chat_ph_follow),
    }
}

/// When a saved chat last changed, as History says it (`now`: unix seconds):
/// "Just now", "18m ago", "3h ago", "Yesterday", then its date.
pub(super) fn ago(then: u64, now: u64) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    match now.saturating_sub(then) {
        s if s < MIN => t!(chat_just).into(),
        s if s < HOUR => fmt(S::chat_min_ago, &[&(s / MIN)]),
        s if s < DAY => fmt(S::chat_hour_ago, &[&(s / HOUR)]),
        s if s < 2 * DAY => t!(chat_yesterday).into(),
        _ => crate::log::date(then),
    }
}
