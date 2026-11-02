//! `.oc-track` with `.oc-pills` and the gliding thumb: one choice of a few.
//! `boolean` (Off / On) is the only on/off control (the design has no switch).
use iced::widget::{Space, button, container, row, stack, text};
use iced::{Border, Element, Fill, Size};

use super::themes::c;
use super::tokens::{Role, radius, size, space};

/// A pill's width: its label (12 px / 500) and 12 px each side, at least 42 px
/// (the design's Off / On are 45 / 42 px) and 7 px a letter (the design's
/// pills, measured for Latin), never narrower than the label as drawn.
fn pill_width(label: &str) -> f32 {
    let drawn = super::text::width(label, Role::CONTROL.size, super::text::font(Role::CONTROL));
    (24.0 + (7.0 * label.chars().count() as f32).max(drawn.ceil())).max(42.0)
}

/// Radius 10 track (`track`, divider edge, 3 px inside), 7.5 px pills; the
/// thumb (`elevated`) slides from `from` to `chosen` as `glide` goes 0 → 1
/// (the caller's clock: `motion::GLIDE`, standard). As wide as a settings
/// row's control column (`size::CONTROL_COLUMN`): the room left over is
/// shared out equally between the pills.
pub fn segmented<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
    options: &[(T, &'a str)],
    chosen: T,
    from: T,
    glide: f32,
    on: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let mut widths: Vec<f32> = options.iter().map(|&(_, l)| pill_width(l)).collect();
    let n = widths.len().max(1) as f32;
    let room = size::CONTROL_COLUMN
        - 2.0 * space::TRACK_PAD
        - space::TRACK_GAP * (n - 1.0)
        - widths.iter().sum::<f32>();
    for w in &mut widths {
        *w += (room / n).max(0.0);
    }
    track(options, &widths, Role::CONTROL, chosen, from, glide, on)
}

/// `segmented` only as wide as its labels (a toolbar, the installer).
pub fn inline<'a, Message: Clone + 'a, T: Copy + PartialEq + 'a>(
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

/// Three size presets in a small track (Snip's toolbar panels: S / M / L):
/// each a dot (4, 6, 8 px) or `letter` (10, 12.5, 15 px); `chosen` raised on
/// `elevated` (none: the size is between presets, or Auto is on); `dim`: the
/// marks faded (Auto decides; a press still picks one).
pub fn presets<'a, Message: Clone + 'a>(
    chosen: Option<usize>,
    letter: Option<&'a str>,
    dim: bool,
    on: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message> {
    const SIDE: [f32; 3] = [4.0, 6.0, 8.0];
    const PX: [f32; 3] = [10.0, 12.5, 15.0];
    const CELL: Size = Size::new(30.0, 24.0);
    let cells = (0..3).map(|i| {
        let active = chosen == Some(i);
        let ink = move |hot: bool| {
            let ink = if active || hot {
                c().text
            } else {
                c().text_sub
            };
            if dim {
                super::button::faded(ink, 0.45)
            } else {
                ink
            }
        };
        let mark: Element<'a, Message> = match letter {
            Some(l) => text(l)
                .size(PX[i])
                .font(super::text::font(Role::CONTROL))
                .into(),
            None => container(Space::new())
                .width(SIDE[i])
                .height(SIDE[i])
                .style(move |_| container::Style {
                    background: Some(ink(false).into()),
                    border: Border::default().rounded(radius::FULL),
                    ..container::Style::default()
                })
                .into(),
        };
        button(container(mark).center(Fill))
            .width(CELL.width)
            .height(CELL.height)
            .padding(0)
            .on_press(on(i))
            .style(move |_, status| {
                let hot = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: match () {
                        _ if active => Some(c().elevated.into()),
                        _ if hot => Some(c().hover.into()),
                        _ => None,
                    },
                    text_color: ink(hot),
                    border: Border::default()
                        .rounded(radius::PILL)
                        .width(if active { 1 } else { 0 })
                        .color(c().elevated_border),
                    ..button::Style::default()
                }
            })
            .into()
    });
    container(row(cells).spacing(space::TRACK_GAP))
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

/// `boolean` only as wide as its labels (the installer).
pub fn inline_boolean<'a, Message: Clone + 'a>(
    labels: [&'a str; 2],
    on: bool,
    from: bool,
    glide: f32,
    pick: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    inline(
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
