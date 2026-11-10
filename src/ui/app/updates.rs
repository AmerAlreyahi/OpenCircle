//! The update experience (owner, 2026-11-10): a waiting update's pill, tray dot
//! and row, Settings status; Restart now; the install at a lock; What's new or
//! the "Updated" pill in the new process. The decisions are `update::flow`'s.

use std::time::{Duration, Instant};

use iced::Task;

use crate::ui::app::background::background;
use crate::ui::app::message::{Hiding, Message};
use crate::ui::screens::update_pill::{self, Kind, Pill};
use crate::ui::{update_card, windows};
use crate::update::{Event, How, Installed, Moment, Step};

use super::{App, Waiting};

/// The update clock: every `TICK` while a pill may show or the session may lock,
/// every `TICK_SHOWN` while a pill shows (it fades on time).
const TICK: Duration = Duration::from_secs(15);
const TICK_SHOWN: Duration = Duration::from_secs(5);
/// The Updating pill shows this long before this process exits into the new one
/// (and before the new one opens What's new).
const UPDATING_FOR: Duration = Duration::from_millis(1200);

impl App {
    /// From the update thread.
    pub(in crate::ui) fn on_update(&mut self, event: Event) -> Task<Message> {
        let waiting = match event {
            Event::Status(status) => {
                return self
                    .windows
                    .update(windows::Msg::UpdateStatus(status))
                    .map(Message::Windows);
            }
            Event::Manual { version, url, .. } => Waiting {
                version,
                url,
                how: How::GetIt,
                staged: None,
                pill: Default::default(),
                lock: Default::default(),
            },
            Event::Ready(staged) => Waiting {
                version: staged.version.clone(),
                url: staged.url.clone(),
                how: How::of(&staged.install),
                staged: Some(staged),
                pill: Default::default(),
                lock: Default::default(),
            },
        };
        crate::log!(
            "[opencircle] update {} waits ({:?})",
            waiting.version,
            waiting.how
        );
        self.update = Some(waiting);
        let accent = crate::ui::kit::themes::c().accent.into_rgba8();
        crate::ui::platform::tray::show_update(Some([accent[0], accent[1], accent[2]]));
        self.windows.refresh_update();
        self.update_tick()
    }

    /// Nothing of OpenCircle is in use: no overlay (editor, snip, panel), capture,
    /// live translate, Settings, onboarding or pinned image.
    pub(in crate::ui) fn is_idle(&self) -> bool {
        self.o.is_none()
            && self.chat.is_none()
            && self.capturing.is_none()
            && self.live.is_none()
            && self.hiding == Hiding::No
            && !self.windows.busy()
    }

    /// The update clock: the pills wait while the user is away and fade on time;
    /// a swap install restarts into its update once the session has been locked
    /// a minute with nothing open.
    pub(in crate::ui) fn update_tick(&mut self) -> Task<Message> {
        let now = Instant::now();
        let locked = crate::sys::session::locked();
        let away = crate::update::away(locked, crate::sys::session::idle_for());
        let mut tasks = Vec::new();
        let idle = self.is_idle();
        if let Some(w) = &mut self.update {
            match w.pill.step(now, away) {
                Step::Show => {
                    let pill = Pill::new(Kind::Ready {
                        version: w.version.clone(),
                        how: w.how,
                    });
                    tasks.push(self.windows.update_pill(Some(pill)).map(Message::Windows));
                }
                Step::Hide => tasks.push(self.close_pill(|k| matches!(k, Kind::Ready { .. }))),
                Step::Nothing => {}
            }
            let w = self.update.as_mut().expect("still waiting");
            let lock_moment = w
                .staged
                .as_ref()
                .is_some_and(|s| crate::update::installs_at(&s.install, Moment::Locked));
            if lock_moment && w.lock.long_enough(now, locked) && idle {
                let staged = w.staged.clone().expect("checked");
                match crate::update::restart(&staged, Moment::Locked) {
                    Ok(()) => return iced::exit(),
                    Err(e) => crate::log!("[opencircle] update {}: {e}", staged.version),
                }
            }
        }
        if let Some((pill, installed)) = &mut self.updated {
            match pill.step(now, away) {
                Step::Show => {
                    let pill = Pill::new(Kind::Updated {
                        version: installed.version.clone(),
                    });
                    tasks.push(self.windows.update_pill(Some(pill)).map(Message::Windows));
                }
                Step::Hide => tasks.push(self.close_pill(|k| matches!(k, Kind::Updated { .. }))),
                Step::Nothing => {}
            }
        }
        let shown = |p: &crate::update::Pill| p.is_shown();
        let more = self.update.is_some()
            || (self.updated.as_ref()).is_some_and(|(p, _)| *p != crate::update::Pill::Done);
        let pace = match (self.update.as_ref().is_some_and(|w| shown(&w.pill)))
            || (self.updated.as_ref()).is_some_and(|(p, _)| shown(p))
        {
            true => TICK_SHOWN,
            false => TICK,
        };
        self.update_ticks = more;
        if more {
            tasks.push(after(pace, Message::UpdateTick));
        }
        Task::batch(tasks)
    }

    /// Quit: a swap install's update is in place already (`update::prepare`), so
    /// it is what starts next; a setup's waits for Restart now (never a prompt
    /// at quit). Says which in the log.
    pub(in crate::ui) fn on_quit(&self) {
        let Some(w) = &self.update else { return };
        let at_quit = (w.staged.as_ref())
            .is_some_and(|s| crate::update::installs_at(&s.install, Moment::Quit));
        match at_quit {
            true => crate::log!(
                "[opencircle] update {}: starts with the next run",
                w.version
            ),
            false => crate::log!("[opencircle] update {}: waits for Restart now", w.version),
        }
    }

    /// Starts the update clock unless it runs.
    fn start_ticks(&mut self) -> Task<Message> {
        match self.update_ticks {
            true => Task::none(),
            false => self.update_tick(),
        }
    }

    /// The open pill goes if it is one `of` these.
    fn close_pill(&mut self, of: impl Fn(&Kind) -> bool) -> Task<Message> {
        match self.windows.shown_pill().is_some_and(of) {
            true => self.windows.update_pill(None).map(Message::Windows),
            false => Task::none(),
        }
    }

    /// A pill's button.
    pub(in crate::ui) fn on_update_pill(&mut self, msg: update_pill::Msg) -> Task<Message> {
        match msg {
            update_pill::Msg::Later => {
                if let Some(w) = &mut self.update {
                    w.pill.dismiss(); // the dot, the tray row and Settings stay
                }
                self.windows.update_pill(None).map(Message::Windows)
            }
            update_pill::Msg::Main => self.restart_to_update(),
            update_pill::Msg::WhatsNew => {
                let Some((pill, i)) = &mut self.updated else {
                    return Task::none();
                };
                pill.dismiss();
                let card = update_card::Card::installed(&i.version, &i.notes, &i.url);
                Task::batch([
                    self.windows.update_pill(None).map(Message::Windows),
                    self.windows.update_card(card).map(Message::Windows),
                ])
            }
        }
    }

    /// Restart now (the pill, the tray row, Settings): the Updating pill shows
    /// and this process exits into the new version; a setup's copy asks Windows
    /// once (declined: nothing changes, it can be asked again). "Get it": the
    /// release page.
    pub(in crate::ui) fn restart_to_update(&mut self) -> Task<Message> {
        let Some(w) = &mut self.update else {
            return Task::none();
        };
        w.pill.dismiss();
        let Some(staged) = w.staged.clone() else {
            crate::ui::windows::open_url(&w.url);
            return self.windows.update_pill(None).map(Message::Windows);
        };
        match crate::update::restart(&staged, Moment::Button) {
            Ok(()) => {
                // the clock starts once the pill's window is there (GNOME on Wayland
                // maps a new window late: it would never show)
                let pill = Pill::new(Kind::Updating);
                (self.windows.update_pill(Some(pill)).map(Message::Windows))
                    .chain(after(UPDATING_FOR, ()).then(|()| iced::exit()))
            }
            Err(e) => {
                crate::log!("[opencircle] update {}: {e}", staged.version);
                self.windows.update_pill(None).map(Message::Windows)
            }
        }
    }

    /// This process started with an update: after Restart now, the Updating
    /// pill a moment longer, then What's new; after a quiet install (a quit, a
    /// lock, a reboot) its "Updated" pill once, when the user is there.
    pub(in crate::ui) fn on_installed(&mut self, installed: Installed) -> Task<Message> {
        if installed.quiet {
            self.updated = Some((Default::default(), installed));
            return self.start_ticks();
        }
        let card =
            update_card::Card::installed(&installed.version, &installed.notes, &installed.url);
        let pill = self.windows.update_pill(Some(Pill::new(Kind::Updating)));
        Task::batch([
            pill.map(Message::Windows),
            after(
                UPDATING_FOR,
                Message::Windows(windows::Msg::ShowCard(Box::new(card))),
            ),
        ])
    }
}

/// `message` after `wait` (a thread sleeps; the UI never waits).
fn after<T: Send + 'static>(wait: Duration, message: T) -> Task<T> {
    background(move |tx| {
        std::thread::sleep(wait);
        let _ = tx.unbounded_send(message);
    })
}
