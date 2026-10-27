//! `.oc-track` with `.oc-pills` and the gliding thumb: one choice of a few.
//! `boolean` (Off / On) is the only on/off control (the design has no switch).
use iced::widget::{Space, button, container, row, stack, text};
use iced::{Border, Element, Fill};

use super::themes::c;
use super::tokens::{Role, radius, space};

/// A pill's width: its label (12 px / 500) and 12 px each side, at least 42 px
/// (the design's Off / On are 45 / 42 px) and 7 px a letter (the design's
/// pills, measured for Latin), never narrower than the label as drawn.
fn pill_width(label: &str) -> f32 {
    let drawn = super::text::width(label, Role::CONTROL.size, super::text::font(Role::CONTROL));
    (24.0 + (7.0 * label.chars().count() as f32).max(drawn.ceil())).max(42.0)
}

/// Radius 10 track (`track`, divider edge, 3 px inside), 7.5 px pills; the
/// thumb (`elevated`) slides from `from` to `chosen` as `glide` goes 0 → 1
/// (the caller's clock: `motion::GLIDE`, standard).
pub fn segmented<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
    options: &[(T, &'a str)],
    chosen: T,
    from: T,
    glide: f32,
    on: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    const GAP: f32 = space::TRACK_GAP;
    const PILL: f32 = 27.0; // 12 px x 1.25 and 6 px above and below
    let at = |value: T| {
        let mut x = 0.0;
        for &(v, label) in options {
            if v == value {
                return (x, pill_width(label));
            }
            x += pill_width(label) + GAP;
        }
        (0.0, 0.0)
    };
    let ((x0, w0), (x1, w1)) = (at(from), at(chosen));
    let glide = glide.clamp(0.0, 1.0);
    let (x, w) = (x0 + (x1 - x0) * glide, w0 + (w1 - w0) * glide);
    let thumb = row![
        Space::new().width(x),
        container(Space::new().width(w).height(PILL)).style(|_| container::Style {
            background: Some(c().elevated.into()),
            border: Border::default()
                .rounded(radius::PILL)
                .width(1)
                .color(c().elevated_border),
            ..container::Style::default()
        }),
    ]
    // the base layer of the stack sets its size: as wide as all the pills
    .width(
        options
            .iter()
            .map(|&(_, l)| pill_width(l) + GAP)
            .sum::<f32>()
            - GAP,
    );
    let pills = options.iter().map(|&(value, label)| {
        let active = value == chosen;
        // centred by its box: a centred Arabic line is drawn off to the right
        button(
            container(
                text(label)
                    .size(Role::CONTROL.size)
                    .font(super::text::font(Role::CONTROL))
                    .line_height(1.25),
            )
            .center_x(Fill),
        )
        .width(pill_width(label))
        .padding([6, 0])
        .height(PILL)
        .on_press(on(value))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: (!active && hovered).then(|| c().hover.into()),
                text_color: if active || hovered {
                    c().text
                } else {
                    c().text_sub
                },
                border: Border::default().rounded(radius::PILL),
                ..button::Style::default()
            }
        })
        .into()
    });
    let pills = row(pills).spacing(GAP).height(PILL);
    container(stack![thumb, pills])
        .padding(space::TRACK_PAD)
        .style(|_| container::Style {
            background: Some(c().track.into()),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// Off / On (`labels` in that order): `segmented` over a bool.
pub fn boolean<'a, Message: Clone + 'a>(
    labels: [&'a str; 2],
    on: bool,
    from: bool,
    glide: f32,
    pick: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    segmented(
        &[(false, labels[0]), (true, labels[1])],
        on,
        from,
        glide,
        pick,
    )
}

#[cfg(test)]
mod tests {
    /// Every pill holds its label as drawn, Arabic too (7 px a letter was
    /// too narrow for Arabic: the labels ran into each other).
    #[test]
    fn every_pill_holds_its_label() {
        use super::super::text::{font, width};
        use super::super::tokens::Role;
        for label in [
            "Off",
            "On",
            "Keep",
            "Remove",
            "إيقاف",
            "تشغيل",
            "النظام",
            "داكن",
        ] {
            let drawn = width(label, Role::CONTROL.size, font(Role::CONTROL));
            assert!(drawn > 0.0, "{label}");
            assert!(super::pill_width(label) >= drawn + 24.0, "{label}: {drawn}");
        }
    }
}
