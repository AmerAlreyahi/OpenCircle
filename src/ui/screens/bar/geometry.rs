//! The overlay's bottom bar geometry (the one action hub, `view`): its size
//! and its docking spots (anchors) inside the work area: bottom centre
//! (default), top centre, bottom left, bottom right. A drag ends at the
//! nearest one; a selection under it sends it to the nearest free one. Also
//! placing small things beside a selection, and the Tab order.
use iced::{Point, Rectangle, Size};

use crate::actions::Key;
use crate::config::BarAnchor;
use crate::labs::BarButton;
use crate::ui::kit::tokens::{size, space};
use crate::ui::kit::{field, grip};

/// Between a bar or chip and the selection, and the minimum distance to screen edges.
pub(in crate::ui) const GAP: f32 = 16.0;

/// The bar (the design's search bar, solid): its gap to the work area's edges
/// (above the taskbar), its top spot's distance from the top (clear of the
/// hint pill), its padding (the design's 6 px inside its 1 px edge), the
/// space between its controls and between its pills, its grip's width, its AI
/// field's width with the focus ring, its three pills' widths (Search in
/// browser, Copy text, Translate: fixed, so the bar never changes size), and
/// a pill's width with its icon alone (narrow screens).
pub(in crate::ui) const BOTTOM_GAP: f32 = 28.0;
pub(in crate::ui) const TOP: f32 = 84.0;
pub(in crate::ui) const PAD: f32 = space::BAR_PAD + 1.0;
pub(in crate::ui) const SPACING: f32 = space::GAP;
pub(in crate::ui) const PILL_GAP: f32 = 4.0;
pub(in crate::ui) const HANDLE: f32 = grip::SIZE.width;
pub(in crate::ui) const FIELD: f32 = 270.0 + 2.0 * field::RING;
const MIN_FIELD: f32 = 160.0;
pub(in crate::ui) const PILLS: [f32; 3] = [132.0, 88.0, 88.0];
pub(in crate::ui) const ICON_PILL: f32 = 36.0;
/// The round lab buttons after the pills.
pub(in crate::ui) const BUBBLE: f32 = size::ICON_BUTTON;
/// The thin line between the field, the pills and the icon buttons (16 px
/// tall), and the room it takes (2 px either side).
pub(in crate::ui) const DIVIDER_H: f32 = 16.0;
pub(in crate::ui) const DIVIDER: f32 = 5.0;
/// The bar's height: its tallest control (the icon buttons) and its padding.
pub(in crate::ui) const HEIGHT: f32 = BUBBLE + 2.0 * PAD;
/// The QR code chip: as tall as the bar; its padding across and the space
/// between its parts; the longest QR text it shows, in characters.
pub(in crate::ui) const QR_PAD: f32 = 14.0;
pub(in crate::ui) const QR_GAP: f32 = 10.0;
pub(in crate::ui) const QR_SHOWN: usize = 36;

/// The room the field's text has, `field` wide (its slots and ring aside).
pub(in crate::ui) fn text_room(field: f32) -> f32 {
    field - 2.0 * field::RING - field::SLOTS_LEFT - field::SLOTS_RIGHT
}

/// What Tab can land on in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The logo button (opens the AI chat).
    Logo,
    /// The AI field.
    Field,
    /// The Include screen switch.
    Include,
    /// The "AI ↵" badge.
    Badge,
    Button(BarButton),
}

/// The Tab order, as the design's DOM: logo, AI field, AI badge, the pills,
/// then the icon buttons (`shown`, in bar order). A dimmed pill (Search in
/// browser and Copy text before a selection) is skipped, as a disabled
/// control is.
pub fn stops(selected: bool, shown: &[BarButton]) -> Vec<Stop> {
    let live = |b: &BarButton| selected || !matches!(b, BarButton::Search | BarButton::CopyText);
    let pills = shown.iter().filter(|b| b.is_pill() && live(b));
    let icons = shown.iter().filter(|b| !b.is_pill());
    [Stop::Logo, Stop::Field, Stop::Include, Stop::Badge]
        .into_iter()
        .chain(pills.chain(icons).map(|b| Stop::Button(*b)))
        .collect()
}

/// Where Tab (or Shift+Tab, `back`) goes from `at` (none yet: the first stop
/// going forward, the last going back); it wraps.
pub fn tab(stops: &[Stop], at: Option<Stop>, back: bool) -> Option<Stop> {
    let n = stops.len();
    let i = match at.and_then(|a| stops.iter().position(|s| *s == a)) {
        Some(i) if back => (i + n - 1) % n,
        Some(i) => (i + 1) % n,
        None if back => n.checked_sub(1)?,
        None => 0,
    };
    stops.get(i).copied()
}

/// A shortcut as a tooltip shows it, e.g. "Ctrl+C".
pub(in crate::ui) fn key_hint(key: Key) -> String {
    let letter = key.letter.to_ascii_uppercase();
    if key.ctrl {
        format!("Ctrl+{letter}")
    } else {
        letter.to_string()
    }
}

/// The bar without its field: grip, dividers, `pills` and `bubbles` round buttons.
fn fixed(pills: [f32; 3], bubbles: f32) -> f32 {
    let pills: f32 = pills.iter().sum::<f32>() + 2.0 * PILL_GAP;
    let buttons = match bubbles > 0.0 {
        true => SPACING + DIVIDER + bubbles * (BUBBLE + SPACING),
        false => 0.0,
    };
    2.0 * PAD + HANDLE + 3.0 * SPACING + DIVIDER + pills + buttons
}

/// The pills' widths on a screen `width` wide: labelled (`PILLS`), or icons
/// only (`ICON_PILL` each) where the labelled bar would not fit (the design
/// hides their labels on narrow screens).
pub(in crate::ui) fn pill_widths(width: f32, bubbles: f32) -> [f32; 3] {
    match fixed(PILLS, bubbles) + MIN_FIELD <= width - 2.0 * GAP {
        true => PILLS,
        false => [ICON_PILL; 3],
    }
}

/// The bottom bar's field width and whole size on a `screen` that is `width`
/// wide, with the three pills (`pill_widths`) and `bubbles` round buttons
/// after them: the field gives up width on a narrow screen, then the pills
/// their labels; the buttons never shrink.
pub(in crate::ui) fn bottom_size(width: f32, bubbles: f32) -> (f32, Size) {
    let fixed = fixed(pill_widths(width, bubbles), bubbles);
    let field = (width - 2.0 * GAP - fixed).clamp(MIN_FIELD, FIELD);
    (field, Size::new(fixed + field, HEIGHT))
}

/// How long a held bar takes to catch up with the pointer (95 % of the way).
pub(in crate::ui) const FOLLOW: std::time::Duration = std::time::Duration::from_millis(50);

/// A held bar drawn at `at`, `since` the last frame, eased toward `to` (where
/// the pointer holds it): critically damped, no overshoot, by the time passed
/// (a late frame catches up more), so uneven mouse events still draw an even
/// glide; within half a pixel it is there.
pub(in crate::ui) fn follow(at: Point, to: Point, since: std::time::Duration) -> Point {
    // 95 % in FOLLOW: e^-3 = 5 %
    let left = (-3.0 * since.as_secs_f32() / FOLLOW.as_secs_f32()).exp();
    let p = to + (at - to) * left;
    match p.distance(to) < 0.5 {
        true => to,
        false => p,
    }
}

/// Every spot the bar docks at.
pub(in crate::ui) const ANCHORS: [BarAnchor; 6] = [
    BarAnchor::Bottom,
    BarAnchor::Top,
    BarAnchor::BottomLeft,
    BarAnchor::BottomRight,
    BarAnchor::TopLeft,
    BarAnchor::TopRight,
];

/// The bar's top-left at `anchor` on `screen` (`size`), inside the work area
/// (`inset`: how much of the screen's bottom the taskbar takes).
pub(in crate::ui) fn anchor_at(anchor: BarAnchor, screen: Size, size: Size, inset: f32) -> Point {
    let bottom = screen.height - inset - BOTTOM_GAP - size.height;
    let centre = (screen.width - size.width) / 2.0;
    let (x, y) = match anchor {
        BarAnchor::Bottom => (centre, bottom),
        BarAnchor::Top => (centre, TOP),
        BarAnchor::BottomLeft => (BOTTOM_GAP, bottom),
        BarAnchor::BottomRight => (screen.width - BOTTOM_GAP - size.width, bottom),
        BarAnchor::TopLeft => (BOTTOM_GAP, TOP),
        BarAnchor::TopRight => (screen.width - BOTTOM_GAP - size.width, TOP),
    };
    Point::new(x.max(GAP), y.max(GAP))
}

/// The anchor nearest to a bar let go at top-left `at`.
pub(in crate::ui) fn nearest(at: Point, screen: Size, size: Size, inset: f32) -> BarAnchor {
    let off = |a: BarAnchor| anchor_at(a, screen, size, inset).distance(at);
    let mut all = ANCHORS;
    all.sort_by(|a, b| off(*a).total_cmp(&off(*b)));
    all[0]
}

/// Where the bar goes: `chosen`, unless it would cover something `blocked`
/// (the selection): then the free anchor nearest to it; none free, `chosen`.
pub(in crate::ui) fn free(
    chosen: BarAnchor,
    blocked: &[Rectangle],
    screen: Size,
    size: Size,
    inset: f32,
) -> BarAnchor {
    let rect = |a| Rectangle::new(anchor_at(a, screen, size, inset), size);
    let clear = |a| blocked.iter().all(|b| !b.intersects(&rect(a)));
    let home = anchor_at(chosen, screen, size, inset);
    let mut all = ANCHORS;
    all.sort_by(|a, b| {
        let off = |x: BarAnchor| anchor_at(x, screen, size, inset).distance(home);
        off(*a).total_cmp(&off(*b))
    });
    all.into_iter().find(|a| clear(*a)).unwrap_or(chosen)
}

/// Top-left corner for a `bar`: centred under the selection, else above it, else
/// over its bottom edge (selection fills the screen). Always inside the screen,
/// `GAP` from its edges, unless the bar is bigger than the screen.
pub(in crate::ui) fn place(selection: Rectangle, bar: Size, screen: Size) -> Point {
    let x = (selection.center_x() - bar.width / 2.0)
        .min(screen.width - GAP - bar.width)
        .max(GAP);
    let below = selection.y + selection.height + GAP;
    let above = selection.y - GAP - bar.height;
    let y = if below + bar.height <= screen.height - GAP {
        below
    } else if above >= GAP {
        above
    } else {
        screen.height - GAP - bar.height
    };
    Point::new(x, y.max(GAP))
}

/// Where Snip's editing toolbar (`bar`) docks for the live area `area`
/// (logical px): centred under it, else above it, else inside its top edge,
/// else inside its bottom edge; on the screen of `screens` (work areas) where
/// the area leaves the most room above or below; never off that screen, and
/// never over `avoid` (the handle being dragged) while another spot is free.
pub(in crate::ui) fn dock(
    area: Rectangle,
    bar: Size,
    screens: &[Rectangle],
    avoid: Option<Point>,
) -> Point {
    let room = |s: &Rectangle| {
        let part = area.intersection(s)?;
        let below = s.y + s.height - (part.y + part.height);
        let above = part.y - s.y;
        Some((part, below.max(above)))
    };
    let (screen, part) = (screens.iter())
        .filter_map(|s| room(s).map(|(part, free)| (s, part, free)))
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(s, part, _)| (*s, part))
        .unwrap_or_else(|| {
            let s = screens.first().copied().unwrap_or(area);
            (s, area)
        });
    let (top, bottom) = (screen.y + GAP, screen.y + screen.height - GAP - bar.height);
    let x = (part.center_x() - bar.width / 2.0)
        .min(screen.x + screen.width - GAP - bar.width)
        .max(screen.x + GAP);
    let spots = [
        part.y + part.height + GAP,
        part.y - GAP - bar.height,
        part.y + GAP,
        part.y + part.height - GAP - bar.height,
    ];
    let fits = |y: f32| y >= top && y <= bottom;
    // The handle's own reach around it stays clear.
    let clear = |y: f32| {
        avoid.is_none_or(|p| {
            let r = Rectangle::new(Point::new(x, y), bar).expand(GAP);
            !r.contains(p)
        })
    };
    let y = (spots.iter().copied())
        .find(|&y| fits(y) && clear(y))
        .or_else(|| spots.iter().copied().find(|&y| fits(y)))
        .unwrap_or(bottom);
    Point::new(x, y.min(bottom).max(top))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size::new(1920.0, 1080.0);
    const BAR: Size = Size::new(400.0, 56.0);

    fn on_screen(p: Point) -> bool {
        p.x >= GAP
            && p.y >= GAP
            && p.x + BAR.width <= SCREEN.width - GAP
            && p.y + BAR.height <= SCREEN.height - GAP
    }

    fn sel(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn bar_goes_centred_under_the_selection() {
        let p = place(sel(800.0, 300.0, 200.0, 100.0), BAR, SCREEN);
        assert_eq!(p, Point::new(700.0, 416.0));
    }

    #[test]
    fn bar_goes_above_when_there_is_no_room_below() {
        let s = sel(800.0, 900.0, 200.0, 150.0);
        let p = place(s, BAR, SCREEN);
        assert_eq!(p.y, 900.0 - GAP - BAR.height);
        assert!(on_screen(p));
    }

    #[test]
    fn bar_stays_on_screen_everywhere() {
        let cases = [
            sel(0.0, 0.0, 30.0, 30.0),        // top-left corner
            sel(1890.0, 500.0, 30.0, 30.0),   // right edge
            sel(1890.0, 1050.0, 30.0, 30.0),  // bottom-right corner
            sel(0.0, 0.0, 1920.0, 1080.0),    // whole screen
            sel(-50.0, 1000.0, 100.0, 200.0), // partly off screen
            sel(100.0, 40.0, 300.0, 1000.0),  // tall: no room above or below
        ];
        for s in cases {
            let p = place(s, BAR, SCREEN);
            assert!(on_screen(p), "{s:?} -> {p:?}");
        }
    }

    #[test]
    fn the_bar_docks_at_anchors_above_the_taskbar() {
        let (field, size) = bottom_size(1920.0, 3.0);
        assert_eq!(field, FIELD);
        // default: centred, BOTTOM_GAP above the work area (a 48 px taskbar)
        let at = anchor_at(BarAnchor::Bottom, SCREEN, size, 48.0);
        assert_eq!(at.x, (1920.0 - size.width) / 2.0);
        assert_eq!(at.y + size.height, 1080.0 - 48.0 - BOTTOM_GAP);
        for a in ANCHORS {
            let r = Rectangle::new(anchor_at(a, SCREEN, size, 48.0), size);
            assert!(r.x >= GAP && r.x + r.width <= 1920.0 - GAP, "{a:?}");
            assert!(
                r.y >= GAP && r.y + r.height <= 1080.0 - 48.0,
                "{a:?} in the work area"
            );
        }
        // let go anywhere: the nearest anchor
        let near = |x, y| nearest(Point::new(x, y), SCREEN, size, 48.0);
        assert_eq!(near(700.0, 900.0), BarAnchor::Bottom);
        assert_eq!(near(800.0, 200.0), BarAnchor::Top);
        assert_eq!(near(10.0, 700.0), BarAnchor::BottomLeft);
        assert_eq!(near(1500.0, 1000.0), BarAnchor::BottomRight);
        assert_eq!(near(10.0, 100.0), BarAnchor::TopLeft);
        assert_eq!(near(1500.0, 50.0), BarAnchor::TopRight);
        // a selection where the bar is: the nearest free anchor; none in the way: home
        let home = Rectangle::new(anchor_at(BarAnchor::Bottom, SCREEN, size, 48.0), size);
        let under = Rectangle::new(
            Point::new(home.center_x() - 50.0, home.y - 30.0),
            Size::new(100.0, 60.0),
        );
        let moved = free(BarAnchor::Bottom, &[under], SCREEN, size, 48.0);
        assert_ne!(moved, BarAnchor::Bottom);
        let r = Rectangle::new(anchor_at(moved, SCREEN, size, 48.0), size);
        assert!(!r.intersects(&under));
        assert_eq!(
            free(BarAnchor::Bottom, &[], SCREEN, size, 48.0),
            BarAnchor::Bottom
        );
        // the whole screen selected: nowhere free, it stays
        let all = Rectangle::with_size(SCREEN);
        assert_eq!(
            free(BarAnchor::Top, &[all], SCREEN, size, 48.0),
            BarAnchor::Top
        );
        // a narrow screen: the field gives up width, the buttons keep theirs
        let (field, s) = bottom_size(760.0, 3.0);
        assert!(
            field < FIELD && s.width <= 760.0 - 2.0 * GAP,
            "{field} {s:?}"
        );
    }

    #[test]
    fn tab_order_is_the_designs_and_skips_dimmed_pills() {
        let all = crate::labs::bar(&crate::labs::Labs::all_on());
        let order = |selected| stops(selected, &all);
        let b = Stop::Button;
        assert_eq!(
            order(false),
            [
                Stop::Logo,
                Stop::Field,
                Stop::Include,
                Stop::Badge,
                b(BarButton::Translate),
                b(BarButton::Live),
                b(BarButton::Song),
                b(BarButton::Edit),
            ]
        );
        assert_eq!(
            order(true)[4..7],
            [
                b(BarButton::Search),
                b(BarButton::CopyText),
                b(BarButton::Translate),
            ]
        );
        // forward and back wrap; with no focus yet, first / last
        let s = order(false);
        assert_eq!(tab(&s, None, false), Some(Stop::Logo));
        assert_eq!(tab(&s, None, true), Some(b(BarButton::Edit)));
        assert_eq!(tab(&s, Some(b(BarButton::Edit)), false), Some(Stop::Logo));
        assert_eq!(tab(&s, Some(Stop::Logo), true), Some(b(BarButton::Edit)));
        // a stop that is gone (a pill dimmed meanwhile) starts over
        assert_eq!(tab(&s, Some(b(BarButton::Search)), false), Some(Stop::Logo));
    }

    #[test]
    fn a_held_bar_eases_to_the_pointer_by_the_clock() {
        use std::time::Duration;
        let (at, to) = (Point::new(0.0, 0.0), Point::new(100.0, -40.0));
        let ms = |n| Duration::from_millis(n);
        // most of the way in one frame, never past it, there by FOLLOW
        let one = follow(at, to, ms(16));
        assert!(
            one.x > 50.0 && one.x < 100.0 && one.y < -20.0 && one.y > -40.0,
            "{one:?}"
        );
        let mut p = at;
        for _ in 0..3 {
            p = follow(p, to, ms(16));
            assert!(p.x <= to.x && p.y >= to.y, "{p:?}");
        }
        assert!(p.distance(to) < 0.06 * at.distance(to), "{p:?}");
        // a late frame catches up as far as the frames it stood for
        let two = follow(follow(at, to, ms(16)), to, ms(16));
        assert!(follow(at, to, ms(32)).distance(two) < 0.01);
        // close enough: there, so the frame clock stops
        assert_eq!(follow(to + iced::Vector::new(0.3, 0.0), to, ms(16)), to);
    }

    #[test]
    fn key_hints_read_like_shortcuts() {
        let key = |ctrl, letter| Key { ctrl, letter };
        assert_eq!(key_hint(key(true, 's')), "Ctrl+S");
        assert_eq!(key_hint(key(false, 'a')), "A");
    }

    #[test]
    fn snips_toolbar_docks_below_then_above_then_inside_never_off_screen() {
        let work = [Rectangle::with_size(SCREEN)];
        let at = |a: Rectangle| dock(a, BAR, &work, None);
        // Room below: centred under the area.
        let a = sel(800.0, 300.0, 200.0, 100.0);
        assert_eq!(at(a), Point::new(700.0, 416.0));
        // None below: above.
        let low = sel(800.0, 950.0, 200.0, 100.0);
        assert_eq!(at(low).y, 950.0 - GAP - BAR.height);
        // As tall as the screen: inside its top edge.
        let tall = sel(100.0, 0.0, 600.0, 1080.0);
        assert_eq!(at(tall).y, GAP);
        // Corners and edges: always fully on the screen.
        for a in [
            sel(0.0, 0.0, 30.0, 30.0),
            sel(1890.0, 1050.0, 30.0, 30.0),
            sel(0.0, 0.0, 1920.0, 1080.0),
            sel(-300.0, 500.0, 320.0, 40.0),
        ] {
            assert!(on_screen(at(a)), "{a:?} -> {:?}", at(a));
        }
    }

    #[test]
    fn snips_toolbar_keeps_off_the_handle_being_dragged() {
        let work = [Rectangle::with_size(SCREEN)];
        let a = sel(800.0, 300.0, 200.0, 100.0);
        let below = dock(a, BAR, &work, None);
        // Dragging the bottom edge's handle down into where the toolbar sits.
        let handle = Point::new(900.0, 430.0);
        let moved = dock(a, BAR, &work, Some(handle));
        assert!(!Rectangle::new(moved, BAR).expand(GAP).contains(handle));
        assert_eq!(moved.y, 300.0 - GAP - BAR.height, "above instead");
        // A handle far away changes nothing.
        assert_eq!(dock(a, BAR, &work, Some(Point::new(10.0, 10.0))), below);
    }

    #[test]
    fn snips_toolbar_goes_to_the_screen_with_the_most_room() {
        // Two screens side by side; the area spans both, low on the left one
        // (little room) and high on the right one (lots below).
        let left = sel(0.0, 0.0, 1920.0, 1080.0);
        let right = sel(1920.0, -600.0, 1280.0, 1800.0);
        let a = sel(1500.0, 900.0, 800.0, 150.0);
        let p = dock(a, BAR, &[left, right], None);
        let r = Rectangle::new(p, BAR);
        assert!(r.x >= right.x + GAP && r.x + r.width <= right.x + right.width - GAP);
        assert_eq!(
            p.y,
            900.0 + 150.0 + GAP,
            "under the area on the right screen"
        );
    }
}
