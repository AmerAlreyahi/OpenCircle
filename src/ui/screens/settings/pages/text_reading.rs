//! Text reading (`settings-window.js` "TEXT READING (OCR)"): who reads the
//! screen's text (Windows: Windows OCR or the downloaded reader), with a note
//! and a way to add a missing language. Listed on Windows only (Linux has the
//! one reader). The design's "Keep ready", GPU and NVIDIA rows are not
//! settings OpenCircle has.
use crate::t;
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, Pick, State};
use crate::shared::config::TextReader;
use crate::shared::kit::button::Kind;
use crate::shared::kit::segmented::segmented;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::text::sub;
use crate::shared::kit::tokens::{Role, space};
use crate::translate::lang;

pub fn view(state: &State) -> Element<'_, Message> {
    let reader = state.draft.selection.text_reader;
    let (from, t) = state.thumb(reader, |p| match p {
        Pick::Reader(v) => Some(v),
        _ => None,
    });
    let mut control = column![segmented(
        &[
            (TextReader::Auto, t!(t_auto)),
            (TextReader::Windows, t!(t_fast)),
            (TextReader::PpOcr, t!(t_accurate)),
        ],
        reader,
        from,
        t,
        Message::TextReader,
    )]
    .spacing(space::GAP);
    if let Some(note) = state.reader_note() {
        let add = setting_row::action(
            t!(t_add),
            Kind::Elevated,
            Some(Message::OpenLanguageSettings),
        );
        control = control.push(sub(Role::ROW_DESC, note)).push(add);
    }
    setting_row::row(title(t!(t_engine)), t!(t_engine_desc_src), control)
}

/// Windows' language settings (never installs anything itself).
pub fn open_language_settings() {
    #[cfg(windows)]
    {
        let run =
            crate::shared::platform::sys::open::open_system_settings("ms-settings:regionlanguage");
        if let Err(e) = run {
            crate::log!("[opencircle] could not open Windows' language settings: {e}");
        }
    }
}

impl State {
    /// The languages Windows has no text recognition for, among the ones this
    /// user needs: the Windows display language and the translate source.
    fn missing_recognizers(&self) -> Vec<String> {
        let source = &self.draft.translate.source;
        let wanted: Vec<String> = (self.profile.iter())
            .chain((source != lang::AUTO).then_some(source))
            .cloned()
            .collect();
        crate::ocr::system::missing(&self.recognizers, &wanted)
    }

    /// The one quiet note under the engine row, if a language is missing.
    pub(crate) fn reader_note(&self) -> Option<String> {
        let missing = self.missing_recognizers();
        if missing.is_empty() || self.draft.selection.text_reader == TextReader::PpOcr {
            return None;
        }
        let names: Vec<&str> = missing
            .iter()
            .map(|c| crate::shared::app_language::language_name(c))
            .collect();
        Some(crate::shared::app_language::fmt(
            crate::shared::app_language::S::t_note_lang,
            &[&names.join(", ")],
        ))
    }
}
