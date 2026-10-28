//! The toast pill (`.oc-toast`, design 11.2): an elevated pill, a dot and
//! one 12 px line, padding 14x8. Exactly `size(label)` big: its window is the
//! pill (cut to its corners, so nothing shows around it).
use iced::widget::{Space, container, row, stack};
use iced::{Border, Center, Element, Fill, Size};

use super::text::{font, width};
use super::themes::c;
use super::tokens::{Role, fixed, size};

/// What the dot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Done (`SUCCESS`): copied, saved.
    Ok,
    /// It failed (`DANGER`; the design's dashed edge, which the renderer can't draw).
    Error,
    /// Just so you know (the accent): the first-run notice.
    Neutral,
}

/// `ok` = done, else it failed.
impl From<bool> for Tone {
    fn from(ok: bool) -> Tone {
        if ok { Tone::Ok } else { Tone::Error }
    }
}

const PAD: [f32; 2] = [8.0, 14.0];
const GAP: f32 = 8.0;
const LINE: f32 = 16.0;

/// The pill's size for `label` (logical px, whole): its window's size.
pub fn size(label: &str) -> Size {
    let text = width(label, Role::NOTE.size, font(Role::NOTE));
    Size::new(
        (2.0 * PAD[1] + size::TOAST_DOT + GAP + text + 2.0).ceil(),
        2.0 * PAD[0] + LINE + 2.0,
    )
}

/// The pill with `label`, corners of `radius` (the window's cut).
pub fn toast<'a, Message: 'a>(label: &'a str, tone: Tone, radius: f32) -> Element<'a, Message> {
    let dot = match tone {
        Tone::Ok => fixed::SUCCESS,
        Tone::Error => fixed::DANGER,
        Tone::Neutral => c().accent,
    };
    let dot =
        container(Space::new().width(size::TOAST_DOT).height(size::TOAST_DOT)).style(move |_| {
            container::Style {
                background: Some(dot.into()),
                border: Border::default().rounded(size::TOAST_DOT / 2.0),
                ..container::Style::default()
            }
        });
    let line = super::text::text(Role::NOTE, label)
        .line_height(iced::Pixels(LINE))
        .wrapping(iced::widget::text::Wrapping::None);
    let pill = container(row![dot, line].spacing(GAP).align_y(Center))
        .padding(PAD)
        .center_y(Fill)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(c().elevated.into()),
            border: Border::default()
                .rounded(radius)
                .width(1)
                .color(c().elevated_border),
            ..container::Style::default()
        });
    // `inset 0 1px 0 --ui-elevated-hi`: a 1 px line under the top edge, between the corners
    let hi = container(Space::new().width(Fill).height(1))
        .style(|_| container::Style::default().background(c().elevated_hi));
    let at = size(label);
    stack![pill, container(hi).padding([1.0, radius])]
        .width(at.width)
        .height(at.height)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window fits the line: longer text, a wider pill; Arabic is measured
    /// as shaped (not by its character count).
    #[test]
    fn the_pill_fits_its_line() {
        let short = size("Copied");
        let long = size("Saved to C:\\Users\\me\\Pictures\\OpenCircle");
        assert!(long.width > short.width + 100.0, "{short:?} {long:?}");
        assert_eq!((short.height, long.height), (34.0, 34.0));
        let ar = size("تم النسخ");
        assert!(ar.width > 50.0 && ar.width < short.width + 40.0, "{ar:?}");
        assert_eq!(Tone::from(true), Tone::Ok);
        assert_eq!(Tone::from(false), Tone::Error);
    }
}
