//! The colour picker everyone knows (Paint, Figma, Snagit, the macOS and GNOME pickers;
//! owner, 2026-11-25): a saturation / brightness square, a hue slider under it, a preview
//! (the colour before | the colour now) with the eyedropper and a small hex read-out you may
//! click to edit, the user's recent colours and a small row of preset swatches. Drag on the
//! square and the slider; the arrows move their handles (Shift = 10x). Right to left: the hue
//! slider runs mirrored (the first colour is at the right) and the rows follow `kit::dir`.
//! ONE component for Snip's colour button and the Studio's "+".
use std::rc::Rc;

use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke, gradient};
use iced::widget::{column, container};
use iced::{Border, Color, Element, Event, Fill, Point, Rectangle, Renderer, Theme};

use super::field::input;
use super::focus;
use super::text::sub;
use super::themes::c;
use super::tokens::{Role, radius, space};

/// The picker's width (its content and the padding).
pub const WIDTH: f32 = 264.0;
const PAD: f32 = 10.0;
const SQUARE_H: f32 = 150.0;
const HUE_H: f32 = 14.0;
const PREVIEW_H: f32 = 28.0;
const HEX_W: f32 = 86.0;
/// A preset or recent swatch: its dot, and the room around it.
const DOT: f32 = 15.0;
/// A step of an arrow key on the square (share of its side) and on the hue (degrees).
const NUDGE: f32 = 0.01;
const HUE_NUDGE: f32 = 2.0;

/// A colour as hue (0..360), saturation and brightness (0..1): what the square and the
/// slider drag. Kept apart from RGB so the hue survives a grey (s = 0) or black (v = 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    /// As `[h, s, v]` (what the editor holds: it names no UI type).
    pub fn array(self) -> [f32; 3] {
        [self.h, self.s, self.v]
    }

    /// From `[h, s, v]`.
    pub fn from_array([h, s, v]: [f32; 3]) -> Hsv {
        Hsv { h, s, v }
    }

    /// The hue, saturation and brightness of an RGB colour (hue 0 for a grey).
    pub fn of([r, g, b]: [u8; 3]) -> Hsv {
        let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
        let max = r.max(g).max(b);
        let d = max - r.min(g).min(b);
        let h = match () {
            _ if d == 0.0 => 0.0,
            _ if max == r => 60.0 * ((g - b) / d).rem_euclid(6.0),
            _ if max == g => 60.0 * ((b - r) / d + 2.0),
            _ => 60.0 * ((r - g) / d + 4.0),
        };
        Hsv {
            h,
            s: if max == 0.0 { 0.0 } else { d / max },
            v: max,
        }
    }

    /// The RGB colour it is.
    pub fn rgb(self) -> [u8; 3] {
        let (h, s, v) = (self.h.rem_euclid(360.0) / 60.0, self.s, self.v);
        let (i, f) = (h.floor(), h - h.floor());
        let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
        let (r, g, b) = match i as u8 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        [r, g, b].map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8)
    }

    pub fn colour(self) -> Color {
        let [r, g, b] = self.rgb();
        Color::from_rgb8(r, g, b)
    }

    /// The same colour with its hue `to` (the slider).
    pub fn with_hue(self, h: f32) -> Hsv {
        Hsv {
            h: h.clamp(0.0, 360.0),
            ..self
        }
    }

    /// `self` moved on the square by `(ds, dv)`.
    pub fn nudged(self, ds: f32, dv: f32) -> Hsv {
        Hsv {
            s: (self.s + ds).clamp(0.0, 1.0),
            v: (self.v + dv).clamp(0.0, 1.0),
            ..self
        }
    }
    /// The fully saturated, full-brightness colour of this hue.
    pub fn pure(self) -> Color {
        Hsv {
            s: 1.0,
            v: 1.0,
            ..self
        }
        .colour()
    }
}

/// What the picker shows and says. `hsv`: the colour now; `old`: the one before the change.
pub struct Picker<'a, Message> {
    pub hsv: Hsv,
    pub old: Color,
    /// The colour as `#RRGGBB` (the optional read-out).
    pub hex: &'a str,
    /// The few preset swatches.
    pub presets: &'a [Color],
    /// The user's recent colours, newest first.
    pub recent: &'a [[u8; 3]],
    /// The eyedropper button (the caller's, so it can pick from its own picture).
    pub eyedropper: Option<Element<'a, Message>>,
    /// The caption over the recent colours.
    pub recent_label: &'a str,
    /// A drag moved the colour (many times).
    pub on_hsv: Rc<dyn Fn(Hsv) -> Message + 'a>,
    /// A drag ended: remember the colour.
    pub done: Message,
    /// A key moved the colour (a finished change: remembered at once).
    pub on_set: Rc<dyn Fn(Hsv) -> Message + 'a>,
    /// A preset or a recent colour was chosen.
    pub on_rgb: Rc<dyn Fn([u8; 3]) -> Message + 'a>,
    /// The hex read-out was edited.
    pub on_hex: Rc<dyn Fn(String) -> Message + 'a>,
}

/// The picker, on the dropdown material (as the popovers around it).
pub fn view<'a, Message: Clone + 'a>(p: Picker<'a, Message>) -> Element<'a, Message> {
    let hsv = p.hsv;
    let square = {
        let (on, done, set) = (p.on_hsv.clone(), p.done.clone(), p.on_set.clone());
        let pad = canvas::Canvas::new(Square { hsv, on, done })
            .width(Fill)
            .height(SQUARE_H);
        focus::stop(pad, radius::MENU_ITEM, None)
            .arrows(move |dx, dy| Some(set(hsv.nudged(dx as f32 * NUDGE, dy as f32 * NUDGE))))
    };
    let slider = {
        let rtl = super::dir::rtl();
        let (on, set) = (p.on_hsv.clone(), p.on_set.clone());
        let strip = canvas::Canvas::new(Strip { hsv, rtl, on })
            .width(Fill)
            .height(HUE_H);
        focus::stop(strip, radius::FULL, None).steps(move |step| {
            let by = match step {
                focus::Step::On => HUE_NUDGE,
                focus::Step::Back => -HUE_NUDGE,
            };
            Some(set(hsv.with_hue(hsv.h + by)))
        })
    };
    // (the first half is the reading start: right to left it sits at the right)
    let compare = super::dir::row([
        half(p.old, true),
        half(
            Color::from_rgb8(hsv.rgb()[0], hsv.rgb()[1], hsv.rgb()[2]),
            false,
        ),
    ])
    .width(Fill);
    let hex = input("#RRGGBB", p.hex, true, false).on_input({
        let on = p.on_hex.clone();
        move |s| on(s)
    });
    let mut line: Vec<Element<'a, Message>> = Vec::new();
    line.extend(p.eyedropper);
    line.push(compare.into());
    line.push(container(hex).width(HEX_W).into());
    let preview = super::dir::row(line)
        .spacing(space::GAP)
        .align_y(iced::Center);
    let swatches = |colours: Vec<Color>| -> Element<'a, Message> {
        let on = p.on_rgb.clone();
        super::dir::row(colours.into_iter().map(|col| {
            let rgb = [col.r, col.g, col.b].map(|x| (x * 255.0).round() as u8);
            let on = on.clone();
            chip(col, rgb == hsv.rgb(), on(rgb))
        }))
        .spacing(1)
        .into()
    };
    let mut body = column![square, slider, preview].spacing(space::GAP);
    if !p.recent.is_empty() {
        let recent = p.recent.iter().map(|&[r, g, b]| Color::from_rgb8(r, g, b));
        body = body.push(
            column![
                sub(Role::META, p.recent_label.to_string()),
                swatches(recent.collect())
            ]
            .spacing(2),
        );
    }
    body = body.push(swatches(p.presets.to_vec()));
    container(body)
        .padding(PAD)
        .width(WIDTH)
        .style(|_| container::Style {
            background: Some(c().dropdown.into()),
            border: Border::default()
                .rounded(radius::DROPDOWN)
                .width(1)
                .color(c().rim),
            ..container::Style::default()
        })
        .into()
}

/// One half of the before | now preview: rounded on its outer side only.
fn half<'a, Message: 'a>(colour: Color, first: bool) -> Element<'a, Message> {
    // (the first half is the start side: `dir::row` mirrors the pair in Arabic)
    let r = radius::MENU_ITEM;
    let corners = match first {
        true => [r, 0.0, 0.0, r],
        false => [0.0, r, r, 0.0],
    };
    let rtl = super::dir::rtl();
    let [tl, tr, br, bl] = corners;
    let border = match rtl {
        true => iced::border::Radius {
            top_left: tr,
            top_right: tl,
            bottom_right: bl,
            bottom_left: br,
        },
        false => iced::border::Radius {
            top_left: tl,
            top_right: tr,
            bottom_right: br,
            bottom_left: bl,
        },
    };
    container(iced::widget::Space::new())
        .width(Fill)
        .height(PREVIEW_H)
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border {
                radius: border,
                width: 1.0,
                color: c().rim,
            },
            ..container::Style::default()
        })
        .into()
}

/// A swatch: a small dot, ringed in `text` when it is the colour now.
fn chip<'a, Message: Clone + 'a>(colour: Color, chosen: bool, on: Message) -> Element<'a, Message> {
    focus::press(
        iced::widget::button(
            container(iced::widget::Space::new().width(DOT).height(DOT)).style(move |_| {
                container::Style {
                    background: Some(colour.into()),
                    border: Border::default()
                        .rounded(radius::FULL)
                        .width(1)
                        .color(c().rim),
                    ..container::Style::default()
                }
            }),
        )
        .padding(2)
        .style(move |_, status| iced::widget::button::Style {
            background: matches!(status, iced::widget::button::Status::Hovered)
                .then(|| c().hover.into()),
            border: Border::default()
                .rounded(radius::FULL)
                .width(if chosen { 2 } else { 0 })
                .color(c().text),
            ..iced::widget::button::Style::default()
        }),
        radius::FULL,
        Some(on),
    )
    .into()
}

/// A drag along a canvas: its pointer state, shared by the square and the slider.
#[derive(Default)]
struct Drag(bool);

impl Drag {
    /// The point the drag is at (inside the canvas, clamped), and whether it just ended.
    fn follow(
        &mut self,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<(Option<Point>, bool)> {
        let inside = |p: Point| Point::new(p.x - bounds.x, p.y - bounds.y);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                self.0 = true;
                Some((Some(at), false))
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if self.0 => {
                Some((Some(inside(*position)), false))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if self.0 => {
                self.0 = false;
                Some((None, true))
            }
            _ => None,
        }
    }
}

/// The saturation / brightness square of one hue.
struct Square<'a, Message> {
    hsv: Hsv,
    on: Rc<dyn Fn(Hsv) -> Message + 'a>,
    done: Message,
}

impl<Message: Clone> canvas::Program<Message> for Square<'_, Message> {
    type State = Drag;

    fn update(
        &self,
        drag: &mut Drag,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        let (at, ended) = drag.follow(event, bounds, cursor)?;
        let message = match (at, ended) {
            (Some(p), _) => {
                let s = (p.x / bounds.width).clamp(0.0, 1.0);
                let v = 1.0 - (p.y / bounds.height).clamp(0.0, 1.0);
                (self.on)(Hsv { s, v, ..self.hsv })
            }
            (None, _) => self.done.clone(),
        };
        Some(Action::publish(message).and_capture())
    }

    fn draw(
        &self,
        _: &Drag,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let shape = Path::rounded_rectangle(Point::ORIGIN, bounds.size(), radius::MENU_ITEM.into());
        frame.fill(&shape, self.hsv.with_hue(self.hsv.h).pure());
        let white = gradient::Linear::new(Point::ORIGIN, Point::new(w, 0.0))
            .add_stop(0.0, Color::WHITE)
            .add_stop(1.0, Color::WHITE.scale_alpha(0.0));
        frame.fill(&shape, white);
        let black = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, h))
            .add_stop(0.0, Color::BLACK.scale_alpha(0.0))
            .add_stop(1.0, Color::BLACK);
        frame.fill(&shape, black);
        frame.stroke(
            &shape,
            Stroke::default().with_width(1.0).with_color(c().rim),
        );
        handle(
            &mut frame,
            Point::new(self.hsv.s * w, (1.0 - self.hsv.v) * h),
            self.hsv.colour(),
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &Drag, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        match cursor.is_over(bounds) {
            true => mouse::Interaction::Crosshair,
            false => mouse::Interaction::None,
        }
    }
}

/// The hue slider: the rainbow, its handle at the colour's hue.
struct Strip<'a, Message> {
    hsv: Hsv,
    rtl: bool,
    on: Rc<dyn Fn(Hsv) -> Message + 'a>,
}

impl<Message: Clone> canvas::Program<Message> for Strip<'_, Message> {
    type State = Drag;

    fn update(
        &self,
        drag: &mut Drag,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        let (at, _) = drag.follow(event, bounds, cursor)?;
        // (the end of a drag needs no message here: the square's `done` is the same one)
        let p = at?;
        let share = (p.x / bounds.width).clamp(0.0, 1.0);
        let share = if self.rtl { 1.0 - share } else { share };
        Some(Action::publish((self.on)(self.hsv.with_hue(share * 360.0))).and_capture())
    }

    fn draw(
        &self,
        _: &Drag,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let (from, to) = match self.rtl {
            true => (w, 0.0),
            false => (0.0, w),
        };
        let mut rainbow = gradient::Linear::new(Point::new(from, 0.0), Point::new(to, 0.0));
        for i in 0..=6 {
            let hue = Hsv {
                h: i as f32 * 60.0,
                s: 1.0,
                v: 1.0,
            };
            rainbow = rainbow.add_stop(i as f32 / 6.0, hue.colour());
        }
        let shape = Path::rounded_rectangle(Point::ORIGIN, bounds.size(), (h / 2.0).into());
        frame.fill(&shape, rainbow);
        frame.stroke(
            &shape,
            Stroke::default().with_width(1.0).with_color(c().rim),
        );
        let share = self.hsv.h.clamp(0.0, 360.0) / 360.0;
        let x = if self.rtl { 1.0 - share } else { share } * w;
        handle(&mut frame, Point::new(x, h / 2.0), self.hsv.pure());
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &Drag, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        match cursor.is_over(bounds) {
            true => mouse::Interaction::Grab,
            false => mouse::Interaction::None,
        }
    }
}

/// A round handle: the colour under it, a white ring and a soft dark one outside.
fn handle(frame: &mut Frame, at: Point, colour: Color) {
    let outer = Path::circle(at, 7.5);
    frame.stroke(
        &outer,
        Stroke::default()
            .with_width(1.0)
            .with_color(Color::BLACK.scale_alpha(0.4)),
    );
    let ring = Path::circle(at, 6.5);
    frame.fill(&ring, colour);
    frame.stroke(
        &ring,
        Stroke::default().with_width(2.0).with_color(Color::WHITE),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::shot;

    #[derive(Debug, Clone, PartialEq)]
    enum M {
        Hsv(Hsv),
        Done,
        Set(Hsv),
        Rgb([u8; 3]),
        Hex(String),
    }

    const SIZE: iced::Size = iced::Size::new(WIDTH, 340.0);
    static PRESETS: [Color; 2] = [
        Color::from_rgb8(255, 59, 48),
        Color::from_rgb8(10, 132, 255),
    ];

    fn picker(hsv: Hsv) -> Element<'static, M> {
        view(Picker {
            hsv,
            old: Color::BLACK,
            hex: "#000000",
            presets: &PRESETS,
            recent: &[[1, 2, 3]],
            eyedropper: None,
            recent_label: "Recent",
            on_hsv: Rc::new(M::Hsv),
            done: M::Done,
            on_set: Rc::new(M::Set),
            on_rgb: Rc::new(M::Rgb),
            on_hex: Rc::new(M::Hex),
        })
    }

    fn start() -> Hsv {
        Hsv {
            h: 120.0,
            s: 0.5,
            v: 0.5,
        }
    }

    #[test]
    fn hsv_and_rgb_round_trip_and_keep_the_hue_of_a_grey() {
        for rgb in [
            [255, 0, 0],
            [10, 132, 255],
            [255, 214, 10],
            [30, 30, 30],
            [255, 255, 255],
            [0, 0, 0],
        ] {
            assert_eq!(Hsv::of(rgb).rgb(), rgb, "{rgb:?}");
        }
        assert_eq!(Hsv::of([0, 0, 255]).h, 240.0);
        // a grey chosen on the square keeps the slider's hue
        let grey = Hsv {
            h: 200.0,
            s: 0.0,
            v: 0.5,
        };
        assert_eq!(grey.rgb()[0], grey.rgb()[2]);
        assert_eq!(grey.nudged(0.0, 0.1).h, 200.0);
    }

    #[test]
    fn nudging_stays_on_the_square() {
        let h = Hsv {
            h: 10.0,
            s: 0.995,
            v: 0.002,
        };
        let n = h.nudged(0.05, -0.05);
        assert_eq!((n.s, n.v), (1.0, 0.0));
    }

    /// W112: a drag on the square sets saturation (across) and brightness (up) live, stays on
    /// the square when the pointer leaves it, keeps the hue, and ends with `done`.
    #[test]
    fn dragging_the_square_sets_saturation_and_brightness() {
        // the square is the picker's first control: PAD in, SQUARE_H tall
        let from = Point::new(PAD + 0.5 * (WIDTH - 2.0 * PAD), PAD + 0.5 * SQUARE_H);
        let to = Point::new(WIDTH + 300.0, -40.0); // far outside, top right
        let (got, _) = shot::drag(
            || picker(start()),
            SIZE,
            (from, to, 1),
            &[],
            &mut shot::Clip::default(),
        );
        let moved: Vec<Hsv> = got
            .iter()
            .filter_map(|m| match m {
                M::Hsv(h) => Some(*h),
                _ => None,
            })
            .collect();
        assert!(moved.len() >= 2, "{got:?}");
        assert!(
            (moved[0].s - 0.5).abs() < 0.02 && (moved[0].v - 0.5).abs() < 0.02,
            "the press picks under the pointer: {:?}",
            moved[0]
        );
        let last = moved.last().unwrap();
        assert_eq!(
            (last.s, last.v, last.h),
            (1.0, 1.0, 120.0),
            "clamped; hue kept"
        );
        assert_eq!(got.last(), Some(&M::Done), "{got:?}");
    }

    /// W112: the hue slider picks the hue across it, mirrored right to left (the first
    /// colour is at the right), keeping saturation and brightness.
    #[test]
    fn the_hue_slider_runs_mirrored_in_arabic() {
        let y = PAD + SQUARE_H + space::GAP + HUE_H / 2.0;
        let near_start = Point::new(PAD + 2.0, y);
        let hue_at = |p: Point| {
            let (got, _) = shot::drag(
                || picker(start()),
                SIZE,
                (p, p, 1),
                &[],
                &mut shot::Clip::default(),
            );
            got.into_iter().find_map(|m| match m {
                M::Hsv(h) => Some(h),
                _ => None,
            })
        };
        let ltr = hue_at(near_start).expect("a drag on the slider");
        assert!(ltr.h < 10.0, "left end = the first colour: {ltr:?}");
        assert_eq!((ltr.s, ltr.v), (0.5, 0.5));
        let _ar = crate::shared::kit::testing::arabic();
        let rtl = hue_at(near_start).expect("a drag on the slider");
        assert!(
            rtl.h > 350.0,
            "left end = the last colour in Arabic: {rtl:?}"
        );
    }

    /// W112: a recent or preset swatch sends its colour; the hex read-out is only a small field.
    #[test]
    fn swatches_send_their_colour() {
        let texts = shot::texts(picker(start()));
        assert!(texts.iter().any(|t| t == "Recent"), "{texts:?}");
        let mut seen = Vec::new();
        for x in (0..WIDTH as usize).step_by(3) {
            for y in (200..SIZE.height as usize).step_by(4) {
                seen.extend(shot::click(
                    picker(start()),
                    SIZE,
                    Point::new(x as f32, y as f32),
                ));
            }
        }
        assert!(seen.contains(&M::Rgb([1, 2, 3])), "the recent colour");
        assert!(seen.contains(&M::Rgb([255, 59, 48])), "a preset");
    }

    #[test]
    fn arrows_are_read_as_seen_with_shift_ten_times() {
        use iced::keyboard::{self, Key, Modifiers, key::Named};
        let press = |named, modifiers| {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let n = Modifiers::empty();
        assert_eq!(focus::arrow_dir(&press(Named::ArrowRight, n)), Some((1, 0)));
        assert_eq!(focus::arrow_dir(&press(Named::ArrowDown, n)), Some((0, -1)));
        assert_eq!(
            focus::arrow_dir(&press(Named::ArrowUp, Modifiers::SHIFT)),
            Some((0, 10))
        );
        assert_eq!(
            focus::arrow_dir(&press(Named::ArrowLeft, Modifiers::CTRL)),
            None
        );
    }
}
