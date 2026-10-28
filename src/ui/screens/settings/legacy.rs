//! Today's Settings pages, drawn by the old form widgets inside the new shell
//! (ported to the kit page by page, the UI plan's step 33), and the custom
//! look's rows (Look & behaviour, until the kit has a slider and a picker).

use iced::widget::column;
use iced::widget::tooltip::Position;
use iced::{Color, Element};

use crate::config::{
    BracketColour, BracketStyle, ColourSet, EdgeMode, EdgeStyle, Engine, Format, Provider,
    SnipAfter, SnipConfig, SongSource, TextReader,
};
use crate::labs::{Feature, Labs};
use crate::mt;
use crate::music::SERVICE;
use crate::secrets::Secret;
use crate::translate::lang::{self, LANGUAGES};
use crate::ui::theme;
use crate::ui::widgets::{
    self, FORM_WIDTH, FieldState, button_row, caption, colour_swatch, divider, error_text,
    form_button, form_row, segmented, shortcut_field, slider, text_field, tip, value_field,
};

use super::*;

impl KeyField {
    pub fn view(&self) -> Element<'_, KeyMessage> {
        let l = self.labels;
        let mut col = column![].spacing(8).width(FORM_WIDTH);
        let wait = self.busy.map(Busy::label);
        match (&self.status, self.replacing) {
            (KeyStatus::Unavailable(why), _) => col = col.push(error_text(why.as_str())),
            (KeyStatus::Saved, false) => {
                let saved = value_field("Saved in your keyring", true);
                let replace = form_button("Replace", false, Some(KeyMessage::Replace));
                let remove = form_button("Remove", false, Some(KeyMessage::Remove));
                col = col.push(saved);
                col = match wait {
                    Some(w) => col.push(caption(w)),
                    None => col.push(button_row([
                        tip(replace, l.replace, Position::Bottom),
                        tip(remove, l.remove, Position::Bottom),
                    ])),
                };
            }
            _ => {
                let mut field = text_field(l.placeholder, self.input.expose(), false, |s| {
                    KeyMessage::Input(Secret::new(s))
                })
                .secure(true);
                if wait.is_none() {
                    field = field.on_submit(KeyMessage::Save);
                }
                let save = form_button("Save key", true, Some(KeyMessage::Save));
                let mut buttons = vec![tip(save, l.save, Position::Bottom)];
                if self.replacing {
                    let cancel = form_button("Cancel", false, Some(KeyMessage::Cancel));
                    buttons.push(tip(cancel, l.keep, Position::Bottom));
                }
                col = col.push(tip(field, l.field, Position::Top));
                col = match wait {
                    Some(w) => col.push(caption(w)),
                    None => col.push(button_row(buttons)),
                };
            }
        }
        if let Some(e) = &self.error {
            col = col.push(error_text(e.as_str()));
        }
        col.into()
    }
}

// ---- view ----

/// A page not yet ported: its rows, split by rules (the shell gives the
/// title and the scrolling).
pub(super) fn rows(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    // (the page's name: `pages::PAGES` gives the title)
    let (_, rows): (&str, Vec<Element<'_, Message>>) = match state.section {
        Section::General | Section::About | Section::Labs | Section::Appearance | Section::Look => {
            unreachable!("ported: settings/pages")
        }
        Section::Shortcuts => ("Shortcuts", shortcut_rows(state)),
        Section::Snip => ("Snip", snip_rows(&state.draft.snip)),
        Section::Ai => ("AI", ai_rows(state)),
        Section::Song => ("Song search", song_rows(state)),
        Section::Translate => {
            let mut rows = vec![
                form_row(
                    "Engine",
                    match d.translate.engine {
                        Engine::Ai => "Your AI provider; offline when it isn't set up",
                        Engine::Offline => "No key, nothing leaves your PC; more literal than AI",
                    },
                    segmented(
                        &[
                            (
                                Engine::Ai,
                                "AI",
                                "Translate with your AI provider (Settings > AI)",
                            ),
                            (
                                Engine::Offline,
                                "Offline",
                                "Translate on this PC (English to and from some languages)",
                            ),
                        ],
                        d.translate.engine,
                        Message::Engine,
                    ),
                    None,
                ),
                form_row(
                    "Source language",
                    "What the text on screen is in; picks the text recognizer",
                    tip(
                        widgets::dropdown(
                            &SOURCES,
                            SOURCES
                                .iter()
                                .find(|l| l.code == d.translate.source)
                                .copied(),
                            &d.translate.source,
                            Message::Source,
                        ),
                        "Source language",
                        Position::Top,
                    ),
                    issue(&state.problems, "translate.source"),
                ),
                form_row(
                    "Target language",
                    // Offline can't do every pair: say so (AI is the other way).
                    if d.translate.engine == Engine::Offline
                        && !mt::available(&d.translate.source, &d.translate.target)
                    {
                        "No offline model for this pair: choose AI to translate it"
                    } else {
                        "What the text is translated into"
                    },
                    tip(
                        widgets::dropdown(
                            LANGUAGES,
                            LANGUAGES
                                .iter()
                                .find(|l| l.code == d.translate.target)
                                .copied(),
                            &d.translate.target,
                            Message::Target,
                        ),
                        "Target language",
                        Position::Top,
                    ),
                    issue(&state.problems, "translate.target"),
                ),
            ];
            // Live translate is a Labs feature (until 0.3.0): its row only while on.
            if state.draft.labs.has(Feature::Live) {
                rows.push({
                    let error = issue(&state.problems, "translate.live_interval_ms");
                    form_row(
                        "Live refresh",
                        "Milliseconds between updates in live translate",
                        tip(
                            text_field(
                                "1500",
                                &state.interval,
                                error.is_some(),
                                Message::LiveInterval,
                            ),
                            "Live translate refresh interval",
                            Position::Top,
                        ),
                        error,
                    )
                });
            }
            ("Translate", rows)
        }
    };
    let mut list = column![];
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            list = list.push(divider());
        }
        list = list.push(r);
    }
    list.into()
}

/// A long folder path cut in the middle so it fits its field.
pub(super) fn shorten(path: &str) -> String {
    const MAX: usize = 38;
    let n = path.chars().count();
    if n <= MAX {
        return path.to_string();
    }
    let tail: String = path.chars().skip(n - (MAX - 1)).collect();
    format!("…{tail}")
}

/// The Start-with-the-OS switch's label (Settings and onboarding).
pub const START_LABEL: &str = if cfg!(windows) {
    "Start OpenCircle when Windows starts"
} else {
    "Start OpenCircle when you log in"
};

/// The Screen edge segments: Live only while live translate is on in Labs.
pub(super) fn edge_modes(labs: &Labs) -> Vec<(EdgeMode, &'static str, &'static str)> {
    let mut modes = vec![(EdgeMode::Overlay, "Circle", "The edge while circling")];
    if labs.has(Feature::Live) {
        modes.push((EdgeMode::Live, "Live", "The edge of live translate"));
    }
    modes.push((EdgeMode::Snip, "Snip", "The edge while snipping"));
    modes
}

/// Windows' language settings (never installs anything itself).
pub(super) fn open_language_settings() {
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
    pub(super) fn missing_recognizers(&self) -> Vec<String> {
        let source = &self.draft.translate.source;
        let wanted: Vec<String> = (self.profile.iter())
            .chain((source != lang::AUTO).then_some(source))
            .cloned()
            .collect();
        crate::ocr::system::missing(&self.recognizers, &wanted)
    }

    /// The one quiet note under the text reader row, if a language is missing.
    pub(super) fn reader_note(&self) -> Option<String> {
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

/// The custom look's controls.
pub(super) fn custom_rows(state: &State) -> Vec<Element<'_, Message>> {
    let c = &state.draft.look.custom;
    let edge = c.edge(state.edge_mode);
    let mut rows = vec![form_row(
        "Colours",
        "The screen edge, selection and buttons",
        tip(
            widgets::dropdown(
                &PALETTES,
                Some(PaletteChoice(c.palette)),
                "Choose colours",
                Message::Palette,
            ),
            "Look colours",
            Position::Top,
        ),
        None,
    )];
    if c.palette == ColourSet::TwoColour {
        let rgb = |i: usize| {
            let [r, g, b] = c.colours[i].0;
            Color::from_rgb8(r, g, b)
        };
        let buttons = button_row([
            widgets::hint(
                colour_swatch(rgb(0), state.picker == Some(0), Message::Picker(0)),
                "First colour",
                widgets::Why::Icon,
                Position::Top,
            ),
            widgets::hint(
                colour_swatch(rgb(1), state.picker == Some(1), Message::Picker(1)),
                "Second colour",
                widgets::Why::Icon,
                Position::Top,
            ),
        ]);
        let mut control = column![buttons].spacing(8);
        if let Some(i) = state.picker {
            control = control.push(widgets::colour_picker(
                &theme::ANNOTATION,
                rgb(i),
                &state.hex,
                Message::PickColour,
                Message::Hex,
            ));
        }
        rows.push(form_row(
            "Your two colours",
            "From the top-left corner round to the opposite one",
            control,
            None,
        ));
    }
    rows.extend([
        form_row(
            "Glow strength",
            "How bright the edge glow is",
            tip(
                slider(crate::config::GLOW, c.glow, " %", Message::Glow),
                "Glow strength",
                Position::Top,
            ),
            None,
        ),
        form_row(
            "Dim",
            "How much the frozen screen darkens",
            tip(
                slider(crate::config::DIM, c.dim, " %", Message::Dim),
                "Dim strength",
                Position::Top,
            ),
            None,
        ),
        form_row(
            "Selection corners",
            "The brackets on a snapped box",
            segmented(
                &[
                    (BracketColour::White, "White", "White corners"),
                    (
                        BracketColour::Accent,
                        "Accent",
                        "Corners in the look's colour",
                    ),
                ],
                c.brackets,
                Message::Brackets,
            ),
            None,
        ),
        form_row(
            "Corner style",
            "Four corners, or a frame all round",
            segmented(
                &[
                    (BracketStyle::Corners, "Corners", "Four corner brackets"),
                    (BracketStyle::Frame, "Frame", "A thin frame all round"),
                ],
                c.bracket_style,
                Message::BracketStyle,
            ),
            None,
        ),
        form_row(
            "Bars and pills",
            "The action bar, chips and panels",
            tip(
                widgets::dropdown(
                    &BARS,
                    Some(BarChoice(c.bar)),
                    "Choose a style",
                    Message::Bar,
                ),
                "Bar style",
                Position::Top,
            ),
            None,
        ),
        form_row(
            "Screen edge for",
            "Each mode has its own edge",
            segmented(
                &edge_modes(&state.draft.labs),
                state.edge_mode,
                Message::EdgeMode,
            ),
            None,
        ),
        form_row(
            "Edge style",
            "A soft glow along the screen edge, or none",
            segmented(
                &[
                    (EdgeStyle::Glow, "Glow", "A soft glow fading inward"),
                    (EdgeStyle::None, "None", "No screen edge"),
                ],
                // a "thin" edge from an older config shows as the glow it now is
                if edge.style == EdgeStyle::None {
                    EdgeStyle::None
                } else {
                    EdgeStyle::Glow
                },
                Message::EdgeStyle,
            ),
            None,
        ),
    ]);
    rows
}

/// Snip: what a capture does, the size and file it is copied and saved as,
/// the pointer, and Recent (off = nothing kept on disk).
pub(super) fn snip_rows(d: &SnipConfig) -> Vec<Element<'_, Message>> {
    // The draft's snip choices with one changed.
    let edit = |change: &dyn Fn(&mut SnipConfig)| {
        let mut next = d.clone();
        change(&mut next);
        Message::Snip(next)
    };
    vec![
        form_row(
            "After a capture",
            "Copy: ready to paste into an AI chat, Edit a click away",
            segmented(
                &[
                    (SnipAfter::Copy, "Copy", "Copy it; the toast offers Edit"),
                    (SnipAfter::Editor, "Editor", "Open the editor on it"),
                    (SnipAfter::Both, "Both", "Copy it and open the editor"),
                ],
                d.after,
                move |after| edit(&|s| s.after = after),
            ),
            None,
        ),
        form_row(
            "Image size",
            "Long edge for Copy and Save; AI chats shrink bigger ones anyway",
            segmented(
                &[
                    (1024, "1024", "Scale the long edge to 1024 px"),
                    (1568, "1568", "Scale the long edge to 1568 px"),
                    (2048, "2048", "Scale the long edge to 2048 px"),
                    (0, "Original", "Keep the original size"),
                ],
                d.max_edge,
                move |max_edge| edit(&|s| s.max_edge = max_edge),
            ),
            None,
        ),
        form_row(
            "Save as",
            "The file Save writes",
            segmented(
                &[
                    (Format::Png, "PNG", "Save PNG files (sharp text)"),
                    (Format::Jpeg, "JPEG", "Save JPEG files (smaller)"),
                ],
                d.format,
                move |format| edit(&|s| s.format = format),
            ),
            None,
        ),
        form_row(
            "Save size",
            "Copy always uses the image size",
            segmented(
                &[
                    (false, "Image size", "Save at the image size"),
                    (true, "Original", "Save at the original size"),
                ],
                d.save_original,
                move |original| edit(&|s| s.save_original = original),
            ),
            None,
        ),
        form_row(
            "Mouse pointer",
            "Draw the pointer into the capture",
            segmented(
                &[
                    (false, "Off", "Leave the pointer out"),
                    (true, "On", "Draw the pointer in"),
                ],
                d.cursor,
                move |cursor| edit(&|s| s.cursor = cursor),
            ),
            None,
        ),
        form_row(
            "Timer",
            "Wait before a snip captures, so you can open a menu first",
            segmented(
                &[
                    (0, "Off", "Capture at once"),
                    (3, "3 s", "Hide, wait 3 seconds, then capture"),
                    (5, "5 s", "Hide, wait 5 seconds, then capture"),
                    (10, "10 s", "Hide, wait 10 seconds, then capture"),
                ],
                d.delay,
                move |delay| edit(&|s| s.delay = delay),
            ),
            None,
        ),
        form_row(
            "Recent captures",
            "The last 20, on this PC only. Off deletes them",
            segmented(
                &[
                    (false, "Off", "Keep no captures"),
                    (true, "On", "Keep the last 20 captures"),
                ],
                d.history,
                move |history| edit(&|s| s.history = history),
            ),
            None,
        ),
    ]
}

pub(super) fn shortcut_rows(state: &State) -> Vec<Element<'_, Message>> {
    state
        .slots()
        .map(|slot| {
            let (label, hint, tip_text) = match slot {
                Slot::Capture => (
                    "Open OpenCircle",
                    "Starts a capture",
                    "Open OpenCircle shortcut",
                ),
                Slot::Quick(i) => {
                    let t = &state.quick_text[i];
                    (t.label.as_str(), t.hint.as_str(), t.tip.as_str())
                }
            };
            let recording = state.recording == Some(slot);
            let error = state
                .shortcut_error
                .as_ref()
                .filter(|(s, _)| *s == slot)
                .map(|(_, e)| e.clone());
            // Never claim a key the desktop doesn't use.
            let note = (!recording).then(|| state.desktop_note(slot)).flatten();
            let error = error.or(note);
            let field_state = match (recording, &error) {
                (_, Some(_)) => FieldState::Invalid,
                (true, None) => FieldState::Recording,
                _ => FieldState::Idle,
            };
            let hint = if recording {
                match slot {
                    Slot::Capture => "Esc cancels",
                    Slot::Quick(_) => "Esc cancels · Delete clears",
                }
            } else {
                hint
            };
            let field = shortcut_field(
                key_labels(state.shortcut(slot)),
                field_state,
                "Press keys…",
                Message::Record(slot),
            );
            form_row(label, hint, tip(field, tip_text, Position::Top), error)
        })
        .collect()
}

pub(super) fn ai_rows(state: &State) -> Vec<Element<'_, Message>> {
    let ai = &state.draft.ai;
    let compat = ai.provider == Provider::OpenaiCompatible;
    let mut rows = vec![form_row(
        "Provider",
        if ai.provider == Provider::Browser {
            "Google AI Mode in your browser. No key"
        } else {
            "Who answers your questions"
        },
        segmented(
            &[
                (
                    Provider::Browser,
                    "Browser",
                    "Google AI Mode in your browser, no key",
                ),
                (Provider::Anthropic, "Anthropic", "Use Anthropic (Claude)"),
                (
                    Provider::OpenaiCompatible,
                    "OpenAI-like",
                    "Use an OpenAI-compatible server",
                ),
            ],
            ai.provider.clone(),
            Message::Provider,
        ),
        // An unknown value from the file: none is selected, Save waits for a pick.
        issue(&state.problems, "ai.provider"),
    )];
    if matches!(
        ai.provider,
        Provider::Browser | Provider::Invalid(_) | Provider::Unreadable
    ) {
        return rows; // no model, server or key: the browser, or nothing chosen yet
    }
    if compat {
        rows.push(form_row(
            "Preset",
            "Fills in the server URL",
            tip(
                widgets::dropdown(
                    PRESETS,
                    PRESETS.iter().find(|p| p.base_url == ai.base_url).copied(),
                    "Choose a preset",
                    Message::Preset,
                ),
                "Server preset",
                Position::Top,
            ),
            None,
        ));
        let error = issue(&state.problems, "ai.base_url");
        rows.push(form_row(
            "Server URL",
            "Your own server or any compatible API",
            tip(
                text_field(
                    "https://api.openai.com/v1",
                    &ai.base_url,
                    error.is_some(),
                    Message::BaseUrl,
                ),
                "Server URL",
                Position::Top,
            ),
            error,
        ));
    }
    let error = issue(&state.problems, "ai.model");
    rows.push(form_row(
        "Model",
        "The model id sent to the provider",
        tip(
            text_field(
                if compat && ai.base_url.contains("openrouter.ai") {
                    "e.g. anthropic/claude-haiku-5.5 (fast, paid)"
                } else if compat {
                    "e.g. gpt-4o-mini"
                } else {
                    "claude-sonnet-5-5"
                },
                &ai.model,
                error.is_some(),
                Message::Model,
            ),
            "Model name",
            Position::Top,
        ),
        error,
    ));
    let local = PRESETS.iter().any(|p| p.local && p.base_url == ai.base_url);
    rows.push(form_row(
        "API key",
        if local {
            "Not needed for a server on this PC"
        } else {
            "Kept in your system keyring, never in a file"
        },
        state.key.view().map(Message::ApiKey),
        None,
    ));
    rows
}

/// What song search sends, and when (also in docs/privacy.md).
pub const SONG_PRIVACY: &str = "When you press the music button, a ~10-second clip of the audio you choose is sent to AudD. Nothing is recorded at any other time.";

pub(super) fn song_rows(state: &State) -> Vec<Element<'_, Message>> {
    let song = &state.draft.song;
    let mut rows = vec![form_row(
        "Service",
        "Free fingerprint lookups can't name a short clip, so this needs your own key",
        value_field(SERVICE.name(), false),
        None,
    )];
    if let Some(field) = &state.song_key {
        rows.push(form_row(
            "API token",
            "Sign up at dashboard.audd.io and paste your token. Kept in your system keyring",
            field.view().map(Message::SongKey),
            None,
        ));
    }
    rows.push(form_row(
        "Listen to",
        match song.source {
            SongSource::System => "What this PC plays: a video, a game, a stream",
            SongSource::Microphone => "Your microphone: music playing near you",
        },
        segmented(
            &[
                (
                    SongSource::System,
                    "This PC",
                    "Listen to what this PC plays",
                ),
                (
                    SongSource::Microphone,
                    "Microphone",
                    "Listen with the microphone",
                ),
            ],
            song.source,
            Message::SongSource,
        ),
        None,
    ));
    rows.push(form_row(
        "History",
        "Recent songs, kept only on this PC. Off deletes them",
        segmented(
            &[
                (false, "Off", "Keep no song history"),
                (true, "On", "Keep recent songs on this PC"),
            ],
            song.history,
            Message::SongHistory,
        ),
        None,
    ));
    rows.push(form_row(
        "Privacy",
        "",
        widgets::caption(SONG_PRIVACY),
        None,
    ));
    rows
}
