//! A progress bar.
use iced::widget::{Space, container, row, stack};
use iced::{Border, Element, Fill, Length};

use super::button::faded;
use super::themes::c;
use super::tokens::radius;

/// 6 px, `share` (0..1) filled with `accent` on a `hover` track, the fill
/// glowing (`0 0 12px` accent: drawn as two soft rounded rings, no blur on the
/// CPU renderer). 18 px tall: the bar in its middle, the glow round it.
pub fn bar<'a, Message: 'a>(share: f32) -> Element<'a, Message> {
    const GLOW: f32 = 6.0;
    let share = share.clamp(0.0, 1.0);
    let portion = |s: f32| Length::FillPortion((s * 1000.0).round() as u16);
    // a part of no share is left out: iced lays a `FillPortion(0)` out as if
    // it had no portion, so an empty bar showed full and a full one empty
    let rest = || (share < 1.0).then(|| Space::new().width(portion(1.0 - share)));
    let glow = |grow: f32, alpha: f32| {
        let ring =
            container(Space::new().width(Fill).height(Fill)).style(move |_| container::Style {
                background: Some(faded(c().accent, alpha).into()),
                border: Border::default().rounded(radius::FULL),
                ..container::Style::default()
            });
        let pad = GLOW - grow;
        row![
            container(ring)
                .padding(pad)
                .width(portion(share))
                .height(Fill)
        ]
        .push(rest())
        .height(Fill)
    };
    let fill = container(Space::new().width(Fill).height(6))
        .width(portion(share))
        .style(|_| container::Style {
            background: Some(c().accent.into()),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        });
    let track = container(
        row![]
            .push((share > 0.0).then_some(fill))
            .push(rest().map(|r| r.height(6))),
    )
    .width(Fill)
    .height(6)
    .style(|_| container::Style {
        background: Some(c().hover.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    });
    let bar = container(track).padding([GLOW, 0.0]).width(Fill);
    let glows = (share > 0.0).then(|| stack![glow(GLOW, 0.10), glow(GLOW / 2.0, 0.18)]);
    let mut layers = stack![].width(Fill).height(6.0 + 2.0 * GLOW);
    if let Some(glows) = glows {
        layers = layers.push(glows);
    }
    layers.push(bar).into()
}

#[cfg(test)]
mod tests {
    use crate::ui::kit::testing::shot;
    use crate::ui::kit::themes::c;

    /// An empty bar shows no fill and a full one is filled to its end (iced
    /// lays out a `FillPortion(0)` as if it had none: the two were swapped).
    #[test]
    fn an_empty_bar_is_empty_and_a_full_one_full() {
        let accent = c().accent.into_rgba8();
        // the bar's middle line, a quarter and three quarters across
        let at = |share: f32, x: usize| {
            let px = shot::render(super::bar::<()>(share), 200, 18);
            let i = (9 * 200 + x) * 4;
            px[i..i + 4] == accent
        };
        assert!(!at(0.0, 50) && !at(0.0, 150), "empty");
        assert!(at(1.0, 50) && at(1.0, 150), "full");
        assert!(at(0.5, 50) && !at(0.5, 150), "half");
    }
}
