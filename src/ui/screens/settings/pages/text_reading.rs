//! Text reading (`settings-window.js` "TEXT READING (OCR)"): who reads the
//! screen's text (Windows: Windows OCR or the downloaded reader), with a note
//! and a way to add a missing language. Listed on Windows only (Linux has the
//! one reader). The design's "Keep ready", GPU and NVIDIA rows are not
//! settings OpenCircle has.
use iced::Element;
use iced::widget::{Space, column, row};

use super::super::state::{Message, Pick, State};
use crate::config::TextReader;
use crate::translate::lang;
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::sub;
use crate::ui::kit::tokens::{Role, space};

pub fn view(state: &State) -> Element<'_, Message> {
    let reader = state.draft.selection.text_reader;
    let (from, t) = state.thumb(reader, |p| match p {
        Pick::Reader(v) => Some(v),
        _ => None,
    });
    let mut control = column![segmented(
        &[
            (TextReader::Auto, "Auto"),
            (TextReader::Windows, "Fast"),
            (TextReader::PpOcr, "Accurate"),
        ],
        reader,
        from,
        t,
        Message::TextReader,
    )]
    .spacing(space::GAP);
    if let Some(note) = state.reader_note() {
        let add = button(
            label("Add in Windows"),
            Kind::Elevated,
            Some(Message::OpenLanguageSettings),
        );
        control = control
            .push(sub(Role::ROW_DESC, note))
            .push(row![Space::new().width(iced::Fill), add]);
    }
    setting_row::row(
        title("Engine"),
        "Auto picks the best for the language on screen. Fast: Windows' own; Accurate: PaddleOCR",
        control,
    )
}

/// Windows' language settings (never installs anything itself).
pub fn open_language_settings() {
    #[cfg(windows)]
    {
        let run = crate::sys::open::open_system_settings("ms-settings:regionlanguage");
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
    pub(in super::super) fn reader_note(&self) -> Option<String> {
        let missing = self.missing_recognizers();
        if missing.is_empty() || self.draft.selection.text_reader == TextReader::PpOcr {
            return None;
        }
        let names: Vec<&str> = missing.iter().map(|c| lang::name(c)).collect();
        Some(format!(
            "Windows has no text recognition for {}. Using the downloaded reader.",
            names.join(", ")
        ))
    }
}
