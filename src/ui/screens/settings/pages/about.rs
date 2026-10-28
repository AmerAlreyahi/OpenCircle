//! About (`settings-window.js` "ABOUT"): the app card, the developer card,
//! who made what we ship or download, problems and telemetry. A link opens
//! only when its button is clicked (the page itself never touches the
//! network: the avatar and the profile are bundled, refreshed at each release
//! by `scripts/refresh-about.sh`).
use std::sync::LazyLock;

use iced::widget::image;
use iced::widget::image::Handle;
use iced::widget::{column, row};
use iced::{Center, ContentFit, Element, Fill};
use serde::Deserialize;

use super::super::state::{Message, State};
use crate::ui::kit::button::{Kind, button, fill, label};
use crate::ui::kit::card::card;
use crate::ui::kit::chip::{chip, pill};
use crate::ui::kit::divider::divider;
use crate::ui::kit::logo::logo;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{accent, sub, text};
use crate::ui::kit::tokens::{Role, radius, space};
use crate::ui::kit::{icon, themes};
use crate::ui::widgets::chrome::png_handle;

pub const PROJECT: &str = "https://github.com/AmerAlreyahi/OpenCircle";
pub const LICENSE: &str = "https://github.com/AmerAlreyahi/OpenCircle/blob/main/LICENSE";
pub const WEBSITE: &str = "https://ameralreyahi.com/projects/opencircle";
pub const NOTICE: &str = "https://github.com/AmerAlreyahi/OpenCircle/blob/main/NOTICE";
pub const PROFILE_URL: &str = "https://github.com/AmerAlreyahi";

/// Who made what we ship or download: (what, made by, licence). The full texts
/// are in the repository's NOTICE file.
pub const NOTICES: [(&str, &str, &str); 11] = [
    (
        "Text recognition",
        "PaddleOCR PP-OCRv5, Baidu",
        "Apache-2.0",
    ),
    ("OCR ONNX export", "oar-ocr, GreatV", "Apache-2.0"),
    ("ONNX Runtime", "Microsoft", "MIT"),
    (
        "Offline translation",
        "OPUS-MT, Helsinki-NLP; ONNX by Xenova",
        "CC BY 4.0 / Apache-2.0",
    ),
    ("iced", "The iced contributors", "MIT"),
    ("tiny-skia", "Yevhenii Reizner", "BSD-3-Clause"),
    ("cosmic-text", "System76", "MIT / Apache-2.0"),
    ("Lucide icons", "Lucide contributors", "ISC"),
    ("Inter font", "Rasmus Andersson", "OFL-1.1"),
    ("Noto Sans Arabic", "Google", "OFL-1.1"),
    ("JetBrains Mono", "JetBrains", "OFL-1.1"),
];

/// Between the cards (`space-y-3.5`) and inside one; between a tile and its text (`gap-4`).
const CARD_GAP: f32 = 14.0;
const TILE_GAP: f32 = 16.0;
/// The grid of a card's buttons (`gap-2`), a credit row's height (`py-2.5`).
const BUTTON_GAP: f32 = 8.0;
const CREDIT_PAD: f32 = 10.0;
/// The logo tile (`w-16`, `p-1.5`), the avatar's (`w-[72px]`), a licence pill.
const LOGO_TILE: f32 = 64.0;
const AVATAR_TILE: f32 = 72.0;
const TILE_PAD: f32 = 6.0;
const LICENCE: f32 = 155.0;

/// The developer card's data: `assets/about/profile.toml`.
#[derive(Debug, Deserialize)]
pub struct Profile {
    pub name: String,
    pub login: String,
    pub website: String,
    pub bio: String,
}

pub fn profile() -> &'static Profile {
    static PROFILE: LazyLock<Profile> = LazyLock::new(|| {
        toml::from_str(include_str!("../../../../../assets/about/profile.toml"))
            .expect("assets/about/profile.toml parses")
    });
    &PROFILE
}

fn avatar() -> Handle {
    static AVATAR: LazyLock<Handle> =
        LazyLock::new(|| png_handle(include_bytes!("../../../../../assets/about/avatar.png")));
    AVATAR.clone()
}

/// A picture in a rounded tile (`rounded-2xl`, divider edge).
fn tile<'a>(handle: Handle, size: f32) -> Element<'a, Message> {
    let inner = size - 2.0 * TILE_PAD;
    let picture = image(handle)
        .width(inner)
        .height(inner)
        .content_fit(ContentFit::Cover)
        .border_radius(radius::CARD);
    surface(Material::Recessed, radius::WINDOW, picture)
        .padding(TILE_PAD)
        .into()
}

fn link<'a>(name: &'a str, url: &str) -> Element<'a, Message> {
    fill(label(name), Some(Message::OpenLink(url.into())))
}

pub fn view(_: &State) -> Element<'_, Message> {
    column![app(), developer(), notices(), problems(), telemetry()]
        .spacing(CARD_GAP)
        .into()
}

/// Logo, name, version, what it is, and its links.
fn app<'a>() -> Element<'a, Message> {
    let names = row![
        text(Role::APP_NAME, "OpenCircle"),
        chip(concat!("v", env!("CARGO_PKG_VERSION")).into()),
        pill("Pure Rust · iced", false, None),
        pill("MIT", false, None),
    ]
    .spacing(BUTTON_GAP)
    .align_y(Center);
    let what = sub(
        Role::NOTE,
        "Open-source desktop Circle to Search for Windows & Linux: circle anything on \
         screen to search in your browser, read text (OCR), translate in place, or ask AI.",
    );
    let head = row![
        tile(logo((LOGO_TILE - 2.0 * TILE_PAD) as u32), LOGO_TILE),
        column![names, what].spacing(space::GAP).width(Fill),
    ]
    .spacing(TILE_GAP)
    .align_y(Center);
    let links = row![
        link("GitHub", PROJECT),
        link("Website", WEBSITE),
        link("MIT licence", LICENSE),
    ]
    .spacing(BUTTON_GAP);
    card(column![head, links].spacing(CARD_GAP))
}

/// The developer: avatar, name, handle, bio, and their links.
fn developer<'a>() -> Element<'a, Message> {
    let p = profile();
    let head = row![
        tile(avatar(), AVATAR_TILE),
        column![
            row![
                text(Role::PAGE_TITLE, p.name.as_str()),
                accent(Role::KEYCAP, format!("@{}", p.login)),
            ]
            .spacing(BUTTON_GAP)
            .align_y(Center),
            sub(Role::NOTE, p.bio.as_str()),
        ]
        .spacing(space::GAP)
        .width(Fill),
    ]
    .spacing(TILE_GAP)
    .align_y(Center);
    let star = fill(
        row![
            icon::icon(icon::Icon::Star, 12.0).color(themes::current().c.accent),
            accent(Role::CONTROL, "Star OpenCircle").line_height(1.5),
        ]
        .spacing(space::GAP)
        .align_y(Center),
        Some(Message::OpenLink(PROJECT.into())),
    );
    let links = row![
        link("GitHub profile", PROFILE_URL),
        star,
        fill(label("Website"), Some(Message::OpenLink(p.website.clone()))),
    ]
    .spacing(BUTTON_GAP);
    card(column![head, links].spacing(CARD_GAP))
}

/// Who made what we ship: one row each (name and maker, then licence), and
/// the link to the full texts.
fn notices<'a>() -> Element<'a, Message> {
    let head = row![
        column![
            text(Role::ROW_TITLE, "Made by others"),
            sub(
                Role::ROW_DESC,
                "Downloaded on first use, under their own licences"
            ),
        ]
        .width(Fill),
        button(
            label("All licences"),
            Kind::Elevated,
            Some(Message::OpenLink(NOTICE.into()))
        ),
    ]
    .align_y(Center);
    let mut list = column![];
    for (i, (what, by, licence)) in NOTICES.into_iter().enumerate() {
        if i > 0 {
            list = list.push(divider());
        }
        list = list.push(
            row![
                column![text(Role::CONTROL, what), sub(Role::ROW_DESC, by)].width(Fill),
                pill(licence, true, Some(LICENCE)),
            ]
            .align_y(Center)
            .padding([CREDIT_PAD, 0.0]),
        );
    }
    card(column![head, list].spacing(space::GAP))
}

fn problems<'a>() -> Element<'a, Message> {
    let b = |s, m| button(label(s), Kind::Elevated, Some(m));
    setting_row::row(
        title("Problems"),
        "Copy a report to paste into an issue. Nothing is sent",
        row![
            b("Copy diagnostics", Message::CopyDiagnostics),
            b("Report a problem", Message::ReportProblem),
        ]
        .spacing(space::GAP),
    )
}

fn telemetry<'a>() -> Element<'a, Message> {
    setting_row::row(
        title("Telemetry"),
        "A capture leaves this PC only when you choose an action",
        pill("None", false, None),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn https(url: &str) -> bool {
        url.starts_with("https://") && crate::sys::open::web_host(url).is_some()
    }

    #[test]
    fn profile_parses_and_every_link_is_https() {
        let p = profile();
        assert_eq!(p.login, "AmerAlreyahi");
        assert!(!p.name.is_empty() && !p.bio.is_empty());
        assert!(https(&p.website), "{}", p.website);
        for url in [PROJECT, WEBSITE, LICENSE, NOTICE, PROFILE_URL] {
            assert!(https(url), "{url}");
        }
        assert!(include_bytes!("../../../../../assets/about/avatar.png").starts_with(b"\x89PNG"));
    }

    #[test]
    fn notices_credit_the_models_we_download() {
        let notice = include_str!("../../../../../NOTICE");
        for key in ["Helsinki-NLP", "Xenova", "PaddleOCR", "ONNX Runtime"] {
            assert!(notice.contains(key), "{key} is in NOTICE");
        }
        let all: String = NOTICES.iter().map(|n| format!("{} {}", n.1, n.2)).collect();
        for key in [
            "Helsinki-NLP",
            "Xenova",
            "CC BY 4.0",
            "PaddleOCR",
            "Microsoft",
        ] {
            assert!(all.contains(key), "{key} is credited on the page");
        }
    }
}
