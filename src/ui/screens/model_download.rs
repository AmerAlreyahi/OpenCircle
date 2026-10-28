//! The model download card (`newdesing/widgets/model-download.js`, #127):
//! the one question before an on-device model is downloaded, its progress, its
//! check, done, and why it failed. Every place that needs a model shows it:
//! the bar's text-reader note (also Copy text), translate. Nothing downloads
//! without its Download click (the callers start the download on that message
//! only). What each model says is data (`OCR`, `TRANSLATION`).
use iced::widget::{Space, column, container, pin, row, text};
use iced::{Center, Element, Fill, Padding, Rectangle, Size};

use crate::ui::kit::button::{Kind, button, regular};
use crate::ui::kit::chip::emblem;
use crate::ui::kit::icon::Icon;
use crate::ui::kit::icon_button::{self, icon_button};
use crate::ui::kit::panel_frame::{Side, side};
use crate::ui::kit::progress;
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{accent, sub};
use crate::ui::kit::themes;
use crate::ui::kit::tokens::{Role, radius};

/// A model the card offers (the design's `models` catalog).
pub struct Model {
    pub name: &'static str,
    pub icon: Icon,
    /// The question's title.
    pub title: &'static str,
    /// What it does, before "on this PC; none of your content is sent."
    pub does: &'static str,
    /// Where it comes from.
    pub source: &'static str,
    /// What runs once it is in place ("Continuing to …").
    pub then: &'static str,
}

pub const OCR: Model = Model {
    name: "Text recognition",
    icon: Icon::Text,
    title: "Text recognition needs a small download",
    does: "Read text in images",
    source: "GitHub",
    then: "read text",
};

pub const TRANSLATION: Model = Model {
    name: "Offline translation",
    icon: Icon::Translate,
    title: "Offline translation needs a download",
    does: "Translate without internet",
    source: "Hugging Face",
    then: "translate the screen",
};

/// Where the download is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage<'a> {
    /// Asks; nothing has been fetched.
    Ask,
    /// Fetching, 0..1 of the whole.
    Downloading(f32),
    /// All fetched: checked and set up.
    Verifying,
    // ponytail: the design's Done stage; the callers go straight on to their
    // work once it is Done, so only the gallery and tests show it.
    #[cfg_attr(not(test), allow(dead_code))]
    Done,
    /// Why it stopped.
    Failed(&'a str),
}

impl Stage<'_> {
    /// A download at `share`: the files are checked as the last ones land.
    pub fn at(share: f32) -> Self {
        match share >= 1.0 {
            true => Stage::Verifying,
            false => Stage::Downloading(share),
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
}

/// The card's width (`.oc-model-card`; less when the room is narrower).
pub const WIDTH: f32 = 440.0;
/// Its header (8 12 0 18) and content (16 20 20) padding.
const HEADER: Padding = Padding {
    top: 8.0,
    right: 12.0,
    bottom: 0.0,
    left: 18.0,
};
const CONTENT: Padding = Padding {
    top: 16.0,
    right: 20.0,
    bottom: 20.0,
    left: 20.0,
};
/// Between the emblem and the title; the title and its meta line.
const HEADING_GAP: f32 = 12.0;
const META_GAP: f32 = 3.0;
/// Above the description, the buttons, the provider link, the transfer
/// lines (the progress bar keeps 6 px of its own above for its glow).
const ABOVE_TEXT: f32 = 12.0;
const ABOVE_BUTTONS: f32 = 18.0;
const ABOVE_LINK: f32 = 12.0;
const ABOVE_TRANSFER: f32 = 14.0;
const ABOVE_BAR: f32 = 4.0;
const ABOVE_CANCEL: f32 = 8.0;
/// Between buttons.
const BUTTON_GAP: f32 = 8.0;
/// The title's and the description's line heights.
const TITLE_LINE: f32 = 1.4;
const TEXT_LINE: f32 = 1.6;

/// `bytes` as the design writes it: whole MB, at least 1.
fn mb(bytes: u64) -> u64 {
    bytes.div_ceil(1_000_000).max(1)
}

/// The card: `model` (`bytes` to download, the whole set) at `stage`.
pub fn card<'a, M: Clone + 'a>(
    model: &'static Model,
    bytes: u64,
    stage: Stage<'a>,
    on: On<M>,
) -> Element<'a, M> {
    let c = &themes::current().c;
    let close = icon_button(
        Icon::Close.glyph(),
        icon_button::State::default(),
        on.dismiss.clone(),
    );
    let header = row![
        text("On-device models")
            .size(Role::META.size)
            .color(c.text_muted)
            .width(Fill),
    ]
    .push(on.dismiss.is_some().then_some(close))
    .padding(HEADER)
    .align_y(Center);
    let content = column![heading(model, bytes, stage)]
        .push(body(model, bytes, stage, on))
        .padding(CONTENT);
    surface(Material::Widget, radius::WINDOW, column![header, content])
        .width(WIDTH)
        .into()
}

/// The emblem, the stage's title and the model's line.
fn heading<'a, M: 'a>(model: &'static Model, bytes: u64, stage: Stage<'a>) -> Element<'a, M> {
    let (icon, title) = match stage {
        Stage::Ask => (model.icon, model.title.to_string()),
        Stage::Downloading(_) => (
            model.icon,
            format!("Downloading {}", model.name.to_lowercase()),
        ),
        Stage::Verifying => (model.icon, "Checking your download".into()),
        Stage::Done => (Icon::Check, "Ready".into()),
        Stage::Failed(_) => (Icon::Close, "Download could not finish".into()),
    };
    let meta = format!("{} · {} MB · from {}", model.name, mb(bytes), model.source);
    let words = column![
        crate::ui::kit::text::text(Role::CARD_HEADING, title).line_height(TITLE_LINE),
        sub(Role::META, meta).line_height(TEXT_LINE),
    ]
    .spacing(META_GAP)
    .width(Fill);
    row![emblem(icon), words]
        .spacing(HEADING_GAP)
        .align_y(Center)
        .into()
}

/// What the stage says and its buttons.
fn body<'a, M: Clone + 'a>(
    model: &'static Model,
    bytes: u64,
    stage: Stage<'a>,
    on: On<M>,
) -> Element<'a, M> {
    let said = |s: String| {
        column![
            Space::new().height(ABOVE_TEXT),
            sub(Role::NOTE, s).line_height(TEXT_LINE)
        ]
    };
    let buttons = |primary: &'a str, other: &'a str| {
        let mut r = row![regular(primary, true, Some(on.download.clone()))].spacing(BUTTON_GAP);
        if let Some(dismiss) = on.dismiss.clone() {
            r = r.push(regular(other, false, Some(dismiss)));
        }
        column![Space::new().height(ABOVE_BUTTONS), r]
    };
    match stage {
        Stage::Ask => {
            let mut c = said(format!(
                "{} on this PC; none of your content is sent.",
                model.does
            ))
            .push(buttons("Download", "Not now"));
            if let Some(provider) = on.provider {
                let link = text("Use an AI provider instead").size(Role::META.size);
                c = c.push(Space::new().height(ABOVE_LINK)).push(button(
                    link,
                    Kind::Quiet,
                    Some(provider),
                ));
            }
            c.into()
        }
        Stage::Downloading(share) => {
            let total = bytes as f64 / 1e6;
            let done = total * share as f64;
            let line = row![
                sub(Role::META, format!("{done:.1} / {} MB", mb(bytes))).width(Fill),
                accent(Role::META_STRONG, format!("{:.0}%", share * 100.0)),
            ]
            .align_y(Center);
            let mut c = column![
                Space::new().height(ABOVE_TRANSFER),
                line,
                Space::new().height(ABOVE_BAR),
                progress::bar(share),
            ];
            if let Some(dismiss) = on.dismiss {
                c = c.push(Space::new().height(ABOVE_CANCEL)).push(row![
                    Space::new().width(Fill),
                    regular("Cancel", false, Some(dismiss))
                ]);
            }
            c.into()
        }
        // ponytail: the design's sliding bar is drawn full and still (no
        // frame clock for a step that lasts a moment)
        Stage::Verifying => said("Checking integrity and setting up on this PC…".into())
            .push(Space::new().height(ABOVE_BAR))
            .push(progress::bar(1.0))
            .into(),
        Stage::Done => said(format!(
            "{} is installed. Continuing to {}…",
            model.name, model.then
        ))
        .into(),
        Stage::Failed(why) => said(why.to_string())
            .push(buttons("Try again", "Cancel"))
            .into(),
    }
}

/// `card` in a result card's spot `at` on `screen`: as tall as it is, at the
/// spot's end facing the screen's middle (a short card, not a slab).
pub fn placed<'a, M: 'a>(card: Element<'a, M>, at: Rectangle, screen: Size) -> Element<'a, M> {
    let end = match side(at, screen) {
        Side::Top => iced::Bottom,
        _ => iced::Top,
    };
    let spot = container(card)
        .width(at.width)
        .height(at.height)
        .align_y(end);
    pin(spot).position(at.position()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::{golden, shot};
    use crate::ui::kit::themes::{DAYLIGHT, SAPPHIRE};

    const FAILED: &str = "No internet connection. Reconnect, then try again.";

    fn stages() -> Vec<(&'static str, &'static Model, Stage<'static>, bool)> {
        vec![
            ("ask-ocr", &OCR, Stage::Ask, false),
            ("ask-translation", &TRANSLATION, Stage::Ask, true),
            ("downloading", &OCR, Stage::Downloading(0.42), false),
            ("verifying", &OCR, Stage::Verifying, false),
            ("done", &OCR, Stage::Done, false),
            ("failed", &OCR, Stage::Failed(FAILED), false),
        ]
    }

    fn view(model: &'static Model, stage: Stage<'static>, provider: bool) -> Element<'static, u8> {
        let on = On {
            download: 1,
            dismiss: Some(2),
            provider: provider.then_some(3),
        };
        card(model, 15_000_000, stage, on)
    }

    const SIZE: Size = Size::new(WIDTH, 240.0);

    /// Each stage, Sapphire and Daylight, matches its golden image, and no
    /// text is cut off.
    #[test]
    fn every_stage_matches_its_golden_image() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for (name, model, stage, provider) in stages() {
                let lost = shot::out_of_reach(view(model, stage, provider), SIZE);
                assert!(lost.is_empty(), "{name}: {lost:?}");
                let file = format!("model-download-{name}-{}", theme.id);
                failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                    view(model, stage, provider)
                }));
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
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
        assert!(failed.iter().any(|t| t == "Try again") && failed.iter().any(|t| t == FAILED));
        let downloading = texts(Stage::Downloading(0.5), false);
        assert!(downloading.iter().any(|t| t == "50%"), "{downloading:?}");
        assert!(
            downloading.iter().any(|t| t == "7.5 / 15 MB"),
            "{downloading:?}"
        );
    }

    /// No dismiss (a download the caller can't stop): no Cancel, no close.
    #[test]
    fn a_download_that_cannot_stop_offers_no_cancel() {
        let on = On {
            download: 1u8,
            dismiss: None,
            provider: None,
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
        assert_eq!(Stage::at(0.4), Stage::Downloading(0.4));
        assert_eq!(Stage::at(1.0), Stage::Verifying);
    }
}
