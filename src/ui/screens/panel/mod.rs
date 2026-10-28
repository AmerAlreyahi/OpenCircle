//! Result panels (DESIGN.md Flow step 5): Search (Lens), Text and Ask AI. One
//! floating card (`kit::panel_frame::card`: on the side away from the
//! selection, a card at the bottom on narrow screens; dragged and resized by
//! its frame) over the frozen frame, so the selection stays visible. Slow work
//! runs on its own thread (`background`); its results arrive as `Msg`s.
//! Dropping a panel stops its running work. What it shows: `view`.
use std::sync::Arc;

use iced::widget::{image, operation, text_editor};
use iced::{Task, task};

use crate::actions::ask::{self, Chat, PRESETS, Setup, Status};
use crate::actions::entities::{self, Entity, Kind};
use crate::actions::text::{self, Progress};
use crate::actions::{self, Crop, lens};
use crate::ai;
use crate::ui::background;
use crate::ui::keys;
use crate::ui::kit::panel_frame;

mod view;
pub use view::{actions, card, consent, downloading, note};

/// The follow-up input. Never focused by itself: until it is clicked, letters
/// are the overlay's action shortcuts.
pub(in crate::ui) const INPUT: &str = "panel-input";
/// The Ask AI conversation (scrolled to the end as the answer grows).
const LOG: &str = "panel-log";

#[derive(Debug, Clone)]
pub enum Msg {
    /// The card's grip or corner was pressed (the App moves / resizes it).
    Card(panel_frame::Event),
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
                    crate::history::search::keep("ai", &prompt, &url, Some(&crop));
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
}

/// What a chip click does: open the link or a new message, or copy the number.
fn act_on(entity: &Entity) -> Result<&'static str, String> {
    match entity.kind {
        Kind::Link => crate::sys::open::open_link(&entity.text)
            .map(|()| "Opened in your browser.")
            .map_err(|e| format!("Could not open the link: {e}")),
        Kind::Email => crate::sys::open::open_mailto(&entity.text)
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
mod tests;
