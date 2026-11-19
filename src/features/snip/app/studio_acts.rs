//! The Studio's picture panel: Copy text, Translate, Ask AI on the open
//! picture. The work is the one Snip's More and main mode's bar run
//! (`read_crop`, `pill::Session`, the pinned AI chat / `ai_chat::hand_off`);
//! only the pictures differ (`Studio::read_picture` / `translate_frame` /
//! `annotated`: Copy text and Translate see the clean capture with Blur /
//! Redact / Spotlight-dimmed parts painted out, Ask AI the annotated
//! picture). Missing models show the overlay's own download card inside the
//! Studio; the row runs when the download is done.
use iced::Task;

use crate::app::App;
use crate::app::background::background;
use crate::app::message::Message;
use crate::app::windows::Msg as W;
#[cfg(test)]
use crate::features::snip::app::snip_more::MODELS_HERE;
use crate::features::snip::app::snip_more::{
    hand_off, hands_off, models_present, pill, read_crop, text_download, text_progress,
};
use crate::features::snip::screens::studio::{Card, Step, What};
use crate::features::snip::snip_more::{self as more, Row};
use crate::features::text_reading;
use crate::shared::app_language::S;
use crate::shared::config;
use crate::t;

impl App {
    /// A picture-panel row, or its quick-action key while the Studio has the keyboard.
    pub(crate) fn studio_act(&mut self, row: Row) -> Task<Message> {
        match row {
            Row::CopyText => self.studio_copy_text(),
            Row::AskAi => self.studio_ask(),
            Row::Translate => self.studio_translate_row(),
        }
    }

    fn studio_say(&mut self, text: &str, ok: bool) -> Task<Message> {
        self.windows.say(text, ok).map(Message::Windows)
    }

    /// The picture's text (hidden parts out) on the clipboard, the Copied pill.
    fn studio_copy_text(&mut self) -> Task<Message> {
        let Some((crop, hidden)) = self.windows.studio().and_then(|s| s.read_picture()) else {
            return Task::none();
        };
        let setting = config::current().selection.text_reader;
        let helper = text_reading::route(setting, Some(text_reading::system::language()))
            == text_reading::Source::Helper;
        if helper && !models_present() {
            // the overlay's card, in the Studio; Download reads it, then copies
            if let Some(s) = self.windows.studio_mut() {
                s.set_card(Some(Card {
                    what: What::Reader,
                    bytes: text_reading::download_size(),
                    step: Step::Ask,
                }));
            }
            return Task::none();
        }
        if let Some(s) = self.windows.studio_mut() {
            s.set_busy(Some(Row::CopyText));
        }
        background(move |tx| {
            let _ = tx.unbounded_send(read_text(&crop, setting, &hidden));
        })
        .map(|r| Message::Windows(W::StudioText(r)))
    }

    /// Ask AI with the annotated picture: the browser provider opens Google AI
    /// Mode with it; an AI provider asks in the pinned chat (a new window
    /// docked on the pointer's screen, or the chat already pinned), as main
    /// mode does.
    fn studio_ask(&mut self) -> Task<Message> {
        let Some(crop) = self.windows.studio().and_then(|s| s.annotated()) else {
            return Task::none();
        };
        if !hands_off() {
            return self.ask_from_window(crop);
        }
        background(move |tx| {
            let _ = tx.unbounded_send(hand_off(&crop, None, true));
        })
        .map(|r| Message::Windows(W::StudioToast(r)))
    }

    /// Translate the picture in place; while it is on the row reads Show
    /// original and takes it back.
    fn studio_translate_row(&mut self) -> Task<Message> {
        let Some(studio) = self.windows.studio_mut() else {
            return Task::none();
        };
        self.studio_translate = None; // an older one (or the one being shown)
        if studio.translated() {
            studio.show_original();
            studio.set_busy(None);
            return Task::none();
        }
        let Some((frame, area)) = studio.translate_frame() else {
            return Task::none();
        };
        studio.set_busy(Some(Row::Translate));
        let epoch = studio.epoch();
        let (session, task) =
            pill::Session::start(frame, area, 1.0, self.cache.clone(), models_present(), 0);
        self.translates = self.translates.wrapping_add(1);
        let id = self.translates;
        self.studio_translate = Some((id, epoch, session));
        let after = self.studio_sync();
        let task = task.map(move |m| Message::StudioTranslate(id, m));
        Task::batch([after, task])
    }

    /// The session's answers: the translation goes into the picture as it comes.
    pub(crate) fn on_studio_translate(&mut self, id: u32, msg: pill::Msg) -> Task<Message> {
        let Some((_, epoch, session)) = self.studio_translate.as_mut().filter(|t| t.0 == id) else {
            return Task::none();
        };
        let current = self.windows.studio().map(|s| s.epoch());
        if current != Some(*epoch) {
            self.studio_translate = None; // another picture is open now
            return Task::none();
        }
        let task = session
            .update(msg)
            .map(move |m| Message::StudioTranslate(id, m));
        if let Some((from, area, whole)) = session.translated() {
            let (from, area) = (from.clone(), area);
            if let Some(studio) = self.windows.studio_mut() {
                studio.show_translation(&from, area, whole);
            }
        }
        Task::batch([self.studio_sync(), task])
    }

    /// The Studio shows what the session is at: its download card (asking or
    /// fetching), the spinner while it translates; a failure has no card
    /// here, so the picture goes back and a pill says why.
    fn studio_sync(&mut self) -> Task<Message> {
        let Some((_, _, session)) = self.studio_translate.as_ref() else {
            return Task::none();
        };
        let card = session.fetching().map(|f| Card {
            what: match f.what {
                pill::Fetch::Pair => What::Pair,
                pill::Fetch::Scripts(names) => What::Scripts(names),
                pill::Fetch::Reader => What::Reader,
            },
            bytes: f.bytes,
            step: match (f.failed, f.share) {
                (Some(why), _) => Step::Failed(why),
                (None, Some(share)) => Step::At(share),
                (None, None) => Step::Ask,
            },
        });
        let working = card.is_none() && !session.is_shown();
        let stuck = session.stuck();
        if let Some(studio) = self.windows.studio_mut() {
            studio.set_card(card);
            studio.set_busy(working.then_some(Row::Translate));
        }
        match stuck {
            Some(pill::Stuck::Failed(why)) => {
                self.studio_translate = None;
                if let Some(studio) = self.windows.studio_mut() {
                    studio.show_original();
                    studio.set_busy(None);
                }
                self.studio_say(&why, false)
            }
            _ => Task::none(),
        }
    }

    /// The card's Download / Try again: the translation's models through its
    /// session, the text reader's directly; the row runs when they are in.
    pub(crate) fn studio_download(&mut self) -> Task<Message> {
        if let Some((id, _, session)) = self.studio_translate.as_mut() {
            let id = *id;
            let task = session
                .update(pill::Msg::Download)
                .map(move |m| Message::StudioTranslate(id, m));
            return Task::batch([self.studio_sync(), task]);
        }
        let Some(epoch) = self.windows.studio().map(|s| s.epoch()) else {
            return Task::none();
        };
        if cfg!(test) {
            return Task::none(); // never the network
        }
        background(move |tx| {
            let progress = |p| {
                let _ = tx.unbounded_send(Ok(p));
            };
            let done = text_download(progress)
                .map(|()| text_progress::Reading)
                .map_err(|e| crate::features::snip::app::snip_more::why_key(&e));
            let _ = tx.unbounded_send(done);
        })
        .map(move |step| Message::StudioReader(epoch, step))
    }

    /// The reader download's steps: progress on the card; done, Copy text runs.
    pub(crate) fn on_studio_reader(
        &mut self,
        epoch: u32,
        step: Result<text_progress, S>,
    ) -> Task<Message> {
        let Some(studio) = self.windows.studio_mut().filter(|s| s.epoch() == epoch) else {
            return Task::none();
        };
        let card = |step| {
            Some(Card {
                what: What::Reader,
                bytes: text_reading::models::full_size(),
                step,
            })
        };
        match step {
            Ok(text_progress::Download(share)) => studio.set_card(card(Step::At(share))),
            Ok(text_progress::Reading) => {
                studio.set_card(None);
                return self.studio_act(Row::CopyText);
            }
            Err(why) => studio.set_card(card(Step::Failed(
                crate::shared::app_language::tr(why).into(),
            ))),
        }
        Task::none()
    }

    /// The card's Not now / Cancel: what waited on it goes (a translation's
    /// session, with the picture back as it was).
    pub(crate) fn studio_dismissed(&mut self) -> Task<Message> {
        if self.studio_translate.take().is_some()
            && let Some(studio) = self.windows.studio_mut()
        {
            studio.show_original();
        }
        Task::none()
    }
}

/// Blocking: the picture's text, hidden parts left out; or why not.
fn read_text(
    crop: &crate::shared::picture::Crop,
    setting: config::TextReader,
    hidden: &more::Hidden,
) -> Result<String, String> {
    let blocks = read_crop(crop, setting, 1.0).map_err(|e| {
        crate::log!("[opencircle] Studio: the text was not read: {e}");
        t!(st_read_failed).to_string()
    })?;
    let text = more::text_of(&more::visible(blocks, hidden));
    if text.trim().is_empty() {
        return Err(t!(sn_no_text).to_string());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::Trigger;
    use crate::features::snip::app::snip_more::text_progress;
    use crate::features::snip::screens::studio as st;
    use crate::shared::picture::Crop;

    fn app() -> App {
        let mut app = App::idle(iced::window::Id::unique());
        let rgba = [200, 120, 40, 255].repeat(100 * 60);
        let crop = Crop {
            width: 100,
            height: 60,
            rgba,
        };
        let _ = app.windows.open_studio(Some(st::Picture::Crop(crop)));
        app
    }

    fn pixels(app: &App) -> Vec<u8> {
        app.windows.studio().unwrap().read_picture().unwrap().0.rgba
    }

    /// Translate with the models missing: the overlay's download card comes up
    /// inside the Studio, the picture is as it was; Not now drops the session.
    #[test]
    fn translate_without_models_asks_with_the_card() {
        MODELS_HERE.set(Some(false));
        let mut app = app();
        let before = pixels(&app);
        let _ = app.studio_act(Row::Translate);
        let studio = app.windows.studio().unwrap();
        assert!(studio.card().is_some_and(|c| c.step == Step::Ask));
        assert!(app.studio_translate.is_some(), "waits for the click");
        assert_eq!(pixels(&app), before);
        assert!(app.windows.toasts().is_empty());
        let _ = app.studio_dismissed();
        assert!(app.studio_translate.is_none());
        assert_eq!(pixels(&app), before);
    }

    /// Copy text with the reader's models missing asks with the card; the
    /// download's steps show on it; done, the card goes and the row runs (the
    /// spinner shows while it reads); a failure offers it again.
    #[test]
    fn copy_text_without_models_asks_downloads_then_reads() {
        MODELS_HERE.set(Some(false));
        let mut app = app();
        let _ = app.studio_act(Row::CopyText);
        let epoch = app.windows.studio().unwrap().epoch();
        let step = |app: &App| app.windows.studio().unwrap().card().map(|c| c.step.clone());
        assert_eq!(step(&app), Some(Step::Ask));
        // (Download itself is the network: the tests drive its answers)
        let _ = app.on_studio_reader(epoch, Ok(text_progress::Download(0.5)));
        assert_eq!(step(&app), Some(Step::At(0.5)));
        let _ = app.on_studio_reader(epoch, Err(S::md_err_offline));
        assert!(matches!(step(&app), Some(Step::Failed(_))));
        // an answer for another picture is dropped
        let _ = app.on_studio_reader(epoch + 1, Ok(text_progress::Reading));
        assert!(step(&app).is_some());
        MODELS_HERE.set(Some(true));
        let _ = app.on_studio_reader(epoch, Ok(text_progress::Reading));
        assert_eq!(step(&app), None);
        assert_eq!(
            app.windows.studio().unwrap().busy(),
            Some(Row::CopyText),
            "it reads: the row spins"
        );
    }

    /// Ask AI with an AI provider opens the pinned chat in its own window; a
    /// second ask goes into the same chat.
    #[test]
    fn ask_ai_opens_the_pinned_chat_and_asks_into_it_again() {
        let mut app = app();
        assert!(app.chat.is_none());
        let _ = app.studio_act(Row::AskAi);
        let first = app.chat.as_ref().map(|c| (c.id, c.panel.pinned()));
        assert!(matches!(first, Some((_, true))), "pinned from the start");
        assert!(app.chat_win.is_some(), "its own window");
        let _ = app.studio_act(Row::AskAi);
        assert_eq!(app.chat.as_ref().map(|c| c.id), first.map(|f| f.0));
        assert_eq!(app.panels, first.unwrap().0, "no second chat");
    }

    /// The row's answer for another picture is dropped, and Show original
    /// takes the translation back.
    #[test]
    fn show_original_drops_the_session() {
        MODELS_HERE.set(Some(true));
        let mut app = app();
        let _ = app.studio_act(Row::Translate);
        assert!(app.windows.studio().unwrap().translated());
        let _ = app.studio_act(Row::Translate);
        assert!(app.studio_translate.is_none());
        assert!(!app.windows.studio().unwrap().translated());
    }

    /// The quick-action key of a row acts on the Studio's picture while the
    /// window has the keyboard, instead of starting a capture.
    #[test]
    fn a_quick_key_acts_on_the_studio_picture() {
        MODELS_HERE.set(Some(false));
        let mut app = app();
        let qa = (config::current().quick_actions.into_iter())
            .find(|q| q.action == Row::Translate.quick() && !q.live)
            .unwrap();
        assert!(!app.windows.studio_takes_quick_keys(), "no keyboard yet");
        let _ = app
            .windows
            .studio_mut()
            .unwrap()
            .update(st::Msg::Focus(true));
        let _ = app.update(Message::Trigger(Trigger::Quick(Instant::now(), qa)));
        assert!(
            app.windows.studio().unwrap().card().is_some(),
            "Translate ran: models missing"
        );
    }
}
