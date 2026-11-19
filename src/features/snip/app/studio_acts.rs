//! The Studio's picture panel: Copy text, Translate, Ask AI on the open
//! picture. The work is the one Snip's More and main mode's bar run
//! (`read_crop`, `pill::Session`, `ai_chat::hand_off`); only the pictures
//! differ (`Studio::read_picture` / `translate_frame` / `annotated`: Copy text
//! and Translate see the clean capture with Blur / Redact / Spotlight-dimmed
//! parts painted out, Ask AI the annotated picture).
use iced::Task;

use crate::app::App;
use crate::app::background::background;
use crate::app::message::Message;
use crate::app::windows::Msg as W;
#[cfg(test)]
use crate::features::snip::app::snip_more::MODELS_HERE;
use crate::features::snip::app::snip_more::{hand_off, hands_off, models_present, pill, read_crop};
use crate::features::snip::snip_more::{self as more, Row};
use crate::features::text_reading;
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
            return self.studio_say(t!(st_no_models), false);
        }
        background(move |tx| {
            let _ = tx.unbounded_send(read_text(&crop, setting, &hidden));
        })
        .map(|r| Message::Windows(W::StudioText(r)))
    }

    /// Ask AI with the annotated picture: the browser provider opens Google AI
    /// Mode with it, as in main mode.
    fn studio_ask(&mut self) -> Task<Message> {
        let Some(crop) = self.windows.studio().and_then(|s| s.annotated()) else {
            return Task::none();
        };
        // ponytail: the in-app chat docks on the overlay's screen (`chat_rect`);
        // with no overlay it has no place yet: only the browser path here.
        if !hands_off() {
            return self.studio_say(t!(st_ask_chat), false);
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
            return Task::none();
        }
        let Some((frame, area)) = studio.translate_frame() else {
            return Task::none();
        };
        let epoch = studio.epoch();
        let (session, task) =
            pill::Session::start(frame, area, 1.0, self.cache.clone(), models_present(), 0);
        self.translates = self.translates.wrapping_add(1);
        let id = self.translates;
        self.studio_translate = Some((id, epoch, session));
        let stuck = self.studio_stuck();
        let task = task.map(move |m| Message::StudioTranslate(id, m));
        Task::batch([stuck, task])
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
        Task::batch([self.studio_stuck(), task])
    }

    /// A session that needs the user's download or failed: it has no card
    /// here, so the picture goes back and a pill says why.
    fn studio_stuck(&mut self) -> Task<Message> {
        let Some(stuck) = (self.studio_translate.as_ref()).and_then(|(_, _, s)| s.stuck()) else {
            return Task::none();
        };
        self.studio_translate = None;
        if let Some(studio) = self.windows.studio_mut() {
            studio.show_original();
        }
        let words = match stuck {
            pill::Stuck::Models => t!(st_no_models).to_string(),
            pill::Stuck::Failed(why) => why,
        };
        self.studio_say(&words, false)
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

    /// Translate with the models missing has no card in the Studio: the
    /// picture is as it was and a pill says why (nothing downloads by itself).
    #[test]
    fn translate_without_models_leaves_the_picture_and_says_why() {
        MODELS_HERE.set(Some(false));
        let mut app = app();
        let before = pixels(&app);
        let _ = app.studio_act(Row::Translate);
        assert!(app.studio_translate.is_none());
        assert!(!app.windows.studio().unwrap().translated());
        assert_eq!(pixels(&app), before);
        assert_eq!(app.windows.toasts().len(), 1, "the pill");
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
        assert_eq!(
            app.windows.toasts().len(),
            1,
            "Translate ran: models missing"
        );
    }
}
