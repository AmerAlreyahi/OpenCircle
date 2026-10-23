//! Result panels (DESIGN.md Flow step 5): Search (Lens), Text and Ask AI. One
//! `surface` docked right (a bottom sheet on narrow screens) over the frozen
//! frame, so the selection stays visible. Slow work runs on its own thread
//! (`super::background`); its results arrive as `Msg`s. Dropping a panel stops
//! its running work.
use std::sync::Arc;

use iced::widget::{column, container, image, opaque, operation, pin, row, space, text_editor};
use iced::{Center, ContentFit, Element, Fill, Point, Rectangle, Size, Task, task};

use super::background;
use super::widgets::{self, SEND, STOP};
use crate::actions::ask::{self, Chat, Setup, Status};
use crate::actions::text::{self, Progress};
use crate::actions::{self, Crop, lens};

/// Panel width when docked (DESIGN.md).
const WIDTH: f32 = 420.0;
/// Distance to the screen edges.
const GAP: f32 = 16.0;
/// Narrower screens (logical px) get a bottom sheet instead.
const NARROW: f32 = 900.0;
/// A bottom sheet is half the screen, but at least this tall (if the screen is).
const SHEET_MIN: f32 = 260.0;
/// The crop's thumbnail in the Ask AI panel.
const THUMB_HEIGHT: f32 = 112.0;
/// The follow-up input (focused when it appears).
const INPUT: &str = "panel-input";
/// The Ask AI conversation (scrolled to the end as the answer grows).
const LOG: &str = "panel-log";
const PROMPTS: [&str; 3] = ["Explain", "Summarize", "What is this?"];

#[derive(Debug, Clone)]
pub enum Msg {
    /// The Lens page is written and the browser launched (the App holds the page).
    LensOpened(Result<lens::Held, String>),
    Input(String),
    /// Enter / send in the follow-up input.
    Submit,
    Prompt(&'static str),
    Stop,
    Ready(Result<Arc<Setup>, String>),
    Delta(String),
    Answered(Result<(), String>),
    /// The user agreed to the model download.
    Download,
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
    },
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
    pub fn open(kind: actions::Panel, crop: Crop, question: Option<String>) -> (Panel, Task<Msg>) {
        let crop = Arc::new(crop);
        let mut panel = Panel {
            crop: crop.clone(),
            state: State::Lens(None),
            input: question.unwrap_or_default(),
            work: None,
            translation: None,
        };
        let task = match kind {
            actions::Panel::Lens => background(move |tx| {
                let _ = tx.unbounded_send(Msg::LensOpened(lens::open(&crop)));
            })
            .chain(operation::focus(INPUT)),
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
                };
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
                    ..
                },
                Msg::Submit,
            ) if chat.status != Status::Streaming => {
                let question = self.take_input();
                return self.send(&question);
            }
            (State::Ask { .. }, Msg::Prompt(prompt)) => return self.send(prompt),
            (State::Ask { chat, .. }, Msg::Stop) => {
                chat.cancel();
                self.work = None;
            }
            (State::Ask { setup, .. }, Msg::Ready(result)) => {
                let ready = result.is_ok();
                *setup = Some(result);
                if ready && !self.input.trim().is_empty() {
                    let question = self.take_input();
                    return self.send(&question).chain(operation::focus(INPUT));
                }
                if ready {
                    return operation::focus(INPUT);
                }
            }
            (State::Ask { chat, .. }, Msg::Delta(piece)) => {
                chat.delta(&piece);
                // Follow the answer. (anchor_bottom() hid short content in iced 0.14.)
                return operation::snap_to_end(LOG);
            }
            (State::Ask { chat, .. }, Msg::Answered(result)) => {
                chat.finish(result);
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
                    Ok(text) => Reading::Done(text_editor::Content::with_text(&text)),
                    Err(e) => Reading::Failed(e),
                };
            }
            (State::Text(Reading::Done(content)), Msg::Edit(action)) => content.perform(action),
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
            ..
        } = &mut self.state
        else {
            return Task::none();
        };
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

    pub fn view(&self, screen: Size) -> Element<'_, Msg> {
        let area = place(screen);
        let (action, body) = match &self.state {
            State::Lens(status) => (&actions::lens::ACTION, self.lens_view(status.as_ref())),
            State::Text(reading) => (&actions::text::ACTION, self.text_view(reading)),
            State::Ask { thumb, setup, chat } => (
                &actions::ask::ACTION,
                self.ask_view(thumb, setup.as_ref(), chat),
            ),
        };
        let mut header = row![widgets::icon(action.icon), widgets::heading(action.label)]
            .spacing(10)
            .align_y(Center);
        if let State::Ask {
            setup: Some(Ok(setup)),
            ..
        } = &self.state
        {
            header = header
                .push(space::horizontal())
                .push(widgets::caption(&setup.cfg.model));
        }
        let panel = widgets::surface(column![header, body].spacing(16).height(Fill))
            .padding(20)
            .width(area.width)
            .height(area.height);
        // opaque: clicks on the panel never reach the selection canvas below.
        pin(opaque(panel)).position(area.position()).into()
    }

    fn lens_view(&self, status: Option<&Result<(), String>>) -> Element<'_, Msg> {
        let status: Element<'_, Msg> = match status {
            None => widgets::body("Opening Google Lens in your browser…").into(),
            Some(Ok(())) => widgets::body("Opened in your browser").into(),
            Some(Err(e)) => widgets::error(format!("Could not open the browser: {e}")).into(),
        };
        column![
            status,
            widgets::caption("Only this selection goes to Google Lens, from your browser."),
            space::vertical(),
            self.follow_up("Ask AI about it…", false),
        ]
        .spacing(8)
        .into()
    }

    fn ask_view<'a>(
        &'a self,
        thumb: &'a image::Handle,
        setup: Option<&'a Result<Arc<Setup>, String>>,
        chat: &'a Chat,
    ) -> Element<'a, Msg> {
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
            return column![thumb, note].spacing(16).into();
        };

        let mut log = column![].spacing(12).width(Fill);
        for (i, turn) in chat.turns.iter().enumerate() {
            log = log
                .push(container(widgets::bubble(widgets::body(&turn.question))).align_right(Fill));
            let last = i + 1 == chat.turns.len();
            if !turn.answer.is_empty() {
                log = log.push(widgets::body(&turn.answer));
            } else if last && chat.status == Status::Streaming {
                log = log.push(widgets::caption("Thinking…"));
            }
        }
        match &chat.status {
            Status::Failed(e) => log = log.push(widgets::error(e)),
            Status::Cancelled => log = log.push(widgets::caption("Stopped.")),
            _ => {}
        }
        if chat.turns.is_empty() {
            let prompts = PROMPTS.map(|p| widgets::chip(p, Msg::Prompt(p)).into());
            log = log.push(row(prompts).spacing(8));
        }
        column![
            widgets::scroll(column![thumb, log].spacing(16))
                .id(LOG)
                .height(Fill),
            self.follow_up("Ask a follow-up…", chat.status == Status::Streaming),
            widgets::caption("Only this selection is sent, and only to the provider you chose."),
        ]
        .spacing(12)
        .into()
    }

    /// The input with its send (or, while streaming, stop) button.
    fn follow_up(&self, placeholder: &str, streaming: bool) -> Element<'_, Msg> {
        let button = if streaming {
            widgets::icon_button(STOP, false, Msg::Stop)
        } else {
            widgets::icon_button(SEND, true, Msg::Submit)
        };
        row![
            widgets::input(INPUT, placeholder, &self.input, Msg::Input, Msg::Submit).width(Fill),
            button,
        ]
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
                let mut body = column![widgets::editor(content, Msg::Edit)].spacing(12);
                body = match &self.translation {
                    _ if self.work.is_some() => body.push(widgets::caption("Translating…")),
                    Some(Ok(text)) => {
                        body.push(widgets::scroll(widgets::body(text).width(Fill)).height(Fill))
                    }
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

/// Where the panel goes on a `screen`-sized overlay (logical px): docked right,
/// full height; on narrow screens a bottom sheet. Always inside the screen.
fn place(screen: Size) -> Rectangle {
    let inner = Size::new(
        (screen.width - 2.0 * GAP).max(0.0),
        (screen.height - 2.0 * GAP).max(0.0),
    );
    if screen.width >= NARROW {
        return Rectangle::new(
            Point::new(screen.width - GAP - WIDTH, GAP),
            Size::new(WIDTH, inner.height),
        );
    }
    let height = (screen.height / 2.0).max(SHEET_MIN).min(inner.height);
    Rectangle::new(
        Point::new(GAP, screen.height - GAP - height),
        Size::new(inner.width, height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(r: Rectangle, screen: Size) -> bool {
        r.x >= GAP
            && r.y >= GAP
            && r.x + r.width <= screen.width - GAP + 0.01
            && r.y + r.height <= screen.height - GAP + 0.01
    }

    #[test]
    fn panel_docks_right_at_full_height() {
        let screen = Size::new(1920.0, 1080.0);
        let r = place(screen);
        assert_eq!(
            r,
            Rectangle::new(Point::new(1484.0, 16.0), Size::new(420.0, 1048.0))
        );
        assert!(inside(r, screen));
        let ultrawide = Size::new(3440.0, 1440.0);
        assert_eq!(place(ultrawide).x, 3440.0 - GAP - WIDTH);
    }

    #[test]
    fn narrow_screens_get_a_bottom_sheet() {
        let phone = Size::new(600.0, 1000.0);
        let r = place(phone);
        assert_eq!(
            r,
            Rectangle::new(Point::new(16.0, 484.0), Size::new(568.0, 500.0))
        );
        // Short and narrow: at least SHEET_MIN tall, never off screen.
        for screen in [
            Size::new(800.0, 400.0),
            Size::new(300.0, 200.0),
            Size::new(899.0, 600.0),
        ] {
            let r = place(screen);
            assert!(inside(r, screen), "{screen:?} -> {r:?}");
            assert!(
                r.height >= SHEET_MIN.min(screen.height - 2.0 * GAP),
                "{r:?}"
            );
            assert_eq!(r.y + r.height, screen.height - GAP);
        }
    }
}
