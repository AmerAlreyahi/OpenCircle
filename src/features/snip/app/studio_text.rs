//! The Studio's selectable text, the App's part (R-16, `studio::text_select`): reads the picture's
//! words ONCE (when the user first starts "Select text") and runs the selection's menu rows. The
//! rows are main mode's: Copy, Search the web, Ask AI, Translate (the selected span of each line,
//! in place), Open link / Send email. Ask AI opens the chat the picture panel's Ask AI does, with
//! the selected text in its composer (the browser provider gets the text as its question).
use iced::Task;

use crate::app::App;
use crate::app::background::background;
use crate::app::message::Message;
use crate::app::windows::Msg as W;
use crate::features::circle::clipboard as copy;
use crate::features::circle::text_menu::Row;
use crate::features::snip::app::snip_more::{models_present, pill, read_crop};
use crate::features::snip::screens::studio::{Fetching, TextWork, text_lines};
use crate::features::snip::snip_more as more;
use crate::features::text_reading::{self, TextBlock, words::Line};
use crate::shared::config;
use crate::t;

impl App {
    /// The selection's work: read the words, or run a menu row on the selected text.
    pub(crate) fn studio_select(&mut self, work: TextWork) -> Task<Message> {
        match work {
            TextWork::Read => self.studio_read_words(),
            TextWork::Row(row, text, segments) => self.studio_text_row(row, text, segments),
        }
    }

    fn studio_read_words(&mut self) -> Task<Message> {
        let Some((crop, hidden)) = self.windows.studio().and_then(|s| s.read_picture()) else {
            return Task::none();
        };
        let setting = config::current().selection.text_reader;
        let helper = text_reading::route(setting, Some(text_reading::system::language()))
            == text_reading::Source::Helper;
        if helper && !models_present() {
            // the reader's card; the row again, once it is in
            if let Some(s) = self.windows.studio_mut() {
                s.cancel_text();
                s.set_card(Some(Fetching::reader()));
            }
            return Task::none();
        }
        let epoch = self.windows.studio().map_or(0, |s| s.epoch());
        background(move |tx| {
            let _ = tx.unbounded_send(read_words(&crop, setting, &hidden));
        })
        .map(move |r| Message::Windows(W::StudioWords(epoch, r)))
    }

    fn studio_text_row(
        &mut self,
        row: Row,
        text: String,
        segments: Vec<(String, iced::Rectangle)>,
    ) -> Task<Message> {
        match row {
            Row::Copy => background(move |tx| {
                let _ = tx.unbounded_send(copy::text(text).map(|()| copy::COPIED_PILL.to_string()));
            })
            .map(|r| Message::Windows(W::StudioToast(r))),
            Row::Search => self.search_text(text),
            Row::Open(entity) => self.open_entity(entity),
            Row::Ask => self.studio_ask_text(text),
            Row::Translate => self.studio_translate_text(segments),
            Row::SelectAll => Task::none(), // (the Studio's own)
        }
    }

    /// Ask AI about the selected text: the text only (main mode's menu path), never the picture.
    fn studio_ask_text(&mut self, text: String) -> Task<Message> {
        let Some(crop) = self.windows.studio().and_then(|s| s.annotated()) else {
            return Task::none();
        };
        self.ask_with_text(crop, crate::features::ask_ai::text_context(&text))
    }

    /// Translate the selected span of each line in place, as the picture panel's Translate does the
    /// whole picture (one session; Show original takes it back).
    fn studio_translate_text(&mut self, segments: Vec<(String, iced::Rectangle)>) -> Task<Message> {
        let Some(studio) = self.windows.studio_mut() else {
            return Task::none();
        };
        studio.cancel_text();
        if studio.translated() || segments.is_empty() {
            return Task::none();
        }
        let Some((frame, _)) = studio.translate_frame() else {
            return Task::none();
        };
        let lines: Vec<TextBlock> = segments
            .into_iter()
            .map(|(text, r)| TextBlock {
                text,
                bounds: [r.x, r.y, r.width, r.height],
                confidence: 1.0,
                words: Vec::new(),
            })
            .collect();
        let epoch = studio.epoch();
        let Some((session, task)) =
            pill::Session::start_text(frame, lines, 1.0, self.cache.clone())
        else {
            return Task::none();
        };
        self.studio_translate = None;
        self.translates = self.translates.wrapping_add(1);
        let id = self.translates;
        self.studio_translate = Some((id, epoch, session));
        let after = self.studio_sync();
        let task = task.map(move |m| Message::StudioTranslate(id, m));
        Task::batch([after, task])
    }
}

/// Blocking: the picture's words (hidden parts left out) as lines in image px; or why not.
fn read_words(
    crop: &crate::shared::picture::Crop,
    setting: config::TextReader,
    hidden: &more::Hidden,
) -> Result<Vec<Line>, String> {
    let blocks = read_crop(crop, setting, 1.0).map_err(|e| {
        crate::log!("[opencircle] Studio: the words were not read: {e}");
        t!(st_read_failed).to_string()
    })?;
    Ok(text_lines(
        more::visible(blocks, hidden),
        &crop.rgba,
        crop.width,
        crop.height,
    ))
}
