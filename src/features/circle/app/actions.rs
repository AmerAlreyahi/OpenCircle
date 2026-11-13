//! Running actions on a selection: pills, quick actions, auto-search, QR, translate, song.

use crate::shared::app_language::{S, Said};
use std::time::Instant;

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::image;
use iced::{Point, Rectangle, Size, Task};

use crate::Trigger;
use crate::features::settings::labs::BarButton;
use crate::features::{
    circle::actions::{self, Action, Run, copy, qr, search_in_browser},
    text_reading::text,
};
use crate::shared::config::{self, QuickAction, Target};

use crate::app::background::background;
use crate::app::message::{Message, PickAct, PillPlace, Purpose};
use crate::features::circle::app::quick::Quick;
use crate::shared::kit::panel_frame as card;
use crate::shared::platform::monitor::{self, taskbar_inset};
use crate::{
    app::windows,
    features::{
        ask_ai::screens::ai_chat, live_translate::screens::live_layer, settings,
        song::screens::listen, translate::screens::pill,
    },
};

use crate::app::{App, Reader, TOAST_MARGIN};
use crate::shared::picture::{self, Crop};

impl App {
    /// Scans the new selection for a QR code and a black (protected) capture, on a
    /// thread; the result comes back as `Scanned` and is dropped if the selection moved on.
    pub(crate) fn scan(&mut self, area: Rectangle) -> Task<Message> {
        let Some(crop) = self.crop() else {
            return Task::none();
        };
        let hole = self.chat_hole();
        let size = (crop.width, crop.height);
        let origin = self.crop_area().and_then(|a| self.crop_origin(a));
        background(move |tx| {
            // the windows their apps keep out of captures, where the crop is
            let windows = origin.map_or_else(Vec::new, |o| {
                qr::crop_rects(
                    o,
                    size,
                    &crate::shared::platform::capture::window_rect::protected(),
                )
            });
            let _ = tx.unbounded_send(qr::scan_around(&crop, hole, &windows));
        })
        .map(move |scan| Message::Scanned(area, scan))
    }

    /// The QR chip was clicked: copy its text, or open it (a link, on this click only).
    pub(crate) fn on_qr(&mut self, act: qr::Act) -> Task<Message> {
        let Some(text) = self.o.scan.as_ref().and_then(|s| s.qr.clone()) else {
            return Task::none();
        };
        let failed = |e: String| {
            crate::log!("QR chip failed: {e}");
            Said(S::pill_failed)
        };
        match act {
            qr::Act::Copy => background(move |tx| {
                let pill = copy::text(text).map_or_else(failed, |()| Said(S::pill_qr_copied));
                let _ = tx.unbounded_send(Message::ThenClose(Some(pill)));
            }),
            qr::Act::Open => match qr::web_url(&text).map(str::to_string) {
                // the browser has the screen now
                Some(url) => background(move |tx| {
                    let opened = crate::shared::platform::sys::open::open_link(&url);
                    let pill = opened.err().map(|e| failed(e.to_string()));
                    let _ = tx.unbounded_send(Message::ThenClose(pill));
                }),
                None => Task::none(),
            },
        }
    }

    /// The link / email chip beside the selection was clicked: open it in the
    /// user's browser or mail app (on this click only), then give them the screen.
    pub(crate) fn open_entity(&mut self, entity: actions::entities::Entity) -> Task<Message> {
        use actions::entities::Kind;
        background(move |tx| {
            let opened = match entity.kind {
                Kind::Link => crate::shared::platform::sys::open::open_link(&entity.text),
                Kind::Email => crate::shared::platform::sys::open::open_mailto(&entity.text),
                Kind::Phone => return, // (no chip)
            };
            let pill = opened.err().map(|e| {
                crate::log!("link chip failed: {e}");
                Said(S::pill_failed)
            });
            let _ = tx.unbounded_send(Message::ThenClose(pill));
        })
    }

    /// The first link or email in the selected words' text (the chip's).
    pub(crate) fn chip_entity(&self) -> Option<actions::entities::Entity> {
        use actions::entities::{Kind, find};
        let text = self.picked_text()?;
        find(&text).into_iter().find(|e| e.kind != Kind::Phone)
    }

    /// Search in browser on a text selection: the text, as a Google search in
    /// the user's own browser (the user picked it; no web view). Like Lens,
    /// the overlay closes.
    pub(crate) fn search_text(&mut self, text: String) -> Task<Message> {
        let text = text.trim();
        if text.is_empty() {
            return Task::none();
        }
        let url = crate::features::ask_ai::browser::search_url(text);
        background(move |tx| {
            let pill = match crate::shared::platform::sys::open::open_link(&url) {
                Ok(()) => None,
                Err(e) => {
                    crate::log!("[opencircle] text search not opened: {e}");
                    Some(Said(S::pill_browser_short))
                }
            };
            let _ = tx.unbounded_send(Message::ThenClose(pill));
        })
    }

    /// Search in browser on a picture: Lens opens in the user's own browser and
    /// the overlay closes; no panel of ours shows meanwhile.
    fn search_image(&mut self, crop: Crop) -> Task<Message> {
        background(move |tx| {
            let _ = tx.unbounded_send(Message::SearchOpened(search_in_browser::open(&crop)));
        })
    }

    /// The browser has the Lens page, or not.
    pub(crate) fn on_search_opened(&mut self, opened: Result<(), String>) -> Task<Message> {
        match opened {
            _ if self.o.frame.is_none() => Task::none(), // the overlay closed meanwhile
            Ok(()) => self.close(),
            Err(e) => self.confirm_after(move || Err(e)),
        }
    }

    /// A pill of the bottom bar: Search in browser and Copy text act on the
    /// selection (dimmed without one); Translate on it, else the whole screen.
    pub(crate) fn on_pill(&mut self, pill: BarButton) -> Task<Message> {
        let picked = self.picked_in_box();
        match pill {
            BarButton::Search => self.run(&search_in_browser::ACTION),
            BarButton::CopyText if picked || self.o.word.is_some() => self.on_picked(PickAct::Copy),
            BarButton::CopyText => self.run_quick(Quick::CopyText),
            BarButton::Translate if self.o.selection.is_none() => {
                self.update(Message::TranslateScreen)
            }
            BarButton::Translate if picked => self.on_picked(PickAct::Translate),
            BarButton::Translate => self.run(&crate::features::translate::action::ACTION),
            _ => Task::none(),
        }
    }

    /// Runs `action` on the selection: a finishing action closes the overlay first
    /// (so dialogs aren't hidden under it); a panel action opens its panel; Edit
    /// turns the bar into the editing toolbar. No selection yet = nothing to do.
    pub(crate) fn run(&mut self, action: &'static Action) -> Task<Message> {
        // Search in browser on picked words: their text, not a picture of them.
        if action.id == search_in_browser::ACTION.id
            && self.o.edit.is_none()
            && let Some(text) = self.picked_text()
        {
            return self.search_text(text);
        }
        match action.run {
            // Snip is one flow: from main mode its selection is the pick.
            Run::Snip if matches!(self.o.purpose, Purpose::Snip(_)) => {
                return self.open_snip_toolbar();
            }
            Run::Snip => return self.snip_here(self.o.selection.map(|s| s.area)),
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
        let all = matches!(action.run, Run::AskAi);
        let Some(crop) = (if all { self.stacked() } else { self.crop() }) else {
            return Task::none();
        };
        match action.run {
            Run::Finish(_) => self.finish(action, crop),
            Run::AskAi => self.open_chat(crop, None),
            Run::SearchInBrowser => self.search_image(crop),
            Run::Snip | Run::Translate => Task::none(), // handled above
        }
    }

    /// Closes the overlay, then runs a finishing action (Copy, Save) on `crop`.
    pub(crate) fn finish(&mut self, action: &Action, crop: Crop) -> Task<Message> {
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
    /// it is done (#123). Wayland: the overlay stays until then and draws the
    /// pill itself (`toast_for`).
    pub(crate) fn confirm_after(
        &mut self,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) -> Task<Message> {
        let place = self.pill_place();
        let done = background(move |tx| {
            let _ = tx.unbounded_send(work());
        })
        .map(move |result| Message::Confirm(place, result));
        if !self.can_hide {
            return done;
        }
        Task::batch([self.close(), done])
    }

    /// Where the Copy / Save pill goes (`pill_at`): beside the selection, else
    /// beside the bottom bar, on the screen holding most of it (#130), else at
    /// the starting screen's corner.
    pub(crate) fn pill_place(&self) -> PillPlace {
        let selected = self.o.selection.as_ref().map(|s| s.area);
        let bar = (self.bottom_bar_shown() && self.o.edit.is_none())
            .then(|| Rectangle::new(self.bar_at(), self.dock_size()));
        let Some(near) = selected.or(bar) else {
            return (self.o.origin, self.o.screen, self.o.scale, None);
        };
        let on = self.on_area(near);
        match on.view {
            None => {
                let near = match selected {
                    Some(area) => self.o.view.rect_to_screen(area),
                    None => near,
                };
                (self.o.origin, self.o.screen, self.o.scale, Some(near))
            }
            Some(_) => {
                let p = on.place;
                (
                    p.logical_origin(),
                    p.size(),
                    p.scale,
                    Some(on.to_local(near)),
                )
            }
        }
    }

    /// The Copy / Save pill for an overlay at `place` (origin, size, scale,
    /// what it goes beside).
    pub(crate) fn toast_for(
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
        // Wayland can't cut a window to the pill (it would be a square box):
        // the overlay, still up, draws it at its place, then closes; Snip's
        // toolbar is still there too, so it goes beside both (beside the
        // toolbar alone when both fill the screen). On another screen's
        // window, or once closed, the window as elsewhere.
        // ponytail: the starting screen's window only; the others' could draw it too.
        if !self.can_hide && self.o.frame.is_some() && origin == self.o.origin {
            let toolbar = (self.o.edit.as_ref().is_some_and(|e| e.is_open()))
                .then(|| Rectangle::new(self.bar_at(), self.dock_size()));
            let local = |near| {
                let at = pill_at((origin, screen, scale, near), size, inset);
                let px = |v: i32| v as f32 / scale;
                Point::new(px(at.origin.0), px(at.origin.1)) - (origin - Point::ORIGIN)
            };
            let mut at = local(match (place.3, toolbar) {
                (Some(near), Some(toolbar)) => Some(near.union(&toolbar)),
                (near, toolbar) => near.or(toolbar),
            });
            if let Some(toolbar) = toolbar
                && Rectangle::new(at, size).intersects(&toolbar)
            {
                at = local(Some(toolbar));
            }
            self.o.result = Some((text, ok, at));
            return Self::hold_then_close();
        }
        let at = pill_at(place, size, inset);
        self.windows
            .toast(&text, ok, Some(at), windows::TOAST_MS)
            .map(Message::Windows)
    }

    /// Runs a quick action on the selection (no action bar). Only once: a new
    /// selection after it gets the bar.
    pub(crate) fn run_quick(&mut self, quick: Quick) -> Task<Message> {
        self.o.purpose = Purpose::Search;
        match quick {
            Quick::Action(action) => self.run(action),
            // No OCR models yet: the download card asks first (in place), then
            // the text is copied once it is Done (`Reader` in update.rs).
            Quick::CopyText if !text::models_present() => {
                self.o.copy_after = true;
                self.o.reader = Reader::Missing(crate::features::text_reading::download_size());
                Task::none()
            }
            Quick::CopyText => {
                let Some(crop) = self.stacked() else {
                    return Task::none();
                };
                self.o.toast = Some(Said(S::bar_reading));
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
                Some(crop) => self.open_chat(
                    crop,
                    Some(
                        crate::features::ask_ai::ask::PRESETS[0]
                            .text(&config::current().translate.target),
                    ),
                ),
                None => Task::none(),
            },
        }
    }

    /// The music button: song search with a key (`has_key`); without one its
    /// setup (Settings > Song search), and nothing is recorded.
    pub(crate) fn song(&mut self, has_key: bool) -> Task<Message> {
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
    pub(crate) fn start_song(&mut self) -> Task<Message> {
        self.songs = self.songs.wrapping_add(1);
        let id = self.songs;
        // The song's card takes the result card's turn: one card at a time.
        self.o.ai_chat = None;
        let (session, work) = listen::Session::start();
        self.o.song = Some((id, session));
        work.map(move |msg| Message::Song(id, msg))
    }

    pub(crate) fn on_song(&mut self, id: u32, msg: listen::Msg) -> Task<Message> {
        let Some((current, session)) = &mut self.o.song else {
            return Task::none();
        };
        if *current != id {
            return Task::none();
        }
        match session.update(msg) {
            listen::Outcome::None => Task::none(),
            listen::Outcome::Close => {
                self.o.song = None;
                Task::none()
            }
            listen::Outcome::Retry => self.start_song(),
            // The browser comes to the front: the overlay closes so it isn't on top.
            listen::Outcome::Open(url) => {
                let open = background(move |_: &UnboundedSender<()>| {
                    if let Err(e) = crate::shared::platform::sys::open::open_link(&url) {
                        crate::log!("[opencircle] can't open the link: {e}");
                    }
                })
                .discard();
                Task::batch([open, self.close()])
            }
        }
    }

    /// The music button shows only once a key is known to be saved.
    pub(crate) fn has_song_key(&self) -> bool {
        crate::features::song::key_known() == Some(true)
    }

    /// First open: ask the keyring (off the UI thread) whether a song key is saved.
    pub(crate) fn check_song_key(&self) -> Task<Message> {
        if crate::features::song::key_known().is_some() || cfg!(test) {
            return Task::none();
        }
        background(|tx| {
            let _ = tx.unbounded_send(crate::features::song::check_key());
        })
        .map(|_| Message::SongKey)
    }

    /// Translate in place: the selection (or, with no selection left, the whole
    /// frame) is translated on a thread, then swapped in.
    pub(crate) fn start_translate(&mut self) -> Task<Message> {
        let frame = self.o.frame.clone();
        let (Some(frame @ image::Handle::Rgba { width, height, .. }), Some(selection)) =
            (&frame, self.o.selection)
        else {
            return Task::none();
        };
        let Some(area) = picture::crop_bounds(*width, *height, self.o.scale, selection.area) else {
            return Task::none();
        };
        self.o.ai_chat = None;
        self.o.edit = None;
        self.o.view = self.o.view.reset(); // the translation is drawn at 1x
        self.o.zoom = None;
        let models = text::models_present();
        let (session, task) = pill::Session::start(
            frame.clone(),
            area,
            self.o.scale,
            self.cache.clone(),
            models,
            (self.o.work_inset * self.o.scale).round() as u32,
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

    pub(crate) fn on_translate(&mut self, id: u32, msg: pill::Msg) -> Task<Message> {
        let Some((_, session)) = self.o.translate.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            pill::Msg::Card(card::Event::Grab(how)) => {
                self.o.chat_drag = Some((how, None));
                self.frame_times.borrow_mut().clear();
                Task::none()
            }
            pill::Msg::Card(card::Event::Hover(on)) => {
                self.o.chat_hover = on;
                Task::none()
            }
            pill::Msg::Card(card::Event::Reset) => {
                self.card = None;
                Task::none()
            }
            // (the cards have no close button: Esc, or a click on the screen)
            pill::Msg::Close => self.close(),
            pill::Msg::Snip => self.snip_here(None),
            pill::Msg::OpenSettings => {
                let closing = self.close();
                let settings = self.windows.open(windows::Kind::Settings);
                closing.chain(settings.map(Message::Windows))
            }
            // The word card goes on as an Ask AI chat (its crop, its explanation).
            pill::Msg::Ask => match session.take_chat() {
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
                    let (panel, task) = ai_chat::AiChat::open_chat(crop, chat, self.o.include);
                    self.o.ai_chat = Some((id, panel));
                    self.keep_chat();
                    task.map(move |msg| Message::AiChat(id, msg))
                }
                None => Task::none(),
            },
            msg => {
                let task = session
                    .update(msg)
                    .map(move |msg| Message::Translate(id, msg));
                // Snip's More: the translation goes into the picture under the drawing
                self.sync_translation();
                task
            }
        }
    }

    /// Ctrl+Shift+L (or the tray): live translate on, or off again.
    pub(crate) fn toggle_live(&mut self, at: Instant) -> Task<Message> {
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

    pub(crate) fn on_live(&mut self, msg: live_layer::Msg) -> Task<Message> {
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
    pub(crate) fn translate_once(&mut self, at: Instant) -> Task<Message> {
        let once = QuickAction {
            action: crate::features::translate::action::ACTION.id.into(),
            shortcut: String::new(),
            target: Target::FullScreen,
            live: false,
        };
        self.update(Message::Trigger(Trigger::Quick(at, once)))
    }
}

/// Where the Copy / Save pill of `size` goes for an overlay at `place`
/// (its origin, size and scale, logical px; what it goes beside, overlay px),
/// `inset` px of taskbar under it: centred below what it goes beside, else
/// above it, kept `TOAST_MARGIN` inside the screen; nothing to go beside:
/// bottom right. In physical desktop px, so it lands there at any scale and
/// on any monitor (#149).
pub(crate) fn pill_at(
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
