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
    let widths: Vec<f32> = options.iter().map(|&(_, l)| pill_width(l)).collect();
    track(options, &widths, Role::CONTROL, chosen, from, glide, on)
}

/// The full-width track (onboarding's steps, `oc-track w-full` with `flex-1`
/// pills, 11 px): `width` across, the pills sharing it equally.
pub fn spread<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
    width: f32,
    options: &[(T, &'a str)],
    chosen: T,
    from: T,
    glide: f32,
    on: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let n = options.len().max(1) as f32;
    let inner = width - 2.0 * (space::TRACK_PAD + 1.0) - space::TRACK_GAP * (n - 1.0);
    let widths = vec![inner / n; options.len()];
    track(options, &widths, Role::STEP, chosen, from, glide, on)
}

fn track<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
    options: &[(T, &'a str)],
    widths: &[f32],
    role: Role,
    chosen: T,
    from: T,
    glide: f32,
    on: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    const GAP: f32 = space::TRACK_GAP;
    const PILL: f32 = 27.0; // 12 px x 1.25 and 6 px above and below
    let at = |value: T| {
        let mut x = 0.0;
        for (&(v, _), &w) in options.iter().zip(widths) {
            if v == value {
                return (x, w);
            }
            x += w + GAP;
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
    .width(widths.iter().map(|w| w + GAP).sum::<f32>() - GAP);
    let pills = options.iter().zip(widths).map(|(&(value, label), &w)| {
        let active = value == chosen;
        // centred by its box, the text exactly as wide as drawn: a wider box
        // lays a right-to-left line out from its right edge (Arabic sat right
        // of the thumb on Windows), and the text's own centring shifts it too
        let drawn = super::text::width(label, role.size, super::text::font(role));
        button(
            container(
                text(label)
                    .size(role.size)
                    .font(super::text::font(role))
                    .line_height(1.25)
                    .width(drawn.ceil()),
            )
            .center_x(Fill),
        )
        .width(w)
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
    /// The width a pill gives its label is at least what the text widget lays
    /// out for it, so a label never wraps ("Wait for me" broke on Windows:
    /// measured kerned, drawn unkerned).
    #[test]
    fn a_label_measures_as_the_text_widget_lays_it_out() {
        use super::super::text::{font, width};
        use super::super::tokens::Role;
        use iced::advanced::text::{self, Paragraph as _};
        type P = <iced::Renderer as text::Renderer>::Paragraph;
        let _ = super::super::text::ui_font_name(); // the fonts loaded
        for label in [
            "Wait for me",
            "Run at once",
            "Snap to box",
            "AVA Wave",
            "Off",
        ] {
            let laid = P::with_text(text::Text {
                content: label,
                bounds: iced::Size::INFINITE,
                size: iced::Pixels(Role::CONTROL.size),
                line_height: text::LineHeight::Relative(1.25),
                font: font(Role::CONTROL),
                align_x: text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::default(),
                wrapping: text::Wrapping::default(),
            })
            .min_width();
            let ours = width(label, Role::CONTROL.size, font(Role::CONTROL));
            assert!(
                ours.ceil() >= laid,
                "{label}: measured {ours}, laid out {laid}"
            );
        }
    }

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
