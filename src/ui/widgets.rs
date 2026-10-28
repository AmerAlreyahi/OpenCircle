//! Shared floating components. Every floating widget is built here, styled only
//! from `theme.rs`; screens never style a container or button themselves.
//!
//! Glass later: blur the frozen frame once per overlay and paint the blurred crop
//! behind each surface (no nested/live blur).
use iced::widget::text::IntoFragment;
use iced::widget::{
    Button, Container, Text, TextInput, Tooltip, button, container, progress_bar, row, scrollable,
    space, text, text_editor, text_input, tooltip,
};
use iced::{Background, Border, Color, Element, gradient};

use super::theme;
use crate::config::BarStyle;

/// The fonts, the icon font and the UI font live in the kit.
pub use super::kit::icon::FONT as ICON_FONT;
#[cfg(test)]
pub use super::kit::icon::FONT_BYTES as ICON_FONT_BYTES;
pub use super::kit::text::{FONTS, ui_font};

/// How wide `text` is on one line at `size` in the UI font.
pub fn text_width(text: &str, size: f32) -> f32 {
    super::kit::text::width(text, size, ui_font())
}

/// The first of `texts` (longest first) that fits `room` px on one line at
/// `size`, else the last: a placeholder is never cut off, in any font.
pub fn fitting<'a>(texts: &[&'a str], size: f32, room: f32) -> &'a str {
    let last = texts.len() - 1;
    (texts.iter().position(|t| text_width(t, size) <= room)).map_or(texts[last], |i| texts[i])
}

/// A line of text is this many times its font size tall wherever its box
/// clips it (text fields): Arabic with every vowel mark stays inside
/// (Noto Sans Arabic inks about 1.6 times its size).
pub const LINE: f32 = 1.7;
/// Glyphs of the icon font used outside the action registry.
pub const SEND: char = '\u{e04e}'; // lucide "arrow-up"
pub const STOP: char = '\u{e16a}'; // lucide "square"
pub const CLOSE: char = super::kit::icon::Icon::Close.glyph();
const MINUS: char = '\u{e11f}'; // lucide "minus" (window header: minimise)
const RESTORE: char = '\u{e0a2}'; // lucide "copy" (two squares: restore)
pub const DRAG: char = '\u{e0ee}'; // lucide "grip-vertical" (drag-out handle)
const PIPETTE: char = '\u{e13e}'; // lucide "pipette" (colour popover: eyedropper)

/// Every button is this tall (DESIGN.md: 44 px controls).
pub const CONTROL: f32 = 44.0;
/// Text buttons share one width (consistency rule), so bars have a known size.
pub const TEXT_BUTTON_WIDTH: f32 = 136.0;
/// Corner radius of every surface (glass, solid, docked) and so of a floating
/// window that holds one.
pub const SURFACE_RADIUS: f32 = 20.0;
/// A pill's corner radius: more than any pill is tall, so its ends are half circles.
pub const PILL_RADIUS: f32 = 999.0;
/// Inner padding of a `surface`.
pub const SURFACE_PADDING: f32 = 6.0;

/// Fully rounded glass pill with one line of text (overlay hint, tooltips).
pub fn pill<'a, Message: 'a>(label: impl IntoFragment<'a>) -> Container<'a, Message> {
    let palette = theme::floating();
    let blurred = super::glass::backdrop().is_some();
    let inner = container(text(label).size(14).color(palette.text))
        .padding([8, 18])
        .style(move |_| glass(999.0, blurred));
    glassed(inner, 999.0)
}

/// `inner` (a `glass` container) over the blurred screen when the widget theme
/// has one for the widgets being built; else `inner` as it is.
fn glassed<'a, Message: 'a>(inner: Container<'a, Message>, radius: f32) -> Container<'a, Message> {
    match super::glass::backdrop() {
        Some(back) => container(super::glass::frosted(inner, back, radius)),
        None => inner,
    }
}

/// Glass panel (20 px radius): action bar, toolbars, popovers, side panels.
pub fn surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    let blurred = super::glass::backdrop().is_some();
    let inner = container(content)
        .padding(SURFACE_PADDING)
        .style(move |_| glass(20.0, blurred));
    glassed(inner, 20.0)
}

/// A `surface` docked over busy content (the result panel over the frozen
/// frame): the same glass, but solid, so nothing behind shows through.
pub fn docked<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content).padding(SURFACE_PADDING).style(|_| {
        let glass = glass(20.0, false);
        container::Style {
            background: Some(theme::floating().panel_solid.into()),
            ..glass
        }
    })
}

pub mod card {
    //! The floating result card (Search, Text, Ask AI):
    //! where it goes, how a drag moves or resizes it, the space it leaves free,
    //! and its frame (header with icon, title, chip and close; body; footer).
    //! One place for all of them, so every card floats, opens and moves the
    //! same way (the Ask AI card is the reference look).
    use iced::widget::{column, container, mouse_area, opaque, pin, row, space, stack, text};
    use iced::{Center, Element, Fill, Length, Point, Rectangle, Size, Vector, mouse};

    use super::{CLOSE, CONTROL, heading, icon, icon_button, theme};

    /// Distance to the screen edges, the side card's least width, the smallest
    /// a resize makes it, and below which width (or on a portrait screen) it
    /// floats at the bottom instead; the side card's share of the screen width,
    /// and the bottom card's width and least height.
    pub const GAP: f32 = 20.0;
    pub const WIDTH: f32 = 420.0;
    const SHARE: f32 = 0.4;
    const BOTTOM_WIDTH: f32 = 640.0;
    const BOTTOM_MIN: f32 = 260.0;
    /// How far a card may overlap the selection before it opens on the other side.
    const COVER_SLACK: f32 = 8.0;
    pub const MIN: Size = Size::new(320.0, 220.0);
    pub const NARROW: f32 = 900.0;
    /// The resize strip along the inner edge, and its grip pill.
    const EDGE: f32 = 10.0;
    const PILL: f32 = 40.0;
    const RADIUS: f32 = 20.0;
    /// The card's shadow rings: their width, and how much lower they sit.
    const SHADOW_STEP: f32 = 4.0;
    const SHADOW_DROP: f32 = 4.0;

    /// What a press on the card's frame does.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Drag {
        /// The header: move it.
        Move,
        /// The inner edge: resize it.
        Resize,
    }

    /// What the card's frame reports.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Event {
        Grab(Drag),
        /// The pointer is over the inner edge (its grip shows) or left it.
        Hover(bool),
        /// Double-click on the inner edge: back to the default place and size.
        Reset,
        Close,
    }

    /// The card's edge facing the rest of the screen (the one that resizes).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Side {
        Left,
        Right,
        Top,
    }

    /// Narrow or portrait: the card floats at the bottom.
    fn bottom(screen: Size) -> bool {
        screen.width < NARROW || screen.height > screen.width
    }

    /// Where the card goes on a `screen`-sized overlay (logical px): where the
    /// user holds it (`user`, kept on screen), else full height on the right, on
    /// the left when the right would cover the `selection` and the left would
    /// not; on a narrow / portrait screen at the bottom.
    pub fn place(screen: Size, selection: Option<Rectangle>, user: Option<Rectangle>) -> Rectangle {
        let inner = Size::new(
            (screen.width - 2.0 * GAP).max(0.0),
            (screen.height - 2.0 * GAP).max(0.0),
        );
        if let Some(r) = user {
            let size = Size::new(
                r.width.clamp(MIN.width.min(inner.width), inner.width),
                r.height.clamp(MIN.height.min(inner.height), inner.height),
            );
            return keep_on(Rectangle::new(r.position(), size), screen);
        }
        if bottom(screen) {
            let width = inner.width.min(BOTTOM_WIDTH);
            let height = (screen.height / 2.0).max(BOTTOM_MIN).min(inner.height);
            let at = Point::new((screen.width - width) / 2.0, screen.height - GAP - height);
            return Rectangle::new(at, Size::new(width, height));
        }
        let width = (screen.width * SHARE).max(WIDTH).min(inner.width);
        let size = Size::new(width, inner.height);
        let right = Rectangle::new(Point::new(screen.width - GAP - width, GAP), size);
        let left = Rectangle::new(Point::new(GAP, GAP), size);
        let covers = |card: Rectangle| {
            selection.is_some_and(|s| {
                let overlap = (card.x + card.width).min(s.x + s.width) - card.x.max(s.x);
                overlap > COVER_SLACK
            })
        };
        if covers(right) && !covers(left) {
            left
        } else {
            right
        }
    }

    /// A short prompt's card (a download question, a progress bar, an
    /// error): `at`'s width and place, but only as tall as `chars` of body
    /// text wrapped to it plus `below` px (buttons, a bar) need, never a
    /// slab of empty panel. A card at the bottom keeps its bottom edge.
    // ponytail: estimated (7.2 px per character at the 14 px body size),
    // not measured; generous by a line. A measured height if prompts grow.
    pub fn short(at: Rectangle, screen: Size, chars: usize, below: f32) -> Rectangle {
        const CHROME: f32 = 61.0 + 40.0; // header and rule, body padding
        let lines = (chars as f32 * 7.2 / (at.width - 40.0).max(1.0))
            .ceil()
            .max(1.0)
            + 1.0;
        let height = (CHROME + lines * 18.5 + below).min(at.height);
        let y = match side(at, screen) {
            Side::Top => at.y + at.height - height,
            _ => at.y,
        };
        Rectangle::new(Point::new(at.x, y), Size::new(at.width, height))
    }

    /// Which edge of the card `at` resizes it: the one facing the screen's middle.
    pub fn side(at: Rectangle, screen: Size) -> Side {
        if bottom(screen) {
            Side::Top
        } else if at.center_x() > screen.width / 2.0 {
            Side::Left
        } else {
            Side::Right
        }
    }

    /// `r` moved, or resized by its inner edge, by `by`; kept on the screen and
    /// at least `MIN` big.
    pub fn drag(r: Rectangle, how: Drag, by: Vector, screen: Size) -> Rectangle {
        if how == Drag::Move {
            return keep_on(r + by, screen);
        }
        let (x0, y0) = (r.x, r.y);
        let (x1, y1) = (r.x + r.width, r.y + r.height);
        let (x0, x1, y0) = match side(r, screen) {
            Side::Left => ((x0 + by.x).clamp(GAP, x1 - MIN.width), x1, y0),
            Side::Right => (
                x0,
                (x1 + by.x).clamp(x0 + MIN.width, screen.width - GAP),
                y0,
            ),
            Side::Top => (x0, x1, (y0 + by.y).clamp(GAP, y1 - MIN.height)),
        };
        Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
    }

    /// `r` moved inside the screen, `GAP` from its edges where it fits.
    fn keep_on(r: Rectangle, screen: Size) -> Rectangle {
        let x = r.x.min(screen.width - GAP - r.width).max(GAP);
        let y = r.y.min(screen.height - GAP - r.height).max(GAP);
        Rectangle::new(Point::new(x, y), r.size())
    }

    /// The part of the screen the card at `at` leaves free (with `GAP` to it):
    /// beside it, or above a card at the bottom. Floating bars and pills go there.
    pub fn free(screen: Size, at: Option<Rectangle>) -> Rectangle {
        let all = Rectangle::with_size(screen);
        let Some(at) = at else { return all };
        match side(at, screen) {
            Side::Left => Rectangle::new(
                Point::ORIGIN,
                Size::new((at.x - GAP).max(0.0), screen.height),
            ),
            Side::Right => {
                let x = (at.x + at.width + GAP).min(screen.width);
                Rectangle::new(
                    Point::new(x, 0.0),
                    Size::new(screen.width - x, screen.height),
                )
            }
            Side::Top => Rectangle::new(
                Point::ORIGIN,
                Size::new(screen.width, (at.y - GAP).max(0.0)),
            ),
        }
    }

    /// The header: the action's icon and name, an optional chip (the model).
    pub struct Header<'a> {
        pub icon: char,
        pub title: &'a str,
        pub chip: Option<String>,
    }

    /// The card at `at` on a `screen`: opaque rounded glass with a thin border,
    /// `header` (drag it to move the card; × closes), `body`, and `footer`
    /// under a rule. The inner edge resizes it: its grip pill shows while
    /// `hover`; a double-click there resets the card. Presses never reach the
    /// canvas below.
    #[allow(clippy::too_many_arguments)]
    pub fn frame<'a, Message: Clone + 'a>(
        header: Header<'a>,
        body: impl Into<Element<'a, Message>>,
        footer: Option<Element<'a, Message>>,
        at: Rectangle,
        screen: Size,
        hover: bool,
        on: impl Fn(Event) -> Message,
    ) -> Element<'a, Message> {
        let back = crate::ui::glass::backdrop();
        framed(header, body, footer, at, screen, hover, on, back)
    }

    /// `back`: the blurred screen behind a glass card, `None` = solid.
    #[allow(clippy::too_many_arguments)]
    fn framed<'a, Message: Clone + 'a>(
        header: Header<'a>,
        body: impl Into<Element<'a, Message>>,
        footer: Option<Element<'a, Message>>,
        at: Rectangle,
        screen: Size,
        hover: bool,
        on: impl Fn(Event) -> Message,
        back: Option<std::rc::Rc<crate::ui::glass::Backdrop>>,
    ) -> Element<'a, Message> {
        let palette = theme::floating();
        let mut top = row![icon(header.icon), heading(header.title)]
            .spacing(10)
            .align_y(Center);
        if let Some(chip) = header.chip {
            let chip = container(text(chip).size(12).color(palette.muted))
                .padding([3, 8])
                .style(move |_| container::Style {
                    background: Some(palette.hover.into()),
                    border: iced::Border::default().rounded(999),
                    ..container::Style::default()
                });
            top = top.push(chip);
        }
        let top = row![
            mouse_area(container(top).width(Fill).center_y(CONTROL))
                .on_press(on(Event::Grab(Drag::Move)))
                .interaction(mouse::Interaction::Grab),
            super::hint(
                icon_button(CLOSE, false, on(Event::Close)),
                "Close",
                super::Why::Keys("Esc"),
                iced::widget::tooltip::Position::Bottom,
            ),
        ]
        .align_y(Center)
        .padding(iced::Padding::new(8.0).left(20.0).right(12.0));
        let rule = || {
            container(space::vertical())
                .width(Fill)
                .height(1)
                .style(move |_| container::Style {
                    background: Some(palette.border.into()),
                    ..container::Style::default()
                })
        };
        let mut parts = column![top, rule()];
        parts = parts.push(container(body).padding(20).height(Fill));
        if let Some(footer) = footer {
            parts = parts.push(rule()).push(container(footer).padding([16, 20]));
        }
        // Frost / Graphite: the tint over the blurred screen instead of solid.
        let tint = if back.is_some() {
            palette.panel
        } else {
            palette.panel_solid
        };
        let card = container(parts)
            .width(at.width)
            .height(at.height)
            .style(move |_| container::Style {
                background: Some(tint.into()),
                border: iced::Border {
                    color: palette.border,
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                ..container::Style::default()
            });
        // the inner edge: a thin strip, its pill only while hovered
        let side = side(at, screen);
        let (strip, pill, cursor, place) = match side {
            Side::Left | Side::Right => (
                Size::new(EDGE, at.height - 2.0 * RADIUS),
                Size::new(4.0, PILL),
                mouse::Interaction::ResizingHorizontally,
                if side == Side::Left {
                    Point::new(0.0, RADIUS)
                } else {
                    Point::new(at.width - EDGE, RADIUS)
                },
            ),
            Side::Top => (
                Size::new(at.width - 2.0 * RADIUS, EDGE),
                Size::new(PILL, 4.0),
                mouse::Interaction::ResizingVertically,
                Point::new(RADIUS, 0.0),
            ),
        };
        let grip: Element<'a, Message> = match hover {
            true => container(space::horizontal())
                .width(pill.width)
                .height(pill.height)
                .style(move |_| container::Style {
                    background: Some(palette.muted.into()),
                    border: iced::Border::default().rounded(2),
                    ..container::Style::default()
                })
                .into(),
            false => space::horizontal().width(0).into(),
        };
        let edge = mouse_area(
            container(grip)
                .center_x(Length::Fixed(strip.width))
                .center_y(Length::Fixed(strip.height)),
        )
        .on_press(on(Event::Grab(Drag::Resize)))
        .on_double_click(on(Event::Reset))
        .on_enter(on(Event::Hover(true)))
        .on_exit(on(Event::Hover(false)))
        .interaction(cursor);
        let card: Element<'a, Message> = match back {
            Some(back) => crate::ui::glass::frosted(card, back, RADIUS),
            None => card.into(),
        };
        let layered = stack![card, pin(edge).position(place)]
            .width(at.width)
            .height(at.height);
        // its shadow: rings of quads under it (see `glass` for why no
        // `iced::Shadow`), a little lower than the card
        let out = 2.0 * SHADOW_STEP; // the widest ring
        let ring = |d: f32, alpha: f32| {
            let size = Size::new(at.width + 2.0 * d, at.height + 2.0 * d);
            let ring = container(space::horizontal())
                .width(size.width)
                .height(size.height)
                .style(move |_| container::Style {
                    background: Some(palette.shadow.scale_alpha(alpha).into()),
                    border: iced::Border::default().rounded(RADIUS + d),
                    ..container::Style::default()
                });
            pin(ring).position(Point::new(out - d, out + SHADOW_DROP - d))
        };
        // In layers of its own bounds (an empty first child: a stack draws
        // the rest in layers clipped to its container): dragged, the card
        // redraws as its old and new place, not as each of its parts.
        let whole = Size::new(at.width + 2.0 * out, at.height + 2.0 * out + SHADOW_DROP);
        let layers = stack![
            space(),
            ring(2.0 * SHADOW_STEP, 0.25),
            ring(SHADOW_STEP, 0.5),
            pin(opaque(layered)).position(Point::new(out, out))
        ]
        .width(whole.width)
        .height(whole.height);
        let layers = container(layers).width(whole.width).height(whole.height);
        pin(layers.clip(true))
            .position(at.position() - Vector::new(out, out))
            .into()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const SCREEN: Size = Size::new(1920.0, 1080.0);

        fn inside(r: Rectangle, screen: Size) -> bool {
            r.x >= GAP - 0.01
                && r.y >= GAP - 0.01
                && r.x + r.width <= screen.width - GAP + 0.01
                && r.y + r.height <= screen.height - GAP + 0.01
        }

        fn sel(x: f32) -> Option<Rectangle> {
            Some(Rectangle::new(
                Point::new(x, 300.0),
                Size::new(200.0, 100.0),
            ))
        }

        #[test]
        fn it_floats_on_the_users_side_unless_it_would_cover_the_selection() {
            // the default side (right), 40 % wide, inset all round
            let r = place(SCREEN, sel(200.0), None);
            assert_eq!(r.width, 768.0);
            assert_eq!(r.x, SCREEN.width - GAP - 768.0);
            assert_eq!((r.y, r.height), (GAP, SCREEN.height - 2.0 * GAP));
            assert!(inside(r, SCREEN));
            assert_eq!(side(r, SCREEN), Side::Left, "it resizes by its inner edge");
            // right of the middle but clear of the card: still right (as Google's card)
            assert_eq!(place(SCREEN, sel(860.0), None).x, r.x);
            // under the card: the other side, for this one only
            assert_eq!(place(SCREEN, sel(1100.0), None).x, GAP);
            assert_eq!(place(SCREEN, sel(1500.0), None).x, GAP);
            assert_eq!(side(place(SCREEN, sel(1500.0), None), SCREEN), Side::Right);
            // a small screen: at least WIDTH wide
            let small = Size::new(1000.0, 700.0);
            assert_eq!(place(small, None, None).width, WIDTH);
        }

        #[test]
        fn narrow_or_portrait_screens_get_a_card_at_the_bottom() {
            for screen in [
                Size::new(600.0, 1000.0),
                Size::new(1080.0, 1920.0),
                Size::new(800.0, 400.0),
                Size::new(300.0, 200.0),
            ] {
                let r = place(screen, sel(100.0), None);
                assert!(inside(r, screen), "{screen:?} -> {r:?}");
                assert!((r.y + r.height - (screen.height - GAP)).abs() < 0.01);
                assert!((r.center_x() - screen.width / 2.0).abs() < 0.01, "centred");
                assert!(r.width <= 640.0);
                assert_eq!(side(r, screen), Side::Top);
            }
        }

        #[test]
        fn a_moved_or_resized_card_stays_where_put_and_on_screen() {
            let r = place(SCREEN, None, None); // right side: resizes by its left edge
            let moved = drag(r, Drag::Move, Vector::new(-300.0, -10.0), SCREEN);
            assert!(inside(moved, SCREEN));
            assert_eq!(place(SCREEN, sel(1500.0), Some(moved)), moved, "remembered");
            let far = drag(r, Drag::Move, Vector::new(5000.0, 5000.0), SCREEN);
            assert!(inside(far, SCREEN), "{far:?}");
            // the left edge out: wider, the right edge stays
            let wider = drag(r, Drag::Resize, Vector::new(-100.0, 0.0), SCREEN);
            assert_eq!((wider.x, wider.width), (r.x - 100.0, r.width + 100.0));
            assert_eq!(wider.x + wider.width, r.x + r.width);
            // never under MIN, never past the screen
            let thin = drag(r, Drag::Resize, Vector::new(5000.0, 0.0), SCREEN);
            assert_eq!(thin.width, MIN.width);
            let huge = drag(r, Drag::Resize, Vector::new(-5000.0, 0.0), SCREEN);
            assert!(inside(huge, SCREEN), "{huge:?}");
            // a card on the left grows by its right edge; one at the bottom by its top
            let left = place(SCREEN, sel(1500.0), None);
            let grown = drag(left, Drag::Resize, Vector::new(50.0, 0.0), SCREEN);
            assert_eq!((grown.x, grown.width), (left.x, left.width + 50.0));
            let phone = Size::new(600.0, 1000.0);
            let b = place(phone, None, None);
            let taller = drag(b, Drag::Resize, Vector::new(0.0, -100.0), phone);
            assert_eq!((taller.y, taller.height), (b.y - 100.0, b.height + 100.0));
            // a remembered card on a smaller screen shrinks to fit
            let small_screen = Size::new(1280.0, 600.0);
            let fit = place(small_screen, None, Some(huge));
            assert!(inside(fit, small_screen), "{fit:?}");
        }

        #[test]
        fn the_free_part_is_beside_or_above_the_card() {
            assert_eq!(free(SCREEN, None), Rectangle::with_size(SCREEN));
            let right = place(SCREEN, sel(200.0), None);
            let f = free(SCREEN, Some(right));
            assert_eq!((f.x, f.width), (0.0, right.x - GAP));
            assert!(f.intersection(&right).is_none());
            let left = place(SCREEN, sel(1500.0), None);
            let f = free(SCREEN, Some(left));
            assert_eq!(f.x, left.x + left.width + GAP);
            assert!(f.intersection(&left).is_none());
            let phone = Size::new(600.0, 1000.0);
            let b = place(phone, None, None);
            let f = free(phone, Some(b));
            assert_eq!(f.height, b.y - GAP);
        }
    }
}

/// A `surface` for a colour-keyed window (the live controller): opaque and
/// without a shadow.
pub fn solid_surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Container<'a, Message> {
    let palette = theme::floating();
    container(content)
        .padding(SURFACE_PADDING)
        .style(move |_| container::Style {
            background: Some(palette.window.into()),
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: SURFACE_RADIUS.into(),
            },
            ..container::Style::default()
        })
}

/// A round accent button with nothing on it (a minimised control).
pub fn accent_dot<'a, Message: Clone + 'a>(size: f32, on_press: Message) -> Button<'a, Message> {
    button(space::horizontal())
        .width(size)
        .height(size)
        .padding(0)
        .on_press(on_press)
        .style(|_, status| button::Style {
            background: Some(accent(status)),
            border: Border::default().rounded(super::shape::radius(PILL_RADIUS)),
            ..button::Style::default()
        })
}

/// The thin frame around a pinned image (a pin window has no title bar).
pub fn pinned<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    container(content)
        .width(iced::Fill)
        .height(iced::Fill)
        .padding(1)
        .style(|_| container::Style {
            background: Some(theme::floating().border.into()),
            ..container::Style::default()
        })
}

/// Icon + label + key hint. `primary` = accent gradient (the default action),
/// otherwise quiet (transparent until hovered).
pub fn text_button<'a, Message: Clone + 'a>(
    icon: char,
    label: &'a str,
    key_hint: String,
    primary: bool,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::floating();
    let (fg, hint) = if primary {
        (theme::ON_ACCENT, theme::ON_ACCENT)
    } else {
        (palette.text, palette.muted)
    };
    let content = row![
        self::icon(icon).color(fg),
        text(label).size(14).color(fg),
        space::horizontal(),
        text(key_hint).size(11).color(hint),
    ]
    .spacing(8)
    .height(iced::Fill)
    .align_y(iced::Center);
    button(content)
        .width(TEXT_BUTTON_WIDTH)
        .height(CONTROL)
        .padding([0, 12])
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: quiet_or_accent(primary, status),
            text_color: fg,
            border: Border::default().rounded(14),
            ..button::Style::default()
        })
}

/// A glyph from the icon font.
pub fn icon<'a>(glyph: char) -> Text<'a> {
    text(glyph)
        .font(ICON_FONT)
        .size(16)
        .color(theme::floating().text)
}

/// Panel title.
pub fn heading<'a>(label: impl IntoFragment<'a>) -> Text<'a> {
    text(label).size(16).color(theme::floating().text)
}

/// Running text: answers, messages.
pub fn body<'a>(label: impl IntoFragment<'a>) -> Text<'a> {
    text(label).size(14).color(theme::floating().text)
}

/// Secondary small text: notes, model name, progress.
pub fn caption<'a>(label: impl IntoFragment<'a>) -> Text<'a> {
    text(label).size(12).color(theme::floating().muted)
}

/// An error message.
pub fn error<'a>(label: impl IntoFragment<'a>) -> Text<'a> {
    text(label).size(14).color(theme::floating().danger)
}

/// A suggestion (e.g. a prompt): small, fully rounded, outlined, quiet.
pub fn chip<'a, Message: Clone + 'a>(
    label: impl IntoFragment<'a>,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::floating();
    chip_style(button(text(label).size(13).color(palette.text)), on_press)
}

/// A `chip` that fills its row's share, text centred: a row of choices
/// (presets) with one width.
pub fn wide_chip<'a, Message: Clone + 'a>(
    label: impl IntoFragment<'a>,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::floating();
    let label = text(label)
        .size(13)
        .color(palette.text)
        .width(iced::Fill)
        .align_x(iced::Center);
    chip_style(button(label).width(iced::Fill), on_press)
}

fn chip_style<'a, Message: Clone + 'a>(
    button: Button<'a, Message>,
    on_press: Message,
) -> Button<'a, Message> {
    let palette = theme::floating();
    button
        .padding([8, 14])
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: quiet_or_accent(false, status),
            text_color: palette.text,
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: 999.0.into(),
            },
            ..button::Style::default()
        })
}

/// A pill button floating over the frozen frame ("Select text", "More"): the
/// glass of `pill`, `TEXT_BUTTON_WIDTH` wide, `CONTROL` tall.
pub fn float_chip<'a, Message: Clone + 'a>(
    label: &'a str,
    on_press: Message,
) -> Container<'a, Message> {
    let palette = theme::floating();
    // the pill's own glass (frosted with the widget theme), the button on it
    // only lights up under the pointer
    let blurred = super::glass::backdrop().is_some();
    let button = button(text(label).size(14).color(palette.text).center())
        .width(TEXT_BUTTON_WIDTH)
        .height(CONTROL)
        .on_press(on_press)
        .style(move |_, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(palette.hover.into()),
                _ => None,
            },
            text_color: palette.text,
            border: Border::default().rounded(super::shape::radius(PILL_RADIUS)),
            ..button::Style::default()
        });
    glassed(
        container(button).style(move |_| glass(999.0, blurred)),
        999.0,
    )
}

/// One-line text field, `CONTROL` tall.
pub fn input<'a, Message: Clone + 'a>(
    id: &'static str,
    placeholder: &str,
    value: &str,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
) -> TextInput<'a, Message> {
    let palette = theme::floating();
    text_input(placeholder, value)
        .id(id)
        .on_input(on_input)
        .on_submit(on_submit)
        .size(14)
        .line_height(LINE)
        .padding([(CONTROL - 14.0 * LINE) / 2.0, 14.0])
        .style(move |_, status| text_input::Style {
            background: palette.hover.into(),
            border: field_border(matches!(status, text_input::Status::Focused { .. })),
            icon: palette.muted,
            placeholder: palette.muted,
            value: palette.text,
            selection: theme::SELECTION,
        })
}

/// Multi-line editable text (the Text panel's result).
pub fn editor<'a, Message: Clone + 'a>(
    content: &'a text_editor::Content,
    on_action: impl Fn(text_editor::Action) -> Message + 'a,
) -> Element<'a, Message> {
    let palette = theme::floating();
    text_editor(content)
        .on_action(on_action)
        .size(14)
        .padding(14)
        .style(move |_, status| text_editor::Style {
            background: palette.hover.into(),
            border: field_border(matches!(status, text_editor::Status::Focused { .. })),
            placeholder: palette.muted,
            value: palette.text,
            selection: theme::SELECTION,
        })
        .height(iced::Fill)
        .into()
}

/// Progress bar, `value` in 0..=1.
pub fn progress<'a, Message: 'a>(value: f32) -> Element<'a, Message> {
    let palette = theme::floating();
    progress_bar(0.0..=1.0, value)
        .girth(6)
        .style(move |_| progress_bar::Style {
            background: palette.hover.into(),
            bar: accent(button::Status::Active),
            border: Border::default().rounded(3),
        })
        .into()
}

/// The user's own message in a chat: a soft rounded bubble.
pub fn bubble<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Container<'a, Message> {
    let palette = theme::floating();
    container(content)
        .padding([8, 12])
        .style(move |_| container::Style {
            background: Some(palette.pressed.into()),
            border: Border::default().rounded(14),
            ..container::Style::default()
        })
}

/// Vertical scroll area with a thin scroller from the palette.
pub fn scroll<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> scrollable::Scrollable<'a, Message> {
    let palette = theme::floating();
    scrollable(content)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(4).scroller_width(4),
        ))
        .style(move |theme, status| {
            let mut style = scrollable::default(theme, status);
            style.vertical_rail = scrollable::Rail {
                background: None,
                border: Border::default(),
                scroller: scrollable::Scroller {
                    background: palette.border.into(),
                    border: Border::default().rounded(2),
                },
            };
            style
        })
}

fn field_border(focused: bool) -> Border {
    let palette = theme::floating();
    Border {
        color: if focused {
            theme::accent().0
        } else {
            palette.border
        },
        width: 1.0,
        radius: 14.0.into(),
    }
}

/// Button background: accent gradient for `primary`, else clear until hovered.
fn quiet_or_accent(primary: bool, status: button::Status) -> Option<Background> {
    let palette = theme::floating();
    match (primary, status) {
        (true, _) => Some(accent(status)),
        (false, button::Status::Hovered) => Some(palette.hover.into()),
        (false, button::Status::Pressed) => Some(palette.pressed.into()),
        (false, _) => None,
    }
}

/// Shows `tip` in a pill beside `content` on hover. Outside this file use `hint`
/// (it keeps the policy and the test registry), not this.
fn with_tip<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    tip: impl IntoFragment<'a>,
    side: tooltip::Position,
) -> Tooltip<'a, Message> {
    tooltip(content, pill(tip), side).gap(10)
}

/// Why a control shows a visible tooltip at all. A tooltip that only repeats
/// what the control already says is noise, so these are the only three reasons.
#[derive(Debug, Clone, Copy)]
pub enum Why<'a> {
    /// Nothing but a glyph on it: the label is its only name.
    Icon,
    /// The shortcut is not written on the control: "Label · Ctrl+C".
    Keys(&'a str),
    /// Its text is cut short: the tooltip is the whole text.
    Cut(&'a str),
}

/// A control's accessible name (screen readers, automation, the unique-label
/// test) with a visible tooltip for one of the `Why` reasons.
pub fn hint<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    why: Why<'a>,
    side: tooltip::Position,
) -> Element<'a, Message> {
    #[cfg_attr(not(test), allow(unused_mut))]
    let mut content = content.into();
    let shown = match why {
        Why::Icon => label.to_string(),
        Why::Keys(keys) => format!("{label} · {keys}"),
        Why::Cut(full) => full.to_string(),
    };
    #[cfg(test)]
    tests_support::record_shown(&mut content, label, why, &shown);
    with_tip(content, shown, side).into()
}

/// Square 44x44 icon button (toolbar tools, back, undo/redo): quiet, hover,
/// `active` = accent gradient.
pub fn icon_button<'a, Message: Clone + 'a>(
    glyph: char,
    active: bool,
    on_press: Message,
) -> Button<'a, Message> {
    icon_button_maybe(glyph, active, Some(on_press))
}

/// `icon_button`; `None` = dimmed and dead, same size and place.
pub fn icon_button_maybe<'a, Message: Clone + 'a>(
    glyph: char,
    active: bool,
    on_press: Option<Message>,
) -> Button<'a, Message> {
    let fg = match (active, on_press.is_some()) {
        (true, _) => theme::ON_ACCENT,
        (false, true) => theme::floating().text,
        (false, false) => theme::floating().muted.scale_alpha(0.6),
    };
    square_button(
        text(glyph).font(ICON_FONT).size(18).color(fg),
        active,
        on_press,
    )
}

/// A 44x44 handle that starts a drag when the button goes down (an icon
/// button reacts on release, too late for an OS drag): quiet, grab cursor.
pub fn drag_handle<'a, Message: Clone + 'a>(glyph: char, on_down: Message) -> Element<'a, Message> {
    let fg = theme::floating().text;
    let look = square_button(text(glyph).font(ICON_FONT).size(18).color(fg), false, None);
    iced::widget::mouse_area(look)
        .on_press(on_down)
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

/// A small picture as a 44x44 quiet button (Snip's Recent strip).
pub fn thumbnail<'a, Message: Clone + 'a>(
    picture: iced::widget::image::Handle,
    on_press: Message,
) -> Button<'a, Message> {
    let shown = iced::widget::image(picture)
        .width(CONTROL - 8.0)
        .height(CONTROL - 8.0)
        .content_fit(iced::ContentFit::Contain);
    square_button(shown, false, Some(on_press))
}

/// Thin vertical line between toolbar groups.
pub fn toolbar_divider<'a, Message: 'a>() -> Element<'a, Message> {
    container(space::horizontal())
        .width(1)
        .height(24)
        .style(|_| container::Style {
            background: Some(theme::floating().border.into()),
            ..container::Style::default()
        })
        .into()
}

/// The ONE colour control: a 44x44 button showing the current colour; it opens
/// `colour_popover`.
pub fn colour_button<'a, Message: Clone + 'a>(
    colour: Color,
    open: bool,
    on_press: Message,
) -> Button<'a, Message> {
    square_button(dot(colour, 22.0, true), open, Some(on_press))
}

/// The colour picker popover: the colour swatches, stroke widths (thin/medium/
/// thick, `width` = the chosen one), a custom colour as hex with an eyedropper
/// (`on_pick`: pick from the image), then `more` (the editor's presets).
#[allow(clippy::too_many_arguments)]
pub fn colour_popover<'a, Message: Clone + 'a>(
    colours: &[Color],
    current: Color,
    width: usize,
    hex: &'a str,
    on_colour: impl Fn(Color) -> Message,
    on_width: impl Fn(usize) -> Message,
    on_hex: impl Fn(String) -> Message + 'a,
    on_pick: Message,
    more: Element<'a, Message>,
) -> Container<'a, Message> {
    let text = theme::floating().text;
    let widths = row((0..3).map(|i| {
        let size = [4.0, 8.0, 12.0][i];
        square_button(dot(text, size, false), i == width, Some(on_width(i))).into()
    }))
    .spacing(4);
    picker(
        colours,
        current,
        Some(widths.into()),
        hex,
        on_colour,
        on_hex,
        Some((on_pick, more)),
    )
}

/// The colour swatches, `extra` (stroke widths), a custom colour as hex and,
/// with `pick`, an eyedropper beside it and its `more` below.
fn picker<'a, Message: Clone + 'a>(
    colours: &[Color],
    current: Color,
    extra: Option<Element<'a, Message>>,
    hex: &'a str,
    on_colour: impl Fn(Color) -> Message,
    on_hex: impl Fn(String) -> Message + 'a,
    pick: Option<(Message, Element<'a, Message>)>,
) -> Container<'a, Message> {
    let palette = theme::floating();
    let swatch = |&c: &Color| -> Element<'a, Message> {
        let ring = if c == current {
            palette.text
        } else {
            palette.border
        };
        button(dot(c, 26.0, true))
            .padding(3)
            .on_press(on_colour(c))
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered).then(|| palette.hover.into()),
                border: Border {
                    color: ring,
                    width: if c == current { 2.0 } else { 0.0 },
                    radius: 999.0.into(),
                },
                ..button::Style::default()
            })
            .into()
    };
    let rows = colours
        .chunks(6)
        .map(|chunk| row(chunk.iter().map(swatch)).spacing(6).into());
    use iced::widget::{column, text_input};
    let custom = text_input("#RRGGBB", hex)
        .on_input(on_hex)
        .size(14)
        .padding([8, 12])
        .style(|_, _| text_input::Style {
            background: palette.hover.into(),
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: 10.0.into(),
            },
            icon: palette.muted,
            placeholder: palette.muted,
            value: palette.text,
            selection: theme::accent().0.scale_alpha(0.4),
        });
    let (custom, more): (Element<'a, Message>, _) = match pick {
        Some((on_pick, more)) => {
            let pick = tooltip(
                icon_button(PIPETTE, false, on_pick),
                pill("Pick a colour from the image (D)"),
                tooltip::Position::Bottom,
            )
            .gap(10);
            let custom = custom.padding([12, 12]); // as tall as the eyedropper
            (
                row![custom, pick].spacing(6).align_y(iced::Center).into(),
                Some(more),
            )
        }
        None => (custom.into(), None),
    };
    surface(
        column![column(rows).spacing(6)]
            .push(extra)
            .push(custom)
            .push(more)
            .spacing(10)
            .padding(4)
            .width(6.0 * 32.0 + 5.0 * 6.0 + 8.0),
    )
}

/// A filled circle; `ring` adds a thin outline (so white reads on a light panel).
fn dot<'a, Message: 'a>(colour: Color, size: f32, ring: bool) -> Element<'a, Message> {
    let border = theme::floating().border;
    container(space::horizontal())
        .width(size)
        .height(size)
        .style(move |_| container::Style {
            background: Some(colour.into()),
            border: Border {
                color: border,
                width: if ring { 1.5 } else { 0.0 },
                radius: 999.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// 44x44 button around centred `content`, styled like a quiet `text_button`
/// (accent gradient when `active`).
/// A square option in a floating popover (a text's size, font, backdrop):
/// `content` (a letter or a preview) on the shared square button.
pub fn option_button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    active: bool,
    on_press: Message,
) -> Button<'a, Message> {
    square_button(content, active, Some(on_press))
}

fn square_button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    active: bool,
    on_press: Option<Message>,
) -> Button<'a, Message> {
    let palette = theme::floating();
    button(container(content).center(iced::Fill))
        .width(CONTROL)
        .height(CONTROL)
        .padding(0)
        .on_press_maybe(on_press)
        .style(move |_, status| {
            let background = match (active, status) {
                (true, _) => Some(accent(status)),
                (false, button::Status::Hovered) => Some(palette.hover.into()),
                (false, button::Status::Pressed) => Some(palette.pressed.into()),
                (false, _) => None,
            };
            button::Style {
                background,
                text_color: palette.text,
                border: Border::default().rounded(14),
                ..button::Style::default()
            }
        })
}

/// The accent gradient; hover/press brighten it a little.
fn accent(status: button::Status) -> Background {
    let lift = match status {
        button::Status::Hovered => 0.08,
        button::Status::Pressed => 0.14,
        _ => 0.0,
    };
    let (start, end) = theme::accent();
    let mix = |c: Color| Color {
        r: c.r + (1.0 - c.r) * lift,
        g: c.g + (1.0 - c.g) * lift,
        b: c.b + (1.0 - c.b) * lift,
        a: c.a,
    };
    gradient::Linear::new(std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, mix(start))
        .add_stop(1.0, mix(end))
        .into()
}

/// The one glass style: panel colour, 1 px border; the active look's bar
/// style makes it solid or a see-through outline. Never an `iced::Shadow`
/// anywhere: tiny-skia draws a shadow whole on every redraw, outside the part
/// being redrawn too, so it piles up into a black block (and a floating
/// window can't show one: no alpha there).
fn glass(radius: f32, blurred: bool) -> container::Style {
    let palette = theme::floating();
    // Frost / Graphite: the translucent tint only over the blurred screen, else
    // (glass off, a popup window) the solid tint.
    let themed = crate::config::widgets().0 != crate::config::WidgetTheme::Default;
    let (background, border, width) = match crate::config::look().bar {
        BarStyle::Solid => (palette.panel_solid, palette.border, 1.0),
        BarStyle::Outline => (palette.panel, theme::accent().0, 1.5),
        _ if themed && !blurred => (palette.panel_solid, palette.border, 1.0),
        _ => (palette.panel, palette.border, 1.0),
    };
    container::Style {
        background: Some(background.into()),
        border: Border {
            color: border,
            width,
            radius: radius.into(),
        },
        ..container::Style::default()
    }
}

mod forms {
    use iced::Element;
    use iced::widget::tooltip;

    /// A control's accessible name (iced has no a11y tree): what automation calls it,
    /// and what the test registry checks is unique per screen. It shows NO tooltip,
    /// since the control's own text says it; `hint` is for icon-only, cut or
    /// shortcut-carrying controls. `side` stays so a call can become a `hint` in place.
    pub fn tip<'a, Message: 'a>(
        content: impl Into<Element<'a, Message>>,
        label: &'a str,
        _side: tooltip::Position,
    ) -> Element<'a, Message> {
        #[cfg(test)]
        super::TIPS.with(|t| t.borrow_mut().push(label.to_string()));
        #[cfg(not(test))]
        let _ = label;
        content.into()
    }
}
#[allow(unused_imports)]
pub use forms::*;

pub mod chrome {
    //! The header and frame of our normal windows (Settings, Onboarding, the
    //! permission prompt): the OS title bar is off, so the window draws its own, the
    //! same everywhere. App icon and title, drag anywhere on it, double-click to
    //! maximise, minimise / maximise / close; a thin strip along every edge resizes.
    use std::sync::LazyLock;

    use iced::widget::{Space, button, column, container, image, mouse_area, row, stack, text};
    use iced::window::Direction;
    use iced::{Border, Center, Element, Fill, Length, mouse};

    use super::{ICON_FONT, theme};

    /// The header's height (logical px) and the resize strip along each edge.
    pub const HEADER: f32 = 40.0;
    pub const EDGE: f32 = 6.0;
    /// One caption button's width (the three share it).
    const BUTTON: f32 = 46.0;

    /// What the frame reports.
    #[derive(Debug, Clone, Copy)]
    pub enum Event {
        /// Pressed on the header's free area: a drag, or the second half of a
        /// double-click (the shell tells them apart by time).
        Press,
        Minimise,
        /// The maximise / restore button.
        Maximise,
        Close,
        /// Pressed on an edge or corner: the OS resizes from that side.
        Resize(Direction),
    }

    /// What the window can do: a fixed-size window has no maximise button and no
    /// edges; a maximised one has no edges either.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Mode {
        pub resizable: bool,
        pub maximised: bool,
    }

    /// Our logo, decoded once: the window icon (taskbar, Alt+Tab) and the header's.
    pub struct Logo {
        pub width: u32,
        pub height: u32,
        pub rgba: Vec<u8>,
    }

    pub fn logo() -> &'static Logo {
        static LOGO: LazyLock<Logo> = LazyLock::new(|| {
            let bytes = include_bytes!("../../assets/icons/64x64.png");
            let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes[..]))
                .read_info()
                .expect("the bundled logo is a PNG");
            let mut rgba = vec![0; reader.output_buffer_size().expect("sized")];
            let info = reader
                .next_frame(&mut rgba)
                .expect("the bundled logo decodes");
            rgba.truncate(info.buffer_size());
            Logo {
                width: info.width,
                height: info.height,
                rgba,
            }
        });
        &LOGO
    }

    /// A bundled RGBA PNG as an image (decode it once: keep the handle).
    pub(crate) fn png_handle(bytes: &[u8]) -> image::Handle {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .expect("a bundled PNG");
        let mut rgba = vec![0; reader.output_buffer_size().expect("sized")];
        let info = reader.next_frame(&mut rgba).expect("the PNG decodes");
        rgba.truncate(info.buffer_size());
        image::Handle::from_rgba(info.width, info.height, rgba)
    }

    pub(crate) fn logo_handle() -> image::Handle {
        // One handle, so the renderer's image cache keeps its pixels between frames.
        static HANDLE: LazyLock<image::Handle> = LazyLock::new(|| {
            let l = logo();
            image::Handle::from_rgba(l.width, l.height, l.rgba.clone())
        });
        HANDLE.clone()
    }

    /// A caption button: quiet, a hover tint; `close` tints red.
    fn caption<'a, Message: Clone + 'a>(
        glyph: char,
        on_press: Message,
        close: bool,
    ) -> Element<'a, Message> {
        let palette = theme::current();
        button(container(text(glyph).font(ICON_FONT).size(14)).center(Fill))
            .width(BUTTON)
            .height(HEADER)
            .padding(0)
            .on_press(on_press)
            .style(move |_, status| {
                let (background, text_color) = match (status, close) {
                    (button::Status::Hovered | button::Status::Pressed, true) => {
                        (Some(theme::CLOSE_HOVER.into()), theme::ON_CLOSE_HOVER)
                    }
                    (button::Status::Hovered, false) => (Some(palette.hover.into()), palette.text),
                    (button::Status::Pressed, false) => {
                        (Some(palette.pressed.into()), palette.text)
                    }
                    _ => (None, palette.text),
                };
                button::Style {
                    background,
                    text_color,
                    border: Border::default(),
                    ..button::Style::default()
                }
            })
            .into()
    }

    fn header<'a, Message: Clone + 'a>(
        title: &'a str,
        mode: Mode,
        on: &impl Fn(Event) -> Message,
    ) -> Element<'a, Message> {
        let palette = theme::current();
        let grip = row![
            image(logo_handle()).width(18).height(18),
            text(title).size(13).color(palette.text)
        ]
        .spacing(10)
        .align_y(Center);
        let mut bar = row![
            mouse_area(
                container(grip)
                    .padding([0, 14])
                    .center_y(HEADER)
                    .width(Fill)
            )
            .on_press(on(Event::Press)),
            caption(super::MINUS, on(Event::Minimise), false),
        ];
        if mode.resizable {
            let glyph = if mode.maximised {
                super::RESTORE
            } else {
                super::STOP // lucide "square"
            };
            bar = bar.push(caption(glyph, on(Event::Maximise), false));
        }
        bar = bar.push(caption(super::CLOSE, on(Event::Close), true));
        container(bar)
            .height(HEADER)
            .style(move |_| container::Style {
                background: Some(palette.window.into()),
                text_color: Some(palette.text),
                ..container::Style::default()
            })
            .into()
    }

    /// The invisible strips along the edges and corners.
    fn edges<'a, Message: Clone + 'a>(on: &impl Fn(Event) -> Message) -> Element<'a, Message> {
        use mouse::Interaction::{
            ResizingDiagonallyDown as Down, ResizingDiagonallyUp as Up,
            ResizingHorizontally as Horizontal, ResizingVertically as Vertical,
        };
        let zone = |direction, interaction, width: Length, height: Length| {
            mouse_area(Space::new().width(width).height(height))
                .on_press(on(Event::Resize(direction)))
                .interaction(interaction)
        };
        let corner =
            |direction, interaction| zone(direction, interaction, EDGE.into(), EDGE.into());
        let top = row![
            corner(Direction::NorthWest, Down),
            zone(Direction::North, Vertical, Fill, EDGE.into()),
            corner(Direction::NorthEast, Up),
        ];
        let middle = row![
            zone(Direction::West, Horizontal, EDGE.into(), Fill),
            Space::new().width(Fill),
            zone(Direction::East, Horizontal, EDGE.into(), Fill),
        ]
        .height(Fill);
        let bottom = row![
            corner(Direction::SouthWest, Up),
            zone(Direction::South, Vertical, Fill, EDGE.into()),
            corner(Direction::SouthEast, Down),
        ];
        column![top, middle, bottom].into()
    }

    /// `content` under the header, in a window-coloured frame with a 1 px border
    /// (not when maximised), and the resize edges on top.
    pub fn frame<'a, Message: Clone + 'a>(
        title: &'a str,
        mode: Mode,
        content: impl Into<Element<'a, Message>>,
        on: impl Fn(Event) -> Message,
    ) -> Element<'a, Message> {
        let palette = theme::current();
        let page = column![
            header(title, mode, &on),
            container(content).width(Fill).height(Fill)
        ];
        let border = if mode.maximised { 0.0 } else { 1.0 };
        let window = container(page)
            .width(Fill)
            .height(Fill)
            .padding(border)
            .style(move |_| container::Style {
                background: Some(palette.window.into()),
                border: Border {
                    color: palette.border,
                    width: border,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            });
        with_edges(window, mode, on)
    }

    /// `window` with the resize edges on top (only when it can resize now).
    pub fn with_edges<'a, Message: Clone + 'a>(
        window: impl Into<Element<'a, Message>>,
        mode: Mode,
        on: impl Fn(Event) -> Message,
    ) -> Element<'a, Message> {
        if mode.resizable && !mode.maximised {
            stack![window.into(), edges(&on)].into()
        } else {
            window.into()
        }
    }

    impl From<crate::ui::kit::header::Caption> for Event {
        fn from(caption: crate::ui::kit::header::Caption) -> Event {
            use crate::ui::kit::header::Caption;
            match caption {
                Caption::Drag => Event::Press,
                Caption::Minimise => Event::Minimise,
                Caption::Maximise => Event::Maximise,
                Caption::Close => Event::Close,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::ui::widgets::shot;
        use iced::Size;

        /// What one click at `at` reports, by name.
        fn pressing(at: (f32, f32), mode: Mode) -> Vec<String> {
            let page: Element<'_, Event> = text("page").into();
            let view = frame("Settings", mode, page, |e| e);
            let size = Size::new(400.0, 300.0);
            let events = shot::click(view, size, iced::Point::new(at.0, at.1));
            events.iter().map(|e| format!("{e:?}")).collect()
        }

        fn is(got: Vec<String>, want: &[&str]) {
            assert_eq!(got, want);
        }

        const FREE: Mode = Mode {
            resizable: true,
            maximised: false,
        };

        #[test]
        fn header_hit_zones_drag_buttons_and_edges() {
            // Free header area: drag; the three caption buttons at the right end.
            is(pressing((200.0, 20.0), FREE), &["Press"]);
            is(pressing((284.0, 20.0), FREE), &["Minimise"]);
            is(pressing((330.0, 20.0), FREE), &["Maximise"]);
            is(pressing((380.0, 20.0), FREE), &["Close"]);
            // The page below does nothing.
            is(pressing((200.0, 150.0), FREE), &[]);
            // Edges and corners resize from their side.
            let edge = |x, y| pressing((x, y), FREE);
            is(edge(2.0, 150.0), &["Resize(West)"]);
            is(edge(397.0, 150.0), &["Resize(East)"]);
            is(edge(200.0, 297.0), &["Resize(South)"]);
            is(edge(200.0, 2.0), &["Resize(North)"]);
            is(edge(2.0, 2.0), &["Resize(NorthWest)"]);
            is(edge(397.0, 2.0), &["Resize(NorthEast)"]);
            is(edge(2.0, 297.0), &["Resize(SouthWest)"]);
            is(edge(397.0, 297.0), &["Resize(SouthEast)"]);
        }

        #[test]
        fn a_fixed_or_maximised_window_has_no_edges_and_a_fixed_one_no_maximise() {
            for mode in [
                Mode {
                    resizable: true,
                    maximised: true,
                },
                Mode {
                    resizable: false,
                    maximised: false,
                },
            ] {
                is(pressing((2.0, 150.0), mode), &[]);
                is(pressing((200.0, 297.0), mode), &[]);
                is(pressing((200.0, 20.0), mode), &["Press"]);
            }
            // Fixed: close sits where maximise would be, one slot over.
            let fixed = Mode {
                resizable: false,
                maximised: false,
            };
            is(pressing((380.0, 20.0), fixed), &["Close"]);
            is(pressing((330.0, 20.0), fixed), &["Minimise"]);
        }

        #[test]
        fn the_logo_decodes() {
            let l = logo();
            assert_eq!((l.width, l.height), (64, 64));
            assert_eq!(l.rgba.len(), 64 * 64 * 4);
            assert!(l.rgba.chunks(4).any(|p| p[3] > 0), "not blank");
        }
    }
}

#[cfg(test)]
thread_local! {
    pub static TIPS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Every visible tooltip built (its text, why, and the texts its control shows).
    pub static SHOWN: std::cell::RefCell<Vec<tests_support::Shown>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The tooltip policy as a test: what a screen's visible tooltips say must add
/// something to what their controls already show.
#[cfg(test)]
pub mod tests_support {
    use super::{SHOWN, TIPS, Why};
    use iced::advanced::layout::{Layout, Limits};
    use iced::advanced::widget::{Id, Operation, Tree};
    use iced::{Element, Font, Pixels, Rectangle, Size};

    pub struct Shown {
        pub label: String,
        pub why: String,
        pub tip: String,
        pub visible: Vec<String>,
    }

    /// The texts `content` shows (labels and typed text, glyphs included).
    fn texts<Message>(content: &mut Element<'_, Message>) -> Vec<String> {
        struct Collect(Vec<String>);
        impl Operation for Collect {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
                self.0.push(text.to_string());
            }
        }
        let renderer = iced::Renderer::new(Font::DEFAULT, Pixels(16.0));
        let mut tree = Tree::new(content.as_widget());
        let limits = Limits::new(Size::ZERO, Size::new(900.0, 700.0));
        let node = content
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let mut collect = Collect(Vec::new());
        content
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut collect);
        collect.0
    }

    pub fn record_shown<Message>(
        content: &mut Element<'_, Message>,
        label: &str,
        why: Why<'_>,
        tip: &str,
    ) {
        TIPS.with(|t| t.borrow_mut().push(label.to_string()));
        let visible = texts(content);
        SHOWN.with(|s| {
            s.borrow_mut().push(Shown {
                label: label.to_string(),
                why: format!("{why:?}"),
                tip: tip.to_string(),
                visible,
            })
        });
    }

    /// The visible tooltips built since the last check (their texts).
    pub fn shown() -> Vec<String> {
        SHOWN.with(|s| s.borrow().iter().map(|s| s.tip.clone()).collect())
    }

    fn norm(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }

    /// Asserts that every visible tooltip built since the last call says something
    /// its control does not: not its text again, not its shortcut again, and an
    /// icon-only control has no words on it. Clears the record.
    pub fn assert_tips_add_something(screen: &str) {
        let shown = SHOWN.with(|s| std::mem::take(&mut *s.borrow_mut()));
        for s in shown {
            let (tip, label) = (norm(&s.tip), norm(&s.label));
            for v in s.visible.iter().map(|v| norm(v)).filter(|v| !v.is_empty()) {
                assert!(
                    v != tip && v != label,
                    "{screen}: the tooltip {:?} repeats {:?} on its control",
                    s.tip,
                    s.visible
                );
                if s.why.starts_with("Keys") {
                    assert!(
                        !tip.ends_with(&v),
                        "{screen}: {:?} already shows its shortcut",
                        s.tip
                    );
                }
            }
            if s.why == "Icon" {
                let words = s
                    .visible
                    .iter()
                    .any(|v| v.chars().any(char::is_alphanumeric));
                assert!(!words, "{screen}: {:?} has words, so no tooltip", s.tip);
            }
        }
    }
}

/// Headless renders for tests (the kit's test harness).
#[cfg(test)]
pub use super::kit::testing::shot;

#[cfg(test)]
mod tests {
    /// No `iced::Shadow` with a blur anywhere (see `glass`): tiny-skia draws
    /// it outside the part being redrawn and it piles up into black blocks.
    #[test]
    fn no_drawn_shadows() {
        fn scan(dir: &std::path::Path, found: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    scan(&p, found);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    let n = src.matches(concat!("blur", "_radius:")).count();
                    if n > 0 {
                        found.push(format!("{} ({n})", p.display()));
                    }
                }
            }
        }
        let mut found = Vec::new();
        scan(std::path::Path::new("src"), &mut found);
        assert!(found.is_empty(), "drawn shadows in {found:?}");
    }

    /// The ink of `sample` in `family` at `size` px laid out in one `LINE` line:
    /// (top, bottom) in px from the line's top.
    fn ink(sample: &str, family: &str, size: f32) -> (f32, f32) {
        use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};
        let mut db = cosmic_text::fontdb::Database::new();
        for f in super::FONTS {
            db.load_font_data(f.to_vec());
        }
        let mut fs = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let mut buf = Buffer::new(&mut fs, Metrics::new(size, size * super::LINE));
        let attrs = Attrs::new().family(Family::Name(family));
        buf.set_text(&mut fs, sample, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(&mut fs, false);
        let mut cache = SwashCache::new();
        let (mut top, mut bottom) = (f32::MAX, f32::MIN);
        for run in buf.layout_runs() {
            for glyph in run.glyphs {
                let p = glyph.physical((0.0, 0.0), 1.0);
                let Some(img) = cache.get_image(&mut fs, p.cache_key) else {
                    continue;
                };
                if img.placement.height == 0 {
                    continue;
                }
                let y = run.line_y + p.y as f32 - img.placement.top as f32;
                top = top.min(y);
                bottom = bottom.max(y + img.placement.height as f32);
            }
        }
        (top, bottom)
    }

    /// A field's line holds the tallest Latin and Arabic text (accents, Arabic
    /// marks above and below): a text field clips to its line (#53).
    #[test]
    fn a_line_holds_latin_and_arabic_marks() {
        let line = 14.0 * super::LINE;
        for (sample, family) in [
            ("ÅÉÎÕÛ jgpqy ÇŞ", "Inter"),
            ("إِنَّ الْعِلْمَ نُورٌ ضِّ", "Noto Sans Arabic"),
        ] {
            let (top, bottom) = ink(sample, family, 14.0);
            assert!(
                top >= 0.0 && bottom <= line,
                "{family}: ink {top}..{bottom} outside the {line} px line"
            );
        }
    }

    #[test]
    fn glyphs_are_in_the_subset_font() {
        let script = include_str!("../../assets/lucide/subset.sh");
        for glyph in [
            super::SEND,
            super::STOP,
            super::CLOSE,
            super::MINUS,
            super::RESTORE,
        ] {
            let code = format!("U+{:04X}", glyph as u32);
            assert!(script.contains(&code), "add {code} to subset.sh");
        }
    }
}
