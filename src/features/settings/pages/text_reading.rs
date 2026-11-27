//! Text reading (`settings-window.js` "TEXT READING (OCR)"): who reads the
//! screen's text (Windows: Windows OCR or the downloaded reader), with a note
//! and a way to add a missing language. Listed on Windows only (Linux has the
//! one reader). The design's "Keep ready", GPU and NVIDIA rows are not
//! settings OpenCircle has.
use crate::shared::kit::icon::Icon;
use crate::t;
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, State};
use crate::features::translate::lang;
use crate::shared::app_language::S;
use crate::shared::config::TextReader;
use crate::shared::kit::button::Kind;
use crate::shared::kit::choice_card::{Choice, Mark, choice_cards};
use crate::shared::kit::section::section;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::text::sub;
use crate::shared::kit::tokens::{Role, space};

pub fn view(state: &State) -> Element<'_, Message> {
    section_for(state, true)
}

/// The Text reading section on Circle to Search: engine chooser on Windows only (`windows`),
/// plus the Reading language row (bound to `translate.source`, which OCR uses on both platforms).
pub fn section_for(state: &State, windows: bool) -> Element<'_, Message> {
    let source = super::super::state::SOURCES
        .iter()
        .find(|l| l.0.code == state.draft.translate.source)
        .copied();
    let reading_lang = setting_row::row(
        title(t!(t_read_lang)),
        t!(t_read_lang_d),
        crate::shared::kit::dropdown::dropdown(
            &super::super::state::SOURCES,
            source,
            &state.draft.translate.source,
            |l: super::super::state::Named| Message::Source(l.0),
        ),
    );
    if !windows {
        return section(
            Some(crate::shared::app_language::tr(S::nav_text)),
            None,
            || vec![reading_lang],
        );
    }
    let reader = state.draft.selection.text_reader;
    let engines = [
        (
            TextReader::Auto,
            Mark::Glyph(Icon::Refresh.glyph()),
            t!(t_auto),
            t!(t_auto_d),
        ),
        (
            TextReader::Windows,
            Mark::Glyph(Icon::Zap.glyph()),
            t!(t_fast),
            t!(t_fast_d),
        ),
        (
            TextReader::PpOcr,
            Mark::Glyph(Icon::Target.glyph()),
            t!(t_accurate),
            t!(t_accurate_d),
        ),
    ];
    let cards: Vec<_> = (engines.iter())
        .map(|&(_, mark, title, desc)| Choice { mark, title, desc })
        .collect();
    let chosen = engines.iter().position(|e| e.0 == reader).unwrap_or(0);
    let mut control = column![choice_cards(&cards, chosen, move |i| Message::TextReader(
        engines[i].0
    ))]
    .spacing(space::GAP);
    if let Some(note) = state.reader_note() {
        let add = setting_row::action(
            t!(t_add),
            Kind::Elevated,
            Some(Message::OpenLanguageSettings),
        );
        control = control.push(sub(Role::ROW_DESC, note)).push(add);
    }
    let engine = setting_row::block(title(t!(t_engine)), t!(t_engine_desc_src), control);
    section(
        Some(crate::shared::app_language::tr(S::nav_text)),
        None,
        || vec![engine, reading_lang],
    )
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
        crate::features::text_reading::system::missing(&self.recognizers, &wanted)
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
