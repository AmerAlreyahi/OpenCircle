//! Recording (R-02), the App's part: the bar's and the thumbnail's messages go to the state
//! machine (`record::state`), its effects to the FAKE recorder (no capture until R-04), and what
//! it says to the windows. Where the controls live (the bar's window, or the tray where the whole
//! screen is recorded and our window cannot be kept out of it) is `record::placement`'s rule.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::{Point, Rectangle, Size, Task};

use crate::app::App;
use crate::app::message::Message;
use crate::features::snip::record::clock::{self, Clock};
use crate::features::snip::record::fake::Fake;
use crate::features::snip::record::placement::{self, Ctx, Row, Surface, Target};
use crate::features::snip::record::state::{Effect, Event, State};
use crate::features::snip::screens::record_bar::{self, Bar, Phase};
use crate::features::snip::screens::record_result::{self as thumb, Thumb};
use crate::shared::platform::{self, CAN_KEEP_WINDOWS_OUT_OF_CAPTURE};

/// The recording the App holds (`App::rec`): its state, clock and what shows it.
pub(crate) struct Recording {
    pub(crate) state: State,
    clock: Clock,
    fake: Fake,
    /// Time zero of `clock` and the beat.
    t0: Instant,
    last_beat: Instant,
    /// What the bar shows (the window gets a copy at every change).
    bar: Bar,
    target: Target,
    surface: Surface,
    screen: Rectangle,
    /// The finished file (none until R-05 writes real ones).
    file: Option<PathBuf>,
    /// The tray's last label (the tray is told only when it changes).
    tray: Option<String>,
}

impl Default for Recording {
    fn default() -> Self {
        Recording {
            state: State::Idle,
            clock: Clock::default(),
            fake: Fake::default(),
            t0: Instant::now(),
            last_beat: Instant::now(),
            bar: Bar::new(false),
            target: Target::WholeScreen,
            surface: Surface::Bar,
            screen: Rectangle::new(Point::ORIGIN, Size::new(1920.0, 1080.0)),
            file: None,
            tray: None,
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn record_start(&mut self, audio_only: bool, target: Target) -> Task<Message> {
        self.record_start_on(audio_only, target, CAN_KEEP_WINDOWS_OUT_OF_CAPTURE)
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
        rec.bar.mics = (platform::audio::devices(platform::audio::Kind::Mic(None)).ok())
            .map(|d| d.into_iter().map(|d| d.name).collect())
            .unwrap_or_default();
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

    /// One event into the machine, its effect into the fake, the answer back in; then the screen.
    fn record_event(&mut self, ev: Event, at: Instant) -> Task<Message> {
        let rec = &mut self.rec;
        let (next, effect) = rec.state.on(ev);
        rec.state = next;
        let now = at.saturating_duration_since(rec.t0);
        match effect {
            Some(Effect::Begin) => rec.clock = Clock::start(now),
            Some(Effect::Pause) => rec.clock.pause(now),
            Some(Effect::Resume) => rec.clock.resume(now),
            _ => {}
        }
        let answer = effect.and_then(|e| rec.fake.run(e));
        let shown = self.record_show(at);
        match answer {
            Some(ev) => Task::batch([shown, self.record_event(ev, at)]),
            None => shown,
        }
    }

    /// The windows (or the tray) as the state says.
    fn record_show(&mut self, at: Instant) -> Task<Message> {
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
                b.levels = match b.audio_only {
                    true => Fake::levels(elapsed),
                    false => Vec::new(),
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
        let ctx = Ctx {
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
            // Record: the pick is not drawn yet (R-04), so it is "the whole thing, at once"
            M::Record => {
                let picked = self.record_event(Event::Picked { ask: false }, at);
                return Task::batch([picked, self.record_event(Event::Skip, at)]);
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
                return self.record_show(at);
            }
            M::Sound(on) => {
                self.rec.bar.sound_on = on;
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
                let again = self.record_start(audio_only, target);
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
            Row::Device | Row::ErrMic | Row::ErrRefused => Task::none(),
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

    /// Copy / Save / Edit / Open with, the one place: the files are R-05's (Copy puts the file
    /// on the clipboard, Save moves it to the save folder, Edit opens the Studio on it), so only
    /// Open with does something now, once there is a file. The thumbnail goes after it.
    fn record_action(&mut self, act: Act) -> Task<Message> {
        match (act, self.rec.file.clone()) {
            (Act::OpenWith, Some(file)) => {
                // the dialog blocks on Windows: off the UI thread
                std::thread::spawn(move || {
                    if let Err(e) = platform::sys::open::open_with(&file) {
                        crate::log!("[opencircle] Open with failed: {e}");
                    }
                });
            }
            _ => crate::log!("[opencircle] recording {act:?}: waits for R-05 (the file)"),
        }
        self.record_event(Event::Dismiss, Instant::now())
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
                // slid away by itself (an unsaved recording goes to Recent captures: R-05)
                (true, closed) => {
                    let idle = self.record_event(Event::Dismiss, at);
                    Task::batch([closed.map(Message::Windows), idle])
                }
                (false, task) => task.map(Message::Windows),
            },
            s if s.is_live() || matches!(s, State::Stopping { .. }) => self.record_show(at),
            _ => Task::none(),
        }
    }

    /// `Fault`s the real recorder will raise (R-04); the fake raises one for the tests.
    #[cfg(test)]
    pub(crate) fn record_fault(
        &mut self,
        f: crate::features::snip::record::state::Fault,
    ) -> Task<Message> {
        self.record_event(Event::Fault(f), Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::snip::record::state::Fault;
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

    /// R02-25: drag-out waits for the file only where it is supported and there is none.
    #[test]
    fn r02_25_drag_out_reads_the_right_way_round() {
        assert_eq!(drag_out(true, false), DragOut::WaitsForFile);
        assert_eq!(drag_out(true, true), DragOut::Go);
        assert_eq!(drag_out(false, true), DragOut::Unsupported);
        assert_eq!(drag_out(false, false), DragOut::Unsupported);
    }
}
