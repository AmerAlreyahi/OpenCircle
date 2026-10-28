//! A floating card (the design's `.as-card` on `.oc-widget`: the update card,
//! later the model download and error cards): a heading (an accent icon and
//! a 16 px title; pressing it drags the window), a body that scrolls when it
//! is long, and an actions row (a call to action's height). Fills its window,
//! padding 22; `height` sizes the window to its body (`.as-card` grows with
//! its content).
//!
//! The result card on the overlay (Search, Text, Ask AI, Translate, Song) is
//! the same `.as-card` floating over the frozen frame (`card`): where it goes,
//! how a drag moves or resizes it, the room it leaves free, and its frame. One
//! place, so every card floats, opens and moves the same way.
use iced::widget::{Space, column, container, mouse_area, opaque, pin, row, scrollable, stack};
use iced::{Border, Center, Element, Fill, Length, Point, Rectangle, Size, Vector, mouse};

use super::icon::{Icon, icon};
use super::surface::{Material, surface};
use super::text::text;
use super::themes::c;
use super::tokens::Role;

const PAD: f32 = 22.0;
/// Under the heading, above the actions (`.as-heading` 14, `.as-actions` 18).
const UNDER_HEADING: f32 = 14.0;
const ABOVE_ACTIONS: f32 = 18.0;
const ICON: f32 = 20.0;
/// The heading row (the 20 px icon, the 16 px title).
const HEADING: f32 = 24.0;
/// The card's width (`.as-card`: `min(100%, 480px)`).
pub const WIDTH: f32 = 480.0;
const ICON_GAP: f32 = 10.0;
/// The scroller: 4 px, no rail.
const SCROLLER: f32 = 4.0;

/// The body's width (to wrap its text at).
pub const BODY_WIDTH: f32 = WIDTH - 2.0 * PAD;

/// The card's height for a body `body` px tall, at most `max` (then the body scrolls).
pub fn height(body: f32, max: f32) -> f32 {
    let chrome = 2.0 * PAD + HEADING + UNDER_HEADING + ABOVE_ACTIONS + super::tokens::size::CTA_H;
    (chrome + body).ceil().min(max)
}

/// The card with corners of `radius` (the window's cut).
pub fn frame<'a, Message: Clone + 'a>(
    glyph: Icon,
    title: &'a str,
    body: impl Into<Element<'a, Message>>,
    actions: impl Into<Element<'a, Message>>,
    radius: f32,
    on_drag: Message,
) -> Element<'a, Message> {
    let heading = row![
        icon(glyph, ICON).color(c().accent),
        text(Role::PAGE_TITLE, title)
    ]
    .spacing(ICON_GAP)
    .height(HEADING)
    .align_y(Center);
    let heading = mouse_area(container(heading).width(Fill).padding(iced::Padding {
        bottom: UNDER_HEADING,
        ..iced::Padding::ZERO
    }))
    .on_press(on_drag);
    let bar = scrollable::Scrollbar::new()
        .width(SCROLLER)
        .scroller_width(SCROLLER);
    let body = scrollable(body)
        .direction(scrollable::Direction::Vertical(bar))
        .height(Fill)
        .style(|theme, status| {
            let mut style = scrollable::default(theme, status);
            style.vertical_rail = scrollable::Rail {
                background: None,
                border: Border::default(),
                scroller: scrollable::Scroller {
                    background: c().rim.into(),
                    border: Border::default().rounded(SCROLLER / 2.0),
                },
            };
            style
        });
    let actions = container(actions).padding(iced::Padding {
        top: ABOVE_ACTIONS,
        ..iced::Padding::ZERO
    });
    surface(Material::Widget, radius, column![heading, body, actions])
        .padding(PAD)
        .width(Fill)
        .height(Fill)
        .into()
}

/// Distance to the screen edges, the side card's least width, the smallest
/// a resize makes it, and below which width (or on a portrait screen) it
/// floats at the bottom instead.
pub const GAP: f32 = 20.0;
pub const SIDE_WIDTH: f32 = 420.0;
/// The side card's share of the screen width; the bottom card's width and
/// least height.
const SHARE: f32 = 0.4;
const BOTTOM_WIDTH: f32 = 640.0;
const BOTTOM_MIN: f32 = 260.0;
/// How far a card may overlap the selection before it opens on the other side.
const COVER_SLACK: f32 = 8.0;
pub const MIN: Size = Size::new(320.0, 220.0);
pub const NARROW: f32 = 900.0;
/// The resize strip along the inner edge, and its grip.
const EDGE: f32 = 10.0;
const GRIP: f32 = 40.0;
const GRIP_GIRTH: f32 = 4.0;
/// The result card's corners (`.as-card`).
pub const CARD_RADIUS: f32 = super::tokens::radius::DRAWER;
/// Its top row: the heading and the close button.
const TOP: f32 = super::tokens::size::ICON_BUTTON;
/// The close button reaches this far into the card's padding (top, right).
const CLOSE_IN: f32 = 8.0;

/// What a press on the card's frame does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drag {
    /// The heading: move it.
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
    let width = (screen.width * SHARE).max(SIDE_WIDTH).min(inner.width);
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

/// A short prompt's card (a download question, a progress bar, an error):
/// `at`'s width and place, but only as tall as `chars` of body text wrapped
/// to it plus `below` px (buttons, a bar) need, never a slab of empty card.
/// A card at the bottom keeps its bottom edge.
// ponytail: estimated (7.2 px per character, generous for the 12-13 px card
// text, 20 px a line), not measured; generous by a line. Measure if prompts grow.
pub fn short(at: Rectangle, screen: Size, chars: usize, below: f32) -> Rectangle {
    // the padding, the top row and the gap under it
    const CHROME: f32 = 2.0 * PAD - CLOSE_IN + TOP + UNDER_HEADING;
    let lines = (chars as f32 * 7.2 / (at.width - 2.0 * PAD).max(1.0))
        .ceil()
        .max(1.0)
        + 1.0;
    let height = (CHROME + lines * 20.0 + below).min(at.height);
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

/// The AI chat's card (`#widgetAiContainer`): 35 % of the screen wide
/// (340..640 px, never past the screen), 95 % of the height above the
/// taskbar (`taskbar` px at the bottom), 16 px from the `side` (left or
/// right; `Top` counts as right) and 14 px from the top. It is not resized:
/// a drag moves it, and where it is let go it docks (`dock_side`).
pub fn docked(screen: Size, side: Side, taskbar: f32) -> Rectangle {
    const SHARE: f32 = 0.35;
    const WIDE: (f32, f32) = (340.0, 640.0);
    const TALL: f32 = 0.95;
    const SIDE_GAP: f32 = 16.0;
    const TOP_GAP: f32 = 14.0;
    let room = (screen.width - 2.0 * SIDE_GAP).max(0.0);
    let width = (screen.width * SHARE).clamp(WIDE.0, WIDE.1).min(room);
    let height = ((screen.height - taskbar) * TALL).min(screen.height - TOP_GAP);
    let x = match side {
        Side::Left => SIDE_GAP,
        _ => screen.width - SIDE_GAP - width,
    };
    Rectangle::new(Point::new(x, TOP_GAP), Size::new(width, height.max(0.0)))
}

/// The side a dragged chat card docks to where it is let go: the one its
/// middle is on.
pub fn dock_side(r: Rectangle, screen: Size) -> Side {
    match r.center_x() < screen.width / 2.0 {
        true => Side::Left,
        false => Side::Right,
    }
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

/// The card's heading: the action's glyph (accent, 20 px) or, for AI, the
/// OpenCircle `logo` in its place, its name (`PAGE_TITLE`), and an optional
/// pill (the AI model).
pub struct Header<'a> {
    pub glyph: char,
    /// AI cards: the logo instead of the glyph (never a generic AI glyph).
    pub logo: Option<iced::widget::image::Handle>,
    pub title: &'a str,
    pub chip: Option<&'a str>,
    /// A docked card (the AI chat, `docked`): no resize edge.
    pub docked: bool,
}

/// The result card at `at` on a `screen` (`.as-card`: the widget surface,
/// radius 18, 22 px in): `header` (pressing it moves the card), `close` at
/// its end (the screen's close button, with its tooltip), `body`, and
/// `footer` 18 px under it (`.as-actions`). The inner edge resizes it: its
/// grip shows while `hover`; a double-click there resets the card. Solid and
/// without a shadow (glass joins with the GPU renderer). Presses never reach
/// the canvas below.
#[allow(clippy::too_many_arguments)]
pub fn card<'a, Message: Clone + 'a>(
    header: Header<'a>,
    close: Element<'a, Message>,
    body: impl Into<Element<'a, Message>>,
    footer: Option<Element<'a, Message>>,
    at: Rectangle,
    screen: Size,
    hover: bool,
    on: impl Fn(Event) -> Message,
) -> Element<'a, Message> {
    let glyph: Element<'a, Message> = match header.logo {
        Some(logo) => iced::widget::image(logo).width(ICON).height(ICON).into(),
        None => iced::widget::text(header.glyph)
            .font(super::icon::FONT)
            .size(ICON)
            .color(c().accent)
            .into(),
    };
    let heading = row![glyph, text(Role::PAGE_TITLE, header.title)]
        .push(header.chip.map(|chip| super::chip::pill(chip, false, None)))
        .spacing(ICON_GAP)
        .align_y(Center);
    let heading = mouse_area(container(heading).width(Fill).center_y(TOP))
        .on_press(on(Event::Grab(Drag::Move)))
        .interaction(mouse::Interaction::Grab);
    let top = row![heading, close].align_y(Center);
    let mut parts = column![top, container(body).height(Fill)].spacing(UNDER_HEADING);
    if let Some(footer) = footer {
        let above = ABOVE_ACTIONS - UNDER_HEADING;
        parts = parts.push(container(footer).padding(iced::Padding::ZERO.top(above)));
    }
    let pad = iced::Padding::new(PAD)
        .top(PAD - CLOSE_IN)
        .right(PAD - CLOSE_IN);
    let card = surface(Material::Widget, CARD_RADIUS, parts)
        .padding(pad)
        .width(at.width)
        .height(at.height);
    // the inner edge: a thin strip, its grip only while hovered
    let side = side(at, screen);
    let (strip, grip, cursor, place) = match side {
        Side::Left | Side::Right => (
            Size::new(EDGE, at.height - 2.0 * CARD_RADIUS),
            Size::new(GRIP_GIRTH, GRIP),
            mouse::Interaction::ResizingHorizontally,
            match side {
                Side::Left => Point::new(0.0, CARD_RADIUS),
                _ => Point::new(at.width - EDGE, CARD_RADIUS),
            },
        ),
        Side::Top => (
            Size::new(at.width - 2.0 * CARD_RADIUS, EDGE),
            Size::new(GRIP, GRIP_GIRTH),
            mouse::Interaction::ResizingVertically,
            Point::new(CARD_RADIUS, 0.0),
        ),
    };
    let grip: Element<'a, Message> = match hover {
        true => container(Space::new().width(grip.width).height(grip.height))
            .style(|_| container::Style {
                background: Some(c().text_muted.into()),
                border: Border::default().rounded(GRIP_GIRTH / 2.0),
                ..container::Style::default()
            })
            .into(),
        false => Space::new().into(),
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
    // In layers of its own bounds (an empty first child: a stack draws the
    // rest in layers clipped to its container): dragged, the card redraws as
    // its old and new place, not as each of its parts.
    let edge = match header.docked {
        true => None,
        false => Some(pin(edge).position(place)),
    };
    let layers = stack![Space::new(), opaque(card)]
        .push(edge)
        .width(at.width)
        .height(at.height);
    let layers = container(layers).width(at.width).height(at.height);
    pin(layers.clip(true)).position(at.position()).into()
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
        // a small screen: at least SIDE_WIDTH wide
        let small = Size::new(1000.0, 700.0);
        assert_eq!(place(small, None, None).width, SIDE_WIDTH);
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
    fn the_chat_docks_left_or_right_at_the_designs_size() {
        // 1920: 35 % = 672, capped at 640; 95 % of the height over a 48 px taskbar
        let right = docked(SCREEN, Side::Right, 48.0);
        assert_eq!(right.size(), Size::new(640.0, (1080.0 - 48.0) * 0.95));
        assert_eq!((right.x + right.width, right.y), (1920.0 - 16.0, 14.0));
        let left = docked(SCREEN, Side::Left, 48.0);
        assert_eq!((left.x, left.width), (16.0, 640.0));
        // 1440: 504 wide (the design's measure); a small screen: 340, inside it
        assert_eq!(
            docked(Size::new(1440.0, 900.0), Side::Left, 48.0).width,
            504.0
        );
        assert_eq!(
            docked(Size::new(800.0, 600.0), Side::Left, 0.0).width,
            340.0
        );
        let tiny = docked(Size::new(300.0, 200.0), Side::Right, 0.0);
        assert!(tiny.x >= 0.0 && tiny.y + tiny.height <= 200.0, "{tiny:?}");
        // let go: it docks to the side its middle is on
        let dragged = right + Vector::new(-500.0, 30.0);
        assert_eq!(
            dock_side(dragged, SCREEN),
            Side::Right,
            "a short drag stays"
        );
        let dragged = right + Vector::new(-1000.0, 30.0);
        assert_eq!(dock_side(dragged, SCREEN), Side::Left);
        assert_eq!(
            dock_side(left + Vector::new(1200.0, 0.0), SCREEN),
            Side::Right
        );
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
