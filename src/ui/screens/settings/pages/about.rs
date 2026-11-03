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
use crate::ui::kit::logo::png_handle;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::{accent, sub, text};
use crate::ui::kit::tokens::{Role, radius, space};
use crate::ui::kit::{icon, themes};

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

/// One preview-clip credit: the clips it covers, its text, and its links
/// (name, address). It shows only while one of its clips is bundled; the same
/// credits, with full addresses, are in NOTICE (owner's source list, 2026-11-03).
pub struct Credit {
    pub clips: &'static [&'static str],
    pub text: &'static str,
    pub links: &'static [(&'static str, &'static str)],
}

const CC0: (&str, &str) = (
    "CC0 1.0",
    "https://creativecommons.org/publicdomain/zero/1.0/",
);
const CC_BY: (&str, &str) = ("CC BY 4.0", "https://creativecommons.org/licenses/by/4.0/");
const CC_BY_SA: (&str, &str) = (
    "CC BY-SA 4.0",
    "https://creativecommons.org/licenses/by-sa/4.0/",
);

pub const CLIP_CREDITS: [Credit; 9] = [
    Credit {
        clips: &["circle-1"],
        text: "Succulent photograph: José Gabriel Jara Porras, WordPress Photo Directory, \
               CC0 1.0. Cropped and shown in an edited OpenCircle screen recording.",
        links: &[
            ("Photo", "https://wordpress.org/photos/photo/4569e13887/"),
            CC0,
        ],
    },
    Credit {
        clips: &["circle-2"],
        text: "Lotus photograph: Shubham Patil, WordPress Photo Directory, CC0 1.0. \
               Cropped and shown in an edited OpenCircle screen recording.",
        links: &[
            ("Photo", "https://wordpress.org/photos/photo/4766ac73e9/"),
            CC0,
        ],
    },
    Credit {
        clips: &["circle-3"],
        text: "Kingfisher photograph: sebascordero15, WordPress Photo Directory, CC0 1.0. \
               Cropped and shown in an edited OpenCircle screen recording.",
        links: &[
            ("Photo", "https://wordpress.org/photos/photo/11669e53a4/"),
            CC0,
        ],
    },
    Credit {
        clips: &["text"],
        text: "COFFEE photograph: Tim Mossholder on Unsplash, photo jozE5L60Aj4, used under \
               the Unsplash License (not CC0, not MIT). Shown in an edited OpenCircle screen \
               recording. No endorsement implied.",
        links: &[
            (
                "Photo",
                "https://unsplash.com/photos/a-sign-on-a-brick-wall-that-says-coffee-jozE5L60Aj4",
            ),
            ("Unsplash License", "https://unsplash.com/license"),
        ],
    },
    Credit {
        clips: &["translate"],
        text: "Based on \u{201c}Plante d\u{2019}intérieur\u{201d} by Wikipedia contributors, \
               CC BY-SA 4.0. Machine translation into English, German, Arabic and Spanish \
               shown by OpenCircle; framing, timing edits and active-state graphics added \
               for the preview. This preview animation and its poster are licensed under \
               CC BY-SA 4.0, including the adapter\u{2019}s contributions, as a media \
               exception to the app\u{2019}s MIT licence (the code stays MIT). Embedded \
               photograph \u{201c}Her, IFA 2018, Berlin (P1070094).jpg\u{201d} \u{a9} Matti \
               Blume, CC BY-SA 4.0; embedded painting by Carl Larsson, \
               \u{201c}Flowers on the Windowsill\u{201d}, public domain. Third-party names \
               and trademarks keep their separate rights.",
        links: &[
            (
                "Article",
                "https://fr.wikipedia.org/wiki/Plante_d%27int%C3%A9rieur",
            ),
            (
                "History",
                "https://fr.wikipedia.org/w/index.php?title=Plante_d%27int%C3%A9rieur&action=history",
            ),
            CC_BY_SA,
            (
                "Photograph",
                "https://fr.wikipedia.org/wiki/Fichier:Her,_IFA_2018,_Berlin_(P1070094).jpg",
            ),
            (
                "Painting",
                "https://commons.wikimedia.org/wiki/File:Blomsterf%C3%B6nstret_av_Carl_Larsson_1894.jpg",
            ),
            ("Licence text", LICENCE_TEXT),
        ],
    },
    Credit {
        clips: &["ai-chat", "ai-browser"],
        text: "Solar photovoltaic panel prices: Our World in Data; IRENA (2025) and other \
               sources, processed by Our World in Data, as credited in the recording. OWID \
               visualization: CC BY 4.0. Cropped and time-edited screen recordings with \
               OpenCircle active-state graphics.",
        links: &[
            (
                "Chart",
                "https://ourworldindata.org/grapher/solar-pv-prices",
            ),
            CC_BY,
        ],
    },
    Credit {
        clips: &["ai-browser"],
        text: "OpenCircle is an independent project and is not affiliated with, sponsored \
               by, or endorsed by Google. Google and its product names and logos are \
               trademarks of their respective owners. Depicted third-party services and \
               content are shown for demonstration only.",
        links: &[],
    },
    Credit {
        clips: &["snip-long"],
        text: "\u{201c}Tourism\u{201d}, Bastian Herre and Veronika Samborska (2023), Our \
               World in Data, CC BY 4.0 for OWID-produced article and visualizations. \
               Cropped, time-edited screen recording. The chart \u{201c}Arrivals of tourists \
               from abroad\u{201d} credits UN Tourism (2025); other chart and provider notices \
               in the recording are kept.",
        links: &[
            ("Article", "https://ourworldindata.org/tourism"),
            (
                "Chart",
                "https://ourworldindata.org/grapher/international-tourist-trips",
            ),
            CC_BY,
            ("Reuse FAQ", "https://ourworldindata.org/faqs"),
        ],
    },
    Credit {
        clips: &[
            "circle-1",
            "circle-2",
            "circle-3",
            "text",
            "translate",
            "ai-chat",
            "ai-browser",
            "snip-long",
        ],
        text: "Credits are for source identification; they do not clear unrelated search-result \
               thumbnails, third-party interfaces or data-provider terms shown in the clips.",
        links: &[],
    },
];

/// The repository's copy of the CC BY-SA 4.0 legal code (LICENSES/).
pub const LICENCE_TEXT: &str =
    "https://github.com/AmerAlreyahi/OpenCircle/blob/main/LICENSES/CC-BY-SA-4.0.txt";

/// The credits of the clips `bundled` says are in the app.
pub fn clip_credits(bundled: impl Fn(&str) -> bool) -> Vec<&'static Credit> {
    (CLIP_CREDITS.iter())
        .filter(|c| c.clips.iter().any(|n| bundled(n)))
        .collect()
}

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
    let mut page = column![app(), developer(), notices()];
    if let Some(card) = clip_notice() {
        page = page.push(card);
    }
    page.push(problems())
        .push(telemetry())
        .spacing(CARD_GAP)
        .into()
}

/// "Preview clips": the credits of the bundled clips (none bundled, no card).
fn clip_notice<'a>() -> Option<Element<'a, Message>> {
    let lines = clip_credits(|c| super::super::previews::player::clip(c).is_some());
    if lines.is_empty() {
        return None;
    }
    let mut list = column![text(Role::ROW_TITLE, "Preview clips")].spacing(space::GAP);
    for (i, credit) in lines.into_iter().enumerate() {
        if i > 0 {
            list = list.push(divider());
        }
        list = list.push(sub(Role::ROW_DESC, credit.text));
        if !credit.links.is_empty() {
            let links = credit.links.iter().map(|(name, url)| link(name, url));
            list = list.push(row(links).spacing(BUTTON_GAP).wrap());
        }
    }
    Some(card(list))
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
