//! Recording (R-02, R-04), the App's part: the bar's and the thumbnail's messages go to the state
//! machine (`record::state`), its effects to the real recorder (`record::real`; the fake one in
//! tests that only drive the bar), and what
//! it says to the windows. Where the controls live (the bar's window, or the tray where the whole
//! screen is recorded and our window cannot be kept out of it) is `record::placement`'s rule.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use iced::{Point, Rectangle, Size, Task};

use super::tools;
use crate::app::App;
use crate::app::message::Message;
use crate::app::screens::toast::Tone;
use crate::features::snip::record::clock::{self, Clock};
use crate::features::snip::record::fake::Fake;
use crate::features::snip::record::files;
use crate::features::snip::record::placement::{self, Ctx, Row, Surface, Target};
use crate::features::snip::record::real::{Failure, Flushed, Real, Request};
use crate::features::snip::record::state::{Effect, Event, Fault, State};
use crate::features::snip::screens::record_bar::{self, Bar, Phase};
use crate::features::snip::screens::record_result::{self as thumb, Thumb};
use crate::shared::platform::{self, CAN_KEEP_WINDOWS_OUT_OF_CAPTURE};
use crate::t;

/// The recording the App holds (`App::rec`): its state, clock and what shows it.
pub(crate) struct Recording {
    pub(crate) state: State,
    clock: Clock,
    fake: Fake,
    /// The real recorder (none: the fake one answers, as in the tests of the bar).
    real: Option<Real>,
    /// The area being recorded (physical desktop px), video.
    region: Option<(i32, i32, u32, u32)>,
    /// When the countdown last ticked.
    count_at: Instant,
    /// Time zero of `clock` and the beat.
    t0: Instant,
    last_beat: Instant,
    /// What the bar shows (the window gets a copy at every change).
    bar: Bar,
    target: Target,
    surface: Surface,
    screen: Rectangle,
    /// The finished file, until it is saved or deleted.
    file: Option<PathBuf>,
    /// The file is in use (copied to the clipboard, opened elsewhere): it stays where it is.
    in_use: bool,
    /// The tray's last label (the tray is told only when it changes).
    tray: Option<String>,
    /// The ids of the microphones the bar lists (same order as `bar.mics`).
    mic_ids: Vec<String>,
    /// The recorder's worker is closing the file: its answer comes here.
    flushing: Option<Receiver<Flushed>>,
    /// The input raised a write failure: the end says so instead of a silent success.
    write_failed: bool,
    /// The thumbnail offers a recording an earlier run left: it is never deleted, only kept.
    offered: bool,
    /// A recording whose thumbnail slid away with Recent off: still on disk until the
    /// "Recording discarded . Undo" pill `.0` is over (`Message::RecDiscardGone`).
    discard: Option<(u32, PathBuf, bool)>, // (pill number, file, audio only)
    discards: u32,
}

impl Default for Recording {
    fn default() -> Self {
        Recording {
            state: State::Idle,
            clock: Clock::default(),
            fake: Fake::default(),
            real: None,
            region: None,
            count_at: Instant::now(),
            t0: Instant::now(),
            last_beat: Instant::now(),
            bar: Bar::new(false),
            target: Target::WholeScreen,
            surface: Surface::Bar,
            screen: Rectangle::new(Point::ORIGIN, Size::new(1920.0, 1080.0)),
            file: None,
            in_use: false,
            tray: None,
            mic_ids: Vec::new(),
            flushing: None,
            write_failed: false,
            offered: false,
            discard: None,
            discards: 0,
        }
    }
}

/// What the thumbnail's actions do (all routed to `App::record_action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Copy,
    Save,
    Edit,
    OpenWith,
}

/// What a press-and-drag on the thumbnail does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragOut {
    /// This platform cannot drag a file out.
    Unsupported,
    /// It can, but there is no file yet.
    WaitsForFile,
    Go,
}

fn drag_out(supported: bool, has_file: bool) -> DragOut {
    match (supported, has_file) {
        (false, _) => DragOut::Unsupported,
        (true, false) => DragOut::WaitsForFile,
        (true, true) => DragOut::Go,
    }
}

impl App {
    /// The first monitor, logical px (a recording's screen until the pick says another).
    fn record_screen() -> Rectangle {
        let places = platform::monitor::places();
        match places.first() {
            Some(p) => Rectangle::new(p.logical_origin(), p.size()),
            None => Rectangle::new(Point::ORIGIN, Size::new(1920.0, 1080.0)),
        }
    }

    /// Starts a recording of `target` (audio only: no pick). The pick, the desktop's question
    /// and the countdown are not drawn yet (R-04): the bar waits in its Ready state for Record.
    /// Its callers are R-04's start keys (and the tests).
    pub(crate) fn record_start(&mut self, audio_only: bool, target: Target) -> Task<Message> {
        self.record_start_on(audio_only, target, CAN_KEEP_WINDOWS_OUT_OF_CAPTURE)
    }

    /// Starts a REAL recording (`Request::audio_only` or the picked `region`, physical px); the
    /// pick and the countdown come before it for video (`Ready`, then Record).
    pub(crate) fn record_begin(
        &mut self,
        audio_only: bool,
        target: Target,
        region: Option<(i32, i32, u32, u32)>,
    ) -> Task<Message> {
        // a pending discard is final now (its pill goes: an Undo over a new recording means nothing);
        // the card of the last one is moved to Recent, or kept for the next start's offer
        let closed = Task::batch([self.record_discard_now(), self.record_unsaved(false, false)]);
        self.rec.real = Some(Real::default());
        self.rec.region = region;
        self.rec.in_use = false;
        self.rec.offered = false;
        self.rec.write_failed = false;
        let task = Task::batch([closed, self.record_start(audio_only, target)]);
        if let Some((_, _, w, h)) = region {
            // what is written (scaled over the encoders' limit, never cut)
            let v = crate::shared::platform::encode::VideoFmt::of(w & !1, h & !1, 30);
            self.rec.bar.size = (v.width, v.height);
            return Task::batch([task, self.record_show(Instant::now())]);
        }
        task
    }

    /// `record_start` as an OS that can (`can_keep_out`) or cannot keep our window out of the capture.
    fn record_start_on(
        &mut self,
        audio_only: bool,
        target: Target,
        can_keep_out: bool,
    ) -> Task<Message> {
        let rec = &mut self.rec;
        rec.target = target;
        rec.surface = placement::surface(can_keep_out, target);
        rec.screen = Self::record_screen();
        rec.bar = Bar::new(audio_only);
        let mics = platform::audio::devices(platform::audio::Kind::Mic(None)).unwrap_or_default();
        rec.mic_ids = mics.iter().map(|d| d.id.clone()).collect();
        rec.bar.mics = mics.into_iter().map(|d| d.name).collect();
        rec.bar.mic = (!rec.bar.mics.is_empty()).then_some(0);
        rec.bar.sound = platform::audio::devices(platform::audio::Kind::System)
            .ok()
            .and_then(|d| d.into_iter().next())
            .map(|d| d.name);
        rec.t0 = Instant::now();
        rec.last_beat = rec.t0;
        let ev = match audio_only {
            true => Event::StartAudio,
            false => Event::StartVideo,
        };
        self.record_event(ev, Instant::now())
    }

    /// The record-video / record-audio key (`audio`), or the SAME key again: stop and save.
    /// Audio starts at once; video opens Snip's pick (area, window or screen), then the Ready
    /// bar and the countdown (`record_picked`). The other kind's key during a recording does
    /// nothing (Alt+Shift+M never stops a video).
    pub(crate) fn record_key(&mut self, audio: bool, at: Instant) -> Task<Message> {
        match self.rec.state {
            State::Idle | State::Result { .. } | State::Error(_) if audio => {
                self.record_begin(true, Target::WholeScreen, None)
            }
            State::Idle | State::Result { .. } | State::Error(_) => {
                // a card of the last one may still be up: Recent, or kept for the next start
                let closed =
                    Task::batch([self.record_discard_now(), self.record_unsaved(false, false)]);
                self.next = crate::app::message::Purpose::Search;
                self.live_next = false;
                let task =
                    self.capture(at, Some(crate::features::snip::screens::pick::Kind::Region));
                self.record_next = true; // (after: every capture clears it)
                Task::batch([closed, task])
            }
            // the key of what runs: stop and save (the countdown swallows it)
            State::Recording { audio_only }
            | State::Paused { audio_only }
            | State::AskKeep { audio_only, .. }
                if audio_only == audio =>
            {
                self.record_event(Event::Key, at)
            }
            _ => Task::none(),
        }
    }

    /// The pick is done: the overlay closes and the bar waits with Record (then 3, 2, 1).
    pub(crate) fn record_picked(&mut self, area: Rectangle) -> Task<Message> {
        let Some((on, region)) = self.pin_place(area) else {
            return self.close();
        };
        let whole = on.place.size() == area.size();
        let target = if whole {
            Target::WholeScreen
        } else {
            Target::Area
        };
        let closed = self.close();
        let begun = self.record_begin(false, target, Some(region));
        Task::batch([closed, begun])
    }

    /// Esc while recording: stop and ask Keep / Discard.
    pub(crate) fn record_esc(&mut self) -> Task<Message> {
        self.record_event(Event::Esc, Instant::now())
    }

    /// One event into the machine, its effect into the fake, the answer back in; then the screen.
    fn record_event(&mut self, ev: Event, at: Instant) -> Task<Message> {
        let leaving = matches!(self.rec.state, State::Result { .. });
        let audio_only = matches!(self.rec.state, State::Result { audio_only: true });
        let rec = &mut self.rec;
        let (next, effect) = rec.state.on(ev);
        rec.state = next;
        let discarded = match leaving && next == State::Idle {
            true => self.record_unsaved(true, audio_only),
            false => Task::none(),
        };
        let rec = &mut self.rec;
        let now = at.saturating_duration_since(rec.t0);
        match effect {
            Some(Effect::Begin) => rec.clock = Clock::start(now),
            Some(Effect::Pause) => rec.clock.pause(now),
            Some(Effect::Resume) => rec.clock.resume(now),
            _ => {}
        }
        let answer = effect.and_then(|e| match rec.real.is_some() {
            true => Self::run_real(rec, e),
            false => rec.fake.run(e),
        });
        let shown = Task::batch([discarded, self.record_show(at)]);
        match answer {
            Some(ev) => Task::batch([shown, self.record_event(ev, at)]),
            None => shown,
        }
    }

    /// What the real recorder does for an effect, and what it answers. Nothing here waits: the
    /// file is closed on a worker (`Real::flush_async`), its end comes in `record_flushed`.
    fn run_real(rec: &mut Recording, effect: Effect) -> Option<Event> {
        let (audio_only, region) = (rec.bar.audio_only, rec.region);
        let mic = rec
            .bar
            .mic
            .map(|i| Some(rec.mic_ids.get(i).cloned()).flatten());
        let req = Request {
            audio_only,
            region,
            mic,
            sound: rec.bar.sound_on,
            mic_muted: rec.bar.muted,
            fps: rec.bar.fps,
        };
        let real = rec.real.as_mut()?;
        match effect {
            Effect::Begin => match real.begin(&req) {
                Ok(()) => None, // (the mute state was the session's start state)
                Err(Failure { fault, note }) => {
                    rec.bar.note = note;
                    Some(Event::StartFailed(fault))
                }
            },
            Effect::Pause => {
                real.pause();
                None
            }
            Effect::Resume => {
                real.resume();
                None
            }
            Effect::Flush => match real.flush_async() {
                Some(rx) => {
                    rec.flushing = Some(rx);
                    None
                }
                None => Some(Event::Flushed),
            },
            Effect::DeleteTemp => {
                real.discard();
                None
            }
        }
    }

    /// The worker closed the file: the thumbnail, or the card that says what went wrong (what
    /// is valid stays where it is: offered at the next start).
    fn record_flushed(&mut self, done: Flushed, at: Instant) -> Task<Message> {
        self.rec.file = done.file;
        let failed = done.error.is_some() || self.rec.write_failed;
        if failed {
            self.rec.in_use = true; // never routed to Recent / deleted: it is what we have
            return self.record_event(Event::FlushFailed(Fault::WriteFailed), at);
        }
        self.record_event(Event::Flushed, at)
    }

    /// The windows (or the tray) as the state says.
    fn record_show(&mut self, at: Instant) -> Task<Message> {
        self.windows.record_beat = self.record_beating();
        let rec = &mut self.rec;
        let now = at.saturating_duration_since(rec.t0);
        let elapsed = rec.clock.elapsed(now);
        let (phase, ask) = match rec.state {
            State::Picking | State::Asking | State::Countdown(_) => (Some(Phase::Ready), false),
            State::Recording { .. } => (Some(Phase::Recording), false),
            State::Paused { .. } => (Some(Phase::Paused), false),
            State::AskKeep { was_paused, .. } => (
                Some(match was_paused {
                    true => Phase::Paused,
                    false => Phase::Recording,
                }),
                true,
            ),
            State::Stopping { .. } => (Some(Phase::Saving), false),
            State::Error(f) => (Some(Phase::Error(f)), false),
            State::Idle | State::Result { .. } => (None, false),
        };
        let screen = rec.screen;
        let mut tasks = Vec::new();
        match (phase, rec.surface) {
            (Some(phase), Surface::Bar) => {
                let b = &mut rec.bar;
                b.phase = phase;
                b.ask = ask;
                b.elapsed = elapsed;
                b.menu &= matches!(phase, Phase::Recording | Phase::Paused);
                b.count = match rec.state {
                    State::Countdown(n) => Some(n),
                    _ => None,
                };
                b.levels = match (b.audio_only, &rec.real) {
                    (false, _) => Vec::new(),
                    (true, None) => Fake::levels(elapsed),
                    // the real level moves the same bars
                    (true, Some(real)) => {
                        let gain = (0.12 + real.level() * 3.0).min(1.0);
                        (Fake::levels(elapsed).into_iter())
                            .map(|v| (v * gain).clamp(0.05, 1.0))
                            .collect()
                    }
                };
                let bar = b.clone();
                tasks.push(self.windows.show_record_bar(bar, screen));
            }
            (Some(phase), Surface::Tray) => self.record_tray(phase, elapsed),
            (None, _) => {
                tasks.push(self.windows.close_record_bar());
                self.record_tray_off();
            }
        }
        match self.rec.state {
            State::Result { audio_only } => {
                let mut t = Thumb::new(audio_only, self.rec.clock.elapsed(now));
                if audio_only {
                    t.wave = Fake::wave(30);
                }
                tasks.push(self.windows.show_record_thumb(t, screen));
            }
            State::Idle => tasks.push(self.windows.close_record_thumb()),
            _ => {}
        }
        Task::batch(tasks).map(Message::Windows)
    }

    /// The tray as the recording's bar: the dot and time while it runs, the rows by the state.
    fn record_tray(&mut self, phase: Phase, elapsed: Duration) {
        let muted = self.rec.bar.muted;
        let label = format!("● {}", clock::format(elapsed));
        let rows = placement::tray_rows(self.rec.state);
        let key = format!(
            "{label}{rows:?}{muted}{}",
            self.rec.bar.mic.unwrap_or(usize::MAX)
        );
        if self.rec.tray.as_ref() == Some(&key) {
            return;
        }
        self.rec.tray = Some(key);
        let bar = &self.rec.bar;
        let wanted = if bar.audio_only {
            "record-audio"
        } else {
            "record-video"
        };
        let key = crate::shared::config::current()
            .quick_key(wanted, false, None)
            .unwrap_or_default();
        let ctx = Ctx {
            key,
            muted,
            audio_only: bar.audio_only,
            device: bar
                .mic
                .and_then(|i| bar.mics.get(i).cloned())
                .unwrap_or_default(),
        };
        let rows = (rows.into_iter().map(|r| (r, r.label(&ctx)))).collect();
        platform::tray::recording((phase != Phase::Saving).then_some((label, rows)));
    }

    fn record_tray_off(&mut self) {
        if self.rec.tray.take().is_some() {
            platform::tray::recording(None);
        }
    }

    /// The bar's own messages.
    pub(crate) fn on_record_bar(&mut self, msg: record_bar::Msg) -> Task<Message> {
        use record_bar::Msg as M;
        let at = Instant::now();
        let ev = match msg {
            M::Grab => return self.windows.grab_record_bar().map(Message::Windows),
            // Record: 3, 2, 1 (a press during the countdown skips it); the fake recorder (the
            // bar's own tests) starts at once
            M::Record => {
                let ready = matches!(self.rec.state, State::Picking | State::Asking);
                let picked = match ready {
                    true => self.record_event(Event::Picked { ask: false }, at),
                    false => Task::none(),
                };
                self.rec.count_at = at;
                return match self.rec.real.is_some() && ready {
                    true => picked,
                    false => Task::batch([picked, self.record_event(Event::Skip, at)]),
                };
            }
            M::Cancel => Event::Esc,
            M::Pause => Event::Pause,
            M::Resume => Event::Resume,
            M::Stop => Event::Stop,
            M::Discard => Event::Discard,
            M::Keep => Event::Keep,
            M::Menu => {
                self.rec.bar.menu = !self.rec.bar.menu;
                return self.record_show(at);
            }
            // "Off" mutes (the mic is kept, to come back to); a microphone unmutes
            M::Mic(i) => {
                self.rec.bar.muted = i.is_none();
                self.rec.bar.mic = i.or(self.rec.bar.mic);
                self.rec.bar.menu = false;
                self.rec.tray = None;
                if let Some(real) = &self.rec.real {
                    real.mute_mic(self.rec.bar.muted);
                }
                return self.record_show(at);
            }
            M::Sound(on) => {
                self.rec.bar.sound_on = on;
                if let Some(real) = &self.rec.real {
                    real.mute_sound(!on);
                }
                return self.record_show(at);
            }
            // the error card's buttons: without the microphone / again, from the start
            M::Fix | M::Retry => {
                if msg == M::Fix {
                    self.rec.bar.mic = None;
                }
                let audio_only = self.rec.bar.audio_only;
                let target = self.rec.target;
                let (bar, mic) = (self.rec.bar.clone(), self.rec.bar.mic);
                let gone = self.record_event(Event::Dismiss, at);
                let again = match self.rec.real.is_some() {
                    true => self.record_begin(audio_only, target, self.rec.region),
                    false => self.record_start(audio_only, target),
                };
                // the same devices as before, the chosen one off or on
                self.rec.bar.mics = bar.mics;
                self.rec.bar.mic = mic;
                return Task::batch([gone, again]);
            }
        };
        self.record_event(ev, at)
    }

    fn record_mute(&mut self, at: Instant) -> Task<Message> {
        self.rec.bar.muted = !self.rec.bar.muted;
        if let Some(real) = &self.rec.real {
            real.mute_mic(self.rec.bar.muted);
        }
        self.rec.tray = None; // the mic row's words changed
        self.record_show(at)
    }

    /// A row of the tray's recording menu.
    pub(crate) fn on_record_row(&mut self, row: Row) -> Task<Message> {
        let at = Instant::now();
        match row {
            Row::Pause => self.record_event(Event::Pause, at),
            Row::Resume => self.record_event(Event::Resume, at),
            Row::Stop => self.record_event(Event::Stop, at),
            Row::Discard => self.record_event(Event::Discard, at),
            Row::Mic => self.record_mute(at),
            Row::Record => self.on_record_bar(record_bar::Msg::Record),
            Row::Cancel => self.on_record_bar(record_bar::Msg::Cancel),
            Row::Keep => self.on_record_bar(record_bar::Msg::Keep),
            Row::Retry => self.on_record_bar(record_bar::Msg::Retry),
            Row::Fix => self.on_record_bar(record_bar::Msg::Fix),
            Row::Dismiss => self.record_event(Event::Dismiss, at),
            // notes: nothing to do
            Row::Device | Row::ErrMic | Row::ErrRefused | Row::ErrEncoder | Row::ErrWrite => {
                Task::none()
            }
        }
    }

    /// The thumbnail's messages.
    pub(crate) fn on_record_result(&mut self, msg: thumb::Msg) -> Task<Message> {
        let act = match msg {
            thumb::Msg::Hover(_) => {
                self.windows.on_record_result(msg);
                return Task::none();
            }
            thumb::Msg::DragOut => return self.record_drag_out(),
            thumb::Msg::Copy => Act::Copy,
            thumb::Msg::Save => Act::Save,
            thumb::Msg::Edit => Act::Edit,
            thumb::Msg::OpenWith => Act::OpenWith,
        };
        self.record_action(act)
    }

    /// Copy / Save / Edit / Open with, the one place. Copy puts the file on the clipboard (it
    /// stays where it is, the clipboard points at it), Save moves it to the save folder, Open
    /// with asks the system. Edit in the Studio waits for the Studio's trim (S1, another branch).
    /// The thumbnail goes after it.
    fn record_action(&mut self, act: Act) -> Task<Message> {
        let Some(file) = self.rec.file.clone() else {
            crate::log!("[opencircle] recording {act:?}: there is no file");
            return self.record_event(Event::Dismiss, Instant::now());
        };
        match act {
            Act::Copy => {
                if tools::copy_file(&file) {
                    self.rec.in_use = true;
                }
            }
            Act::Save => return self.record_save(tools::save_folder()),
            Act::OpenWith => {
                self.rec.in_use = true;
                // the dialog blocks on Windows: off the UI thread
                std::thread::spawn(move || {
                    if let Err(e) = platform::sys::open::open_with(&file) {
                        crate::log!("[opencircle] Open with failed: {e}");
                    }
                });
            }
            Act::Edit => {
                // (the Studio's trim is another branch's: until it exists the file is treated
                // like any unsaved one)
                crate::log!("[opencircle] recording Edit: waits for the Studio's trim");
            }
        }
        self.record_event(Event::Dismiss, Instant::now())
    }

    /// Save: the file moves into `folder` (None: there is none). A failure keeps the card up and
    /// the file untouched (Save again is the retry): a failed save never loses a recording.
    fn record_save(&mut self, folder: Option<PathBuf>) -> Task<Message> {
        let Some(file) = self.rec.file.clone() else {
            return Task::none();
        };
        let ext = file
            .extension()
            .map_or("mp4".into(), |e| e.to_string_lossy());
        // the pictures' name (`OpenCircle-2026-10-26_19-37-28.mp4`): one function for both
        let name = format!("{}.{ext}", crate::features::circle::stamped_name(""));
        let moved = folder
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
            .and_then(|f| {
                std::fs::create_dir_all(&f)?;
                files::save_as(&file, &files::free_name(&f, &name))
            });
        match moved {
            Ok(saved) => {
                crate::log!("[opencircle] recording saved: {}", saved.display());
                self.rec.file = None;
                self.rec.in_use = false;
                self.rec.offered = false;
                self.record_event(Event::Dismiss, Instant::now())
            }
            Err(e) => {
                crate::log!("[opencircle] recording not saved: {e}");
                // pinned: the thumbnail's slide-away must not settle (delete) a file that
                // could not be saved; it stays for the next start's offer
                self.rec.in_use = true;
                (self.windows)
                    .toast(t!(rec_save_failed), Tone::Error, None, 6000)
                    .map(Message::Windows)
            }
        }
    }

    /// Q4 (owner's recommended default): a recording nobody saved goes to Recent captures when
    /// Recent captures are on (`snip.history`: `<recent>/recordings`, pruned like the pictures), else it is deleted. One in
    /// use (copied, opened elsewhere, a failed end) stays where it is: offered at the next start.
    /// A move that fails keeps the file: nothing is deleted because something else went wrong.
    fn record_unsaved(&mut self, notify: bool, audio_only: bool) -> Task<Message> {
        let Some(file) = self.rec.file.take() else {
            return Task::none();
        };
        if self.rec.in_use || !file.exists() {
            return Task::none();
        }
        let recent = crate::shared::history::recent::recordings_dir();
        let on = crate::shared::config::current().snip.history; // Recent captures on
        let offered = std::mem::take(&mut self.rec.offered);
        if notify && !on && !offered {
            return self.record_discard_later(file, audio_only);
        }
        // (a new recording starting: with Recent off the file is never deleted without an Undo,
        // it stays in the temp folder for the next start's offer)
        let keep = offered || !notify;
        settle_unsaved(&file, recent.as_deref(), on, keep, RECENT_LIMIT);
        if !file.exists() {
            files::unmark_offered(&file);
        }
        Task::none()
    }

    /// Recent is off and nobody saved it: the file stays for `DISCARD_MS` behind the
    /// "Recording discarded . Undo" pill, and is deleted only then. Quit meanwhile deletes
    /// nothing: the next start offers it.
    fn record_discard_later(&mut self, file: PathBuf, audio_only: bool) -> Task<Message> {
        let finished = self.record_discard_now(); // one pill at a time: an older discard is final
        self.snip_undo = None; // (and one Undo: a pending Snip undo's pill is replaced below)
        self.rec.discards = self.rec.discards.wrapping_add(1);
        let n = self.rec.discards;
        self.rec.discard = Some((n, file, audio_only));
        let older = self.windows.close_action_toasts().map(Message::Windows);
        let pill = (self.windows)
            .toast_with(
                t!(rec_discarded),
                Tone::Neutral,
                None,
                DISCARD_MS,
                Some(t!(sn_undo)),
            )
            .map(Message::Windows);
        let over = crate::app::background::background(move |tx| {
            std::thread::sleep(Duration::from_millis(DISCARD_MS));
            let _ = tx.unbounded_send(());
        })
        .map(move |()| Message::RecDiscardGone(n));
        Task::batch([finished, older, pill, over])
    }

    /// A "Recording discarded" pill is up (its Undo is the pill's word).
    pub(crate) fn rec_discard_pending(&self) -> bool {
        self.rec.discard.is_some()
    }

    /// The pill is over (`n` is its number: an older pill's end changes nothing): the file goes.
    pub(crate) fn record_discard_gone(&mut self, n: u32) -> Task<Message> {
        match self.rec.discard.as_ref().is_some_and(|(k, ..)| *k == n) {
            true => self.record_discard_now(),
            false => Task::none(),
        }
    }

    /// Deletes a pending discard now (a failed delete leaves the file for the next start).
    /// Its pill goes with it (the returned task).
    pub(crate) fn record_discard_now(&mut self) -> Task<Message> {
        let Some((_, file, _)) = self.rec.discard.take() else {
            return Task::none();
        };
        if let Err(e) = std::fs::remove_file(&file) {
            crate::log!("[opencircle] recording could not be deleted: {e}");
        }
        files::unmark_offered(&file);
        self.windows.close_action_toasts().map(Message::Windows)
    }

    /// The pill's Undo: the recording is back on the thumbnail (Save, Copy, ...).
    pub(crate) fn record_discard_undo(&mut self) -> Task<Message> {
        let Some((_, file, audio_only)) = self.rec.discard.take() else {
            return Task::none();
        };
        self.rec.file = Some(file);
        self.rec.in_use = false;
        let pill = self.windows.close_action_toasts().map(Message::Windows);
        let back = self.record_event(Event::Offer { audio_only }, Instant::now());
        Task::batch([pill, back])
    }

    /// Quit and Restart-to-update: a recording that runs is stopped and its file closed (bounded
    /// wait: a stuck encoder cannot hold the exit). NOTHING is deleted or moved here, whatever
    /// Recent says: the file stays in the temp folder and the next start offers it.
    pub(crate) fn record_shutdown(&mut self) {
        let wait = Duration::from_secs(8);
        let mut done = self
            .rec
            .flushing
            .take()
            .and_then(|rx| rx.recv_timeout(wait).ok());
        if done.is_none()
            && let Some(rx) = self.rec.real.as_mut().and_then(|r| r.flush_async())
        {
            done = rx.recv_timeout(wait).ok();
        }
        if let Some(done) = done {
            self.rec.file = done.file;
        }
        self.rec.in_use = true;
    }

    /// After `record_shutdown`, when the process does NOT exit after all (a failed Restart now):
    /// the recording is over, so the state says so (the kept file's card, or idle): no bar and no
    /// timer going on over a file that is already closed.
    pub(crate) fn record_after_shutdown(&mut self) -> Task<Message> {
        if self.rec.state != State::Idle {
            let audio_only = self.rec.bar.audio_only;
            self.rec.state = match self.rec.file.is_some() {
                true => State::Result { audio_only },
                false => State::Idle,
            };
        }
        self.record_show(Instant::now())
    }

    /// Boot: the recordings an earlier run left in the temp folder. The newest is offered on the
    /// thumbnail (and marked `.offered`); the older ones go to Recent at once. A leftover that
    /// WAS offered already is settled without asking: into Recent when Recent is on and takes it,
    /// else it is offered again (asking twice beats losing it). Nothing is deleted. The ones the
    /// user discarded (a killed worker left them) are swept first.
    pub(crate) fn record_offer_leftovers(&mut self) -> Task<Message> {
        let recent = crate::shared::history::recent::recordings_dir();
        let on = crate::shared::config::current().snip.history;
        self.record_offer_leftovers_in(&files::temp_dir(), recent.as_deref(), on)
    }

    fn record_offer_leftovers_in(
        &mut self,
        dir: &Path,
        recent: Option<&Path>,
        recent_on: bool,
    ) -> Task<Message> {
        files::sweep_discarded(dir);
        if !recent_on {
            files::cap_leftovers(dir, LEFTOVERS_KEPT);
        }
        let mut fresh = Vec::new();
        for f in files::leftovers(dir) {
            if files::was_offered(&f) {
                if recent_on
                    && settle_unsaved(&f, recent, true, false, RECENT_LIMIT) == Settled::Moved
                {
                    files::unmark_offered(&f);
                    continue;
                }
                files::unmark_offered(&f); // Recent is off or cannot take it: asked again
            }
            fresh.push(f);
        }
        let Some(newest) = fresh.pop() else {
            return Task::none();
        };
        for old in fresh {
            // Recent on: into it; off or unable: it stays in temp (asked about one per start)
            settle_unsaved(&old, recent, recent_on, true, RECENT_LIMIT);
        }
        let audio_only = matches!(
            newest.extension().and_then(|e| e.to_str()),
            Some("ogg" | "m4a")
        );
        files::mark_offered(&newest);
        self.rec.file = Some(newest);
        self.rec.offered = true;
        self.rec.in_use = false;
        self.record_event(Event::Offer { audio_only }, Instant::now())
    }

    /// A press and drag on the thumbnail takes the file out into another app, where Snip's
    /// drag-out can (`dragout::SUPPORTED`). The file is R-05's: until it exists there is nothing
    /// to drag (logged); with a file the drag itself is R-05's too.
    fn record_drag_out(&mut self) -> Task<Message> {
        let supported = crate::features::snip::annotate::dragout::SUPPORTED;
        match drag_out(supported, self.rec.file.is_some()) {
            DragOut::Unsupported => {}
            DragOut::WaitsForFile => {
                crate::log!("[opencircle] recording drag-out: waits for R-05 (the file)");
            }
            // R-05: start Snip's drag-out with the file here
            DragOut::Go => crate::log!("[opencircle] recording drag-out: R-05 starts it"),
        }
        Task::none()
    }

    /// The beat: the bar's timer and the thumbnail's slide-away (`at` = when).
    pub(crate) fn on_record_tick(&mut self, at: Instant) -> Task<Message> {
        let dt = at.saturating_duration_since(self.rec.last_beat);
        self.rec.last_beat = at;
        match self.rec.state {
            State::Result { .. } => match self.windows.tick_record_thumb(dt) {
                // slid away by itself (an unsaved recording goes to Recent captures, Q4)
                (true, closed) => {
                    let idle = self.record_event(Event::Dismiss, at);
                    Task::batch([closed.map(Message::Windows), idle])
                }
                (false, task) => task.map(Message::Windows),
            },
            // 3, 2, 1: one step a second
            State::Countdown(_) if self.rec.real.is_some() => {
                match at.saturating_duration_since(self.rec.count_at) >= Duration::from_secs(1) {
                    true => {
                        self.rec.count_at = at;
                        self.record_event(Event::Tick, at)
                    }
                    false => Task::none(),
                }
            }
            // the worker closing the file: the end arrives here
            State::Stopping { .. } => match self.rec.flushing.as_ref().map(|r| r.try_recv()) {
                Some(Ok(done)) => {
                    self.rec.flushing = None;
                    self.record_flushed(done, at)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    self.rec.flushing = None;
                    self.record_flushed(
                        Flushed {
                            file: None,
                            error: Some(TryRecvError::Disconnected.to_string()),
                        },
                        at,
                    )
                }
                _ => self.record_show(at),
            },
            s if s.is_live() => {
                // the input ended by itself (disk low, stream ended, microphone gone, a write failed)
                let fault = (self.rec.real.as_ref()).and_then(|r| r.fault());
                if fault == Some(Fault::WriteFailed) {
                    self.rec.write_failed = true;
                }
                match fault {
                    Some(f) => self.record_event(Event::Fault(f), at),
                    None => self.record_show(at),
                }
            }
            _ => Task::none(),
        }
    }

    /// A recording is on (or being picked / counted down): the beat must run, bar or no bar.
    pub(crate) fn record_beating(&self) -> bool {
        !matches!(self.rec.state, State::Idle)
    }

    /// A fault, as the real recorder raises them: the tests raise one by hand.
    #[cfg(test)]
    pub(crate) fn record_fault(
        &mut self,
        f: crate::features::snip::record::state::Fault,
    ) -> Task<Message> {
        self.record_event(Event::Fault(f), Instant::now())
    }
}

/// "Recording discarded . Undo" shows this long (then the file goes).
const DISCARD_MS: u64 = 6000;

/// With Recent off, how many leftovers the temp folder keeps (the newest) at a start.
const LEFTOVERS_KEPT: usize = 5;

/// How many recordings Recent keeps (the same count as its pictures, oldest out first).
const RECENT_LIMIT: usize = crate::shared::history::recent::HISTORY;

/// Where an unsaved recording goes: into `recent` (then pruned to `limit`) when Recent is `on`;
/// when it is off nothing is ever moved there: the file is deleted, or, for one that must be
/// `keep`t (an offered leftover), left where it is. A failed move leaves the file too. What was
/// done is returned for the log and the tests.
fn settle_unsaved(
    file: &Path,
    recent: Option<&Path>,
    on: bool,
    keep: bool,
    limit: usize,
) -> Settled {
    if on {
        return match recent.map(|d| (d, files::save(file, d))) {
            Some((d, Ok(saved))) => {
                // its time is now: a moved file is the newest in Recent (a rename keeps the old
                // time, and the prune would drop it unseen)
                files::touch(&saved);
                crate::shared::history::recent::prune_recordings(d, limit);
                Settled::Moved
            }
            _ => {
                crate::log!("[opencircle] recording kept in {}", file.display());
                Settled::Kept
            }
        };
    }
    if keep {
        return Settled::Kept;
    }
    match std::fs::remove_file(file) {
        Ok(()) => Settled::Deleted,
        Err(e) => {
            crate::log!("[opencircle] recording could not be deleted: {e}");
            Settled::Kept
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settled {
    Moved,
    Deleted,
    Kept,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::platform::tray::REC_SHOWN;
    use record_bar::Msg as B;

    fn app() -> App {
        App::idle(iced::window::Id::unique())
    }

    fn bar_phase(app: &App) -> Option<Phase> {
        app.windows.record_bar().map(|b| b.phase)
    }

    /// Starts a video recording of an area (a bar on every OS) and presses Record.
    fn recording(app: &mut App) {
        let _ = app.record_start_on(false, Target::Area, true);
        assert_eq!(bar_phase(app), Some(Phase::Ready));
        let _ = app.on_record_bar(B::Record);
        assert_eq!(app.rec.state, State::Recording { audio_only: false });
    }

    /// R02-11: Record then Stop, with the fake recorder, opens the S2 thumbnail and the bar goes.
    #[test]
    fn r02_11_record_then_stop_opens_the_thumbnail() {
        let mut app = app();
        recording(&mut app);
        assert_eq!(bar_phase(&app), Some(Phase::Recording));
        let _ = app.on_record_bar(B::Stop);
        assert_eq!(app.rec.state, State::Result { audio_only: false });
        assert_eq!(app.rec.fake.log, [Effect::Begin, Effect::Flush]);
        assert!(app.windows.record_bar().is_none(), "the bar is gone");
        let thumb = app.windows.record_thumb().expect("the thumbnail is open");
        assert!(!thumb.audio_only);
    }

    /// R02-12: audio only starts at once (no pick) and ends in the small waveform chip.
    #[test]
    fn r02_12_audio_ends_in_the_chip() {
        let mut app = app();
        let _ = app.record_start_on(true, Target::Area, true);
        assert_eq!(bar_phase(&app), Some(Phase::Recording));
        let _ = app.on_record_bar(B::Stop);
        let thumb = app.windows.record_thumb().expect("the chip");
        assert!(thumb.audio_only && !thumb.wave.is_empty());
    }

    /// R02-13: the timer counts while recording, stands still while paused, goes on after.
    #[test]
    fn r02_13_the_timer_stops_while_paused() {
        let mut app = app();
        recording(&mut app);
        let t0 = app.rec.t0;
        let at = |s| t0 + Duration::from_secs(s);
        let shown = |app: &App| app.windows.record_bar().unwrap().elapsed;
        let _ = app.on_record_tick(at(5));
        let five = shown(&app);
        assert!(
            five > Duration::from_secs(4) && five <= Duration::from_secs(5),
            "{five:?}"
        );
        let _ = app.record_event(Event::Pause, at(5));
        let _ = app.on_record_tick(at(20));
        assert_eq!((bar_phase(&app), shown(&app)), (Some(Phase::Paused), five));
        let _ = app.record_event(Event::Resume, at(20));
        let _ = app.on_record_tick(at(23));
        assert_eq!(shown(&app), five + Duration::from_secs(3));
    }

    /// R02-14: the thumbnail slides away by itself after its time, but not while the pointer is on it.
    #[test]
    fn r02_14_the_thumbnail_slides_away_unless_hovered() {
        let mut app = app();
        recording(&mut app);
        let _ = app.on_record_bar(B::Stop);
        let t0 = Instant::now();
        app.rec.last_beat = t0;
        let _ = app.on_record_result(thumb::Msg::Hover(true));
        let _ = app.on_record_tick(t0 + thumb::SLIDE * 2);
        assert!(app.windows.record_thumb().is_some(), "held while hovered");
        let _ = app.on_record_result(thumb::Msg::Hover(false));
        let _ = app.on_record_tick(t0 + thumb::SLIDE * 4);
        assert!(app.windows.record_thumb().is_none());
        assert_eq!(app.rec.state, State::Idle);
    }

    /// R02-15: the bin discards (the temp file goes), Esc asks Keep / Discard first.
    #[test]
    fn r02_15_discard_and_the_keep_question() {
        let mut app = app();
        recording(&mut app);
        let _ = app.record_event(Event::Esc, Instant::now());
        assert!(app.windows.record_bar().unwrap().ask, "the keep card");
        let _ = app.record_event(Event::Esc, Instant::now());
        assert!(!app.windows.record_bar().unwrap().ask, "Esc again: back");
        let _ = app.on_record_bar(B::Discard);
        assert_eq!(app.rec.state, State::Idle);
        assert!(app.windows.record_bar().is_none() && app.windows.record_thumb().is_none());
        assert_eq!(app.rec.fake.log.last(), Some(&Effect::DeleteTemp));
    }

    /// R02-16: no microphone: the error card; "Record without microphone" starts again with it off.
    #[test]
    fn r02_16_a_missing_microphone_can_be_recorded_without() {
        let mut app = app();
        let _ = app.record_start_on(false, Target::Area, true);
        app.rec.fake.fail_start = Some(Fault::Mic);
        let _ = app.on_record_bar(B::Record);
        assert_eq!(bar_phase(&app), Some(Phase::Error(Fault::Mic)));
        let _ = app.on_record_bar(B::Fix);
        assert_eq!(app.rec.bar.mic, None);
        assert_eq!(bar_phase(&app), Some(Phase::Ready), "the pick comes again");
    }

    /// R02-17: "Off" in the device list mutes, a microphone unmutes.
    #[test]
    fn r02_17_off_mutes_and_a_microphone_unmutes() {
        let mut app = app();
        recording(&mut app);
        app.rec.bar.mics = vec!["Mic".into()];
        let _ = app.on_record_bar(B::Mic(None));
        assert!(app.windows.record_bar().unwrap().muted);
        let _ = app.on_record_bar(B::Mic(Some(0)));
        assert!(!app.windows.record_bar().unwrap().muted);
    }

    /// R02-18 (Q1): where the window cannot be kept out of a whole-screen recording, no bar
    /// opens: the tray shows the dot and the time, its rows follow the state, and it is put
    /// back when the recording ends. An area keeps the bar.
    #[test]
    fn r02_18_the_tray_is_the_bar_where_the_screen_is_recorded_whole() {
        REC_SHOWN.with_borrow_mut(Vec::clear);
        let mut app = app();
        let _ = app.record_start_on(false, Target::WholeScreen, false);
        let _ = app.on_record_bar(B::Record);
        assert!(
            app.windows.record_bar().is_none(),
            "no window to be recorded"
        );
        let shown = |i: usize| REC_SHOWN.with_borrow(|s| s[i].clone());
        let (label, rows) = shown(REC_SHOWN.with_borrow(Vec::len) - 1).expect("the tray's state");
        assert!(label.starts_with("● 00:0"), "{label}");
        let kinds: Vec<Row> = rows.iter().map(|(r, _)| *r).collect();
        assert_eq!(
            kinds,
            [Row::Pause, Row::Stop, Row::Discard, Row::Mic, Row::Device]
        );
        let _ = app.on_record_row(Row::Pause);
        assert_eq!(app.rec.state, State::Paused { audio_only: false });
        let _ = app.on_record_row(Row::Stop);
        assert!(app.windows.record_thumb().is_some());
        assert_eq!(
            REC_SHOWN.with_borrow(|s| s.last().cloned()),
            Some(None),
            "the tray is put back"
        );
        // an area keeps the bar even there
        let _ = app.record_start_on(false, Target::Area, false);
        assert!(app.windows.record_bar().is_some());
    }

    /// R02-19: a fault while recording keeps what there is for a full disk, shows the card for a mic.
    #[test]
    fn r02_19_a_full_disk_keeps_the_recording() {
        let mut app = app();
        recording(&mut app);
        let _ = app.record_fault(Fault::DiskLow);
        assert!(app.windows.record_thumb().is_some(), "what we have is kept");
    }

    /// R02-23: a fault in the middle of a recording keeps what was recorded (the thumbnail
    /// opens, nothing is deleted); a fault before it shows the card, whose exits delete nothing real.
    #[test]
    fn r02_23_a_fault_mid_recording_keeps_the_file() {
        for fault in [Fault::Mic, Fault::Refused] {
            let mut app = app();
            recording(&mut app);
            let _ = app.record_fault(fault);
            assert_eq!(app.rec.state, State::Result { audio_only: false });
            assert_eq!(
                app.rec.fake.log,
                [Effect::Begin, Effect::Flush],
                "{fault:?}"
            );
            assert!(app.windows.record_thumb().is_some());
        }
        // before the first frame: the card, and leaving it runs the (harmless) cleanup
        let mut app = app();
        let _ = app.record_start_on(false, Target::Area, true);
        let _ = app.record_fault(Fault::Mic);
        assert_eq!(app.rec.state, State::Error(Fault::Mic));
        let _ = app.on_record_bar(B::Cancel);
        assert_eq!(app.rec.fake.log, [Effect::DeleteTemp]);
    }

    /// R02-24: in the tray (whole screen), Error and the Keep question have rows and a press on
    /// them leaves the state; Ready has Record / Cancel.
    #[test]
    fn r02_24_the_tray_has_a_way_out_of_every_card() {
        let rows = || {
            let last = REC_SHOWN.with_borrow(|s| s.last().cloned());
            last.flatten()
                .map(|(_, r)| r.into_iter().map(|(r, _)| r).collect::<Vec<_>>())
        };
        REC_SHOWN.with_borrow_mut(Vec::clear);
        let mut app = app();
        let _ = app.record_start_on(false, Target::WholeScreen, false);
        assert_eq!(rows(), Some(vec![Row::Record, Row::Cancel]));
        let _ = app.on_record_row(Row::Record);
        let _ = app.record_event(Event::Esc, Instant::now());
        assert_eq!(rows(), Some(vec![Row::Keep, Row::Discard]));
        let _ = app.on_record_row(Row::Keep);
        assert!(app.windows.record_thumb().is_some());
        let mut app2 = App::idle(iced::window::Id::unique());
        let _ = app2.record_start_on(false, Target::WholeScreen, false);
        let _ = app2.record_event(Event::Picked { ask: true }, Instant::now());
        let _ = app2.record_event(Event::Denied, Instant::now());
        assert_eq!(
            rows(),
            Some(vec![Row::ErrRefused, Row::Retry, Row::Dismiss])
        );
        let _ = app2.on_record_row(Row::Dismiss);
        assert_eq!(app2.rec.state, State::Idle);
    }

    /// R04-50: only the SAME key stops what runs: the audio key does not stop a video, the video
    /// key does not stop audio; the countdown swallows both. (The fake recorder: no device is opened.)
    #[test]
    fn r04_50_only_the_same_key_stops_a_recording() {
        let at = Instant::now();
        let mut video = app();
        let _ = video.record_start_on(false, Target::Area, true);
        let _ = video.on_record_bar(B::Record);
        assert_eq!(video.rec.state, State::Recording { audio_only: false });
        let _ = video.record_key(true, at);
        assert_eq!(
            video.rec.state,
            State::Recording { audio_only: false },
            "M keeps a video"
        );
        let _ = video.record_key(false, at);
        assert_eq!(
            video.rec.state,
            State::Result { audio_only: false },
            "R stops and saves it"
        );
        let mut audio = app();
        let _ = audio.record_start_on(true, Target::WholeScreen, true);
        assert_eq!(audio.rec.state, State::Recording { audio_only: true });
        let _ = audio.record_key(false, at);
        assert_eq!(
            audio.rec.state,
            State::Recording { audio_only: true },
            "R keeps audio"
        );
        let _ = audio.record_key(true, at);
        assert_eq!(audio.rec.state, State::Result { audio_only: true });
    }

    /// R04-51: with a real recorder Record counts 3, 2, 1 (the bar shows the number), a press
    /// during it skips, the start key during it is swallowed. (Stops before the first frame:
    /// no device is opened.)
    #[test]
    fn r04_51_the_countdown_counts_down_on_the_bar() {
        let mut app = app();
        app.rec.real = Some(Real::default());
        app.rec.region = Some((0, 0, 640, 360));
        let _ = app.record_start_on(false, Target::Area, true);
        assert_eq!(app.rec.state, State::Picking);
        let _ = app.on_record_bar(B::Record);
        assert_eq!(app.rec.state, State::Countdown(3));
        let at = app.rec.count_at;
        let _ = app.on_record_tick(at + Duration::from_millis(400));
        assert_eq!(app.rec.state, State::Countdown(3), "not a second yet");
        let _ = app.on_record_tick(at + Duration::from_secs(1));
        assert_eq!(app.rec.state, State::Countdown(2));
        let shown = app.windows.record_bar().unwrap().count;
        assert_eq!(shown, Some(2));
        let _ = app.record_key(false, at + Duration::from_secs(1));
        assert_eq!(
            app.rec.state,
            State::Countdown(2),
            "the start key is swallowed"
        );
    }

    /// R04-52 (Q4, hermetic: a temp folder, no real data folder): an unsaved recording goes to
    /// Recent when it is on, is deleted when it is off, is KEPT when the move fails (never deleted
    /// because something else went wrong), and an offered leftover is never deleted at all.
    #[test]
    fn r04_52_an_unsaved_recording_is_moved_deleted_or_kept_but_never_lost() {
        let dir = crate::shared::paths::test_dir("rec-q4");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = |n: &str| {
            let f = dir.join(n);
            std::fs::write(&f, b"x").unwrap();
            f
        };
        let recent = dir.join("recent").join("recordings");
        let a = file("rec-1.mp4");
        assert_eq!(
            settle_unsaved(&a, Some(&recent), true, false, 20),
            Settled::Moved
        );
        assert!(!a.exists() && recent.join("rec-1.mp4").exists());
        // an OLD file moved in is the newest there (its time is now): the prune keeps it
        let old = file("rec-old.mp4");
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400 * 30);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        settle_unsaved(&old, Some(&recent), true, false, 1);
        assert!(recent.join("rec-old.mp4").exists(), "not pruned unseen");
        assert!(
            !recent.join("rec-1.mp4").exists(),
            "the older one made room"
        );
        let b = file("rec-2.mp4");
        assert_eq!(
            settle_unsaved(&b, Some(&recent), false, false, 20),
            Settled::Deleted
        );
        assert!(!b.exists(), "Recent is off: deleted");
        assert!(!recent.join("rec-2.mp4").exists());
        // the move fails (the target folder is a file): kept, not deleted
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, b"i am a file").unwrap();
        let c = file("rec-3.mp4");
        assert_eq!(
            settle_unsaved(&c, Some(&blocked.join("r")), true, false, 20),
            Settled::Kept
        );
        assert!(c.exists(), "a failed move keeps the temp file");
        assert_eq!(settle_unsaved(&c, None, true, false, 20), Settled::Kept);
        // Recent off: NOTHING is moved into it; an offered leftover stays where it is
        let d = file("rec-4.mp4");
        assert_eq!(
            settle_unsaved(&d, Some(&recent), false, true, 20),
            Settled::Kept
        );
        assert!(
            d.exists() && !recent.join("rec-4.mp4").exists(),
            "never into Recent while it is off"
        );
        // the folder is pruned like the pictures: the newest `limit` stay
        for n in 10..16 {
            let f = file(&format!("rec-{n}.mp4"));
            settle_unsaved(&f, Some(&recent), true, false, 3);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let left = std::fs::read_dir(&recent).unwrap().count();
        assert_eq!(left, 3, "no unbounded growth");
        assert!(recent.join("rec-15.mp4").exists(), "the newest stays");
    }

    /// R04-53: a Save that fails keeps the card up and the file where it is (never routed to the
    /// delete); one that works moves it and the card goes; an Edit that waits for the Studio
    /// pins nothing.
    #[test]
    fn r04_53_a_failed_save_keeps_the_file_and_the_card() {
        let dir = crate::shared::paths::test_dir("rec-save-fail");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app();
        let _ = app.record_start_on(false, Target::Area, true);
        let _ = app.on_record_bar(B::Record);
        let _ = app.on_record_bar(B::Stop);
        let file = dir.join("rec-9.mp4");
        std::fs::write(&file, b"film").unwrap();
        app.rec.file = Some(file.clone());
        let blocked = dir.join("not-a-folder");
        std::fs::write(&blocked, b"x").unwrap();
        for folder in [None, Some(blocked.join("sub"))] {
            let _ = app.record_save(folder);
            assert_eq!(
                app.rec.state,
                State::Result { audio_only: false },
                "the card stays"
            );
            assert_eq!(app.rec.file.as_deref(), Some(file.as_path()));
            assert_eq!(
                std::fs::read(&file).unwrap(),
                b"film",
                "the file is untouched"
            );
            assert!(
                app.windows
                    .toasts()
                    .iter()
                    .any(|(t, ok)| !ok && t.contains("could not"))
            );
        }
        let out = dir.join("Videos");
        let _ = app.record_save(Some(out.clone()));
        assert!(!file.exists() && std::fs::read_dir(&out).unwrap().count() == 1);
        assert_eq!(app.rec.state, State::Idle);
        let _ = app.on_record_result(thumb::Msg::Edit);
        assert!(!app.rec.in_use, "Edit is a stub: it pins nothing");
    }

    /// R04-54: a recording started from the Result card first settles the one before (Recent
    /// is off here: it is deleted; without the settle it would stay behind forever).
    #[test]
    fn r04_54_a_new_recording_settles_the_old_one_first() {
        let dir = crate::shared::paths::test_dir("rec-new");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app();
        let _ = app.record_start_on(false, Target::Area, true);
        let _ = app.on_record_bar(B::Record);
        let _ = app.on_record_bar(B::Stop);
        assert_eq!(app.rec.state, State::Result { audio_only: false });
        let old = dir.join("rec-1.mp4");
        std::fs::write(&old, b"x").unwrap();
        app.rec.file = Some(old.clone());
        let mut config = crate::shared::config::Config::default();
        config.snip.history = false;
        crate::shared::config::with_test_config(config, || {
            // (video: the pick comes first, no device opens)
            let _ = app.record_begin(false, Target::Area, Some((0, 0, 640, 360)));
        });
        // (Recent off: never deleted without an Undo: it stays for the next start's offer)
        assert!(old.exists(), "the old one is kept, not deleted unasked");
        assert_eq!(app.rec.file, None);
    }

    /// R04-58: Quit / Restart-to-update NEVER deletes: the file stays in the temp folder (the next
    /// start offers it) whatever Recent says; nothing running is quiet.
    #[test]
    fn r04_58_quitting_never_deletes_a_recording() {
        let dir = crate::shared::paths::test_dir("rec-quit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app();
        let file = dir.join("rec-1.mp4");
        std::fs::write(&file, b"film").unwrap();
        app.rec.file = Some(file.clone());
        let mut config = crate::shared::config::Config::default();
        config.snip.history = false; // Recent off: Q4 would delete it
        crate::shared::config::with_test_config(config, || app.record_shutdown());
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"film",
            "kept for the next start's offer"
        );
    }

    /// R04-59: after a failed Save the thumbnail's slide-away and Dismiss cannot delete the file
    /// (Recent off would): it is pinned and stays.
    #[test]
    fn r04_59_a_failed_save_survives_the_slide_away() {
        let dir = crate::shared::paths::test_dir("rec-slide");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app();
        let _ = app.record_start_on(false, Target::Area, true);
        let _ = app.on_record_bar(B::Record);
        let _ = app.on_record_bar(B::Stop);
        let file = dir.join("rec-2.mp4");
        std::fs::write(&file, b"film").unwrap();
        app.rec.file = Some(file.clone());
        let mut config = crate::shared::config::Config::default();
        config.snip.history = false;
        crate::shared::config::with_test_config(config, || {
            let _ = app.record_save(None); // no folder: the save fails
            let t0 = Instant::now();
            app.rec.last_beat = t0;
            let _ = app.on_record_tick(t0 + thumb::SLIDE * 4); // the card slides away
        });
        assert_eq!(app.rec.state, State::Idle, "it did slide away");
        assert!(
            file.exists(),
            "the slide-away did not delete the unsaved file"
        );
    }

    /// R04-55: the end of a recording whose file could not be written says so (the card), keeps
    /// the file, and never reports a quiet success; a clean end opens the thumbnail.
    #[test]
    fn r04_55_a_failed_end_shows_the_card_and_keeps_the_file() {
        let mut app = app();
        let _ = app.record_start_on(false, Target::Area, true);
        let _ = app.on_record_bar(B::Record);
        let _ = app.record_event(Event::Stop, Instant::now());
        // the fake answered Flushed at once: replay the real end by hand
        let mut app = App::idle(iced::window::Id::unique());
        let _ = app.record_start_on(false, Target::Area, true);
        let _ = app.on_record_bar(B::Record);
        app.rec.state = State::Stopping { audio_only: false };
        let done = Flushed {
            file: Some(PathBuf::from("/nowhere/rec-1.mp4")),
            error: Some("disk".into()),
        };
        let _ = app.record_flushed(done, Instant::now());
        assert_eq!(app.rec.state, State::Error(Fault::WriteFailed));
        assert!(
            app.rec.in_use && app.rec.file.is_some(),
            "kept for the next start"
        );
        let mut ok = App::idle(iced::window::Id::unique());
        let _ = ok.record_start_on(false, Target::Area, true);
        let _ = ok.on_record_bar(B::Record);
        ok.rec.state = State::Stopping { audio_only: false };
        let _ = ok.record_flushed(
            Flushed {
                file: None,
                error: None,
            },
            Instant::now(),
        );
        assert_eq!(ok.rec.state, State::Result { audio_only: false });
    }

    /// R04-56: boot offers the NEWEST leftover on the thumbnail (older ones go to Recent, a
    /// discarded one is swept); nothing is deleted.
    #[test]
    fn r04_56_a_leftover_is_offered_and_never_deleted() {
        let dir = crate::shared::paths::test_dir("rec-left2");
        let _ = std::fs::remove_dir_all(&dir);
        let (temp, recent) = (dir.join("tmp"), dir.join("recent").join("recordings"));
        std::fs::create_dir_all(&temp).unwrap();
        for (n, body) in [
            ("rec-1.ogg", "old"),
            ("rec-2.mp4", "new"),
            ("rec-0.mp4", "gone"),
        ] {
            std::fs::write(temp.join(n), body).unwrap();
        }
        files::mark_discarded(&temp.join("rec-0.mp4")); // a killed worker's discard
        let mut app = app();
        let _ = app.record_offer_leftovers_in(&temp, Some(&recent), true);
        assert_eq!(app.rec.state, State::Result { audio_only: false });
        let shown = app.rec.file.clone().unwrap();
        assert_eq!(
            std::fs::read(&shown).unwrap(),
            b"new",
            "the newest is on the card"
        );
        assert!(app.windows.record_thumb().is_some());
        assert_eq!(
            std::fs::read(recent.join("rec-1.ogg")).unwrap(),
            b"old",
            "older: in Recent"
        );
        assert!(
            !temp.join("rec-0.mp4").exists(),
            "the discarded one is swept, not offered"
        );
        assert!(files::was_offered(&shown), "marked as shown");
        // Recent OFF: nothing goes into it; the older one stays in temp (asked about later)
        let off = dir.join("off");
        let (temp2, recent2) = (off.join("tmp"), off.join("recent").join("recordings"));
        std::fs::create_dir_all(&temp2).unwrap();
        for n in ["rec-1.ogg", "rec-2.mp4"] {
            std::fs::write(temp2.join(n), b"x").unwrap();
        }
        let mut again = App::idle(iced::window::Id::unique());
        let _ = again.record_offer_leftovers_in(&temp2, Some(&recent2), false);
        assert_eq!(again.rec.state, State::Result { audio_only: false });
        assert!(
            temp2.join("rec-1.ogg").exists() && !recent2.exists(),
            "nothing moved into Recent"
        );
    }

    /// R04-63: `offered` never outlives its card: a new recording and a good Save clear it (it
    /// would send the next unsaved file to Recent against the setting).
    #[test]
    fn r04_63_offered_is_reset_by_a_new_recording_and_a_good_save() {
        let mut app = app();
        app.rec.offered = true;
        let _ = app.record_begin(false, Target::Area, Some((0, 0, 640, 360)));
        assert!(!app.rec.offered, "a new recording clears it");
        let dir = crate::shared::paths::test_dir("rec-offered");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut saved = App::idle(iced::window::Id::unique());
        let _ = saved.record_start_on(false, Target::Area, true);
        let _ = saved.on_record_bar(B::Record);
        let _ = saved.on_record_bar(B::Stop);
        let f = dir.join("rec-1.mp4");
        std::fs::write(&f, b"x").unwrap();
        (saved.rec.file, saved.rec.offered) = (Some(f), true);
        let _ = saved.record_save(Some(dir.join("out")));
        assert!(!saved.rec.offered, "a good Save clears it");
    }

    /// R04-61: an offered leftover that was copied / failed to save is, at the next boot, settled
    /// WITHOUT asking when Recent is on and takes it; with Recent off it is offered again (never
    /// lost, never hidden); no marker is left without its file.
    #[test]
    fn r04_61_an_already_offered_leftover_is_settled_or_asked_again() {
        let dir = crate::shared::paths::test_dir("rec-left3");
        let _ = std::fs::remove_dir_all(&dir);
        let (temp, recent) = (dir.join("tmp"), dir.join("recent").join("recordings"));
        std::fs::create_dir_all(&temp).unwrap();
        let file = temp.join("rec-7.mp4");
        std::fs::write(&file, b"film").unwrap();
        let mut first = app();
        let _ = first.record_offer_leftovers_in(&temp, Some(&recent), false);
        assert_eq!(first.rec.state, State::Result { audio_only: false });
        // (the user copied it or a Save failed: it stayed in the temp folder, pinned)
        let mut second = App::idle(iced::window::Id::unique());
        let _ = second.record_offer_leftovers_in(&temp, Some(&recent), false);
        assert_eq!(
            second.rec.state,
            State::Result { audio_only: false },
            "Recent off: asked again"
        );
        assert!(file.exists());
        // Recent on: settled without asking
        let mut third = App::idle(iced::window::Id::unique());
        let _ = third.record_offer_leftovers_in(&temp, Some(&recent), true);
        assert_eq!(third.rec.state, State::Idle, "not asked");
        assert_eq!(
            std::fs::read(recent.join("rec-7.mp4")).unwrap(),
            b"film",
            "in Recent"
        );
        assert!(
            !file.exists() && !files::was_offered(&file),
            "no marker without its file"
        );
        // Recent on but unable to take it: it stays and is asked again
        let other = temp.join("rec-8.mp4");
        std::fs::write(&other, b"x").unwrap();
        files::mark_offered(&other);
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, b"i am a file").unwrap();
        let mut fourth = App::idle(iced::window::Id::unique());
        let _ = fourth.record_offer_leftovers_in(&temp, Some(&blocked.join("r")), true);
        assert_eq!(fourth.rec.state, State::Result { audio_only: false });
        assert!(other.exists());
    }

    /// R04-62: the automatic (locked-screen) install never starts while anything of a recording
    /// is up, and a Quit-time shutdown leaves a state the bar can show (the kept file on the
    /// card), so a failed Restart does not leave a timer counting.
    #[test]
    fn r04_62_a_recording_blocks_the_automatic_install_and_a_shutdown_leaves_a_clean_state() {
        let mut app = app();
        assert!(app.is_idle(), "nothing is up");
        let _ = app.record_start_on(false, Target::Area, true);
        assert!(!app.is_idle(), "the pick / ready bar blocks it");
        let _ = app.on_record_bar(B::Record);
        assert!(matches!(app.rec.state, State::Recording { .. }));
        assert!(!app.is_idle(), "recording blocks it");
        app.rec.file = Some(PathBuf::from("/nowhere/rec-1.mp4"));
        app.record_shutdown(); // (the restart's first step)
        let _ = app.record_after_shutdown(); // (its failure path)
        assert_eq!(
            app.rec.state,
            State::Result { audio_only: false },
            "the kept file's card"
        );
        assert!(app.rec.in_use);
        assert!(app.windows.record_bar().is_none() && app.windows.record_thumb().is_some());
        let mut empty = App::idle(iced::window::Id::unique());
        let _ = empty.record_start_on(true, Target::WholeScreen, true);
        empty.record_shutdown();
        let _ = empty.record_after_shutdown();
        assert_eq!(empty.rec.state, State::Idle, "nothing kept: nothing shown");
    }

    /// R04-80: the pick flag of the record-video key never outlives its capture: the next capture
    /// (Alt+A, a timer) starts with it cleared.
    #[test]
    fn r04_80_the_record_pick_flag_never_leaks_into_alt_a() {
        let mut app = app();
        app.record_next = true;
        let _ = app.capture(
            Instant::now(),
            Some(crate::features::snip::screens::pick::Kind::Region),
        );
        assert!(
            !app.record_next,
            "a capture clears it (the record key sets it after)"
        );
    }

    /// R04B-1: an unsaved recording whose thumbnail slid away with Recent off stays on disk behind
    /// the "Recording discarded . Undo" pill: Undo brings it back, the pill's end deletes it
    /// (an older pill's end changes nothing), and Quit meanwhile deletes nothing: the next
    /// start offers it.
    #[test]
    fn r04b_1_a_discard_waits_for_its_undo_pill() {
        let dir = crate::shared::paths::test_dir("rec-discard");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fresh = |name: &str| {
            let f = dir.join(name);
            std::fs::write(&f, b"film").unwrap();
            f
        };
        let pill = |app: &App| {
            app.windows
                .toasts()
                .iter()
                .any(|(t, _)| t == "Recording discarded")
        };
        // Undo: the file and its card are back, the pill goes
        let (mut app, undo) = (app(), fresh("rec-1.mp4"));
        let _ = app.record_discard_later(undo.clone(), false);
        assert!(undo.exists() && pill(&app), "kept, with the pill");
        let _ = app.update(Message::Windows(crate::app::windows::Msg::ToastAction));
        assert_eq!(app.rec.file.as_ref(), Some(&undo));
        assert_eq!(app.rec.state, State::Result { audio_only: false });
        assert!(undo.exists() && !pill(&app) && app.rec.discard.is_none());
        // the pill's end deletes; another pill's end does not
        let timed = fresh("rec-2.mp4");
        let _ = app.record_discard_later(timed.clone(), true);
        let n = app.rec.discard.as_ref().unwrap().0;
        let _ = app.update(Message::RecDiscardGone(n + 1));
        assert!(timed.exists(), "an older pill's end changes nothing");
        let _ = app.update(Message::RecDiscardGone(n));
        assert!(
            !timed.exists() && app.rec.discard.is_none(),
            "deleted at the end"
        );
        let _ = app.update(Message::Windows(crate::app::windows::Msg::ToastAction));
        assert!(
            app.rec.file.as_ref() == Some(&undo),
            "no Undo after the end"
        );
        // Quit during the window: nothing is deleted, the next start offers it
        let kept = fresh("rec-3.mp4");
        let mut quitting = App::idle(iced::window::Id::unique());
        let _ = quitting.record_discard_later(kept.clone(), false);
        quitting.record_shutdown();
        assert!(kept.exists(), "Quit keeps the file");
        let mut next = App::idle(iced::window::Id::unique());
        let _ = next.record_offer_leftovers_in(&dir, None, false);
        assert!(next.rec.file.is_some(), "the next start offers a leftover");
    }

    /// R04B-5: a new recording starting closes a pending discard's pill (its Undo restores
    /// nothing, no stray state), and never deletes the restored / open recording without an
    /// Undo: with Recent off it stays for the next start's offer.
    #[test]
    fn r04b_5_a_new_recording_ends_the_pill_and_keeps_the_open_card_file() {
        let dir = crate::shared::paths::test_dir("rec-discard2");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (pending, open) = (dir.join("rec-1.mp4"), dir.join("rec-2.mp4"));
        for f in [&pending, &open] {
            std::fs::write(f, b"film").unwrap();
        }
        let pill = |app: &App| !app.windows.toasts().is_empty();
        let mut app = app();
        let _ = app.record_discard_later(pending.clone(), false);
        assert!(pill(&app));
        // the card of the last one is up (Recent off) when the next recording begins
        app.rec.file = Some(open.clone());
        let _ = app.record_begin(false, Target::Area, None);
        assert!(!pending.exists(), "the pending discard is final");
        assert!(!pill(&app), "its pill is closed");
        assert!(open.exists(), "never deleted without an Undo");
        assert!(app.rec.discard.is_none() && app.rec.file.is_none());
        let state = app.rec.state;
        let _ = app.update(Message::Windows(crate::app::windows::Msg::ToastAction));
        assert_eq!(app.rec.state, state, "Undo restores nothing");
        assert!(app.rec.file.is_none() && !pill(&app));
        // ... and it is the next start's offer
        let mut next = App::idle(iced::window::Id::unique());
        let _ = next.record_offer_leftovers_in(&dir, None, false);
        assert_eq!(next.rec.file.as_ref(), Some(&open));
    }

    /// R04B-10: the discard pill's end, a replacement and the Snip pill hand back the task that
    /// closes the old pill's WINDOW (not only the state), and a stale end closes nothing.
    #[test]
    fn r04b_10_ending_a_discard_returns_the_task_that_closes_its_window() {
        let f = crate::shared::paths::test_dir("rec-discard4").join("rec-1.mp4");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, b"x").unwrap();
        let mut app = app();
        assert_eq!(app.record_discard_now().units(), 0, "nothing pending");
        let _ = app.record_discard_later(f.clone(), false);
        let n = app.rec.discard.as_ref().unwrap().0;
        assert_eq!(app.record_discard_gone(n + 1).units(), 0, "a stale end");
        assert!(!app.windows.toasts().is_empty());
        assert!(app.record_discard_gone(n).units() > 0, "the window closes");
        assert!(app.windows.toasts().is_empty() && !f.exists());
        // a second discard replaces the first one's pill (its close is in the task)
        std::fs::write(&f, b"x").unwrap();
        let _ = app.record_discard_later(f.clone(), false);
        let g = f.with_file_name("rec-2.mp4");
        std::fs::write(&g, b"x").unwrap();
        let again = app.record_discard_later(g, false);
        assert!(again.units() >= 4, "{} units", again.units());
        assert_eq!(app.windows.toasts().len(), 1, "one pill");
    }

    /// R04B-11: with Recent off a start keeps at most 5 leftovers (the newest); the oldest go.
    #[test]
    fn r04b_11_a_start_keeps_at_most_five_leftovers_with_recent_off() {
        let dir = crate::shared::paths::test_dir("rec-cap");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in 1..=8 {
            std::fs::write(dir.join(format!("rec-{n}.mp4")), b"x").unwrap();
        }
        let mut app = app();
        let _ = app.record_offer_leftovers_in(&dir, None, false);
        let left = files::leftovers(&dir);
        assert_eq!(left.len(), 5, "{left:?}");
        assert!(!dir.join("rec-1.mp4").exists() && dir.join("rec-8.mp4").exists());
        // Recent on: nothing is deleted here
        let other = crate::shared::paths::test_dir("rec-cap2");
        let _ = std::fs::remove_dir_all(&other);
        std::fs::create_dir_all(&other).unwrap();
        for n in 1..=7 {
            std::fs::write(other.join(format!("rec-{n}.mp4")), b"x").unwrap();
        }
        files::cap_leftovers(&other, 7);
        assert_eq!(files::leftovers(&other).len(), 7);
    }

    /// R04B-6: a discard's pill replaces a pending Snip undo (one pill, one Undo).
    #[test]
    fn r04b_6_a_discard_pill_replaces_a_snip_undo() {
        let f = crate::shared::paths::test_dir("rec-discard3").join("rec-1.mp4");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, b"x").unwrap();
        let mut app = app();
        let image = crate::features::snip::annotate::Image {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        };
        app.snip_undo = Some((
            1,
            Box::new(crate::features::snip::annotate::Annotator::new(image, 1.0)),
        ));
        let _ = app.record_discard_later(f, false);
        assert!(app.snip_undo.is_none());
    }

    /// R04B-2: a saved recording is named like the pictures (`OpenCircle-<date>_<time>.<ext>`).
    #[test]
    fn r04b_2_a_saved_recording_is_named_like_a_picture() {
        let dir = crate::shared::paths::test_dir("rec-name");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = app();
        let f = dir.join("rec-1.mp4");
        std::fs::write(&f, b"x").unwrap();
        app.rec.file = Some(f);
        let _ = app.record_save(Some(dir.join("out")));
        let name = std::fs::read_dir(dir.join("out"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .into_owned();
        let stamp = name
            .strip_prefix("OpenCircle-")
            .and_then(|n| n.strip_suffix(".mp4"));
        let shape = |c: char| if c.is_ascii_digit() { '9' } else { c };
        assert_eq!(
            stamp
                .map(|s| s.chars().map(shape).collect::<String>())
                .as_deref(),
            Some("9999-99-99_99-99-99"),
            "{name}"
        );
    }

    /// R02-25: drag-out waits for the file only where it is supported and there is none.
    #[test]
    fn r02_25_drag_out_reads_the_right_way_round() {
        assert_eq!(drag_out(true, false), DragOut::WaitsForFile);
        assert_eq!(drag_out(true, true), DragOut::Go);
        assert_eq!(drag_out(false, true), DragOut::Unsupported);
        assert_eq!(drag_out(false, false), DragOut::Unsupported);
    }
}
