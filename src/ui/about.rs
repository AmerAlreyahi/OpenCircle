//! The About page of Settings: the app, the notices for what we ship or download,
//! and the developer card. Generic over the window's message; a link is only ever
//! opened by the click on its button (the page itself never touches the network:
//! the avatar and the profile are bundled, refreshed by `scripts/refresh-about.sh`).

use std::sync::LazyLock;

use iced::widget::image::Handle;
use iced::widget::{column, container, image, row, text};
use iced::{Border, Center, ContentFit, Element, Length};
use serde::Deserialize;

use crate::ui::theme;
use crate::ui::widgets::{self, button_group, caption, form_button, label_button};

pub const PROJECT: &str = "https://github.com/AmerAlreyahi/OpenCircle";
pub const LICENSE: &str = "https://github.com/AmerAlreyahi/OpenCircle/blob/main/LICENSE";
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
    (
        "wry (results card)",
        "Tauri contributors",
        "MIT / Apache-2.0",
    ),
    ("Lucide icons", "Lucide contributors", "ISC"),
    ("Inter font", "Rasmus Andersson", "OFL-1.1"),
    ("Noto Sans Arabic", "Google", "OFL-1.1"),
];

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
        toml::from_str(include_str!("../../assets/about/profile.toml"))
            .expect("assets/about/profile.toml parses")
    });
    &PROFILE
}

fn avatar() -> Handle {
    static AVATAR: LazyLock<Handle> = LazyLock::new(|| {
        let bytes = include_bytes!("../../assets/about/avatar.png");
        let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes[..]))
            .read_info()
            .expect("the bundled avatar is a PNG");
        let mut rgba = vec![0; reader.output_buffer_size().expect("sized")];
        let info = reader.next_frame(&mut rgba).expect("the avatar decodes");
        rgba.truncate(info.buffer_size());
        Handle::from_rgba(info.width, info.height, rgba)
    });
    AVATAR.clone()
}

fn logo() -> Handle {
    static LOGO: LazyLock<Handle> = LazyLock::new(|| {
        let l = widgets::chrome::logo();
        Handle::from_rgba(l.width, l.height, l.rgba.clone())
    });
    LOGO.clone()
}

/// A rounded panel, the same fill as the form controls.
fn card<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(|_| {
            let p = theme::current();
            container::Style {
                background: Some(p.field.into()),
                border: Border {
                    color: p.border,
                    width: 1.0,
                    radius: 18.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn picture<'a, M: 'a>(handle: Handle, size: f32, radius: f32) -> Element<'a, M> {
    image(handle)
        .width(size)
        .height(size)
        .content_fit(ContentFit::Cover)
        .border_radius(radius)
        .into()
}

fn name<'a>(s: impl text::IntoFragment<'a>, size: f32) -> iced::widget::Text<'a> {
    text(s).size(size).color(theme::current().text)
}

/// The app card: logo, name, version, what it is, and its links.
pub fn app<'a, M: Clone + 'a>(open: fn(String) -> M) -> Element<'a, M> {
    let links = button_group([
        form_button("GitHub", false, Some(open(PROJECT.into()))).into(),
        form_button("MIT licence", false, Some(open(LICENSE.into()))).into(),
    ]);
    let head = row![
        picture(logo(), 64.0, 14.0),
        column![
            name("OpenCircle", 24.0),
            caption(format!("Version {}", env!("CARGO_PKG_VERSION"))),
            caption("Circle anything on screen to search, read, translate or ask about it"),
        ]
        .spacing(3),
    ]
    .spacing(16)
    .align_y(Center);
    card(column![head, links].spacing(16))
}

/// One licence name in a pill: every pill the same size.
fn licence_pill<'a, M: 'a>(licence: &'a str) -> Element<'a, M> {
    container(text(licence).size(12).color(theme::current().text))
        .width(170)
        .height(28)
        .center_x(170)
        .center_y(28)
        .style(|_| container::Style {
            background: Some(theme::current().window.into()),
            border: Border::default().rounded(14),
            ..container::Style::default()
        })
        .into()
}

/// Who made what we ship: one row each (name and maker, then licence), and the
/// link to the full texts.
pub fn notices<'a, M: Clone + 'a>(open: fn(String) -> M) -> Element<'a, M> {
    let mut list = column![].spacing(0);
    for (i, (what, by, licence)) in NOTICES.into_iter().enumerate() {
        if i > 0 {
            list = list.push(widgets::divider());
        }
        list = list.push(
            row![
                column![name(what, 14.0), caption(by)]
                    .spacing(2)
                    .width(Length::Fill),
                licence_pill(licence),
            ]
            .align_y(Center)
            .height(54),
        );
    }
    card(
        column![
            row![
                column![
                    name("Made by others", 16.0),
                    caption("Downloaded on first use, under their own licences"),
                ]
                .spacing(2)
                .width(Length::Fill),
                label_button("All licences", false, Some(open(NOTICE.into()))),
            ]
            .align_y(Center),
            list,
        ]
        .spacing(10),
    )
}

/// The developer card.
pub fn developer<'a, M: Clone + 'a>(open: fn(String) -> M) -> Element<'a, M> {
    let p = profile();
    let head = row![
        picture(avatar(), 72.0, 36.0),
        column![
            name(p.name.as_str(), 20.0),
            caption(format!("@{}", p.login)),
            caption(p.bio.as_str()),
        ]
        .spacing(3),
    ]
    .spacing(16)
    .align_y(Center);
    let buttons = button_group([
        form_button("GitHub profile", false, Some(open(PROFILE_URL.into()))).into(),
        form_button("Star OpenCircle", true, Some(open(PROJECT.into()))).into(),
        form_button("Website", false, Some(open(p.website.clone()))).into(),
    ]);
    card(column![head, buttons].spacing(16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn https(url: &str) -> bool {
        url.starts_with("https://") && crate::lens::web_host(url).is_some()
    }

    #[test]
    fn profile_parses_and_every_link_is_https() {
        let p = profile();
        assert_eq!(p.login, "AmerAlreyahi");
        assert!(!p.name.is_empty() && !p.bio.is_empty());
        assert!(https(&p.website), "{}", p.website);
        for url in [PROJECT, LICENSE, NOTICE, PROFILE_URL] {
            assert!(https(url), "{url}");
        }
        assert!(include_bytes!("../../assets/about/avatar.png").starts_with(b"\x89PNG"));
    }

    #[test]
    fn notices_credit_the_models_we_download() {
        let notice = include_str!("../../NOTICE");
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
