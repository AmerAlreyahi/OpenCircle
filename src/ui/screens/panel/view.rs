//! What the result cards show (the design's app-states cards: Recognised
//! text, Browser hand-off, Ask AI without a key), composed from kit parts
//! only, in the one result card frame (`kit::panel_frame::card`).
use std::sync::Arc;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, markdown as md, row, stack};
use iced::{Bottom, Center, Element, Fill, Rectangle, Right, Size};

use super::{Browse, LOG, Msg, PICKS, Panel, State, Tab, classifier, picked};
use crate::actions;
use crate::actions::ask::{Chat, PRESETS, Setup, Status, Turn};
use crate::config::{self, StaysOpen};
use crate::ui::keys;
use crate::ui::kit::bubble::bubble;
use crate::ui::kit::button::{Kind, button, cta_label, label};
use crate::ui::kit::chip::Tone;
use crate::ui::kit::dropdown::dropdown;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::markdown;
use crate::ui::kit::motion;
use crate::ui::kit::option_list;
use crate::ui::kit::panel_frame::{self as frame, Event, Header};
use crate::ui::kit::scroll::scroll;
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::spinner;
use crate::ui::kit::text::{error, sub};
use crate::ui::kit::text_area::text_area;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::kit::{chip, field, progress};
use crate::ui::widgets::{self, Why};

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
    frame::card(header, None, body, footer, at, screen, hover, on)
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
    let mut line = row![button(cta_label(main.0, None), Kind::Accent, Some(main.1))]
        .spacing(ACTIONS_GAP)
        .align_y(Center);
    for (name, msg) in others {
        line = line.push(button(label(name), Kind::Quiet, Some(msg)));
    }
    line.into()
}

/// A download's line, its bar and its share (`.as-progress-label` +
/// `.as-progress`): the Ask card's OCR download for its question.
fn downloading<'a, M: 'a>(what: &'a str, done: f32) -> Element<'a, M> {
    let label = row![
        container(note(what)).width(Fill),
        note(format!("{:.0} %", done * 100.0))
    ]
    .spacing(STACK);
    column![label, progress::bar(done)].into()
}

impl Panel {
    /// The panel as the floating result card at `at` on a `screen`;
    /// `hover`: the pointer is on its resize edge.
    pub fn view(&self, at: Rectangle, screen: Size, hover: bool) -> Element<'_, Msg> {
        let (glyph, title, (body, footer)) = match &self.state {
            State::Lens(status) => (
                actions::lens::ACTION.icon,
                actions::lens::ACTION.label,
                self.lens_view(status.as_ref()),
            ),
            State::Ask {
                setup,
                chat,
                browse,
                answers,
                include,
                ..
            } => (
                actions::ask::ACTION.icon,
                CHAT_TITLE,
                self.ask_view(setup.as_ref(), chat, browse, answers, *include),
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
            docked: chatting,
            logo: chatting.then(|| crate::ui::kit::logo::logo(16)),
            menu: chatting,
        };
        if chatting {
            // the header's end: a new chat and the pin (no close button: Esc
            // or a click outside closes an unpinned chat; a pinned one, its pin)
            let new = button(label("+ New"), Kind::Elevated, Some(Msg::NewChat));
            let new = widgets::hint(new, "New chat", Why::Icon, Position::Bottom);
            let state = icon_button::State {
                active: self.pinned,
                ..Default::default()
            };
            let pin = icon_button(Icon::Pin.glyph(), state, Some(Msg::Pin));
            let pin = match self.pinned {
                true => widgets::hint(pin, "Unpin and close", Why::Icon, Position::Bottom),
                false => widgets::hint(pin, "Keep open", Why::Icon, Position::Bottom),
            };
            let ends = row![new, pin].spacing(space::GAP).align_y(Center);
            let card = frame::card(
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
                Some(tab) => stack![card, frame::drawer(self.drawer_view(tab), at)]
                    .width(Fill)
                    .height(Fill)
                    .into(),
                None => card,
            };
        }
        card(header, body, footer, at, screen, hover, Msg::Card)
    }

    /// Body and footer.
    fn lens_view<'a>(&'a self, status: Option<&'a Result<(), String>>) -> Part<'a> {
        let only = note("Only this selection goes to Google Lens.");
        let body = match status {
            None => column![note("Opening Google Lens in your browser…"), only],
            Some(Ok(())) => column![note("Opened in your browser"), only],
            // nothing went (e.g. a blank selection): no note about what goes
            Some(Err(e)) => column![error(Role::NOTE, e)],
        }
        .spacing(STACK);
        (
            body.into(),
            Some(self.follow_up("Ask AI about it…", false, None)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn ask_view<'a>(
        &'a self,
        setup: Option<&'a Result<Arc<Setup>, String>>,
        chat: &'a Chat,
        browse: &'a Browse,
        answers: &'a [(usize, md::Content)],
        include: bool,
    ) -> Part<'a> {
        let Some(Ok(_)) = setup else {
            let line: Element<'a, Msg> = match setup {
                Some(Err(steps)) => scroll(note(steps.as_str())).height(Fill).into(),
                _ => note("Getting ready…").into(),
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
                let line = format!("Thought for {:.1} s", thought.as_secs_f32());
                answer = answer.push(chip::logo(line, crate::ui::kit::logo::logo(14)));
            }
            let busy = last && streaming;
            if let (false, Some((_, parsed))) = (turn.answer.is_empty(), answers.get(i)) {
                answer = answer.push(markdown::markdown(parsed.items()).map(Msg::Answer));
            } else if busy {
                answer = answer.push(match browse {
                    Browse::Downloading(done) => {
                        downloading("Downloading the text-recognition model…", *done)
                    }
                    _ if browser => note("Opening Google AI Mode…").into(),
                    _ => spinner::thinking(
                        crate::ui::kit::logo::logo(16),
                        self.spin.map_or(0.0, |(_, turn)| turn),
                        motion::reduced(),
                        "Thinking…",
                    ),
                });
            }
            if !turn.answer.is_empty() && !busy && !browser {
                let mut tools = row![quiet("Copy", Msg::CopyTurn(i, true))].spacing(ACTIONS_GAP);
                if last {
                    tools = tools.push(quiet("Regenerate", Msg::Regenerate));
                }
                answer = answer.push(tools);
            }
            log = log.push(answer);
        }
        // Questions about the picture itself: Lens takes the crop (browser provider).
        if browser && chat.status == Status::Done {
            let lens = button(
                label("Search the image with Lens"),
                Kind::Quiet,
                Some(Msg::Search),
            );
            log = log.push(lens);
        }
        match &chat.status {
            Status::Failed(e) if browser => log = log.push(error(Role::NOTE, e)),
            Status::Failed(e) => {
                let retry = quiet("Retry", Msg::Regenerate);
                log = log.push(column![error(Role::NOTE, e), retry].spacing(STACK));
            }
            Status::Cancelled => log = log.push(note("Stopped.")),
            _ => {}
        }
        if let Browse::Consent(question) = browse {
            // Waiting for the user's choice: the question is theirs, not yet a turn.
            log = log.push(bubble(question));
        } else if chat.turns.is_empty() {
            let prompts = PRESETS
                .iter()
                .enumerate()
                .map(|(i, p)| chip::action(p.label.to_string(), Msg::Preset(i)));
            log = log.push(
                column![
                    note("Circle anything on your screen or ask a question below."),
                    row(prompts).spacing(CHIPS_GAP).wrap()
                ]
                .spacing(STACK),
            );
        }
        log = match &self.note {
            Some(Ok(done)) => log.push(note(*done)),
            Some(Err(e)) => log.push(error(Role::NOTE, e)),
            None => log,
        };
        let input: Element<'a, Msg> = if matches!(browse, Browse::Consent(_)) {
            column![
                note(
                    "To add the text of your selection to the question, OpenCircle downloads \
                     the text-recognition model once (about 20 MB, from GitHub). \
                     Nothing else is sent."
                ),
                actions(
                    ("Download", Msg::Download),
                    [("Send without the text", Msg::SendWithoutText)]
                ),
            ]
            .spacing(STACK)
            .into()
        } else if browser {
            self.follow_up("Ask a follow-up…", streaming, None)
        } else {
            self.composer(include, streaming)
        };
        let privacy = match (browser, include) {
            (true, true) => {
                "When you send, your question and the text read from the selection go to \
                 Google AI Mode. The image is not uploaded unless you choose Lens."
            }
            (true, false) => "When you send, only your question goes to Google AI Mode.",
            (false, true) => "Only this selection is sent, and only to the provider you chose.",
            (false, false) => "Only your question is sent, to the provider you chose.",
        };
        let body = scroll(log).id(LOG).height(Fill);
        let footer = column![input, note(privacy)].spacing(STACK);
        (body.into(), Some(footer.into()))
    }

    /// The user's turn `i`: the selection's chip when it went along, the
    /// bubble, and Copy / Edit (`editable`: not while an answer streams).
    fn question<'a>(&'a self, i: usize, turn: &'a Turn, editable: bool) -> Element<'a, Msg> {
        let mut side = column![].spacing(STACK).align_x(Right).width(Fill);
        if turn.screen {
            side = side.push(chip::status(
                match turn.shot {
                    Some((w, h)) => format!("Selected screen region · {w}×{h} px"),
                    None => "Screenshot not kept".to_string(),
                },
                Tone::Busy,
            ));
        }
        side = side.push(bubble(&turn.question));
        let mut tools = row![quiet("Copy", Msg::CopyTurn(i, false))].spacing(ACTIONS_GAP);
        if editable {
            tools = tools.push(quiet("Edit", Msg::EditTurn(i)));
        }
        side.push(tools).into()
    }

    /// The chat's composer, on one row: the field (more lines with Shift /
    /// Ctrl+Enter), the Include screen switch, and Send (Stop while answering).
    fn composer(&self, include: bool, streaming: bool) -> Element<'_, Msg> {
        let switch = field::toggle(Icon::Screen, include, false, Msg::IncludeScreen);
        let switch = widgets::hint(
            switch,
            match include {
                true => "Include screen: on (the selection goes with your question)",
                false => "Include screen: off (only your question goes)",
            },
            Why::Icon,
            Position::Top,
        );
        self.follow_up("Message OpenCircle AI…", streaming, Some(switch))
    }

    /// The field, `extra` beside it (the composer's switch), and its send
    /// (or, while streaming, stop) button.
    fn follow_up<'a>(
        &'a self,
        placeholder: &'a str,
        streaming: bool,
        extra: Option<Element<'a, Msg>>,
    ) -> Element<'a, Msg> {
        let top = Position::Top;
        let button = match streaming {
            true => widgets::hint(
                icon_button(
                    Icon::Stop.glyph(),
                    icon_button::State::default(),
                    Some(Msg::Stop),
                ),
                "Stop",
                Why::Icon,
                top,
            ),
            false => widgets::hint(
                icon_button(
                    Icon::Send.glyph(),
                    icon_button::State::default(),
                    Some(Msg::Submit),
                ),
                "Send",
                Why::Keys("Enter"),
                top,
            ),
        };
        let input = text_area(
            super::INPUT,
            placeholder,
            &self.input,
            Msg::Edit,
            Msg::Submit,
        );
        row![container(keys::guard(input, Msg::Key)).width(Fill)]
            .push(extra)
            .push(button)
            .spacing(space::GAP)
            .align_y(Bottom)
            .into()
    }

    /// The drawer (the design's "Workspace"): back, its title and the tabs, then
    /// the tab.
    fn drawer_view(&self, tab: Tab) -> Element<'_, Msg> {
        let back = icon_button(
            Icon::Back.glyph(),
            icon_button::State::default(),
            Some(Msg::Drawer(None)),
        );
        let back = widgets::hint(back, "Back to the chat", Why::Icon, Position::Bottom);
        let tabs = segmented(
            &[(Tab::History, "History"), (Tab::Settings, "Settings")],
            tab,
            tab,
            1.0,
            |t| Msg::Drawer(Some(t)),
        );
        let top = row![
            back,
            container(crate::ui::kit::text::text(Role::CARD_HEADING, "Workspace")).width(Fill),
            tabs
        ]
        .spacing(space::GAP)
        .align_y(Center);
        let body = match tab {
            Tab::History => self.history_view(),
            Tab::Settings => self.settings_view(),
        };
        column![top, scroll(body).height(Fill)]
            .spacing(STACK)
            .into()
    }

    /// This PC's saved chats, newest first: open one, delete one, or all.
    fn history_view(&self) -> Element<'_, Msg> {
        let new = button(label("+ New chat"), Kind::Elevated, Some(Msg::NewChat));
        let mut list = column![new].spacing(STACK).width(Fill);
        if !config::current().chat.keep_history {
            return list
                .push(note(
                    "Chat history is off (Settings > AI). Nothing is saved.",
                ))
                .into();
        }
        if self.listed.is_empty() {
            return list.push(note("No saved chats yet.")).into();
        }
        let open = match &self.state {
            State::Ask { chat, .. } => chat.id.clone(),
            State::Lens(_) => None,
        };
        let mut rows = column![].width(Fill);
        for entry in &self.listed {
            let current = open.as_deref() == Some(entry.id.as_str());
            let item = option_list::item(
                &entry.title,
                current,
                Some(crate::log::date(entry.updated)),
                false,
                Msg::OpenSaved(entry.id.clone()),
            );
            let delete = icon_button(
                Icon::Close.glyph(),
                icon_button::State::default(),
                Some(Msg::DeleteSaved(entry.id.clone())),
            );
            let delete = widgets::hint(delete, "Delete this chat", Why::Icon, Position::Left);
            rows = rows.push(row![container(item).width(Fill), delete].align_y(Center));
        }
        list = list.push(rows).push(button(
            label("Clear all history"),
            Kind::Quiet,
            Some(Msg::ClearHistory),
        ));
        list.push(note(
            "Saved on this PC only, text only: screenshots are never kept.",
        ))
        .into()
    }

    /// "AI chat stays open", the quick provider / model picker, and the way to
    /// the full AI settings.
    fn settings_view(&self) -> Element<'_, Msg> {
        let cfg = config::current();
        let stays = segmented(
            &[
                (StaysOpen::FollowUp, "After a follow-up"),
                (StaysOpen::Always, "Always"),
            ],
            cfg.chat.stays_open,
            cfg.chat.stays_open,
            1.0,
            Msg::StaysOpen,
        );
        let provider = dropdown(&PICKS[..], picked(&cfg.ai), "Choose a provider", Msg::Pick);
        let mut part = column![
            sub(Role::ROW_TITLE, "AI chat stays open"),
            stays,
            sub(Role::ROW_TITLE, "Provider"),
            provider,
        ]
        .spacing(STACK)
        .width(Fill);
        if cfg.ai.provider != config::Provider::Browser {
            let model = field::input("Model id", &self.model, true, false)
                .on_input(Msg::Model)
                .on_submit(Msg::SaveModel);
            part = part.push(sub(Role::ROW_TITLE, "Model")).push(model);
            if classifier(&self.model) {
                part = part.push(error(
                    Role::NOTE,
                    "This looks like a safety classifier: it rates messages instead of \
                     answering them. Pick a chat model.",
                ));
            }
        }
        part.push(button(
            label("Open AI settings →"),
            Kind::Elevated,
            Some(Msg::OpenAiSettings),
        ))
        .into()
    }
}
