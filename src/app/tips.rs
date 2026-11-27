//! The one-time tips on the overlay (R-17): when a feature becomes relevant the engine
//! (`shared::tips`) says yes or no; a yes draws the hint against its widget (the main bar,
//! the Snip toolbar, the translate pill) until Got it, the x, "Don't show tips" or the
//! fade (`FADE_MS`, counts as seen). Nothing here styles: `screens::tip` is the view.
use std::time::Duration;

use iced::widget::pin;
use iced::{Element, Point, Rectangle, Size, Task};

use super::App;
use super::message::{Message, Purpose};
use super::screens::tip;
use crate::features::translate::screens::pill::view::pill_rect;
use crate::shared::config;
use crate::shared::tips::{self, Session, Tip};

/// Between a widget and its tip.
const GAP: f32 = 6.0;
/// Least distance to the screen's edge.
const EDGE: f32 = 8.0;

/// What `App` keeps of the tips.
#[derive(Debug, Default)]
pub(crate) struct Tips {
    session: Session,
    /// The tip on screen now.
    pub(crate) active: Option<Tip>,
    /// A tip an event asked for (a drag began on a shape).
    ask: Option<Tip>,
    /// Tips the engine said no to (seen, off): not asked again this run.
    declined: Vec<Tip>,
    /// Tests that are not about tips keep them out of the picture.
    #[cfg(test)]
    pub(crate) muted: bool,
}

impl Tips {
    /// A new app's: tests that are not about tips keep them out of the picture.
    pub(crate) fn idle() -> Tips {
        Tips {
            #[cfg(test)]
            muted: true,
            ..Tips::default()
        }
    }
}

impl App {
    /// The user is in a fast flow: a pick or a circle is being drawn, a long capture runs.
    fn fast_flow(&self) -> bool {
        self.o.drawing || self.scroll().is_some() || self.snip_drag().is_some()
    }

    /// A tip an event asks for (the editor: a drag began on a shape).
    pub(crate) fn ask_tip(&mut self, tip: Tip) {
        self.tips.ask = Some(tip);
    }

    /// What became relevant: an event's ask, else the state of the overlay.
    fn relevant_tip(&mut self) -> Option<Tip> {
        if let Some(tip) = self.tips.ask.take() {
            return Some(tip);
        }
        let searching = matches!(self.o.purpose, Purpose::Search) && self.o.edit.is_none();
        if searching && self.page_translated() && self.appeared() >= 1.0 {
            return Some(Tip::Original);
        }
        let bar = self.bottom_bar_shown()
            && self.appeared() >= 1.0
            && self.o.selection.is_none()
            && self.bar_window() == self.overlay;
        bar.then_some(Tip::Bar)
    }

    /// After every message: a tip whose widget is gone goes; else a relevant tip may show.
    pub(crate) fn tips_tick(&mut self) -> Task<Message> {
        if let Some(tip) = self.tips.active
            && (self.o.is_none() || self.tip_at(tip).is_none())
        {
            self.tips.active = None; // (not seen: it was not read)
        }
        #[cfg(test)]
        if self.tips.muted {
            return Task::none();
        }
        if self.tips.active.is_some() || self.o.is_none() {
            return Task::none();
        }
        let Some(tip) = self.relevant_tip() else {
            return Task::none();
        };
        if self.tips.declined.contains(&tip) || self.tip_at(tip).is_none() {
            return Task::none();
        }
        let fast = self.fast_flow();
        let cfg = config::current().tips;
        if !self.tips.session.offer(&cfg, tip, fast) {
            if !fast {
                self.tips.declined.push(tip);
            }
            return Task::none();
        }
        self.tips.active = Some(tip);
        // the fade: a thread sleeps, the UI never waits
        let (done, wait) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(tips::FADE_MS));
            let _ = done.send(());
        });
        Task::future(wait).map(move |_| Message::TipFade(tip))
    }

    /// A tip's button, or its fade (a tip that is not the open one is a late answer).
    pub(crate) fn on_tip(&mut self, message: Message) -> Task<Message> {
        let Some(tip) = self.tips.active else {
            return Task::none();
        };
        let (seen, off) = match message {
            Message::TipFade(t) if t == tip => (true, false),
            Message::TipFade(_) => return Task::none(),
            Message::Tip(tip::Msg::GotIt | tip::Msg::Close) => (true, false),
            Message::Tip(tip::Msg::DontShow) => (true, true),
            _ => return Task::none(),
        };
        self.tips.active = None;
        config::update_in_background(move |c| {
            if seen {
                tips::mark_seen(&mut c.tips, tip);
            }
            if off {
                tips::hide_all(&mut c.tips);
            }
        });
        Task::none()
    }

    /// Where `tip`'s top-left goes in the overlay's window, against its widget (`None`: the
    /// widget is not on this window now).
    fn tip_at(&self, tip: Tip) -> Option<Point> {
        let size = tip::size(tip);
        let screen = self.o.screen;
        let clamp = |x: f32| x.clamp(EDGE, (screen.width - size.width - EDGE).max(EDGE));
        match tip {
            // above the main bar, centred on it
            Tip::Bar => {
                let bar = Rectangle::new(self.bar_at(), self.bar_size().1);
                (self.bottom_bar_shown() && self.bar_window() == self.overlay).then(|| {
                    Point::new(
                        clamp(bar.center_x() - size.width / 2.0),
                        bar.y - GAP - size.height,
                    )
                })
            }
            // under the translate pill, from its start edge
            Tip::Original => {
                let pill = pill_rect(screen);
                self.page_translated()
                    .then(|| Point::new(clamp(pill.x), pill.y + pill.height + GAP))
            }
            // under the Snip toolbar and the room of its options row (above both when the
            // screen has no room under them)
            Tip::SnipMove => {
                let edit = self.o.edit.as_ref().filter(|e| e.is_open())?;
                if self.toolbar_on().is_some_and(|w| w != self.overlay) {
                    return None;
                }
                let at = self.bar_at();
                let dock = edit.dock_size();
                let below = at.y + dock.height + GAP;
                let y = match below + size.height <= screen.height {
                    true => below,
                    false => at.y - (dock.height - edit.bar_size().height) - GAP - size.height,
                };
                Some(Point::new(
                    clamp(at.x + edit.bar_size().width / 2.0 - size.width / 2.0),
                    y,
                ))
            }
        }
    }

    /// The tip on screen, against its widget.
    pub(crate) fn tip_layer(&self) -> Option<Element<'_, Message>> {
        let tip = self.tips.active?;
        let at = self.tip_at(tip)?;
        let _: Size = tip::size(tip);
        Some(pin(tip::view(tip).map(Message::Tip)).position(at).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    use crate::Trigger;
    use crate::app::tests::{open_app, settle_bar};
    use crate::app::windows::{Msg as WinMsg, Windows};
    use crate::shared::config::Config;
    use crate::shared::kit::testing::shot;

    fn texts(app: &App) -> Vec<String> {
        shot::texts(app.view(app.overlay))
    }

    fn on_screen(app: &App) -> bool {
        texts(app).iter().any(|t| t == "Got it")
    }

    /// A user's session: a fresh app on `cfg`; the bar settles and may show its tip.
    fn session() -> App {
        let mut app = open_app(900, 500, 1.0);
        app.tips.muted = false;
        settle_bar(&mut app);
        app
    }

    /// R17-AC8: the main bar's tip shows when the bar first opens, as drawn (its words and
    /// the F1 key), Got it writes it as seen, and a later session (the same config) shows
    /// nothing: once per user.
    #[test]
    fn the_bar_tip_shows_once_per_user() {
        crate::shared::config::with_test_config(Config::default(), || {
            let mut app = session();
            assert_eq!(app.tips.active, Some(Tip::Bar));
            let words = texts(&app);
            assert!(words.iter().any(|t| t == "F1"), "{words:?}");
            let _ = app.update(Message::Tip(tip::Msg::GotIt));
            assert_eq!(app.tips.active, None);
            assert!(tips::seen(&config::current().tips, Tip::Bar));
            assert!(!on_screen(&app));
            // another session on the same file
            let app = session();
            assert_eq!(app.tips.active, None, "seen: never again");
        });
    }

    /// R17-AC9: one tip per session: with the bar's tip spent, another that becomes relevant
    /// (a drag over a shape in Snip) shows nothing.
    #[test]
    fn one_tip_per_session() {
        crate::shared::config::with_test_config(Config::default(), || {
            let mut app = session();
            assert_eq!(app.tips.active, Some(Tip::Bar));
            let _ = app.update(Message::Tip(tip::Msg::Close));
            app.ask_tip(Tip::SnipMove);
            let _ = app.tips_tick();
            assert_eq!(app.tips.active, None, "the session's one tip is spent");
        });
    }

    /// R17-AC10: never in a fast flow: while a circle is being drawn nothing shows, and the
    /// tip is still there for when it ends.
    #[test]
    fn no_tip_in_a_fast_flow() {
        crate::shared::config::with_test_config(Config::default(), || {
            let mut app = open_app(900, 500, 1.0);
            app.tips.muted = false;
            app.o.drawing = true;
            settle_bar(&mut app);
            assert_eq!(app.tips.active, None);
            app.o.drawing = false;
            let _ = app.tips_tick();
            assert_eq!(app.tips.active, Some(Tip::Bar));
        });
    }

    /// R17-AC11: a tip that fades (about 8 s) counts as seen: the fade message writes it.
    #[test]
    fn a_faded_tip_counts_as_seen() {
        crate::shared::config::with_test_config(Config::default(), || {
            let mut app = session();
            assert_eq!(app.tips.active, Some(Tip::Bar));
            // a fade of another tip is a late answer: nothing changes
            let _ = app.update(Message::TipFade(Tip::Original));
            assert_eq!(app.tips.active, Some(Tip::Bar));
            let _ = app.update(Message::TipFade(Tip::Bar));
            assert_eq!(app.tips.active, None);
            assert!(tips::seen(&config::current().tips, Tip::Bar));
        });
    }

    /// R17-AC12: "Show tips" off hides every tip; "Don't show tips" on the tip turns it off.
    #[test]
    fn show_tips_off_hides_all() {
        let mut cfg = Config::default();
        cfg.tips.show = false;
        crate::shared::config::with_test_config(cfg, || {
            let app = session();
            assert_eq!(app.tips.active, None);
        });
        crate::shared::config::with_test_config(Config::default(), || {
            let mut app = session();
            let _ = app.update(Message::Tip(tip::Msg::DontShow));
            let tips = config::current().tips;
            assert!(!tips.show && tips::seen(&tips, Tip::Bar));
        });
    }

    /// R17-AC13: an existing user (a file from before tips) still sees the tip once.
    #[test]
    fn an_existing_user_sees_the_tip() {
        let (old, _) = Config::parse("version = 15\ntheme = \"system\"\n");
        crate::shared::config::with_test_config(old, || {
            let app = session();
            assert_eq!(app.tips.active, Some(Tip::Bar));
        });
    }

    /// R17-AC16 (UX C5): in Snip, a drag that begins on a shape with a drawing tool still draws,
    /// and the one-time "Hold Ctrl and drag to move a shape" tip shows against the toolbar;
    /// Got it writes it as seen, so the next such drag shows nothing.
    #[test]
    fn a_drag_over_a_shape_in_snip_shows_the_move_tip_once() {
        use crate::features::settings::labs::BarButton;
        use crate::features::snip::annotate::{Message as E, Tool};
        use crate::features::snip::screens::pick;
        crate::shared::config::with_test_config(Config::default(), || {
            let mut app = open_app(1280, 800, 1.0);
            let edit = app.bar_message(BarButton::Edit).unwrap();
            let _ = app.update(edit);
            let area = Rectangle::new(Point::new(200.0, 150.0), Size::new(300.0, 200.0));
            let _ = app.snip_next(Some(pick::Next::Edit(area)));
            app.tips.muted = false; // (a Snip toolbar is on: the bar's own tip is not a candidate)
            let drag_over_shape = |app: &mut App| {
                let p = |x, y| Point::new(x, y);
                for m in [
                    E::Tool(Tool::Rect),
                    E::Press(p(250.0, 200.0)),
                    E::Move(p(350.0, 270.0)),
                    E::Release,
                    E::Press(p(260.0, 210.0)),
                    E::Move(p(300.0, 240.0)),
                    E::Release,
                ] {
                    let _ = app.update(Message::Annotate(m));
                }
            };
            drag_over_shape(&mut app);
            assert_eq!(app.tips.active, Some(Tip::SnipMove));
            let words = texts(&app);
            assert!(
                words.iter().any(|t| t == "and drag to move a shape"),
                "{words:?}"
            );
            let _ = app.update(Message::Tip(tip::Msg::GotIt));
            assert!(tips::seen(&config::current().tips, Tip::SnipMove));
            drag_over_shape(&mut app);
            assert_eq!(app.tips.active, None, "once per user");
        });
    }

    /// R17-AC14: F1 in one of our windows and the tray's "What can I do?" row open the card;
    /// Esc / its x close it; asking again brings the open one forward.
    #[test]
    fn f1_and_the_tray_open_the_card() {
        use crate::app::screens::what_card;
        let mut w = Windows::default();
        assert!(!w.what_card_open());
        let _ = w.update(WinMsg::F1);
        assert!(w.what_card_open(), "F1");
        let id = w.what_card_id().unwrap();
        // Esc in another window leaves the card alone; Esc in its own closes it
        let acts = |task: Task<WinMsg>| iced_runtime::task::into_stream(task).is_some();
        assert!(!acts(
            w.update(WinMsg::WhatCardEsc(iced::window::Id::unique()))
        ));
        assert!(acts(w.update(WinMsg::WhatCardEsc(id))));
        let _ = w.update(WinMsg::WhatCard(what_card::Msg::Close));
        // (the window closing is the OS's answer)
        let _ = w.update(WinMsg::Closed(id));
        assert!(!w.what_card_open());
        // the tray row runs the trigger the app opens it with
        assert!(matches!(
            crate::shared::platform::tray::Entry::WhatCanIDo.trigger(Instant::now()),
            Trigger::OpenWhatCard
        ));
        let mut app = open_app(400, 300, 1.0);
        let _ = app.update(Message::Trigger(Trigger::OpenWhatCard));
        assert!(app.windows.what_card_open(), "the tray");
    }
}
