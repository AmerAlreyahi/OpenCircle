//! A chip: a short label on a tint. `chip` (mono on `accent_soft`), `tag`
//! (Labs' "Experimental"), `badge` (the sidebar's "Exp") and `status` (a
//! header pill with a dot: Settings' auto-save), `pill` (a round label).
use iced::font::Weight;
use iced::widget::{Space, container, row, text};
use iced::{Border, Center, Color, Element};

use super::button::faded;
use super::text::{mono, weight};
use super::themes::c;
use super::tokens::{fixed, radius, size, space};

/// The design's `rounded` (4 px): a tag's and a badge's corners.
const SMALL: f32 = 4.0;

/// Mono on `accent_soft` (the setup's version): 10.5 px / 500, 2x8 px, 20 px tall.
pub fn chip<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(
        text(label)
            .size(10.5)
            .font(mono(Weight::Medium))
            .color(c().accent),
    )
    .padding([2, 8])
    .height(20)
    .center_y(20)
    .style(|_| container::Style {
        background: Some(c().accent_soft.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    })
    .into()
}

/// 10 px `text_sub` on `hover` with a divider edge, 2x6 px (a Labs row's
/// "Experimental").
pub fn tag<'a, Message: 'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(10).font(weight(Weight::Normal)))
        .padding([2, 6])
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            text_color: Some(c().text_sub),
            border: Border::default().rounded(SMALL).width(1).color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// 9.5 px `text_muted` on `hover`, 2x6 px, no edge (a sidebar item's "Exp").
pub fn badge<'a, Message: 'a>(label: &'a str) -> Element<'a, Message> {
    container(text(label).size(9.5).font(weight(Weight::Normal)))
        .padding([2, 6])
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            text_color: Some(c().text_muted),
            border: Border::default().rounded(SMALL),
            ..container::Style::default()
        })
        .into()
}

/// A round pill: 10.5 px `text_sub` (`mono`: 11 px mono) on `hover` with a
/// divider edge, 2x8 px; `width` = fixed, the label centred (the About page's
/// "MIT", its licence pills).
pub fn pill<'a, Message: 'a>(
    label: &'a str,
    mono: bool,
    width: Option<f32>,
) -> Element<'a, Message> {
    let line = match mono {
        true => text(label).size(11).font(super::text::mono(Weight::Normal)),
        false => text(label).size(10.5).font(weight(Weight::Normal)),
    };
    let mut pill = container(line).padding([2, 8]);
    if let Some(w) = width {
        pill = pill.center_x(w);
    }
    pill.style(|_| container::Style {
        background: Some(c().hover.into()),
        text_color: Some(c().text_sub),
        border: Border::default()
            .rounded(radius::FULL)
            .width(1)
            .color(c().divider),
        ..container::Style::default()
    })
    .into()
}

/// What a status pill's dot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Done, all is well (`SUCCESS`).
    Ok,
    /// On its way (`accent`).
    Busy,
    /// Failed, or held back (`DANGER`).
    Error,
}

/// A header pill (`#autoSaveStatusPill`): a 6 px dot of `tone` and an 11 px
/// line at 85 %, on `hover` with a divider edge, round, 2x10 px.
pub fn status<'a, Message: 'a>(label: impl Into<String>, tone: Tone) -> Element<'a, Message> {
    let dot: Color = match tone {
        Tone::Ok => fixed::SUCCESS,
        Tone::Busy => c().accent,
        Tone::Error => fixed::DANGER,
    };
    let dot = container(Space::new().width(size::DOT).height(size::DOT)).style(move |_| {
        container::Style {
            background: Some(dot.into()),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        }
    });
    let line = text(label.into())
        .size(11)
        .font(weight(Weight::Normal))
        .color(faded(c().text, 0.85));
    container(row![dot, line].spacing(space::GAP).align_y(Center))
        .padding([2, 10])
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}
