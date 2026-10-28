//! Running actions on a selection: pills, quick actions, auto-search, QR, translate, song.

use std::time::Instant;

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::image;
use iced::{Point, Rectangle, Size, Task};

use crate::Trigger;
use crate::actions::{self, Action, Crop, Run, copy, lens, qr, text};
use crate::config::{self, After, QuickAction, SearchWith, SelectionConfig, Target};
use crate::labs::BarButton;

use crate::ui::app::background::background;
use crate::ui::app::message::{Message, PickAct, PillPlace, Purpose};
use crate::ui::app::quick::Quick;
use crate::ui::kit::panel_frame as card;
use crate::ui::platform::monitor::{self, taskbar_inset};
use crate::ui::{live_layer, overlay, panel, settings, song, translate, windows};

use super::{App, Reader, TOAST_MARGIN};

impl App {
    /// Scans the new selection for a QR code and a black (protected) capture, on a
    /// thread; the result comes back as `Scanned` and is dropped if the selection moved on.
    pub(in crate::ui) fn scan(&mut self, area: Rectangle) -> Task<Message> {
        let Some(crop) = self.crop() else {
            return Task::none();
        };
        let hole = self.chat_hole();
        background(move |tx| {
            let _ = tx.unbounded_send(qr::scan_around(&crop, hole));
        })
        .map(move |scan| Message::Scanned(area, scan))
    }

    /// The QR chip was clicked: copy its text, or open it (a link, on this click only).
    pub(in crate::ui) fn on_qr(&mut self, act: qr::Act) -> Task<Message> {
        let Some(text) = self.o.scan.as_ref().and_then(|s| s.qr.clone()) else {
            return Task::none();
        };
        let failed = |e: String| {
            crate::log!("QR chip failed: {e}");
            "That didn't work"
        };
        match act {
            qr::Act::Copy => background(move |tx| {
                let pill = copy::text(text).map_or_else(failed, |()| "QR text copied");
                let _ = tx.unbounded_send(Message::ThenClose(Some(pill)));
            }),
            qr::Act::Open => match qr::web_url(&text).map(str::to_string) {
                // the browser has the screen now
                Some(url) => background(move |tx| {
                    let opened = crate::sys::open::open_link(&url);
                    let pill = opened.err().map(|e| failed(e.to_string()));
                    let _ = tx.unbounded_send(Message::ThenClose(pill));
                }),
                None => Task::none(),
            },
        }
    }

    /// The link / email chip beside the selection was clicked: open it in the
    /// user's browser or mail app (on this click only), then give them the screen.
    pub(in crate::ui) fn open_entity(
        &mut self,
        entity: actions::entities::Entity,
    ) -> Task<Message> {
        use actions::entities::Kind;
        background(move |tx| {
            let opened = match entity.kind {
                Kind::Link => crate::sys::open::open_link(&entity.text),
                Kind::Email => crate::sys::open::open_mailto(&entity.text),
                Kind::Phone => return, // (no chip)
            };
            let pill = opened.err().map(|e| {
                crate::log!("link chip failed: {e}");
                "That didn't work"
            });
            let _ = tx.unbounded_send(Message::ThenClose(pill));
        })
    }

    /// The first link or email in the selected words' text (the chip's).
    pub(in crate::ui) fn chip_entity(&self) -> Option<actions::entities::Entity> {
        use actions::entities::{Kind, find};
        let text = self.picked_text()?;
        find(&text).into_iter().find(|e| e.kind != Kind::Phone)
    }

    /// What runs by itself once a selection is made.
    pub(in crate::ui) fn auto_actions(&self) -> &'static [&'static Action] {
        let word = self
            .o
            .selection
            .is_some_and(|s| s.kind == overlay::Kind::Word);
        // (after the snap: the Settled message runs them)
        if !matches!(self.o.purpose, Purpose::Search)
            || word
            || self.o.selection.is_none()
            || self.settling()
        {
            return &[];
        }
        auto_targets(self.o.behaviour)
    }

    /// Runs `auto_actions` (Lens and Ask AI: the Lens page opens in the browser
    /// and stays held whichever panel then shows).
    pub(in crate::ui) fn auto_search(&mut self) -> Task<Message> {
        let runs: Vec<_> = (self.auto_actions().iter()).map(|a| self.run(a)).collect();
        Task::batch(runs)
    }

    /// Search in browser on a text selection: the text, as a Google search in
    /// the user's own browser (the user picked it; no web view). Like Lens,
    /// the overlay closes.
    pub(in crate::ui) fn search_text(&mut self, text: String) -> Task<Message> {
        let text = text.trim();
        if text.is_empty() {
            return Task::none();
        }
        let url = crate::ai::browser::search_url(text);
        let (text, crop) = (text.to_string(), self.crop());
        background(move |tx| {
            let pill = match crate::sys::open::open_link(&url) {
                Ok(()) => {
                    crate::history::search::keep("text", &text, &url, crop.as_ref());
                    None
                }
                Err(e) => {
                    crate::log!("[opencircle] text search not opened: {e}");
                    Some("Couldn't open your browser")
                }
            };
            let _ = tx.unbounded_send(Message::ThenClose(pill));
        })
    }

    /// A pill of the bottom bar: Search in browser and Copy text act on the
    /// selection (dimmed without one); Translate on it, else the whole screen.
    pub(in crate::ui) fn on_pill(&mut self, pill: BarButton) -> Task<Message> {
        let picked = self.picked_in_box();
        match pill {
            BarButton::Search => self.run(&lens::ACTION),
            BarButton::CopyText if picked || self.o.word.is_some() => self.on_picked(PickAct::Copy),
            BarButton::CopyText => self.run_quick(Quick::CopyText),
            BarButton::Translate if self.o.selection.is_none() => {
                self.update(Message::TranslateScreen)
            }
            BarButton::Translate if picked => self.on_picked(PickAct::Translate),
            BarButton::Translate => self.run(&actions::translate::ACTION),
            _ => Task::none(),
        }
    }

    /// Runs `action` on the selection: a finishing action closes the overlay first
    /// (so dialogs aren't hidden under it); a panel action opens its panel; Edit
    /// turns the bar into the editing toolbar. No selection yet = nothing to do.
    pub(in crate::ui) fn run(&mut self, action: &'static Action) -> Task<Message> {
        // Search in browser on picked words: their text, not a picture of them.
        if action.id == lens::ACTION.id
            && self.o.edit.is_none()
            && let Some(text) = self.picked_text()
        {
            return self.search_text(text);
        }
        match action.run {
            Run::Edit => return self.open_editor(),
            Run::Translate => return self.start_translate(),
            // Picked words (a word selection, or select-text mode's picks): Copy
            // (the bar's or Ctrl+C) copies exactly their text. In select-text
            // mode with nothing picked: all the box's text, never its picture
            // (read first if it isn't yet).
            Run::Finish(_)
                if (action.id == copy::ACTION.id || action.id == text::ACTION.id)
                    && self.o.edit.is_none()
                    && (self.o.word.is_some() || self.o.text_mode.is_some()) =>
            {
                return match self.picked_text().or_else(|| self.box_text()) {
                    Some(text) => self.copy_text(text),
                    None => self.run_quick(Quick::CopyText),
                };
            }
            // Copy text (key T, `run text`): the text straight to the clipboard.
            Run::Finish(_) if action.id == text::ACTION.id => {
                return self.run_quick(Quick::CopyText);
            }
            _ => {}
        }
        // Several selections: Ask AI gets each of them (stacked), the rest the
        // box around all of them.
        let all = matches!(action.run, Run::Panel(actions::Panel::Ask));
        let Some(crop) = (if all { self.stacked() } else { self.crop() }) else {
            return Task::none();
        };
        match action.run {
            Run::Finish(_) => self.finish(action, crop),
            Run::Panel(kind) => self.open_panel(kind, crop, None),
            Run::Edit | Run::Translate => Task::none(), // handled above
        }
    }

    /// Closes the overlay, then runs a finishing action (Copy, Save) on `crop`.
    pub(in crate::ui) fn finish(&mut self, action: &Action, crop: Crop) -> Task<Message> {
        let Run::Finish(finish) = action.run else {
            return Task::none();
        };
        let id = action.id;
        self.confirm_after(move || {
            let result = finish(crop);
            if let Err(e) = &result {
                crate::log!("{id} failed: {e}");
            }
            result
        })
    }

    /// Closes the overlay and tells the user how Copy / Save went: the toast pill
    /// in its own window beside the selection (else the bar), so the screen is
    /// the user's again at once and the pill outlives the overlay. `work`
    /// (encoding, the clipboard, a file) runs on a thread: the pill comes when
    /// it is done (#123).
    pub(in crate::ui) fn confirm_after(
        &mut self,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) -> Task<Message> {
        let place = (self.o.origin, self.o.screen, self.o.scale, self.pill_near());
        let done = background(move |tx| {
            let _ = tx.unbounded_send(work());
        })
        .map(move |result| Message::Confirm(place, result));
        Task::batch([self.close(), done])
    }

    /// What the Copy / Save pill goes beside (overlay px): the selection as
    /// shown, else the bottom bar; `None`: neither shows.
    pub(in crate::ui) fn pill_near(&self) -> Option<Rectangle> {
        let selected = (self.o.selection.as_ref()).map(|s| self.o.view.rect_to_screen(s.area));
        let bar = (self.bottom_bar_shown() && self.o.edit.is_none())
            .then(|| Rectangle::new(self.bar_at(), self.bar_size().1));
        selected.or(bar)
    }

    /// The Copy / Save pill for an overlay at `place` (origin, size, scale,
    /// what it goes beside).
    pub(in crate::ui) fn toast_for(
        &mut self,
        place: PillPlace,
        result: Result<String, String>,
    ) -> Task<Message> {
        let (text, ok) = match result {
            Ok(text) => (text, true),
            Err(text) => (text, false),
        };
        let size = windows::toast_size(&text);
        let (origin, screen, scale, _) = place;
        let inset = taskbar_inset(origin, screen, scale);
        let at = pill_at(place, size, inset);
        self.windows
            .toast(&text, ok, Some(at), windows::TOAST_MS)
            .map(Message::Windows)
    }

    /// Runs a quick action on the selection (no action bar). Only once: a new
    /// selection after it gets the bar.
    pub(in crate::ui) fn run_quick(&mut self, quick: Quick) -> Task<Message> {
        self.o.purpose = Purpose::Search;
        match quick {
            Quick::Action(action) => self.run(action),
            // No OCR models yet: the download card asks first (in place), then
            // the text is copied once it is Done (`Reader` in update.rs).
            Quick::CopyText if !text::models_present() => {
                self.o.copy_after = true;
                self.o.reader = Reader::Missing(crate::ocr::download_size());
                Task::none()
            }
            Quick::CopyText => {
                let Some(crop) = self.stacked() else {
                    return Task::none();
                };
                self.o.toast = Some("Reading text…");
                background(move |tx| {
                    let read = text::read(&crop, |_| {});
                    let copied = read.and_then(|t| match t.trim() {
                        "" => Err("no text found".into()),
                        _ => copy::text(t),
                    });
                    let _ = tx.unbounded_send(copied);
                })
                .map(Message::TextRead)
            }
            Quick::Song => self.start_song(),
            Quick::Explain => match self.crop() {
                Some(crop) => self.open_panel(
                    actions::Panel::Ask,
                    crop,
                    Some(actions::ask::PRESETS[0].text(&config::current().translate.target)),
                ),
                None => Task::none(),
            },
        }
    }

    /// The music button: song search with a key (`has_key`); without one its
    /// setup (Settings > Song search), and nothing is recorded.
    pub(in crate::ui) fn song(&mut self, has_key: bool) -> Task<Message> {
        if has_key {
            return self.start_song();
        }
        let closing = self.close();
        let settings = self.windows.open(windows::Kind::Settings);
        let section = self
            .windows
            .update(windows::Msg::Settings(settings::Message::Section(
                settings::Section::Song,
            )));
        closing.chain(settings.chain(section).map(Message::Windows))
    }

    /// Song search: listens on a thread; its pill, then its card, over the overlay.
    pub(in crate::ui) fn start_song(&mut self) -> Task<Message> {
        self.songs = self.songs.wrapping_add(1);
        let id = self.songs;
        // The song's card takes the result card's turn: one card at a time.
        self.o.panel = None;
        let (session, work) = song::Session::start();
        self.o.song = Some((id, session));
        work.map(move |msg| Message::Song(id, msg))
    }

    pub(in crate::ui) fn on_song(&mut self, id: u32, msg: song::Msg) -> Task<Message> {
        let Some((current, session)) = &mut self.o.song else {
            return Task::none();
        };
        if *current != id {
            return Task::none();
        }
        match session.update(msg) {
            song::Outcome::None => Task::none(),
            song::Outcome::Close => {
                self.o.song = None;
                Task::none()
            }
            song::Outcome::Retry => self.start_song(),
            // The browser comes to the front: the overlay closes so it isn't on top.
            song::Outcome::Open(url) => {
                let open = background(move |_: &UnboundedSender<()>| {
                    if let Err(e) = crate::sys::open::open_link(&url) {
                        crate::log!("[opencircle] can't open the link: {e}");
                    }
                })
                .discard();
                Task::batch([open, self.close()])
            }
        }
    }

    /// The music button shows only once a key is known to be saved.
    pub(in crate::ui) fn has_song_key(&self) -> bool {
        crate::music::key_known() == Some(true)
    }

    /// First open: ask the keyring (off the UI thread) whether a song key is saved.
    pub(in crate::ui) fn check_song_key(&self) -> Task<Message> {
        if crate::music::key_known().is_some() || cfg!(test) {
            return Task::none();
        }
        background(|tx| {
            let _ = tx.unbounded_send(crate::music::check_key());
        })
        .map(|_| Message::SongKey)
    }

    /// Translate in place: the selection (or, with no selection left, the whole
    /// frame) is translated on a thread, then swapped in.
    pub(in crate::ui) fn start_translate(&mut self) -> Task<Message> {
        let frame = self.o.frame.clone();
        let (Some(frame @ image::Handle::Rgba { width, height, .. }), Some(selection)) =
            (&frame, self.o.selection)
        else {
            return Task::none();
        };
        let Some(area) = actions::crop_bounds(*width, *height, self.o.scale, selection.area) else {
            return Task::none();
        };
        self.o.panel = None;
        self.o.edit = None;
        self.o.view = self.o.view.reset(); // the translation is drawn at 1x
        self.o.zoom = None;
        let models = text::models_present();
        let (session, task) = translate::Session::start(
            frame.clone(),
            area,
            self.o.scale,
            self.cache.clone(),
            models,
        );
        if session.full() {
            // nothing to outline: the translation is the whole screen
            self.o.selection = None;
            self.o.stroke.clear();
        }
        self.translates = self.translates.wrapping_add(1);
        let id = self.translates;
        self.o.translate = Some((id, session));
        task.map(move |msg| Message::Translate(id, msg))
    }

    pub(in crate::ui) fn on_translate(&mut self, id: u32, msg: translate::Msg) -> Task<Message> {
        let Some((_, session)) = self.o.translate.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            translate::Msg::Card(card::Event::Grab(how)) => {
                self.o.card_drag = Some((how, None));
                self.frame_times.borrow_mut().clear();
                Task::none()
            }
            translate::Msg::Card(card::Event::Hover(on)) => {
                self.o.card_hover = on;
                Task::none()
            }
            translate::Msg::Card(card::Event::Reset) => {
                self.card = None;
                Task::none()
            }
            // (the cards have no close button: Esc, or a click on the screen)
            translate::Msg::Close => self.close(),
            translate::Msg::OpenSettings => {
                let closing = self.close();
                let settings = self.windows.open(windows::Kind::Settings);
                closing.chain(settings.map(Message::Windows))
            }
            // The word card goes on as an Ask AI chat (its crop, its explanation).
            translate::Msg::Ask => match session.take_chat() {
                // one chat: the explanation goes on in the pinned one
                Some((_, chat)) if self.chat.is_some() => {
                    if let Some(open) = &mut self.chat {
                        open.panel.append(chat);
                    }
                    Task::none()
                }
                Some((crop, chat)) => {
                    self.panels = self.panels.wrapping_add(1);
                    let id = self.panels;
                    let (panel, task) = panel::Panel::open_chat(crop, chat, self.o.include);
                    self.o.panel = Some((id, panel));
                    self.keep_chat();
                    task.map(move |msg| Message::Panel(id, msg))
                }
                None => Task::none(),
            },
            msg => session
                .update(msg)
                .map(move |msg| Message::Translate(id, msg)),
        }
    }

    /// Ctrl+Shift+L (or the tray): live translate on, or off again.
    pub(in crate::ui) fn toggle_live(&mut self, at: Instant) -> Task<Message> {
        if let Some(live) = self.live.take() {
            crate::log!("[opencircle] live translate off");
            return live.stop().map(Message::Live);
        }
        if live_layer::unsupported() {
            crate::log!(
                "[opencircle] live translate: Wayland has no layer over other apps; translating once"
            );
            return self.translate_once(at);
        }
        let (live, task) = live_layer::Live::start(self.cache.clone());
        self.live = Some(live);
        task.map(Message::Live)
    }

    pub(in crate::ui) fn on_live(&mut self, msg: live_layer::Msg) -> Task<Message> {
        let Some(live) = &mut self.live else {
            return Task::none();
        };
        let off = |app: &mut Self| match app.live.take() {
            Some(live) => live.stop().map(Message::Live),
            None => Task::none(),
        };
        match live.update(msg) {
            live_layer::Outcome::Task(task) => task.map(Message::Live),
            live_layer::Outcome::Off => off(self),
            live_layer::Outcome::OneShot => {
                let stopped = off(self);
                stopped.chain(self.translate_once(Instant::now()))
            }
        }
    }

    /// The one-shot full-screen translate (Ctrl+Shift+T).
    pub(in crate::ui) fn translate_once(&mut self, at: Instant) -> Task<Message> {
        let once = QuickAction {
            action: actions::translate::ACTION.id.into(),
            shortcut: String::new(),
            target: Target::FullScreen,
            live: false,
        };
        self.update(Message::Trigger(Trigger::Quick(at, once)))
    }
}

/// The actions a selection runs by itself under `behaviour`.
pub(in crate::ui) fn auto_targets(behaviour: SelectionConfig) -> &'static [&'static Action] {
    match (behaviour.after, behaviour.search_with) {
        (After::Wait, _) => &[],
        (After::Search, SearchWith::Lens) => &[&lens::ACTION],
        (After::Search, SearchWith::Ai) => &[&actions::ask::ACTION],
        (After::Search, SearchWith::Both) => &[&lens::ACTION, &actions::ask::ACTION],
    }
}

/// Where the Copy / Save pill of `size` goes for an overlay at `place`
/// (its origin, size and scale, logical px; what it goes beside, overlay px),
/// `inset` px of taskbar under it: centred below what it goes beside, else
/// above it, kept `TOAST_MARGIN` inside the screen; nothing to go beside:
/// bottom right. In physical desktop px, so it lands there at any scale and
/// on any monitor (#149).
pub(in crate::ui) fn pill_at(
    (origin, screen, scale, near): PillPlace,
    size: Size,
    inset: f32,
) -> monitor::Place {
    let m = TOAST_MARGIN;
    let bottom = screen.height - inset - m - size.height;
    let right = screen.width - m - size.width;
    let at = match near {
        Some(r) => {
            let below = r.y + r.height + m;
            let y = match below <= bottom {
                true => below,
                false => r.y - m - size.height,
            };
            Point::new(r.center_x() - size.width / 2.0, y)
        }
        None => Point::new(right, bottom),
    };
    let (x, y) = (at.x.clamp(m, right.max(m)), at.y.clamp(m, bottom.max(m)));
    let px = |v: f32| (v * scale).round() as i32;
    monitor::Place {
        origin: (px(origin.x + x), px(origin.y + y)),
        px: (px(size.width) as u32, px(size.height) as u32),
        scale,
    }
}
