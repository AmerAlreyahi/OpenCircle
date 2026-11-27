//! The model download card (`newdesing/widgets/model-download.js`, #127):
//! the one question before an on-device model is downloaded, its progress, its
//! check, done, and why it failed. Every place that needs a model shows it:
//! the bar's text-reader note (also Copy text), translate. Nothing downloads
//! without its Download click (the callers start the download on that message
//! only). What each model says is data (`OCR`, `TRANSLATION`).
use iced::widget::{Space, column, container, pin, row, text};
use iced::{Center, Element, Fill, Padding, Rectangle, Size};

use crate::dir_row;
use crate::shared::app_language::{S, fmt, tr};
use crate::shared::kit::button::{Kind, button, regular};
use crate::shared::kit::chip::emblem;
use crate::shared::kit::dir;
use crate::shared::kit::icon::Icon;
use crate::shared::kit::progress;
use crate::shared::kit::surface::{Join, tray};
use crate::shared::kit::text::paragraph;
use crate::shared::kit::text::{accent, sub};
use crate::shared::kit::tokens::{Role, radius};
use crate::t;

/// A model the card offers (the design's `models` catalog).
pub struct Model<'s> {
    pub name: S,
    pub icon: Icon,
    /// The question's title.
    pub title: S,
    /// What it does, before "on this PC; none of your content is sent."
    pub does: Does<'s>,
    /// Where it comes from (a service's name: the same in every language).
    pub source: &'static str,
}

/// What a model does: a sentence of the table, or one made of its parts
/// (the readers that are missing).
#[derive(Clone, Copy)]
pub enum Does<'s> {
    Said(S),
    Text(&'s str),
}

pub const OCR: Model<'static> = Model {
    name: S::md_ocr_name,
    icon: Icon::Text,
    title: S::md_ocr_title,
    does: Does::Said(S::md_ocr_does),
    source: "GitHub",
};

/// The Studio's Cut out model (IS-Net).
pub const CUTOUT: Model<'static> = Model {
    name: S::md_cut_name,
    icon: Icon::Picture,
    title: S::md_cut_title,
    does: Does::Said(S::md_cut_does),
    source: "GitHub",
};

/// The readers for other scripts, for a page read as noise.
pub const SCRIPTS: Model<'static> = Model {
    name: S::md_scripts_name,
    icon: Icon::Text,
    title: S::md_scripts_title,
    does: Does::Said(S::md_scripts_does),
    source: "GitHub",
};

pub const TRANSLATION: Model<'static> = Model {
    name: S::md_tr_name,
    icon: Icon::Translate,
    title: S::md_tr_title,
    does: Does::Said(S::md_tr_does),
    source: "Hugging Face",
};

/// Where the download is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage<'a> {
    /// Asks; nothing has been fetched.
    Ask,
    /// Fetching, 0..1 of the whole.
    Downloading(f32),
    /// Fetching, waiting to try a file again after a server or network error.
    Retrying(f32),
    /// All fetched: checked and set up.
    Verifying,
    /// Why it stopped.
    Failed(&'a str),
}

impl Stage<'_> {
    /// A download at `share`: the files are checked as the last ones land.
    /// Retrying while the download waits for another try.
    pub fn at(share: f32) -> Self {
        Self::of(share, crate::features::text_reading::models::retrying())
    }

    fn of(share: f32, retrying: bool) -> Self {
        match (share >= 1.0, retrying) {
            (true, _) => Stage::Verifying,
            (false, true) => Stage::Retrying(share),
            (false, false) => Stage::Downloading(share),
        }
    }
}

/// What the card's buttons send.
#[derive(Clone)]
pub struct On<M> {
    /// Download, and Try again.
    pub download: M,
    /// Not now, Cancel and the close button (`None`: the caller can't stop
    /// the download, so neither shows).
    pub dismiss: Option<M>,
    /// "Use an AI provider instead" (translation).
    pub provider: Option<M>,
    /// A failed download's Show / Hide details: whether they are open, and the press.
    pub details: Option<(bool, M)>,
}

/// The card's width (`.oc-model-card`; less when the room is narrower).
pub const WIDTH: f32 = 440.0;
/// Its header and content padding: design `.cd` 16 + the 1 px rim, 12 between its parts.
const HEADER: Padding = Padding {
    top: 17.0,
    right: 17.0,
    bottom: 0.0,
    left: 17.0,
};
const CONTENT: Padding = Padding {
    top: 12.0,
    right: 17.0,
    bottom: 17.0,
    left: 17.0,
};
/// Between the emblem and the title; the title and its meta line.
const HEADING_GAP: f32 = 12.0;
const META_GAP: f32 = 3.0;
/// Above the description, the buttons, the provider link, the transfer
/// lines (the progress bar keeps 6 px of its own above for its glow).
const ABOVE_TEXT: f32 = 12.0;
const ABOVE_BUTTONS: f32 = 12.0;
const ABOVE_LINK: f32 = 12.0;
const ABOVE_TRANSFER: f32 = 12.0;
const ABOVE_BAR: f32 = 4.0;
const ABOVE_CANCEL: f32 = 8.0;
/// Between buttons.
const BUTTON_GAP: f32 = 8.0;
/// The title's and the description's line heights.
const TITLE_LINE: f32 = 1.4;
const TEXT_LINE: f32 = 1.6;

/// Why a download stopped, in the app's language: what the user can do about
/// it (the detail goes to the log).
pub fn why(e: &crate::features::text_reading::OcrError) -> String {
    tr(why_key(e)).into()
}

/// The string saying why (a state that crosses threads carries the key, the
/// UI words it in the language it speaks when it draws).
pub fn why_key(e: &crate::features::text_reading::OcrError) -> S {
    use crate::features::text_reading::OcrError;
    crate::log!("[opencircle] model download failed: {e}");
    match e {
        OcrError::Unreachable { server: true } => S::md_err_server,
        OcrError::Unreachable { server: false } => S::md_err_offline,
        OcrError::Checksum(_) => S::md_err_damaged,
        _ => S::md_err_other,
    }
}

/// Which model a host's card is for (`Fetching`).
#[derive(Debug, Clone, PartialEq)]
pub enum Which {
    /// The text reader.
    Reader,
    /// A pair's translation models.
    Pair,
    /// The readers for other scripts (their names).
    Scripts(String),
    /// The Studio's Cut out model.
    CutOut,
}

/// What a host that has no card of its own (the Studio) shows, and the pill
/// shows too: one description, one card (`card`). `share`: `None` asks,
/// `Some` fetches; `failed`: why it stopped; `provider`: offers "Use an AI
/// provider instead" (translation).
#[derive(Debug, Clone, PartialEq)]
pub struct Fetching {
    pub which: Which,
    pub bytes: u64,
    pub share: Option<f32>,
    pub failed: Option<String>,
    pub provider: bool,
    /// The failure's own text is shown (a failed download's Show details).
    pub details: bool,
}

impl Fetching {
    /// The text reader's question (the whole set's size: the same measure
    /// the progress shows).
    pub fn reader() -> Self {
        Fetching {
            which: Which::Reader,
            bytes: crate::features::text_reading::models::full_size(),
            share: None,
            failed: None,
            provider: false,
            details: false,
        }
    }

    /// The Studio's Cut out question (ONNX Runtime too, when OCR never fetched it).
    pub fn cutout() -> Self {
        Fetching {
            which: Which::CutOut,
            bytes: crate::features::cutout::full_size(),
            ..Fetching::reader()
        }
    }

    /// The same, at `share` of the download.
    pub fn at(self, share: f32) -> Self {
        Fetching {
            share: Some(share),
            failed: None,
            ..self
        }
    }

    /// The same, stopped for `why` (Try again).
    pub fn stopped(self, why: String) -> Self {
        Fetching {
            share: None,
            failed: Some(why),
            ..self
        }
    }

    /// The card: `download` is Download / Try again, `dismiss` Not now /
    /// Cancel, `provider` the AI link.
    pub fn card<'a, M: Clone + 'a>(
        &self,
        download: M,
        dismiss: M,
        provider: M,
        details: M,
    ) -> Element<'a, M> {
        let stage = match (&self.failed, self.share) {
            (Some(why), _) => Stage::Failed(why),
            (None, Some(share)) => Stage::at(share),
            (None, None) => Stage::Ask,
        };
        let on = On {
            download,
            dismiss: Some(dismiss),
            provider: self.provider.then_some(provider),
            details: Some((self.details, details)),
        };
        let does;
        let scripts;
        let model = match &self.which {
            Which::Reader => &OCR,
            Which::Pair => &TRANSLATION,
            Which::CutOut => &CUTOUT,
            Which::Scripts(names) => {
                does = fmt(S::md_read_names, &[names]);
                scripts = Model {
                    does: Does::Text(&does),
                    ..SCRIPTS
                };
                &scripts
            }
        };
        card(model, self.bytes, stage, on)
    }
}

/// `bytes` as the design writes it: whole MB, at least 1.
pub fn mb(bytes: u64) -> u64 {
    bytes.div_ceil(1_000_000).max(1)
}

/// The card: `model` (`bytes` to download, the whole set) at `stage`.
pub fn card<'a, M: Clone + 'a>(
    model: &Model,
    bytes: u64,
    stage: Stage<'_>,
    on: On<M>,
) -> Element<'a, M> {
    // F12: a failed download is the one error card (what happened, what to do,
    // one fix button)
    if let Stage::Failed(why) = stage {
        use crate::shared::kit::error_card::{ErrorCard, error_card};
        let again = on.download;
        // a host without a Show details press (the bar's note) says the reason itself
        let (open, toggle, hint, detail) = match on.details {
            Some((open, toggle)) => (
                open,
                toggle,
                t!(md_failed_hint).to_string(),
                why.to_string(),
            ),
            None => (false, again.clone(), why.to_string(), String::new()),
        };
        let card = error_card(ErrorCard {
            headline: t!(md_failed).into(),
            hint,
            detail,
            width: WIDTH,
            open,
            on_details: toggle,
            fix: Some((t!(md_try_again), again)),
            retry: None,
            words: crate::shared::theme::error_words(),
        });
        return container(card).width(WIDTH).into();
    }
    let header = row![
        container(crate::shared::kit::text::muted(Role::META, t!(md_header)))
            .width(Fill)
            .align_x(dir::start()),
    ]
    .padding(dir::pad(HEADER))
    .align_y(Center);
    let content = column![heading(model, bytes, stage)]
        .push(body(model, bytes, stage, on))
        .padding(dir::pad(CONTENT));
    tray(false, radius::WINDOW, Join::None, column![header, content])
        .width(WIDTH)
        .into()
}

/// The emblem, the stage's title and the model's line.
fn heading<'a, M: 'a>(model: &Model, bytes: u64, stage: Stage<'_>) -> Element<'a, M> {
    let (icon, title) = match stage {
        Stage::Ask => (model.icon, tr(model.title).to_string()),
        Stage::Downloading(_) | Stage::Retrying(_) => (
            model.icon,
            fmt(S::md_downloading, &[&tr(model.name).to_lowercase()]),
        ),
        Stage::Verifying => (model.icon, t!(md_checking).into()),
        Stage::Failed(_) => (Icon::Close, t!(md_failed).into()),
    };
    let meta = fmt(
        S::md_meta,
        &[&tr(model.name), &format!("{} MB", mb(bytes)), &model.source],
    );
    let words = column![
        crate::shared::kit::text::text(Role::CARD_HEADING, title).line_height(TITLE_LINE),
        sub(Role::META, meta).line_height(TEXT_LINE),
    ]
    .spacing(META_GAP)
    .align_x(dir::start())
    .width(Fill);
    dir_row![emblem(icon), words]
        .spacing(HEADING_GAP)
        .align_y(Center)
        .into()
}

/// What the stage says and its buttons.
fn body<'a, M: Clone + 'a>(
    model: &Model,
    bytes: u64,
    stage: Stage<'_>,
    on: On<M>,
) -> Element<'a, M> {
    let said = |s: String| {
        column![
            Space::new().height(ABOVE_TEXT),
            paragraph(sub(Role::NOTE, s).line_height(TEXT_LINE))
        ]
    };
    let buttons = |primary: &'a str, other: &'a str| {
        let mut r = vec![regular(primary, true, Some(on.download.clone()))];
        if let Some(dismiss) = on.dismiss.clone() {
            r.push(regular(other, false, Some(dismiss)));
        }
        let r = dir::row(r).spacing(BUTTON_GAP);
        column![
            Space::new().height(ABOVE_BUTTONS),
            container(r).width(Fill).align_x(dir::start())
        ]
    };
    match stage {
        Stage::Ask => {
            let does = match model.does {
                Does::Said(s) => tr(s).to_string(),
                Does::Text(t) => t.to_string(),
            };
            let mut c =
                said(fmt(S::md_on_pc, &[&does])).push(buttons(t!(md_download), t!(md_not_now)));
            if let Some(provider) = on.provider {
                let link = text(t!(md_provider))
                    .size(crate::shared::kit::text::size(Role::META))
                    .font(crate::shared::kit::text::font(Role::META));
                c = c.push(Space::new().height(ABOVE_LINK)).push(
                    container(button(link, Kind::Quiet, Some(provider)))
                        .width(Fill)
                        .align_x(dir::start()),
                );
            }
            c.into()
        }
        Stage::Downloading(share) | Stage::Retrying(share) => {
            let total = bytes as f64 / 1e6;
            let done = total * share as f64;
            // quietly: the download goes on by itself after a server hiccup
            let state: Element<'a, M> = match stage {
                Stage::Retrying(_) => sub(Role::META, t!(md_retrying)).into(),
                _ => accent(Role::META_STRONG, format!("{:.0}%", share * 100.0)).into(),
            };
            let line = dir_row![
                container(sub(
                    Role::META,
                    fmt(S::md_progress, &[&format!("{done:.1} / {} MB", mb(bytes))])
                ))
                .width(Fill)
                .align_x(dir::start()),
                state,
            ]
            .align_y(Center);
            let mut c = column![
                Space::new().height(ABOVE_TRANSFER),
                line,
                Space::new().height(ABOVE_BAR),
                progress::bar(share),
            ];
            if let Some(dismiss) = on.dismiss {
                c = c.push(Space::new().height(ABOVE_CANCEL)).push(dir_row![
                    Space::new().width(Fill),
                    regular(t!(c_cancel), false, Some(dismiss))
                ]);
            }
            c.into()
        }
        // ponytail: the design's sliding bar is drawn full and still (no
        // frame clock for a step that lasts a moment)
        Stage::Verifying => said(t!(md_verifying).into())
            .push(Space::new().height(ABOVE_BAR))
            .push(progress::bar(1.0))
            .into(),
        Stage::Failed(why) => said(why.to_string())
            .push(buttons(t!(md_try_again), t!(c_cancel)))
            .into(),
    }
}

/// Clear space between a floating card and the screen's edges and the pill.
pub const FLOAT_MARGIN: f32 = 32.0;

/// Where `card` floats on `screen`: the top right, `FLOAT_MARGIN` from the
/// edges, never touching the corner or the `pill`'s rect; where it doesn't fit
/// beside the pill, under it (centred on the pill, kept on the screen).
pub fn float_at(screen: Size, pill: Rectangle) -> iced::Point {
    let width = WIDTH.min(screen.width - 2.0 * FLOAT_MARGIN).max(0.0);
    let x = screen.width - FLOAT_MARGIN - width;
    if x >= pill.x + pill.width + FLOAT_MARGIN {
        return iced::Point::new(x, FLOAT_MARGIN);
    }
    let centred = pill.center_x() - width / 2.0;
    let x = centred.clamp(FLOAT_MARGIN.min(x), x.max(FLOAT_MARGIN));
    iced::Point::new(x, pill.y + pill.height + FLOAT_MARGIN / 2.0)
}

/// `card` floating at `float_at`.
pub fn floating<'a, M: 'a>(card: Element<'a, M>, screen: Size, pill: Rectangle) -> Element<'a, M> {
    pin(container(card).width(WIDTH.min(screen.width - 2.0 * FLOAT_MARGIN)))
        .position(float_at(screen, pill))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::{golden, shot};

    const FAILED: &str = "No internet connection. Reconnect, then try again.";

    fn stages() -> Vec<(&'static str, &'static Model<'static>, Stage<'static>, bool)> {
        vec![
            ("ask-ocr", &OCR, Stage::Ask, false),
            ("ask-translation", &TRANSLATION, Stage::Ask, true),
            ("downloading", &OCR, Stage::Downloading(0.42), false),
            ("verifying", &OCR, Stage::Verifying, false),
            ("failed", &OCR, Stage::Failed(FAILED), false),
        ]
    }

    fn view(model: &Model, stage: Stage<'static>, provider: bool) -> Element<'static, u8> {
        let on = On {
            download: 1,
            dismiss: Some(2),
            provider: provider.then_some(3),
            details: Some((false, 4)),
        };
        card(model, 15_000_000, stage, on)
    }

    const SIZE: Size = Size::new(WIDTH, 240.0);

    /// Each stage, Sapphire and Daylight, matches its golden image, and no
    /// text is cut off.
    #[test]
    fn every_stage_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            for (name, model, stage, provider) in stages() {
                let lost = shot::out_of_reach(view(model, stage, provider), SIZE);
                assert!(lost.is_empty(), "{name}: {lost:?}");
                let file = format!("model-download-{name}-{}", theme.id);
                failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                    view(model, stage, provider)
                }));
            }
        });
    }

    /// Arabic: every stage mirrored, from the table, the size isolated.
    #[test]
    fn every_arabic_stage_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            let _ar = crate::shared::kit::testing::arabic();
            let why = t!(md_err_offline);
            let stages = [
                ("ask-ocr", &OCR, Stage::Ask, false),
                ("ask-translation", &TRANSLATION, Stage::Ask, true),
                ("downloading", &OCR, Stage::Downloading(0.42), false),
                ("verifying", &OCR, Stage::Verifying, false),
                ("failed", &OCR, Stage::Failed(why), false),
            ];
            for (name, model, stage, provider) in stages {
                let lost = shot::out_of_reach(view(model, stage, provider), SIZE);
                assert!(lost.is_empty(), "{name}: {lost:?}");
                let file = format!("model-download-{name}-ar-{}", theme.id);
                failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                    view(model, stage, provider)
                }));
            }
        });
    }

    /// Download is the one way to start it; Not now / Cancel dismiss;
    /// translation offers an AI provider instead.
    #[test]
    fn the_buttons_say_what_they_do() {
        let texts = |stage, provider| shot::texts(view(&TRANSLATION, stage, provider));
        let ask = texts(Stage::Ask, true);
        for want in ["Download", "Not now", "Use an AI provider instead"] {
            assert!(ask.iter().any(|t| t == want), "{want}: {ask:?}");
        }
        assert!(texts(Stage::Ask, false).iter().all(|t| !t.contains("AI")));
        let failed = texts(Stage::Failed(FAILED), false);
        assert!(failed.iter().any(|t| t == "Try again"));
        assert!(failed.iter().any(|t| t == "Show details"), "{failed:?}");
        assert!(
            !failed.iter().any(|t| t == FAILED),
            "the system's text is folded: {failed:?}"
        );
        let downloading = texts(Stage::Downloading(0.5), false);
        assert!(downloading.iter().any(|t| t == "50%"), "{downloading:?}");
        assert!(
            downloading.iter().any(|t| t == "7.5 / 15 MB"),
            "{downloading:?}"
        );
    }

    /// F12-6: a failed download says what to do; what the server or the disk said is
    /// folded behind Show details and shows when opened.
    #[test]
    fn a_failed_download_folds_the_systems_words() {
        let on = |open| On {
            download: 1u8,
            dismiss: Some(2),
            provider: None,
            details: Some((open, 4)),
        };
        let texts = |open| shot::texts(card(&OCR, 15_000_000, Stage::Failed(FAILED), on(open)));
        let closed = texts(false);
        assert!(
            closed
                .iter()
                .any(|t| t.starts_with("Try again in a minute")),
            "{closed:?}"
        );
        assert!(!closed.iter().any(|t| t == FAILED), "{closed:?}");
        let open = texts(true);
        assert!(
            open.iter().any(|t| t == FAILED) && open.iter().any(|t| t == "Hide details"),
            "{open:?}"
        );
    }

    /// No dismiss (a download the caller can't stop): no Cancel, no close.
    #[test]
    fn a_download_that_cannot_stop_offers_no_cancel() {
        let on = On {
            download: 1u8,
            dismiss: None,
            provider: None,
            details: None,
        };
        let size = Size::new(WIDTH, 200.0);
        let view = || card(&OCR, 1, Stage::Downloading(0.5), On { ..on.clone() });
        let texts = shot::texts(view());
        assert!(texts.iter().all(|t| t != "Cancel"), "{texts:?}");
        // the close button's corner: nothing there to press
        let close = iced::Point::new(WIDTH - 30.0, 25.0);
        assert!(shot::click(view(), size, close).is_empty());
    }

    #[test]
    fn a_full_download_is_being_checked() {
        // `of`, not `at`: `at` reads the process-wide retry flag other tests set
        assert_eq!(Stage::of(0.4, false), Stage::Downloading(0.4));
        assert_eq!(Stage::of(1.0, false), Stage::Verifying);
        assert_eq!(Stage::of(0.4, true), Stage::Retrying(0.4));
        assert_eq!(Stage::of(1.0, true), Stage::Verifying);
    }

    /// While a file waits for another try, the card says so in place of the
    /// percent (the bar stays).
    #[test]
    fn a_retry_says_so_quietly() {
        let texts = shot::texts(view(&OCR, Stage::Retrying(0.5), false));
        assert!(texts.iter().any(|t| t == "Retrying…"), "{texts:?}");
        assert!(texts.iter().all(|t| t != "50%"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Downloading text recognition"));
    }
}
