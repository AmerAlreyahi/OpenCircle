//! The right-click menu's Translate (owner, 2026-11-25): the SELECTED TEXT's translation in a
//! small card beside the selection, with Copy. The overlay stays open. While it works the card
//! says so; a failure is the one error card (F12). `App` places it and runs the work
//! (`app::text_card`).
use iced::widget::{column, container};
use iced::{Element, Fill, Rectangle};

use crate::shared::app_language::{S, tr};
use crate::shared::kit::button::regular;
use crate::shared::kit::error_card::{ErrorCard, error_card};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{muted, own};
use crate::shared::kit::tokens::{Role, radius};

pub const WIDTH: f32 = 360.0;
const PAD: f32 = 14.0;
/// The card keeps this far from the screen's edges, and from the selected text.
const EDGE: f32 = 8.0;
const GAP_TO_TEXT: f32 = 8.0;
const GAP: f32 = 10.0;

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Translating,
    Done(String),
    Failed(String),
    /// The text is already in the target language: shown as it is, with this quiet note.
    Same(String),
    /// The offline pair is not on disk: the download card asks (its size in bytes).
    NeedsModel(u64),
    /// Downloading it, this share done.
    Downloading(f32),
    /// The download stopped (why): the card offers it again.
    DownloadFailed(String),
}

/// The model download's news.
#[derive(Debug, Clone)]
pub enum Download {
    Progress(f32),
    Done(Result<(), String>),
}

/// The open card: which request it answers, what it shows, and the selection it sits by (screen px).
#[derive(Debug)]
pub struct TextCard {
    pub id: u32,
    /// Set when the card goes (Not now, Esc, a new request): its download stops.
    pub stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The selected words.
    pub text: String,
    /// The pair to download, when it is missing.
    pub model: Option<crate::features::translate::Model>,
    pub state: State,
    pub near: Rectangle,
    pub copied: bool,
    pub details: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Copy,
    Details,
    Download,
    Dismiss,
}

impl Drop for TextCard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl TextCard {
    pub fn new(id: u32, text: String, near: Rectangle) -> Self {
        TextCard {
            id,
            stop: Default::default(),
            text,
            model: None,
            state: State::Translating,
            near,
            copied: false,
            details: false,
        }
    }

    /// About how tall the card is (to keep it on the screen).
    pub fn height(&self) -> f32 {
        let chars = match &self.state {
            State::Translating => 12,
            State::Done(t) | State::Failed(t) => t.chars().count(),
            State::Same(_) => self.text.chars().count() + 24,
            State::NeedsModel(_) | State::Downloading(_) | State::DownloadFailed(_) => 150,
        };
        let lines = (chars as f32 * 7.0 / (WIDTH - 2.0 * PAD))
            .ceil()
            .clamp(1.0, 12.0);
        2.0 * PAD + 36.0 + lines * 20.0
    }

    /// Where the card goes on a `screen`: under the selection (above it when there is no room),
    /// its start edge on the selection's start edge (the left one, the right one in Arabic), kept
    /// on the screen as the menu is (`option_list::place`).
    pub fn at(&self, screen: iced::Size) -> iced::Point {
        use crate::shared::kit::option_list::place;
        let size = iced::Size::new(self.width(), self.height());
        let x = match crate::shared::kit::dir::rtl() {
            true => self.near.x + self.near.width - size.width,
            false => self.near.x,
        };
        let below = self.near.y + self.near.height + GAP_TO_TEXT;
        let y = match below + size.height + EDGE <= screen.height {
            true => below,
            false => self.near.y - GAP_TO_TEXT - size.height,
        };
        place(iced::Point::new(x, y), size, screen, EDGE)
    }

    /// Its width: the download card is wider.
    pub fn width(&self) -> f32 {
        match &self.state {
            State::NeedsModel(_) | State::Downloading(_) | State::DownloadFailed(_) => {
                crate::features::text_reading::model_download::WIDTH
            }
            _ => WIDTH,
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        use crate::features::text_reading::model_download::{Fetching, Which};
        let fetching = |bytes, share, failed: Option<String>| Fetching {
            which: Which::Pair,
            bytes,
            share,
            failed,
            provider: false,
            details: self.details,
        };
        let bytes = self.model.map_or(0, |m| m.bytes());
        let ask = match &self.state {
            State::NeedsModel(bytes) => Some(fetching(*bytes, None, None)),
            State::Downloading(share) => Some(fetching(bytes, Some(*share), None)),
            State::DownloadFailed(why) => Some(fetching(bytes, None, Some(why.clone()))),
            _ => None,
        };
        if let Some(card) = ask {
            // the one download card (Download / Try again, Not now), as the page Translate's
            return card.card(Msg::Download, Msg::Dismiss, Msg::Details, Msg::Details);
        }
        let body: Element<'_, Msg> = match &self.state {
            State::Translating
            | State::NeedsModel(_)
            | State::Downloading(_)
            | State::DownloadFailed(_) => muted(Role::NOTE, tr(S::tp_translating)).into(),
            State::Done(text) => {
                let label = tr(if self.copied {
                    S::pill_copied
                } else {
                    S::c_copy
                });
                column![
                    own(Role::ROW_TITLE, text.as_str())
                        .width(Fill)
                        .align_x(crate::shared::kit::dir::start()),
                    container(regular(label, false, Some(Msg::Copy)))
                        .width(Fill)
                        .align_x(crate::shared::kit::dir::start()),
                ]
                .spacing(GAP)
                .into()
            }
            State::Same(note) => {
                let label = tr(if self.copied {
                    S::pill_copied
                } else {
                    S::c_copy
                });
                column![
                    own(Role::ROW_TITLE, self.text.as_str())
                        .width(Fill)
                        .align_x(crate::shared::kit::dir::start()),
                    muted(Role::NOTE, note.as_str())
                        .width(Fill)
                        .align_x(crate::shared::kit::dir::start()),
                    container(regular(label, false, Some(Msg::Copy)))
                        .width(Fill)
                        .align_x(crate::shared::kit::dir::start()),
                ]
                .spacing(GAP)
                .into()
            }
            State::Failed(message) => {
                let first = message.lines().next().unwrap_or_default().to_string();
                let more = (first.len() < message.trim().len()).then(|| message.clone());
                error_card(ErrorCard {
                    headline: tr(S::tp_failed).into(),
                    hint: first,
                    detail: more.unwrap_or_default(),
                    width: WIDTH - 2.0 * PAD,
                    open: self.details,
                    on_details: Msg::Details,
                    fix: None,
                    retry: None,
                    words: crate::shared::theme::error_words(),
                })
            }
        };
        surface(Material::Widget, radius::CARD, body)
            .padding(PAD)
            .width(WIDTH)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::app_language::{self, Lang};
    use crate::shared::kit::testing::{golden, shot};
    use iced::Size;

    /// W106: the card sits under the selection at its start edge (the right one in Arabic), above it
    /// where there is no room, and always on the screen.
    #[test]
    fn the_card_sits_by_the_selection_at_its_start_edge_and_on_the_screen() {
        use iced::{Point, Size};
        let screen = Size::new(1280.0, 720.0);
        let card = |near: Rectangle| {
            let mut c = TextCard::new(1, "x".into(), near);
            c.state = State::Done("hello".into());
            c
        };
        let near = Rectangle::new(Point::new(300.0, 200.0), Size::new(400.0, 20.0));
        assert_eq!(card(near).at(screen), Point::new(300.0, 228.0));
        let low = Rectangle::new(Point::new(300.0, 690.0), Size::new(400.0, 20.0));
        assert!(card(low).at(screen).y < 690.0, "above it");
        let far = Rectangle::new(Point::new(1200.0, 200.0), Size::new(70.0, 20.0));
        let at = card(far).at(screen);
        assert!(at.x + WIDTH <= screen.width, "kept on the screen: {at:?}");
        app_language::set(Lang::Ar);
        let at = card(near).at(screen);
        app_language::set(Lang::En);
        assert_eq!(at.x + WIDTH, 700.0, "the right edges meet");
    }

    /// The card in every state, English and Arabic, both themes (look at each after an update).
    #[test]
    fn the_translate_card_matches_its_golden_images() {
        crate::shared::kit::text::load();
        let size = Size::new(480.0, 260.0);
        golden::each_theme(|theme, failed| {
            for (lang, tag) in [(Lang::En, ""), (Lang::Ar, "-ar")] {
                app_language::set(lang);
                let states = [
                    ("loading", State::Translating),
                    ("result", State::Done("Hello, how are you today?".into())),
                    (
                        "error",
                        State::Failed("No internet connection. Reconnect, then try again.".into()),
                    ),
                    (
                        "same",
                        State::Same("This text is already in English".into()),
                    ),
                    ("download", State::NeedsModel(81_000_000)),
                ];
                for (name, state) in states {
                    let mut card = TextCard::new(1, "x".into(), Rectangle::default());
                    card.state = state;
                    if name == "same" {
                        card.text = "Hello, how are you today?".into();
                    }
                    if name == "result" && lang == Lang::Ar {
                        card.state = State::Done("مرحبا، كيف حالك اليوم؟".into());
                    }
                    let file = format!("text-card-{name}{tag}-{}", theme.id);
                    let lost = shot::out_of_reach(card.view(), size);
                    assert!(lost.is_empty(), "{file}: {lost:?}");
                    failed.extend(golden::check("screens", &file, size, &[1.0], || {
                        iced::widget::container(card.view()).into()
                    }));
                }
            }
            app_language::set(Lang::En);
        });
    }
}
