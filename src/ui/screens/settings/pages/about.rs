//! About (`settings-window.js` "ABOUT"), clean and minimal (owner, 2026-11-11,
//! planning #177): the app card, "Something not working?" (Copy diagnostics,
//! Report a problem), the developer card, who made what we ship or download,
//! the preview clips' credits (folded) and one line on telemetry. Links are the
//! kit's quiet links; a link opens only when it is clicked (the page itself
//! never touches the network: the avatar and the profile are bundled,
//! refreshed at each release by `scripts/refresh-about.sh`).
use crate::shared::app_language::{S, fmt};
use crate::t;
use std::sync::LazyLock;

use iced::widget::image;
use iced::widget::image::Handle;
use iced::widget::tooltip::Position;
use iced::widget::{Space, column};
use iced::{Center, ContentFit, Element, Fill};
use serde::Deserialize;

use super::super::state::{Message, State};
use crate::dir_row;
use crate::shared::kit::button::{Kind, button, fold, label, regular};
use crate::shared::kit::card::card;
use crate::shared::kit::chip::{chip, pill};
use crate::shared::kit::dir;
use crate::shared::kit::icon::{Icon, icon};
use crate::shared::kit::logo::logo;
use crate::shared::kit::logo::try_decode;
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{accent, paragraph, sub, text};
use crate::shared::kit::tokens::{Role, radius, space};
use crate::shared::kit::tooltip::{Why, hint};

pub const PROJECT: &str = "https://github.com/AmerAlreyahi/OpenCircle";
pub const LICENSE: &str = "https://github.com/AmerAlreyahi/OpenCircle/blob/main/LICENSE.md";
pub const WEBSITE: &str = "https://ameralreyahi.com/projects/opencircle";
pub const NOTICE: &str = "https://github.com/AmerAlreyahi/OpenCircle/blob/main/docs/NOTICE.md";
pub const PROFILE_URL: &str = "https://github.com/AmerAlreyahi";

/// Who made what we ship or download: (what, made by, licence). The full texts
/// are in the repository's docs/NOTICE.md. (A function: what each is, in the
/// app's language; names and licences stay as written.)
pub fn made_by() -> [(&'static str, &'static str, &'static str); 11] {
    [
        (t!(ab_n1), "PaddleOCR PP-OCRv5, Baidu", "Apache-2.0"),
        (t!(ab_n2), "oar-ocr, GreatV", "Apache-2.0"),
        (t!(ab_n3), "Microsoft", t!(ab_mit)),
        (t!(ab_n4), t!(ab_n4b), "CC BY 4.0 / Apache-2.0"),
        ("iced", t!(ab_n5b), t!(ab_mit)),
        ("tiny-skia", "Yevhenii Reizner", "BSD-3-Clause"),
        ("cosmic-text", "System76", "MIT / Apache-2.0"),
        (t!(ab_n8), t!(ab_n8b), "ISC"),
        (t!(ab_n9), "Rasmus Andersson", "OFL-1.1"),
        ("Noto Sans Arabic", "Google", "OFL-1.1"),
        ("JetBrains Mono", "JetBrains", "OFL-1.1"),
    ]
}

/// "Made by others" shows this many until "Show all".
pub const MADE_BY_FIRST: usize = 6;

/// One preview clip's credit: the clips it covers, its one line (what, who,
/// licence and the changes made) and its section in docs/NOTICE.md (the
/// heading's GitHub anchor), which holds the full text and every source link.
/// It shows only while one of its clips is bundled (owner's source list,
/// 2026-11-03). English in every language: legal attribution.
pub struct Credit {
    pub clips: &'static [&'static str],
    pub line: &'static str,
    pub anchor: &'static str,
}

pub const CLIP_CREDITS: [Credit; 8] = [
    Credit {
        clips: &["circle-1"],
        line: "Succulent photo \u{b7} Jos\u{e9} Gabriel Jara Porras, WordPress Photo Directory \
               \u{b7} CC0 1.0",
        anchor: "succulent-photograph-circle-1",
    },
    Credit {
        clips: &["circle-2"],
        line: "Lotus photo \u{b7} Shubham Patil, WordPress Photo Directory \u{b7} CC0 1.0",
        anchor: "lotus-photograph-circle-2",
    },
    Credit {
        clips: &["circle-3"],
        line: "Kingfisher photo \u{b7} sebascordero15, WordPress Photo Directory \u{b7} CC0 1.0",
        anchor: "kingfisher-photograph-circle-3",
    },
    Credit {
        clips: &["text"],
        line: "COFFEE photo \u{b7} Tim Mossholder on Unsplash \u{b7} Unsplash License",
        anchor: "coffee-photograph-text",
    },
    Credit {
        clips: &["translate-page"],
        line: "\u{201c}Plante d\u{2019}int\u{e9}rieur\u{201d} \u{b7} Wikipedia contributors; photo \
               \u{201c}Her, IFA 2018, Berlin\u{201d} \u{a9} Matti Blume \u{b7} CC BY-SA 4.0 \u{b7} \
               machine-translated and edited; this clip is CC BY-SA 4.0",
        anchor: "wikipedia-translation-translate-page",
    },
    Credit {
        clips: &["translate-2", "translate-3"],
        line: "Game screens \u{b7} original images made for OpenCircle (no real game)",
        anchor: "game-screens-translate-2-translate-3",
    },
    Credit {
        clips: &["ai-chat", "ai-browser"],
        line: "\u{201c}Solar photovoltaic panel prices\u{201d} \u{b7} Our World in Data, IRENA \
               (2025) and other sources \u{b7} CC BY 4.0 \u{b7} cropped and edited",
        anchor: "solar-photovoltaic-panel-prices-ai-chat-ai-browser",
    },
    Credit {
        clips: &["snip-long"],
        line: "\u{201c}Tourism\u{201d} \u{b7} Bastian Herre and Veronika Samborska, Our World in \
               Data \u{b7} CC BY 4.0 \u{b7} cropped and edited",
        anchor: "tourism-snip-long",
    },
];

/// Under the credits, folded or not, while the browser clip (Google AI Mode)
/// is bundled: the non-affiliation line, verbatim.
pub const NOT_AFFILIATED: &str = "OpenCircle is an independent project and is not affiliated \
     with, sponsored by, or endorsed by Google. Google and its product names and logos are \
     trademarks of their respective owners. Depicted third-party services and content are \
     shown for demonstration only.";
pub const NOT_AFFILIATED_CLIP: &str = "ai-browser";

/// The last line of the unfolded credits.
pub const CAVEAT: &str = "Credits are for source identification; they do not clear unrelated \
     search-result thumbnails, third-party interfaces or data-provider terms shown in the clips.";

/// The credits of the clips `bundled` says are in the app.
pub fn clip_credits(bundled: impl Fn(&str) -> bool) -> Vec<&'static Credit> {
    (CLIP_CREDITS.iter())
        .filter(|c| c.clips.iter().any(|n| bundled(n)))
        .collect()
}

/// A credit's section in docs/NOTICE.md on GitHub.
pub fn credit_url(c: &Credit) -> String {
    format!("{NOTICE}#{}", c.anchor)
}

/// Between the cards; inside a card; between a tile and its text.
const CARD_GAP: f32 = 14.0;
const TILE_GAP: f32 = 14.0;
/// Between two links, two buttons, a link's words and its icon.
const LINK_GAP: f32 = 16.0;
const BUTTON_GAP: f32 = 8.0;
const ICON_GAP: f32 = 4.0;
const LINK_ICON: f32 = 12.0;
/// A "Made by others" row's padding (above and below).
const LIST_PAD: f32 = 5.0;
/// The logo and avatar tiles, and their inner pad.
const TILE: f32 = 52.0;
const TILE_PAD: f32 = 5.0;
/// A licence chip's font size (`chip::pill`'s mono) and its side pads.
const CHIP_FONT: f32 = 11.0;
const CHIP_PADS: f32 = 2.0 * 8.0 + 4.0;

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
        toml::from_str(include_str!("../../../../../assets/about/profile.toml")).unwrap_or_else(
            |e| {
                crate::log!("[opencircle] about: profile.toml unreadable: {e}");
                Profile {
                    name: "Amer Alreyahi".into(),
                    login: "AmerAlreyahi".into(),
                    website: PROFILE_URL.into(),
                    bio: "Building fast, private desktop tools.".into(),
                }
            },
        )
    });
    &PROFILE
}

/// The avatar is fetched at release time (`scripts/refresh-about.sh`), so it is not a trusted
/// asset: sniff the content (GitHub serves JPEG today), never trust the `.png` name.
fn decode_avatar(bytes: &[u8]) -> Option<Handle> {
    if let Some((w, h, rgba)) = try_decode(bytes) {
        return Some(Handle::from_rgba(w, h, rgba));
    }
    let img = ::image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(Handle::from_rgba(img.width(), img.height(), img.into_raw()))
}

fn avatar() -> Handle {
    static AVATAR: LazyLock<Handle> = LazyLock::new(|| {
        decode_avatar(include_bytes!("../../../../../assets/about/avatar.png")).unwrap_or_else(
            || {
                crate::log!("[opencircle] about: the bundled avatar does not decode");
                logo(TILE as u32)
            },
        )
    });
    AVATAR.clone()
}

/// A picture in a rounded tile (divider edge).
fn tile<'a>(handle: Handle) -> Element<'a, Message> {
    let inner = TILE - 2.0 * TILE_PAD;
    let picture = image(handle)
        .width(inner)
        .height(inner)
        .content_fit(ContentFit::Cover)
        .border_radius(radius::CARD);
    surface(Material::Recessed, radius::WINDOW, picture)
        .padding(TILE_PAD)
        .into()
}

/// A quiet link (the kit's link look: secondary text, brighter under the
/// cursor) with the external-link icon after its words.
fn link<'a>(name: &'a str, url: impl Into<String>) -> Element<'a, Message> {
    let words = dir_row![label(name), icon(Icon::External, LINK_ICON)]
        .spacing(ICON_GAP)
        .align_y(Center);
    button(words, Kind::Quiet, Some(Message::OpenLink(url.into())))
}

/// A card's links in one row, `LINK_GAP` apart, at the start edge.
fn links<'a>(all: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    dir::row(all).spacing(LINK_GAP).align_y(Center).into()
}

/// A tile, then its lines (start-aligned) beside it.
fn headed<'a>(picture: Handle, lines: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    let lines = lines.spacing(space::GAP).align_x(dir::start()).width(Fill);
    card(
        dir_row![tile(picture), lines]
            .spacing(TILE_GAP)
            .align_y(Center),
    )
}

pub fn view(state: &State) -> Element<'_, Message> {
    let mut page = column![app(), trouble(), developer(), others(state)];
    if let Some(credits) = clip_notice(state) {
        page = page.push(credits);
    }
    page.push(paragraph(sub(Role::ROW_DESC, t!(ab_no_telemetry))))
        .spacing(CARD_GAP)
        .into()
}

/// Logo, name and version, one line of what it is, and its links.
fn app<'a>() -> Element<'a, Message> {
    let name = dir_row![
        text(Role::APP_NAME, "OpenCircle"),
        chip(concat!("v", env!("CARGO_PKG_VERSION")).into()),
    ]
    .spacing(BUTTON_GAP)
    .align_y(Center);
    let links = links(vec![
        link(t!(ab_github), PROJECT),
        link(t!(ab_website), WEBSITE),
        link(t!(ab_licence), LICENSE),
    ]);
    let tile = logo((TILE - 2.0 * TILE_PAD) as u32);
    headed(
        tile,
        column![name, paragraph(sub(Role::NOTE, t!(ab_what))), links],
    )
}

/// "Something not working?": copy the diagnostics (the accent: the report
/// needs them), then report; nothing is sent.
fn trouble<'a>() -> Element<'a, Message> {
    let buttons = dir_row![
        regular(t!(ab_diag), true, Some(Message::CopyDiagnostics)),
        regular(t!(ab_report), false, Some(Message::ReportProblem)),
    ]
    .spacing(BUTTON_GAP)
    .align_y(Center);
    card(
        column![
            text(Role::ROW_TITLE, t!(ab_trouble)),
            paragraph(sub(Role::ROW_DESC, t!(ab_trouble_desc))),
            Space::new().height(space::GAP),
            buttons,
        ]
        .spacing(space::GAP)
        .align_x(dir::start()),
    )
}

/// The developer: avatar, name, handle, bio, and their links.
fn developer<'a>() -> Element<'a, Message> {
    let p = profile();
    let name = dir_row![
        text(Role::ROW_TITLE, p.name.as_str()),
        accent(Role::KEYCAP, format!("@{}", p.login)),
    ]
    .spacing(BUTTON_GAP)
    .align_y(Center);
    let star = button(
        dir_row![icon(Icon::Star, LINK_ICON), label(t!(ab_star))]
            .spacing(ICON_GAP)
            .align_y(Center),
        Kind::Quiet,
        Some(Message::OpenLink(PROJECT.into())),
    );
    let links = links(vec![
        link(t!(ab_profile), PROFILE_URL),
        star,
        link(t!(ab_website), p.website.clone()),
    ]);
    // the profile's bio in English; the table's Arabic draft in Arabic
    let bio = paragraph(sub(Role::NOTE, fmt(S::ab_bio, &[&p.bio])));
    headed(avatar(), column![name, bio, links])
}

/// Who made what we ship: one row each (what and its maker, the licence in
/// an aligned column), the first few until "Show all", and the full texts.
fn others(state: &State) -> Element<'_, Message> {
    let all = made_by();
    let chip_w = (all.iter())
        .map(|n| crate::shared::kit::text::width(n.2, CHIP_FONT, crate::shared::kit::text::MONO))
        .fold(0.0, f32::max)
        .ceil()
        + CHIP_PADS;
    let head = dir_row![
        iced::widget::container(text(Role::ROW_TITLE, t!(ab_others)))
            .width(Fill)
            .align_x(dir::start()),
        link(t!(ab_all), NOTICE),
    ]
    .align_y(Center);
    let shown = match state.made_by_all {
        true => all.len(),
        false => MADE_BY_FIRST,
    };
    let mut list = column![head, paragraph(sub(Role::ROW_DESC, t!(ab_others_desc)))];
    for (what, by, licence) in all.into_iter().take(shown) {
        list = list.push(
            dir_row![
                column![text(Role::CONTROL, what), sub(Role::ROW_DESC, by)]
                    .width(Fill)
                    .align_x(dir::start()),
                pill(licence, true, Some(chip_w)),
            ]
            .align_y(Center)
            .padding([LIST_PAD, 0.0]),
        );
    }
    let more = match state.made_by_all {
        true => t!(ab_show_fewer).to_string(),
        false => fmt(S::ab_show_all, &[&all.len()]),
    };
    let toggle = button(
        label(more),
        Kind::Quiet,
        Some(Message::MadeByAll(!state.made_by_all)),
    );
    card(list.push(toggle).spacing(space::GAP).align_x(dir::start()))
}

/// "Preview clip credits": one fold (closed when the window opens); open, one
/// line per bundled clip with a link to its section in docs/NOTICE.md, as one
/// left-to-right English block (legal attribution) in every language, the
/// non-affiliation line at its end, shown folded or not. None bundled: nothing.
fn clip_notice(state: &State) -> Option<Element<'_, Message>> {
    let bundled = |c: &str| super::super::previews::player::still(c).is_some();
    let lines = clip_credits(bundled);
    if lines.is_empty() {
        return None;
    }
    let open = state.credits_open;
    let mut part = column![fold(
        t!(ab_clip_credits),
        lines.len(),
        open,
        Message::Credits(!open)
    )]
    .spacing(space::GAP);
    let google = bundled(NOT_AFFILIATED_CLIP);
    if open || google {
        // one left-to-right block in every language (the words stay English)
        let _ltr = dir::scope(false);
        let mut block = column![].spacing(space::GAP).width(Fill);
        if open {
            for credit in clip_credits(bundled) {
                let link = button(
                    icon(Icon::External, LINK_ICON),
                    Kind::Quiet,
                    Some(Message::OpenLink(credit_url(credit))),
                );
                block = block.push(
                    dir_row![
                        sub(Role::ROW_DESC, credit.line).width(Fill),
                        hint(link, t!(ab_credit_open), Why::Icon, Position::Top),
                    ]
                    .spacing(BUTTON_GAP)
                    .align_y(Center),
                );
            }
            block = block.push(sub(Role::ROW_DESC, CAVEAT));
        }
        if google {
            block = block.push(sub(Role::ROW_DESC, NOT_AFFILIATED));
        }
        part = part.push(card(block));
    }
    Some(part.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn https(url: &str) -> bool {
        url.starts_with("https://") && crate::shared::platform::sys::open::web_host(url).is_some()
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
        for c in &CLIP_CREDITS {
            assert!(https(&credit_url(c)), "{}", credit_url(c));
        }
    }

    #[test]
    fn the_avatar_decodes_whatever_format_the_release_fetched() {
        let bundled = include_bytes!("../../../../../assets/about/avatar.png");
        assert!(decode_avatar(bundled).is_some(), "the bundled file");
        let mut jpeg = Vec::new();
        ::image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[200u8; 8 * 8 * 3], 8, 8, ::image::ExtendedColorType::Rgb8)
            .unwrap();
        assert!(decode_avatar(&jpeg).is_some(), "a JPEG named avatar.png");
        assert!(decode_avatar(b"not an image at all").is_none());
        assert!(decode_avatar(&bundled[..40]).is_none(), "truncated");
    }

    #[test]
    fn notices_credit_the_models_we_download() {
        let notice = include_str!("../../../../../docs/NOTICE.md");
        for key in ["Helsinki-NLP", "Xenova", "OptimaMT", "PaddleOCR", t!(ab_n3)] {
            assert!(notice.contains(key), "{key} is in docs/NOTICE.md");
        }
        let all: String = made_by()
            .iter()
            .map(|n| format!("{} {}", n.1, n.2))
            .collect();

        for key in [
            "Helsinki-NLP",
            "Xenova",
            "OptimaMT",
            "CC BY 4.0",
            "PaddleOCR",
            "Microsoft",
        ] {
            assert!(all.contains(key), "{key} is credited on the page");
        }
    }

    /// GitHub's anchor of a Markdown heading: lower case, letters, digits,
    /// `-` and `_` kept, spaces to `-`, the rest dropped.
    fn anchor(heading: &str) -> String {
        (heading.to_lowercase().chars())
            .filter_map(|c| match c {
                ' ' => Some('-'),
                c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                _ => None,
            })
            .collect()
    }

    /// Each clip's line links to its own section of docs/NOTICE.md, and that
    /// section keeps the full credit with every source link the page showed
    /// before the one-line credits (planning #177: nothing a licence asks
    /// for is lost).
    #[test]
    fn every_clip_credit_links_to_its_full_section_in_notice() {
        let notice = include_str!("../../../../../docs/NOTICE.md").replace("\r\n", "\n");
        let sections: Vec<(String, &str)> = (notice.split("\n### ").skip(1))
            .map(|s| {
                let (head, body) = s.split_once('\n').unwrap();
                (anchor(head), body.split("\n## ").next().unwrap())
            })
            .collect();
        let section = |a: &str| {
            (sections.iter().find(|(h, _)| h == a))
                .unwrap_or_else(|| panic!("no NOTICE heading for #{a}"))
                .1
        };
        let sources: [(&str, &[&str]); 8] = [
            (
                "succulent-photograph-circle-1",
                &[
                    "https://wordpress.org/photos/photo/4569e13887/",
                    "https://creativecommons.org/publicdomain/zero/1.0/",
                ],
            ),
            (
                "lotus-photograph-circle-2",
                &["https://wordpress.org/photos/photo/4766ac73e9/"],
            ),
            (
                "kingfisher-photograph-circle-3",
                &["https://wordpress.org/photos/photo/11669e53a4/"],
            ),
            (
                "coffee-photograph-text",
                &[
                    "https://unsplash.com/photos/a-sign-on-a-brick-wall-that-says-coffee-jozE5L60Aj4",
                    "https://unsplash.com/license",
                    "No endorsement implied",
                ],
            ),
            (
                "wikipedia-translation-translate-page",
                &[
                    "https://fr.wikipedia.org/wiki/Plante_d%27int%C3%A9rieur",
                    "https://fr.wikipedia.org/w/index.php?title=Plante_d%27int%C3%A9rieur&action=history",
                    "https://creativecommons.org/licenses/by-sa/4.0/",
                    "https://fr.wikipedia.org/wiki/Fichier:Her,_IFA_2018,_Berlin_(P1070094).jpg",
                    "https://commons.wikimedia.org/wiki/File:Blomsterf%C3%B6nstret_av_Carl_Larsson_1894.jpg",
                    "https://github.com/AmerAlreyahi/OpenCircle/blob/main/LICENSES/CC-BY-SA-4.0.txt",
                ],
            ),
            ("game-screens-translate-2-translate-3", &["no real game"]),
            (
                "solar-photovoltaic-panel-prices-ai-chat-ai-browser",
                &[
                    "https://ourworldindata.org/grapher/solar-pv-prices",
                    "https://creativecommons.org/licenses/by/4.0/",
                ],
            ),
            (
                "tourism-snip-long",
                &[
                    "https://ourworldindata.org/tourism",
                    "https://ourworldindata.org/grapher/international-tourist-trips",
                    "https://ourworldindata.org/faqs",
                    "https://creativecommons.org/licenses/by/4.0/",
                ],
            ),
        ];
        assert_eq!(sources.len(), CLIP_CREDITS.len());
        for (credit, (a, wants)) in CLIP_CREDITS.iter().zip(sources) {
            assert_eq!(credit.anchor, a);
            let body = section(a).replace('\n', " ");
            for want in wants {
                assert!(body.contains(want), "#{a}: {want}");
            }
            // the clips' names are in the heading the anchor is made of
            assert!(credit.clips.iter().all(|c| a.contains(c)), "#{a}");
        }
        // the non-affiliation line, verbatim, and the caveat
        let flat = notice.replace('\n', " ");
        assert!(flat.contains(NOT_AFFILIATED), "{NOT_AFFILIATED}");
        assert!(flat.contains("do not clear unrelated search-result thumbnails"));
        assert!(section("not-affiliated-with-google").contains("not affiliated"));
    }

    /// A licence that asks for attribution gets it in the line itself: CC BY
    /// and CC BY-SA name the work, its author, the licence and that it was
    /// changed (the link icon opens the rest); the others name theirs.
    #[test]
    fn each_credit_line_names_what_its_licence_asks_for() {
        for c in &CLIP_CREDITS {
            let l = c.line;
            assert!(!l.contains('\n') && !l.contains("  "), "{l}");
            if l.contains("CC BY") {
                assert!(l.contains('\u{201c}'), "a title: {l}");
                assert!(l.contains("edited"), "the changes: {l}");
            }
        }
        let line = |a: &str| CLIP_CREDITS.iter().find(|c| c.anchor == a).unwrap().line;
        let page = line("wikipedia-translation-translate-page");
        for want in [
            "Plante d",
            "Wikipedia contributors",
            "Matti Blume",
            "CC BY-SA 4.0",
        ] {
            assert!(page.contains(want), "{want}: {page}");
        }
        assert!(line("coffee-photograph-text").contains("Unsplash License"));
        let tourism = line("tourism-snip-long");
        assert!(tourism.contains("Bastian Herre") && tourism.contains("Veronika Samborska"));
        assert!(line("solar-photovoltaic-panel-prices-ai-chat-ai-browser").contains("IRENA"));
    }
}
