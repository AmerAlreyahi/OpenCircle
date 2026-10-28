//! Message dispatch: every message to its handler.

use std::time::{Duration, Instant};

use iced::window::{self, Mode};
use iced::{Point, Rectangle, Size, Task};

use crate::Trigger;
use crate::actions::{self, text};
use crate::config::{self};
use crate::labs::{self};
use crate::ocr::{self};

use crate::ui::app::background::background;
use crate::ui::app::keys;
use crate::ui::app::message::{Hiding, Message, Purpose};
use crate::ui::app::quick::Quick;
use crate::ui::edit::Editing;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame as card;
use crate::ui::screens::bar::geometry as bar;
use crate::ui::{overlay, snip, translate, update_card, windows};

use super::capture::{TEST_SHOT, save_test_shot};
use super::{App, BAR_MORPH, CAPTURE_TIMEOUT, GLIDE, REVEAL_WAIT, Reader, SHRINK_FRAMES, SHRUNK};

impl App {
    pub(in crate::ui) fn update(&mut self, message: Message) -> Task<Message> {
        // Late answers for an overlay closed meanwhile (a panel, translate, OCR,
        // song, a frame...) are dropped: nothing of a closed opening comes back.
        if self.o.is_none() && !works_closed(&message) {
            return Task::none();
        }
        match message {
            Message::Trigger(Trigger::Quit) => iced::exit(),
            Message::Trigger(Trigger::OpenSettings) => self
                .windows
                .open(windows::Kind::Settings)
                .map(Message::Windows),
            Message::Windows(msg) => self.windows.update(msg).map(Message::Windows),
            // A hidden (Labs) feature does nothing, whoever asks (no shortcut or tray
            // item reaches here; this is the one place that also covers a late trigger).
            Message::Trigger(Trigger::Quick(_, qa))
                if !labs::quick_on(&config::current().labs, &qa) =>
            {
                crate::log!("[opencircle] \"{}\" is off (Settings > Labs)", qa.action);
                Task::none()
            }
            Message::Trigger(Trigger::Update(event)) => self.on_update(event),
            Message::UpdateTick => self.try_update(),
            // the rebuilt view shows the actions; the opt-in auto action runs now
            Message::Settled if self.o.others.is_empty() => self.auto_search(),
            Message::Settled => Task::none(),
            Message::Trigger(Trigger::Quick(at, qa))
                if qa.live && qa.action == actions::translate::ACTION.id =>
            {
                self.toggle_live(at)
            }
            Message::Live(msg) => self.on_live(msg),
            Message::Translate(id, msg) => self.on_translate(id, msg),
            Message::Peek(down) => {
                if let Some((_, session)) = &mut self.o.translate {
                    let _ = session.update(translate::Msg::Peek(down));
                }
                Task::none()
            }
            // The snip toast is only a pill: a new shot replaces it.
            Message::Trigger(trigger @ (Trigger::Capture(_) | Trigger::Quick(..)))
                if self.snip_toast() =>
            {
                let closing = self.close();
                closing.chain(self.update(Message::Trigger(trigger)))
            }
            Message::Trigger(Trigger::Capture(_) | Trigger::Quick(..))
                if self.capturing.is_some() =>
            {
                crate::log!("[opencircle] shortcut ignored: a capture is still on its way");
                Task::none()
            }
            // Open but maybe behind another app (Alt+Tab, a click elsewhere): its
            // keys (Esc) went there. The shortcut brings it back in front, with
            // the keyboard, instead of nothing happening.
            Message::Trigger(Trigger::Capture(_) | Trigger::Quick(..))
                if self.o.frame.is_some() =>
            {
                crate::log!(
                    "[opencircle] shortcut while the overlay is open: brought to the front"
                );
                window::gain_focus(self.overlay)
            }
            Message::Trigger(Trigger::Capture(at)) => {
                self.next = Purpose::Search;
                self.capture(at, None)
            }
            Message::Trigger(Trigger::Quick(at, qa)) => {
                self.next = Purpose::Search;
                if qa.action == "snip" {
                    return self.capture(at, Some(snip::Kind::for_target(qa.target)));
                }
                match Quick::for_id(&qa.action) {
                    Some(q) => self.next = Purpose::Quick(q, qa.target),
                    None => crate::log!("unknown action {}; opening the overlay", qa.action),
                }
                self.capture(at, None)
            }
            // Cancelled or timed out meanwhile: the frame is dropped (the portal's
            // file is already deleted).
            Message::Captured(n, ..) if self.capturing != Some(n) => Task::none(),
            Message::Captured(_, at, shot) => {
                let opened = self.open(at, shot);
                Task::batch([opened, self.check_song_key()])
            }
            Message::SongKey => Task::none(), // remembered by `music::check_key`
            // Song search is opt-in: without a key the button opens its setup
            // (Settings > Song search) and nothing is recorded.
            Message::SongStart => self.song(self.has_song_key()),
            Message::Song(id, msg) => self.on_song(id, msg),
            Message::CaptureTimedOut(n) if self.capturing == Some(n) => {
                crate::log!(
                    "capture failed: no screenshot after {} s; try again",
                    CAPTURE_TIMEOUT.as_secs()
                );
                self.capturing = None;
                self.next = Purpose::Search;
                Task::none()
            }
            Message::CaptureTimedOut(_) => Task::none(),
            Message::Escape if self.capturing.is_some() => {
                crate::log!("[opencircle] capture cancelled");
                self.capturing = None;
                self.next = Purpose::Search;
                Task::none()
            }
            Message::Overlay(event) => self.on_overlay(event),
            Message::Escape if self.o.is_none() => Task::none(),
            Message::Escape if let Some(scroll) = self.scroll() => {
                scroll.stop(); // what was gathered opens in the editor
                Task::none()
            }
            // The editor has its own Esc (finish typing, close the picker, back).
            Message::Escape if self.o.edit.as_ref().is_some_and(Editing::is_open) => Task::none(),
            Message::Escape if std::env::var_os(TEST_SHOT).is_some() => self.test_shot(),
            // The editor's Esc went back to the bar: the same key's Escape
            // (it may come after) doesn't close the overlay too.
            Message::Escape if self.o.esc_back => {
                self.o.esc_back = false;
                Task::none()
            }
            Message::Escape => self.close(),
            Message::EscapeTaken
                if self.o.edit.is_some()
                    || self.o.esc_back
                    || self.capturing.is_some()
                    || self.scroll().is_some() =>
            {
                self.update(Message::Escape)
            }
            // A field took it: it was that field's Esc (a second one closes).
            Message::EscapeTaken => Task::none(),
            Message::Field(keys::Keyed::Left) => {
                self.o.bar_focus = None;
                Task::none()
            }
            // (a click put the focus in the bar's field: Tab goes on from there)
            Message::Field(keys::Keyed::Focus) => {
                self.o.bar_focus = Some(bar::Stop::Field);
                Task::none()
            }
            Message::Field(keys::Keyed::Copy) => self.on_overlay(overlay::Event::Key {
                letter: 'c',
                ctrl: true,
            }),
            Message::Editor(msg) => self.on_editor(msg),
            Message::Snip(msg) => self.on_snip(msg),
            Message::Confirm(place, result) => self.toast_for(place, result),
            Message::ThenClose(Some(pill)) => self.toast_then_close(pill),
            Message::ThenClose(None) => self.close(),
            Message::TextRead(copied) if self.o.toast.is_some() => {
                self.o.toast = Some(match copied {
                    Ok(()) => "Text copied",
                    Err(e) => {
                        crate::log!("copy text failed: {e}");
                        "No text copied"
                    }
                });
                background(|tx| {
                    std::thread::sleep(Duration::from_millis(900));
                    let _ = tx.unbounded_send(());
                })
                .map(|()| Message::ToastDone)
            }
            Message::TextRead(_) => Task::none(), // closed meanwhile
            Message::Scanned(area, scan) => {
                if self.o.selection.map(|s| s.area) == Some(area) {
                    self.o.scan = Some(scan);
                }
                Task::none()
            }
            Message::Qr(act) => self.on_qr(act),
            Message::ToastDone if self.o.toast.is_some() => self.close(),
            Message::ToastDone => Task::none(),
            Message::Panel(id, msg) => self.on_panel(id, msg),
            Message::Run(action) => self.run(action),
            Message::Tapped(what, read) => self.on_tapped(what, read),
            Message::SelectText => self.select_text(),
            Message::TextLines(area, read) => {
                self.on_text_lines(area, read);
                Task::none()
            }
            Message::BarGrab => {
                let at = self.bar_at();
                self.o.bar_drag = Some((None, at));
                (self.o.bar_now, self.o.bar_glide, self.o.bar_eased) = (Some(at), None, None);
                self.frame_times.borrow_mut().clear();
                Task::none()
            }
            // While held its target follows the pointer (kept on screen); the
            // frames ease it there (`Frame`), never a jump per mouse event.
            Message::BarMoved(p) => {
                if let Some((last, at)) = self.o.bar_drag {
                    let size = self.dock_size();
                    let at = match last {
                        Some(from) => {
                            let to = at + (p - from);
                            let max = |whole: f32, side: f32| (whole - side).max(0.0);
                            Point::new(
                                to.x.clamp(0.0, max(self.o.screen.width, size.width)),
                                to.y.clamp(0.0, max(self.o.screen.height, size.height)),
                            )
                        }
                        None => at,
                    };
                    self.o.bar_drag = Some((Some(p), at));
                }
                Task::none()
            }
            // Let go: it glides to the nearest spot, remembered.
            Message::BarReleased => match self.o.bar_drag.take() {
                Some((_, at)) => {
                    let size = self.dock_size();
                    self.o.bar_anchor = bar::nearest(at, self.o.screen, size, self.o.work_inset);
                    crate::log!("[opencircle] bar docked: {:?}", self.o.bar_anchor);
                    self.log_frames("bar drag");
                    self.o.bar_eased = None; // the glide goes on from where it is drawn
                    self.remember_bar()
                }
                None => Task::none(),
            },
            Message::CardMoved(p) => {
                if let Some((how, last)) = self.o.card_drag {
                    if let (Some(from), true) = (last, self.chatting()) {
                        self.o.chat_off += p - from;
                        self.o.chat_glide = None;
                    } else if let Some(from) = last {
                        let at = self.card_at();
                        self.card = Some(card::drag(at, how, p - from, self.o.screen));
                    }
                    self.o.card_drag = Some((how, Some(p)));
                }
                Task::none()
            }
            // Let go: the card stays where the user put it, until the overlay closes.
            Message::CardReleased => {
                match self.o.card_drag.take() {
                    Some((card::Drag::Move, _)) if self.chatting() => {
                        self.dock_chat(Instant::now());
                        self.log_frames("chat drag");
                    }
                    Some((card::Drag::Move, _)) => self.log_frames("card drag"),
                    Some((card::Drag::Resize, _)) => self.log_frames("card resize"),
                    None => {}
                }
                Task::none()
            }
            Message::AskInput(text) => {
                self.o.ask = text;
                Task::none()
            }
            // The AI field (the new design): Enter, its logo or its "AI ↵" badge.
            // A question asks about the selection (before one: the whole
            // screen); an empty field opens Ask AI with its presets.
            Message::AskSubmit | Message::AskButton => {
                let question = std::mem::take(&mut self.o.ask);
                let question = (!question.trim().is_empty()).then_some(question);
                match self.stacked().or_else(|| self.whole()) {
                    Some(crop) => self.open_panel(actions::Panel::Ask, crop, question),
                    None => Task::none(),
                }
            }
            Message::Pill(pill) => {
                self.o.pill = pill;
                self.on_pill(pill)
            }
            // Live translate runs over the live screen: the frozen one goes.
            Message::LiveToggle => {
                let closing = self.close();
                closing.chain(self.toggle_live(Instant::now()))
            }
            Message::EditScreen => {
                self.o.others.clear();
                self.o.word = None;
                self.select(Rectangle::with_size(self.o.screen));
                self.open_editor()
            }
            Message::TranslateScreen => {
                self.o.others.clear();
                self.o.word = None;
                self.select(Rectangle::with_size(self.o.screen));
                self.start_translate()
            }
            Message::Frame(now) => {
                self.o.esc_back = false;
                // Shown fully clear: once its first full frame is up (or after
                // REVEAL_WAIT), the window fades in and the opening's clock starts.
                let mut revealed = Task::none();
                if let Some(since) = self.o.reveal {
                    if self.first_draw.get().is_some()
                        && now.saturating_duration_since(since) < REVEAL_WAIT
                    {
                        return Task::none();
                    }
                    self.o.reveal = None;
                    self.o.appear = None;
                    self.frame_times.borrow_mut().clear();
                    revealed = windows::fade(self.overlay, true, windows::POPUP, false);
                }
                // a new result card grows open from its first frame
                self.o.card_open = match (&self.o.panel, self.o.card_open) {
                    (Some((id, _)), Some((was, shown, _))) if *id == was => {
                        Some((was, shown, shown.at(now)))
                    }
                    (Some((id, _)), _) => {
                        let shown = motion::Shown::opening(motion::WIDGET, now);
                        Some((*id, shown, shown.at(now)))
                    }
                    (None, _) => None,
                };
                // the chat card docking where it was let go
                if let Some((began, from, _)) = self.o.chat_glide {
                    let t = motion::progress(began, now, motion::DOCK, motion::Curve::Standard);
                    self.o.chat_glide = (t < 1.0).then_some((began, from, t));
                }
                let opening = self.appearing() || self.o.appear.is_none();
                let gliding = self.o.bar_glide.is_some();
                // every motion by the clock since it began (`motion`)
                self.o.appear = match self.o.appear {
                    Some((start, t)) if t < 1.0 => {
                        Some((start, motion::done(start, now, overlay::APPEAR)))
                    }
                    None => Some((now, motion::done(now, now, overlay::APPEAR))),
                    done => done,
                };
                let (target, was) = (self.swap_target(), self.o.swap);
                // to the toolbar (enter) or back to the bar (exit), from where it is
                self.o.swap_at = match self.o.swap_at {
                    Some(m @ (_, _, to)) if to == target => Some(m),
                    _ if self.o.swap == target => None,
                    _ => Some((now, self.o.swap, target)),
                };
                if let Some((began, from, to)) = self.o.swap_at {
                    let length = BAR_MORPH; // the design: the same rise both ways
                    let t = match self.o.purpose {
                        Purpose::Search => motion::done(began, now, length),
                        _ => 1.0, // Snip has no bar to turn into the toolbar
                    };
                    self.o.swap = from + (to - from) * t;
                    if t >= 1.0 {
                        self.o.swap_at = None;
                    }
                }
                if let Some((_, to)) = self.o.bar_drag {
                    let since = self.o.bar_eased.map_or(motion::FRAME, |t| now - t);
                    let at = bar::follow(self.bar_at(), to, since);
                    // caught up, the frames stop: the next move starts a fresh beat
                    self.o.bar_eased = (at != to).then_some(now);
                    self.o.bar_now = Some(at);
                } else {
                    let to = self.bar_target();
                    self.o.bar_now = Some(match (self.o.bar_now, self.o.bar_glide) {
                        // turning into the toolbar (or back): it stays on its spot
                        _ if self.o.swap != was => to,
                        (Some(at), _) if at == to => to,
                        // on its way there already
                        (Some(_), Some((began, from, end))) if end == to => {
                            let t = motion::progress(began, now, GLIDE, motion::Curve::Standard);
                            from + (to - from) * t
                        }
                        // a new place: it glides there from where it is
                        (Some(at), _) => {
                            self.o.bar_glide = Some((now, at, to));
                            at
                        }
                        (None, _) => to,
                    });
                    if self.o.bar_now == Some(to) {
                        self.o.bar_glide = None;
                    }
                }
                if gliding && self.o.bar_glide.is_none() && !opening {
                    self.log_frames("bar glide");
                }
                if !opening || self.appearing() {
                    return revealed;
                }
                // The opening animation is over: its frame times to the log
                // (the VM's budget), then the text reader's helper starts.
                self.log_frames("opening animation");
                Task::batch([revealed, self.warm()])
            }
            Message::ScreenText(opening, band, read)
                if opening == self.opening && self.o.frame.is_some() =>
            {
                self.o.reading = None;
                match read {
                    Ok(lines) => {
                        let system = self.o.text_by == Some(ocr::Source::System);
                        let source = self.o.text_by.unwrap_or(ocr::Source::Helper);
                        let screen = self.o.screen;
                        // (Windows OCR's closer read replaces its quick one)
                        let first = self.o.screen_text.add_read(source, band, lines, screen);
                        // (the helper: its first band and its last, the VM's timings)
                        let whole = self.o.screen_text.is_whole(self.o.screen.height);
                        if system || first || whole {
                            let what = match system || whole {
                                true => "screen text ready",
                                false => "first screen text",
                            };
                            match self.o.appear {
                                Some((at, _)) => crate::log!(
                                    "[opencircle] {what} {} ms after the first frame",
                                    at.elapsed().as_millis()
                                ),
                                None => crate::log!("[opencircle] {what} before the first frame"),
                            }
                        }
                        let done = self.more_text();
                        let released = self.release_held();
                        Task::batch([done, released, self.read_more()])
                    }
                    // Windows OCR couldn't (no recognizer, #63): the helper reads
                    Err(e) if self.o.text_by == Some(ocr::Source::System) => {
                        crate::log!("[opencircle] Windows OCR didn't read the screen: {e}");
                        self.o.text_by = Some(ocr::Source::Helper);
                        self.note_reader();
                        let released = self.release_held();
                        Task::batch([released, self.warm(), self.read_more()])
                    }
                    // (the next press tries again; the helper backs off by itself)
                    Err(e) => {
                        crate::log!("[opencircle] reading the screen's text failed: {e}");
                        self.release_held()
                    }
                }
            }
            Message::ScreenText(..) => Task::none(), // closed or opened again meanwhile
            // Off Windows (no Windows OCR, #62) the whole screen is read now,
            // gently, band by band from the pointer out: the I-beam shows over
            // text before any press, as it does on Windows.
            Message::Warmed(opening) if opening == self.opening && !cfg!(windows) => {
                self.o.text_used = true;
                self.read_more()
            }
            Message::Warmed(_) => Task::none(),
            // Only on this click: nothing is downloaded without it.
            Message::ReaderDownload if self.o.frame.is_some() => {
                self.o.reader = Reader::Downloading(0.0);
                if cfg!(test) {
                    return Task::none(); // never the network
                }
                // ponytail: closed mid-download, the download goes on unseen; a
                // click on the next opening's note starts a second one (the
                // checksums catch a clash). Share one download if that shows up.
                let opening = self.opening;
                background(|tx| {
                    let progress = |p| {
                        let _ = tx.unbounded_send(Ok(p));
                    };
                    let done = text::download(progress).map(|()| text::Progress::Reading);
                    let _ = tx.unbounded_send(done);
                })
                .map(move |step| Message::Reader(opening, step))
            }
            Message::Reader(opening, step) if opening == self.opening && self.o.frame.is_some() => {
                match step {
                    Ok(text::Progress::Download(done)) => self.o.reader = Reader::Downloading(done),
                    Ok(text::Progress::Reading) => {
                        (self.o.ocr_ready, self.o.reader) = (text::models_present(), Reader::Idle);
                        self.note_reader();
                        return self.warm();
                    }
                    Err(e) => self.o.reader = Reader::Failed(e),
                }
                Task::none()
            }
            Message::ReaderDismiss => {
                self.o.reader = Reader::Idle;
                Task::none()
            }
            Message::ReaderDownload | Message::Reader(..) => Task::none(),
            // Leave full-screen while still shown (on Windows, leaving it while hidden
            // restores a placement saved while visible: shows it again), then shrink.
            // open() resizes it back before showing.
            Message::Drawn if self.hiding == Hiding::Empty => {
                self.hiding = Hiding::Shrinking(0);
                window::set_mode(self.overlay, Mode::Windowed)
                    .chain(window::resize(self.overlay, Size::new(1.0, 1.0)))
            }
            Message::Resized(size)
                if matches!(self.hiding, Hiding::Shrinking(_))
                    && size.width.max(size.height) < SHRUNK =>
            {
                self.hiding = Hiding::Shrunk;
                Task::none()
            }
            Message::Drawn
                if matches!(
                    self.hiding,
                    Hiding::Shrunk | Hiding::Shrinking(SHRINK_FRAMES..)
                ) =>
            {
                self.hiding = Hiding::No;
                // Panel threads (PNG, HTTP, OCR I/O) leave freed memory in their
                // own malloc arenas; give it back to the OS while idle.
                crate::sys::mem::trim_heap();
                window::set_mode(self.overlay, Mode::Hidden)
            }
            Message::Drawn => {
                if let Hiding::Shrinking(frames) = &mut self.hiding {
                    *frames += 1;
                }
                Task::none()
            }
            Message::Resized(_) => Task::none(),
            Message::TestShot(shot) => {
                save_test_shot(&shot);
                self.close()
            }
        }
    }

    /// From the update thread.
    pub(in crate::ui) fn on_update(&mut self, event: crate::update::Event) -> Task<Message> {
        use crate::update::Event;
        match event {
            Event::Status(status) => self
                .windows
                .update(windows::Msg::UpdateStatus(status))
                .map(Message::Windows),
            Event::Manual {
                version,
                notes,
                url,
            } => self
                .windows
                .update_card(update_card::Card::available(&version, &notes, &url))
                .map(Message::Windows),
            Event::Ready(staged) => {
                let toast = self
                    .windows
                    .toast(&update_card::ready_toast(&staged.version), true, None, 5000)
                    .map(Message::Windows);
                self.update = Some(staged);
                // the notice stays readable before this process exits to update
                let later = background(|tx| {
                    std::thread::sleep(Duration::from_secs(6));
                    let _ = tx.unbounded_send(());
                })
                .map(|()| Message::UpdateTick);
                Task::batch([toast, later])
            }
        }
    }

    /// Nothing of OpenCircle is in use: no overlay (editor, snip, panel), capture,
    /// live translate, Settings, onboarding or pinned image.
    pub(in crate::ui) fn is_idle(&self) -> bool {
        self.o.is_none()
            && self.capturing.is_none()
            && self.live.is_none()
            && self.hiding == Hiding::No
            && !self.windows.busy()
    }

    /// Installs the waiting update now if idle (then this process exits), else
    /// looks again in a minute.
    pub(in crate::ui) fn try_update(&mut self) -> Task<Message> {
        if self.update.is_none() {
            return Task::none();
        }
        if !self.is_idle() {
            return background(|tx| {
                std::thread::sleep(Duration::from_secs(60));
                let _ = tx.unbounded_send(());
            })
            .map(|()| Message::UpdateTick);
        }
        let Some(staged) = self.update.take() else {
            return Task::none();
        };
        match crate::update::install(&staged) {
            Ok(()) => iced::exit(),
            Err(e) => {
                crate::log!("[opencircle] update {}: {e}", staged.version);
                // can't replace itself (e.g. installed for all users): the page instead
                self.windows
                    .update_card(update_card::Card::available(
                        &staged.version,
                        &staged.notes,
                        &staged.url,
                    ))
                    .map(Message::Windows)
            }
        }
    }
}

/// The messages that mean something while no overlay is open (a trigger, a
/// capture's answer, the other windows, live translate, the hide steps, a
/// Copy / Save pill, a snip timer's shot); every other one needs the opening.
pub(in crate::ui) fn works_closed(message: &Message) -> bool {
    matches!(
        message,
        Message::Trigger(_)
            | Message::Captured(..)
            | Message::CaptureTimedOut(_)
            | Message::Escape
            | Message::EscapeTaken
            | Message::Windows(_)
            | Message::Live(_)
            | Message::Drawn
            | Message::Resized(_)
            | Message::Confirm(..)
            | Message::SongKey
            | Message::UpdateTick
            | Message::TestShot(_)
            | Message::Snip(snip::Msg::Shoot(_))
    )
}
