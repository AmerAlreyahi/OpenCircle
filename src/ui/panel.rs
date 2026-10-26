//! Result panels (DESIGN.md Flow step 5): Search (Lens), Text and Ask AI. One
//! floating card (`widgets::card`: on the side away from the selection, a card
//! at the bottom on narrow screens; dragged and resized by its frame) over the
//! frozen frame, so the selection stays visible. Slow work runs on its own thread
//! (`super::background`); its results arrive as `Msg`s. Dropping a panel stops
//! its running work.
use std::sync::Arc;

use iced::widget::tooltip::Position;
use iced::widget::{column, container, image, operation, row, text_editor};
use iced::{Center, ContentFit, Element, Fill, Rectangle, Size, Task, task};

use super::background;
use super::keys;
use super::widgets::{self, SEND, STOP};
use crate::actions::ask::{self, Chat, PRESETS, Setup, Status};
use crate::actions::entities::{self, Entity, Kind};
use crate::actions::text::{self, Progress};
use crate::actions::{self, Crop, lens};
use crate::ai;

/// A card's body and its footer.
type Part<'a> = (Element<'a, Msg>, Option<Element<'a, Msg>>);

/// The crop's thumbnail in the Ask AI panel.
const THUMB_HEIGHT: f32 = 112.0;
/// The follow-up input. Never focused by itself: until it is clicked, letters
/// are the overlay's action shortcuts.
pub(super) const INPUT: &str = "panel-input";
/// The Ask AI conversation (scrolled to the end as the answer grows).
const LOG: &str = "panel-log";

#[derive(Debug, Clone)]
pub enum Msg {
    /// The card's grip or corner was pressed (the App moves / resizes it).
    Card(widgets::card::Event),
    /// The Lens page is written and the browser launched (the App holds the page).
    LensOpened(Result<lens::Held, String>),
    Input(String),
    /// Enter / send in the follow-up input.
    Submit,
    /// A suggestion chip (index into `PRESETS`).
    Preset(usize),
    Stop,
    Ready(Result<Arc<Setup>, String>),
    Delta(String),
    Answered(Result<(), String>),
    /// The user agreed to the model download.
    Download,
    /// Browser provider: send the question without the selection's text.
    SendWithoutText,
    Cancel,
    Progress(Progress),
    Read(Result<String, String>),
    Edit(text_editor::Action),
    CopyText,
    /// Search the crop with Lens (from the Text panel).
    Search,
    /// Translate the read text (Text panel), shown under it.
    Translate,
    Translated(Result<String, String>),
    /// A link, email or phone chip under the text (index into `entities`).
    Chip(usize),
    /// What that chip's click did (its note).
    Acted(Result<&'static str, String>),
    /// A focused field gave a key back (`keys`); the App handles it.
    Key(keys::Keyed),
}

pub struct Panel {
    crop: Arc<Crop>,
    state: State,
    /// The follow-up input's text.
    input: String,
    /// The running stream, OCR or translation; dropping the handle aborts it.
    work: Option<task::Handle>,
    /// The Text panel's translation of the read text, once asked for.
    translation: Option<Result<String, String>>,
    /// Links, emails and phone numbers in the Text panel's text; a click acts on one.
    entities: Vec<Entity>,
    /// What the last chip click did (or why it could not).
    note: Option<Result<&'static str, String>>,
}

enum State {
    /// `None` while the page is being written.
    Lens(Option<Result<(), String>>),
    Text(Reading),
    Ask {
        thumb: image::Handle,
        /// `None` while loading the config and key.
        setup: Option<Result<Arc<Setup>, String>>,
        chat: Chat,
        browse: Browse,
    },
}

/// Browser provider (Google AI Mode): adding the selection's text to the
/// question needs the OCR model, so it asks before the first download.
#[derive(Default)]
enum Browse {
    #[default]
    Idle,
    /// No models yet: this question waits for the user's choice.
    Consent(String),
    Downloading(f32),
}

enum Reading {
    /// No models yet: ask before downloading.
    Consent,
    Downloading(f32),
    Busy,
    Done(text_editor::Content),
    Failed(String),
}

impl Panel {
    /// Opens `kind` for `crop`. `question` (Ask AI only) is sent once ready:
    /// the user already pressed Enter on it elsewhere (Lens follow-up).
    /// Search (Lens) hands the crop to the user's browser at once: the user
    /// picked it.
    pub fn open(kind: actions::Panel, crop: Crop, question: Option<String>) -> (Panel, Task<Msg>) {
        let crop = Arc::new(crop);
        let mut panel = Panel {
            crop: crop.clone(),
            state: State::Lens(None),
            input: question.unwrap_or_default(),
            work: None,
            translation: None,
            entities: Vec::new(),
            note: None,
        };
        let task = match kind {
            actions::Panel::Lens => background(move |tx| {
                let _ = tx.unbounded_send(Msg::LensOpened(lens::open(&crop)));
            }),
            actions::Panel::Text if text::models_present() => {
                panel.state = State::Text(Reading::Busy);
                panel.read()
            }
            actions::Panel::Text => {
                panel.state = State::Text(Reading::Consent);
                Task::none()
            }
            actions::Panel::Ask => {
                panel.state = State::Ask {
                    thumb: image::Handle::from_rgba(crop.width, crop.height, crop.rgba.clone()),
                    setup: None,
                    chat: Chat::default(),
                    browse: Browse::Idle,
                };
                // No key (Google AI Mode): nothing to load (no keyring, no PNG), so
                // the question goes at once, never "Getting ready…" before it.
                if crate::config::current().ai.provider == crate::config::Provider::Browser {
                    let ready = ask::setup(&crop).map(Arc::new);
                    let task = panel.update(Msg::Ready(ready));
                    return (panel, task);
                }
                background(move |tx| {
                    let _ = tx.unbounded_send(Msg::Ready(ask::setup(&crop).map(Arc::new)));
                })
            }
        };
        (panel, task)
    }

    /// Ask AI, continuing `chat` (e.g. the translate word card's explanation).
    pub fn open_chat(crop: Crop, mut chat: Chat) -> (Panel, Task<Msg>) {
        chat.cancel(); // a stream still running belonged to the card; it stopped
        let (mut panel, task) = Panel::open(actions::Panel::Ask, crop, None);
        if let State::Ask { chat: open, .. } = &mut panel.state {
            *open = chat;
        }
        (panel, task)
    }

    pub fn kind(&self) -> actions::Panel {
        match self.state {
            State::Lens(_) => actions::Panel::Lens,
            State::Text(_) => actions::Panel::Text,
            State::Ask { .. } => actions::Panel::Ask,
        }
    }

    /// Empties the follow-up input and returns what was in it.
    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    /// The Text panel's (edited) text.
    pub fn text(&self) -> Option<String> {
        match &self.state {
            State::Text(Reading::Done(content)) => Some(content.text()),
            _ => None,
        }
    }

    /// Messages for the panel itself (the App handles Cancel, CopyText, Search
    /// and the Lens follow-up first).
    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match (&mut self.state, msg) {
            (State::Lens(status), Msg::LensOpened(result)) => *status = Some(result.map(drop)),
            (_, Msg::Input(value)) => self.input = value,
            // Not ready or still answering: the question stays in the input.
            (
                State::Ask {
                    chat,
                    setup: Some(Ok(_)),
                    browse: Browse::Idle,
                    ..
                },
                Msg::Submit,
            ) if chat.status != Status::Streaming => {
                let question = self.take_input();
                return self.send(&question);
            }
            (State::Ask { .. }, Msg::Preset(i)) => {
                if let Some(preset) = PRESETS.get(i) {
                    return self.send(&preset.text(&crate::config::current().translate.target));
                }
            }
            (
                State::Ask {
                    browse: Browse::Consent(_),
                    ..
                },
                Msg::Download,
            ) => return self.resume(true),
            (
                State::Ask {
                    browse: Browse::Consent(_),
                    ..
                },
                Msg::SendWithoutText,
            ) => return self.resume(false),
            (
                State::Ask {
                    browse: browse @ Browse::Downloading(_),
                    ..
                },
                Msg::Progress(Progress::Download(done)),
            ) => *browse = Browse::Downloading(done),
            (State::Ask { browse, .. }, Msg::Progress(Progress::Reading)) => *browse = Browse::Idle,
            (State::Ask { chat, browse, .. }, Msg::Stop) => {
                chat.cancel();
                *browse = Browse::Idle;
                self.work = None;
            }
            (State::Ask { setup, .. }, Msg::Ready(result)) => {
                let ready = result.is_ok();
                *setup = Some(result);
                if ready && !self.input.trim().is_empty() {
                    let question = self.take_input();
                    return self.send(&question);
                }
            }
            (State::Ask { chat, .. }, Msg::Delta(piece)) => {
                chat.delta(&piece);
                // Follow the answer. (anchor_bottom() hid short content in iced 0.14.)
                return operation::snap_to_end(LOG);
            }
            (State::Ask { chat, browse, .. }, Msg::Answered(result)) => {
                chat.finish(result);
                *browse = Browse::Idle;
                self.work = None;
            }
            (State::Text(reading @ Reading::Consent), Msg::Download) => {
                *reading = Reading::Downloading(0.0);
                return self.read();
            }
            (State::Text(reading), Msg::Progress(progress)) => match (progress, &reading) {
                (Progress::Download(done), Reading::Downloading(_)) => {
                    *reading = Reading::Downloading(done)
                }
                (Progress::Reading, _) => *reading = Reading::Busy,
                _ => {}
            },
            (State::Text(reading), Msg::Read(result)) => {
                self.work = None;
                *reading = match result {
                    Ok(text) if text.trim().is_empty() => {
                        Reading::Failed("No text found in the selection.".into())
                    }
                    Ok(text) => {
                        self.entities = entities::find(&text);
                        Reading::Done(text_editor::Content::with_text(&text))
                    }
                    Err(e) => Reading::Failed(e),
                };
            }
            (State::Text(Reading::Done(content)), Msg::Edit(action)) => {
                let edits = action.is_edit();
                content.perform(action);
                if edits {
                    self.entities = entities::find(&content.text());
                }
            }
            // Only here, on a click: nothing is ever opened by itself.
            (State::Text(Reading::Done(_)), Msg::Chip(i)) => {
                // (on a thread: launching the browser or mail app takes a while)
                if let Some(entity) = self.entities.get(i).cloned() {
                    return background(move |tx| {
                        let _ = tx.unbounded_send(Msg::Acted(act_on(&entity)));
                    });
                }
            }
            (_, Msg::Acted(note)) => self.note = Some(note),
            (State::Text(Reading::Done(content)), Msg::Translate) if self.work.is_none() => {
                let text = content.text();
                self.translation = None;
                let (task, handle) = background(move |tx| {
                    let _ = tx.unbounded_send(Msg::Translated(actions::translate::text(&text)));
                })
                .abortable();
                self.work = Some(handle.abort_on_drop());
                return task;
            }
            (State::Text(_), Msg::Translated(result)) => {
                self.work = None;
                self.translation = Some(result);
            }
            _ => {}
        }
        Task::none()
    }

    /// Starts OCR (with the model download first if needed) on a thread.
    fn read(&mut self) -> Task<Msg> {
        let crop = self.crop.clone();
        let (task, handle) = background(move |tx| {
            let result = text::read(&crop, |progress| {
                let _ = tx.unbounded_send(Msg::Progress(progress));
            });
            let _ = tx.unbounded_send(Msg::Read(result));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }

    /// Asks `question`; the answer streams in on a thread. Nothing is sent before this.
    fn send(&mut self, question: &str) -> Task<Msg> {
        let State::Ask {
            setup: Some(Ok(setup)),
            chat,
            browse,
            ..
        } = &mut self.state
        else {
            return Task::none();
        };
        if setup.browser() {
            let question = question.trim();
            if question.is_empty() || chat.status == Status::Streaming {
                return Task::none();
            }
            if !text::models_present() {
                *browse = Browse::Consent(question.into());
                return Task::none();
            }
            return self.browse(question.into(), true);
        }
        let Some(messages) = chat.ask(question, &setup.png) else {
            return Task::none();
        };
        let setup = setup.clone();
        let (task, handle) = background(move |tx| {
            let result = ask::stream(&setup, &messages, |piece| {
                tx.unbounded_send(Msg::Delta(piece.into())).is_ok()
            });
            let _ = tx.unbounded_send(Msg::Answered(result));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }

    /// The user chose about the text model (Browser provider): go on with the waiting question.
    fn resume(&mut self, read_text: bool) -> Task<Msg> {
        let State::Ask { browse, .. } = &mut self.state else {
            return Task::none();
        };
        let Browse::Consent(question) = std::mem::take(browse) else {
            return Task::none();
        };
        if read_text {
            *browse = Browse::Downloading(0.0);
        }
        self.browse(question, read_text)
    }

    /// Browser provider: read the selection's text (`read_text`; the user agreed
    /// to the model), then open `prompt` + that text in Google AI Mode.
    fn browse(&mut self, prompt: String, read_text: bool) -> Task<Msg> {
        let State::Ask {
            setup: Some(Ok(_)),
            chat,
            ..
        } = &mut self.state
        else {
            return Task::none();
        };
        chat.ask(&prompt, &[]); // the turn (and "busy"); nothing is sent by this
        let crop = self.crop.clone();
        let (task, handle) = background(move |tx| {
            let read = read_text.then(|| {
                text::read(&crop, |progress| {
                    let _ = tx.unbounded_send(Msg::Progress(progress));
                })
            });
            let (question, note) = browser_turn(&prompt, read);
            // The user pressed Send: AI Mode opens in their own browser.
            let line = match ai::browser::ask(&question) {
                Ok(opened) => {
                    let url = ai::browser::url(&question).0;
                    crate::search_history::keep("ai", &prompt, &url, Some(&crop));
                    format!("{opened}{note}")
                }
                Err(e) => e,
            };
            let _ = tx.unbounded_send(Msg::Delta(line));
            let _ = tx.unbounded_send(Msg::Answered(Ok(())));
        })
        .abortable();
        self.work = Some(handle.abort_on_drop());
        task
    }

    /// The panel as a floating card at `at` on a `screen` (`widgets::card`);
    /// `hover`: the pointer is on its resize edge.
    pub fn view(&self, at: Rectangle, screen: Size, hover: bool) -> Element<'_, Msg> {
        let (action, (body, footer)) = match &self.state {
            State::Lens(status) => (&actions::lens::ACTION, self.lens_view(status.as_ref())),
            State::Text(reading) => (&actions::text::ACTION, (self.text_view(reading), None)),
            State::Ask {
                thumb,
                setup,
                chat,
                browse,
            } => (
                &actions::ask::ACTION,
                self.ask_view(thumb, setup.as_ref(), chat, browse),
            ),
        };
        let chip = match &self.state {
            State::Ask {
                setup: Some(Ok(setup)),
                ..
            } if setup.browser() => Some("Google AI Mode".to_string()),
            State::Ask {
                setup: Some(Ok(setup)),
                ..
            } => Some(setup.cfg.model.clone()),
            _ => None,
        };
        let header = widgets::card::Header {
            icon: action.icon,
            title: action.label,
            chip,
        };
        widgets::card::frame(header, body, footer, at, screen, hover, Msg::Card)
    }

    /// Body and footer.
    fn lens_view<'a>(&'a self, status: Option<&'a Result<(), String>>) -> Part<'a> {
        let note = widgets::caption("Only this selection goes to Google Lens.");
        let body = match status {
            None => column![widgets::body("Opening Google Lens in your browser…"), note],
            Some(Ok(())) => column![widgets::body("Opened in your browser"), note],
            // nothing went (e.g. a blank selection): no note about what goes
            Some(Err(e)) => column![widgets::error(e)],
        }
        .spacing(8);
        (body.into(), Some(self.follow_up("Ask AI about it…", false)))
    }

    fn ask_view<'a>(
        &'a self,
        thumb: &'a image::Handle,
        setup: Option<&'a Result<Arc<Setup>, String>>,
        chat: &'a Chat,
        browse: &'a Browse,
    ) -> Part<'a> {
        let thumb = container(
            image(thumb.clone())
                .height(THUMB_HEIGHT)
                .content_fit(ContentFit::Contain),
        )
        .align_right(Fill);
        let Some(Ok(_)) = setup else {
            let note: Element<'a, Msg> = match setup {
                Some(Err(steps)) => widgets::scroll(widgets::body(steps)).height(Fill).into(),
                _ => widgets::caption("Getting ready…").into(),
            };
            return (column![thumb, note].spacing(16).into(), None);
        };

        let browser = matches!(setup, Some(Ok(s)) if s.browser());
        let mut log = column![].spacing(12).width(Fill);
        for (i, turn) in chat.turns.iter().enumerate() {
            log = log
                .push(container(widgets::bubble(widgets::body(&turn.question))).align_right(Fill));
            let last = i + 1 == chat.turns.len();
            if !turn.answer.is_empty() {
                log = log.push(widgets::body(&turn.answer));
            } else if last && chat.status == Status::Streaming {
                log = log.push(match browse {
                    Browse::Downloading(done) => column![
                        widgets::caption("Downloading the text-recognition model…"),
                        widgets::progress(*done),
                    ]
                    .spacing(6)
                    .into(),
                    _ if browser => Element::from(widgets::caption("Opening Google AI Mode…")),
                    _ => widgets::caption("Thinking…").into(),
                });
            }
        }
        // Questions about the picture itself: Lens takes the crop (browser provider).
        if browser && chat.status == Status::Done {
            log = log.push(widgets::text_button(
                actions::lens::ACTION.icon,
                "Search the image with Lens",
                String::new(),
                false,
                Msg::Search,
            ));
        }
        match &chat.status {
            Status::Failed(e) => log = log.push(widgets::error(e)),
            Status::Cancelled => log = log.push(widgets::caption("Stopped.")),
            _ => {}
        }
        if let Browse::Consent(question) = browse {
            // Waiting for the user's choice: the question is theirs, not yet a turn.
            log = log.push(container(widgets::bubble(widgets::body(question))).align_right(Fill));
        } else if chat.turns.is_empty() {
            let prompts = PRESETS
                .iter()
                .enumerate()
                .map(|(i, p)| widgets::chip(p.label, Msg::Preset(i)).into());
            log = log.push(row(prompts).spacing(8).wrap());
        }
        let input: Element<'a, Msg> = if matches!(browse, Browse::Consent(_)) {
            column![
                widgets::body(
                    "To add the text of your selection to the question, OpenCircle downloads \
                     the text-recognition model once (about 20 MB, from GitHub). \
                     Nothing else is sent."
                ),
                row![
                    widgets::text_button(
                        '\u{e0b6}', // download
                        "Download",
                        String::new(),
                        true,
                        Msg::Download
                    ),
                    widgets::text_button(
                        SEND,
                        "Send without the text",
                        String::new(),
                        false,
                        Msg::SendWithoutText
                    ),
                ]
                .spacing(8)
                .wrap(),
            ]
            .spacing(12)
            .into()
        } else {
            self.follow_up("Ask a follow-up…", chat.status == Status::Streaming)
        };
        let privacy = if browser {
            "When you send, your question and the text read from the selection go to Google \
             AI Mode. The image is not uploaded unless you choose Lens."
        } else {
            "Only this selection is sent, and only to the provider you chose."
        };
        let body = widgets::scroll(column![thumb, log].spacing(16))
            .id(LOG)
            .height(Fill);
        let footer = column![input, widgets::caption(privacy)].spacing(10);
        (body.into(), Some(footer.into()))
    }

    /// The input with its send (or, while streaming, stop) button.
    fn follow_up(&self, placeholder: &str, streaming: bool) -> Element<'_, Msg> {
        let button = if streaming {
            widgets::hint(
                widgets::icon_button(STOP, false, Msg::Stop),
                "Stop",
                widgets::Why::Icon,
                Position::Top,
            )
        } else {
            widgets::hint(
                widgets::icon_button(SEND, true, Msg::Submit),
                "Send",
                widgets::Why::Keys("Enter"),
                Position::Top,
            )
        };
        let input = widgets::input(INPUT, placeholder, &self.input, Msg::Input, Msg::Submit);
        row![keys::guard(input.width(Fill), Msg::Key), button]
            .spacing(8)
            .align_y(Center)
            .into()
    }
}

/// Asks before the one-time OCR model download (Text panel, translate).
pub(super) fn consent<'a, M: Clone + 'a>(download: M, cancel: M) -> Element<'a, M> {
    column![
        widgets::body(
            "Download the text-recognition model (about 20 MB) from GitHub? Nothing else is sent."
        ),
        row![
            widgets::text_button(
                '\u{e0b6}', // download
                "Download",
                String::new(),
                true,
                download
            ),
            widgets::text_button(widgets::CLOSE, "Cancel", String::new(), false, cancel),
        ]
        .spacing(8),
    ]
    .spacing(16)
    .into()
}

impl Panel {
    fn text_view<'a>(&'a self, reading: &'a Reading) -> Element<'a, Msg> {
        let no_hint = String::new;
        match reading {
            Reading::Consent => consent(Msg::Download, Msg::Cancel),
            Reading::Downloading(done) => column![
                widgets::body("Downloading the text-recognition model…"),
                widgets::progress(*done),
                widgets::caption(format!("{:.0} %", done * 100.0)),
            ]
            .spacing(10)
            .into(),
            Reading::Busy => widgets::caption("Reading the text…").into(),
            Reading::Done(content) => {
                let translate = &actions::translate::ACTION;
                let mut body =
                    column![keys::guard(widgets::editor(content, Msg::Edit), Msg::Key)].spacing(12);
                body = match &self.translation {
                    _ if self.work.is_some() => body.push(widgets::caption("Translating…")),
                    Some(Ok(text)) => {
                        body.push(widgets::scroll(widgets::body(text).width(Fill)).height(Fill))
                    }
                    Some(Err(e)) => body.push(widgets::error(e)),
                    None => body,
                };
                if !self.entities.is_empty() {
                    let chips = self
                        .entities
                        .iter()
                        .enumerate()
                        .map(|(i, e)| widgets::chip(e.chip(), Msg::Chip(i)).into());
                    body = body.push(row(chips).spacing(8).wrap());
                }
                body = match &self.note {
                    Some(Ok(done)) => body.push(widgets::caption(*done)),
                    Some(Err(e)) => body.push(widgets::error(e)),
                    None => body,
                };
                body.push(
                    row![
                        widgets::text_button(
                            actions::copy::ACTION.icon,
                            "Copy",
                            no_hint(),
                            true,
                            Msg::CopyText
                        ),
                        widgets::text_button(
                            actions::lens::ACTION.icon,
                            "Search",
                            no_hint(),
                            false,
                            Msg::Search
                        ),
                        widgets::text_button(
                            translate.icon,
                            translate.label,
                            no_hint(),
                            false,
                            Msg::Translate
                        ),
                    ]
                    .spacing(8)
                    .wrap(),
                )
                .into()
            }
            Reading::Failed(e) => widgets::error(e).into(),
        }
    }
}

/// What a chip click does: open the link or a new message, or copy the number.
fn act_on(entity: &Entity) -> Result<&'static str, String> {
    match entity.kind {
        Kind::Link => crate::lens::open_link(&entity.text)
            .map(|()| "Opened in your browser.")
            .map_err(|e| format!("Could not open the link: {e}")),
        Kind::Email => crate::lens::open_mailto(&entity.text)
            .map(|()| "Opened your mail app.")
            .map_err(|e| format!("Could not open the mail app: {e}")),
        Kind::Phone => actions::copy::text(entity.text.clone())
            .map(|()| "Number copied.")
            .map_err(|e| format!("Could not copy: {e}")),
    }
}

/// Browser provider: the question to open in AI Mode and the line to add to the
/// answer. `read` is the OCR result (`None` = the user declined to read text).
fn browser_turn(prompt: &str, read: Option<Result<String, String>>) -> (String, String) {
    let alone = |why: String| {
        (
            ai::browser::question(prompt, None),
            format!("\n{why} Only your question was sent."),
        )
    };
    match read {
        Some(Ok(text)) if !text.trim().is_empty() => (
            ai::browser::question(prompt, Some(&text)),
            "\nSent with the text read from your selection.".into(),
        ),
        Some(Ok(_)) => alone("No text was found in the selection.".into()),
        Some(Err(e)) => alone(format!("Could not read the text ({e}).")),
        None => alone("The image itself cannot be passed to AI Mode.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a panel shows on a `screen` (no selection).
    fn at(screen: Size) -> Rectangle {
        widgets::card::place(screen, None, None)
    }

    #[test]
    fn browser_turn_sends_text_or_says_why_not() {
        let (q, note) = browser_turn("Explain", Some(Ok("Sunstone\nRare gem".into())));
        assert_eq!(q, "Explain\n\nText from my screen:\nSunstone\nRare gem");
        assert!(note.contains("Sent with the text"));
        // Declined (no model download), empty, failed: the prompt alone, and the reason.
        for (read, why) in [
            (None, "cannot be passed"),
            (Some(Ok("  \n".into())), "No text was found"),
            (Some(Err("helper crashed".into())), "helper crashed"),
        ] {
            let (q, note) = browser_turn("Explain", read);
            assert_eq!(q, "Explain");
            assert!(note.contains(why) && note.contains("Only your question was sent"));
        }
    }

    #[test]
    fn unknown_provider_shows_the_fix_and_sends_nothing() {
        let crop = Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        };
        let mut panel = Panel {
            state: State::Ask {
                thumb: image::Handle::from_rgba(1, 1, crop.rgba.clone()),
                setup: None,
                chat: Chat::default(),
                browse: Browse::Idle,
            },
            crop: Arc::new(crop),
            input: "Explain".into(), // e.g. the Lens follow-up, sent once ready
            work: None,
            translation: None,
            entities: Vec::new(),
            note: None,
        };
        let msg = ask::tests::invalid_provider_error();
        let _ = panel.update(Msg::Ready(Err(msg.clone())));
        let _ = panel.update(Msg::Preset(0));
        let _ = panel.update(Msg::Submit);
        let State::Ask { chat, browse, .. } = &panel.state else {
            unreachable!()
        };
        assert!(chat.turns.is_empty() && chat.status == Status::Idle);
        assert!(matches!(browse, Browse::Idle) && panel.work.is_none());
        let texts = widgets::shot::texts(panel.view(
            at(Size::new(1920.0, 1080.0)),
            Size::new(1920.0, 1080.0),
            false,
        ));
        assert!(texts.contains(&msg), "{texts:?}");
    }

    fn sample_crop() -> Crop {
        let rgba = (0..120u32)
            .flat_map(|y| (0..240u32).flat_map(move |x| [x as u8, (y * 2) as u8, 160, 255]))
            .collect();
        Crop {
            width: 240,
            height: 120,
            rgba,
        }
    }

    fn panel_in(state: State) -> Panel {
        Panel {
            state,
            crop: Arc::new(sample_crop()),
            input: String::new(),
            work: None,
            translation: None,
            entities: Vec::new(),
            note: None,
        }
    }

    const SCREEN: Size = Size::new(1000.0, 700.0);

    fn not_blank(px: &[u8]) -> bool {
        px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4])
    }

    #[test]
    fn ask_panel_offers_the_six_presets() {
        let crop = sample_crop();
        let mut panel = panel_in(State::Ask {
            thumb: image::Handle::from_rgba(crop.width, crop.height, crop.rgba.clone()),
            setup: None,
            chat: Chat::default(),
            browse: Browse::Idle,
        });
        let _ = panel.update(Msg::Ready(Ok(Arc::new(ask::tests::browser_setup()))));
        let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
        for preset in PRESETS {
            assert!(texts.iter().any(|t| t == preset.label), "{texts:?}");
        }
        assert!(not_blank(&widgets::shot::save(
            "panel-ask-presets",
            panel.view(at(SCREEN), SCREEN, false),
            SCREEN
        )));
        // Once a question was asked the suggestions are gone.
        let State::Ask { chat, .. } = &mut panel.state else {
            unreachable!()
        };
        chat.ask("Explain", &[]);
        let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
        assert!(!texts.iter().any(|t| t == "Fact-check"), "{texts:?}");
    }

    fn text_panel(text: &str) -> Panel {
        let mut panel = panel_in(State::Text(Reading::Busy));
        let _ = panel.update(Msg::Read(Ok(text.into())));
        panel
    }

    const SAMPLE: &str = "Order #2026102412 shipped\n\
        Questions? Call +1 (555) 123-4567 or write to help@shop.example.\n\
        Track it: https://shop.example/track/4412. Total $1,299.99 on 2026-10-24.";

    #[test]
    fn text_panel_shows_chips_for_links_emails_and_numbers_only() {
        let mut panel = text_panel(SAMPLE);
        let kinds: Vec<_> = panel.entities.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [Kind::Link, Kind::Email, Kind::Phone]);
        let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
        for chip in [
            "Open link · shop.example",
            "Write email · help@shop.example",
            "Copy number · +1 (555) 123-4567",
        ] {
            assert!(texts.iter().any(|t| t == chip), "{chip}: {texts:?}");
        }
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("1,299.99") && t.contains(": "))
        );
        // Reading the text does nothing with the chips: no click, no action, no note.
        assert!(panel.note.is_none());
        assert!(not_blank(&widgets::shot::save(
            "panel-text-chips",
            panel.view(at(SCREEN), SCREEN, false),
            SCREEN
        )));

        // Editing the text updates the chips.
        let paste = text_editor::Edit::Paste(Arc::new("new@site.io ".into()));
        let _ = panel.update(Msg::Edit(text_editor::Action::Edit(paste)));
        assert!(panel.entities.iter().any(|e| e.text == "new@site.io"));
        // A click outside the list does nothing.
        let _ = panel.update(Msg::Chip(99));
        assert!(panel.note.is_none());
    }

    #[test]
    fn text_without_entities_has_no_chips() {
        let panel = text_panel("Just some words, 3.50 and e.g. a note");
        assert!(panel.entities.is_empty());
        let texts = widgets::shot::texts(panel.view(at(SCREEN), SCREEN, false));
        assert!(!texts.iter().any(|t| t.starts_with("Open link")));
    }
}
