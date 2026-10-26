//! The overlay's action bar geometry (the one action hub, built in `ui::App`):
//! its size and its docking spots (anchors) inside the work area: bottom
//! centre (default), top centre, bottom left, bottom right. A drag ends at the
//! nearest one; a selection under it sends it to the nearest free one. Also
//! the QR chip and placing small things beside a selection.
use iced::widget::row;
use iced::{Center, Element, Fill, Point, Rectangle, Size};

use super::widgets::{self, CONTROL, SURFACE_PADDING};
use crate::actions::{Key, qr};
use crate::config::BarAnchor;

/// Between a bar or chip and the selection, and the minimum distance to screen edges.
pub(super) const GAP: f32 = 16.0;

/// The bar: its gap to the work area's edges (above the taskbar), its top
/// spot's distance from the top (clear of the hint pill), the space between
/// its controls, its drag handle's and AI field's widths, and its three pills'
/// widths (Search in browser, Copy text, Translate: fixed, so the bar never
/// changes size).
pub(super) const BOTTOM_GAP: f32 = 28.0;
pub(super) const TOP: f32 = 84.0;
pub(super) const SPACING: f32 = 6.0;
pub(super) const HANDLE: f32 = 24.0;
pub(super) const FIELD: f32 = 320.0;
const MIN_FIELD: f32 = 160.0;
pub(super) const PILLS: [f32; 3] = [160.0, 106.0, 106.0];
/// The thin line between the field, the pills and the icon buttons.
const DIVIDER: f32 = 1.0;

/// The longest QR text the chip shows, in characters.
const QR_SHOWN: usize = 36;

/// The chip for a QR code found in the selection: its text (made safe to show, see
/// [`qr::shown`]) copies it, and "Open link · host" (the link's whole host)
/// (only offered for an http(s) link) opens it. Neither happens by itself.
pub fn qr_chip<'a, Message: Clone + 'a>(
    text: &str,
    copy: Message,
    open: Option<Message>,
) -> Element<'a, Message> {
    let shown = qr::shown(text, QR_SHOWN);
    let mut chips = row![
        widgets::icon(crate::actions::copy::ACTION.icon),
        widgets::chip(format!("QR code: {}", shown.text), copy),
    ]
    .spacing(10)
    .height(Fill)
    .align_y(Center);
    if shown.warn {
        chips = chips.push(widgets::error("Hidden text-direction marks"));
    }
    let host = qr::web_url(text).and_then(crate::lens::web_host);
    if let (Some(open), Some(host)) = (open, host) {
        // The whole host on the button (like the Text panel's link chips): the
        // site it really opens, never cut.
        chips = chips.push(widgets::chip(format!("Open link · {host}"), open));
    }
    widgets::surface(chips)
        .height(CONTROL + 2.0 * SURFACE_PADDING)
        .padding([0.0, 14.0])
        .into()
}

/// A shortcut as a tooltip shows it, e.g. "Ctrl+C".
pub(super) fn key_hint(key: Key) -> String {
    let letter = key.letter.to_ascii_uppercase();
    if key.ctrl {
        format!("Ctrl+{letter}")
    } else {
        letter.to_string()
    }
}

/// The bar without its field: handle, dividers, `pills` and `bubbles` round buttons.
fn fixed(pills: [f32; 3], bubbles: f32) -> f32 {
    let pills: f32 = pills.iter().map(|w| w + SPACING).sum();
    let buttons = bubbles * (widgets::BUBBLE + SPACING);
    let dividers = (1.0 + f32::from(bubbles > 0.0)) * (DIVIDER + SPACING);
    HANDLE + SPACING + dividers + pills + buttons + 2.0 * SURFACE_PADDING
}

/// The pills' widths on a screen `width` wide: labelled (`PILLS`), or icons
/// only (`BUBBLE` each) where the labelled bar would not fit (the design hides
/// their labels on narrow screens).
pub(super) fn pill_widths(width: f32, bubbles: f32) -> [f32; 3] {
    match fixed(PILLS, bubbles) + MIN_FIELD <= width - 2.0 * GAP {
        true => PILLS,
        false => [widgets::BUBBLE; 3],
    }
}

/// The bottom bar's field width and whole size on a `screen` that is `width`
/// wide, with the three pills (`pill_widths`) and `bubbles` round buttons
/// after them: the field gives up width on a narrow screen, then the pills
/// their labels; the buttons never shrink.
pub(super) fn bottom_size(width: f32, bubbles: f32) -> (f32, Size) {
    let fixed = fixed(pill_widths(width, bubbles), bubbles);
    let field = (width - 2.0 * GAP - fixed).clamp(MIN_FIELD, FIELD);
    let height = CONTROL.max(widgets::BUBBLE) + 2.0 * SURFACE_PADDING;
    (field, Size::new(fixed + field, height))
}

/// Every spot the bar docks at.
pub(super) const ANCHORS: [BarAnchor; 4] = [
    BarAnchor::Bottom,
    BarAnchor::Top,
    BarAnchor::BottomLeft,
    BarAnchor::BottomRight,
];

/// The bar's top-left at `anchor` on `screen` (`size`), inside the work area
/// (`inset`: how much of the screen's bottom the taskbar takes).
pub(super) fn anchor_at(anchor: BarAnchor, screen: Size, size: Size, inset: f32) -> Point {
    let bottom = screen.height - inset - BOTTOM_GAP - size.height;
    let centre = (screen.width - size.width) / 2.0;
    let (x, y) = match anchor {
        BarAnchor::Bottom => (centre, bottom),
        BarAnchor::Top => (centre, TOP),
        BarAnchor::BottomLeft => (BOTTOM_GAP, bottom),
        BarAnchor::BottomRight => (screen.width - BOTTOM_GAP - size.width, bottom),
    };
    Point::new(x.max(GAP), y.max(GAP))
}

/// The anchor nearest to a bar let go at top-left `at`.
pub(super) fn nearest(at: Point, screen: Size, size: Size, inset: f32) -> BarAnchor {
    let off = |a: BarAnchor| anchor_at(a, screen, size, inset).distance(at);
    let mut all = ANCHORS;
    all.sort_by(|a, b| off(*a).total_cmp(&off(*b)));
    all[0]
}

/// Where the bar goes: `chosen`, unless it would cover something `blocked`
/// (the selection): then the free anchor nearest to it; none free, `chosen`.
pub(super) fn free(
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
pub(super) fn place(selection: Rectangle, bar: Size, screen: Size) -> Point {
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
        let (field, s) = bottom_size(900.0, 3.0);
        assert!(
            field < FIELD && s.width <= 900.0 - 2.0 * GAP,
            "{field} {s:?}"
        );
    }

    #[test]
    fn qr_chip_offers_copy_and_open_only_for_links() {
        let screen = Size::new(1200.0, 420.0);
        let link = "https://example.com/a-long-link/that/goes/on/and/on/forever";
        let with_link = || qr_chip(link, (), Some(()));
        let texts = widgets::shot::texts(with_link());
        assert!(
            texts.contains(
                &"QR code: https://example.com/a-long-link/that… (copies 59 chars)".to_string()
            ),
            "{texts:?}"
        );
        // The whole host, never cut, on the button that opens it.
        assert!(
            texts.contains(&"Open link · example.com".to_string()),
            "{texts:?}"
        );
        let px = widgets::shot::save("bar-qr-chip", with_link(), screen);
        assert!(px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4]));

        let wifi = qr_chip("WIFI:S:cafe;T:WPA;P:secret;;", (), None);
        let texts = widgets::shot::texts(wifi);
        assert!(texts.iter().any(|t| t.starts_with("QR code: WIFI:S:cafe")));
        assert!(!texts.iter().any(|t| t.starts_with("Open link")));
        // Text-direction controls: dropped from the chip, and a warning beside it.
        let rlo = qr_chip("https://evil.com/\u{202e}moc.elgoog", (), None);
        let texts = widgets::shot::texts(rlo);
        assert!(
            texts.contains(&"QR code: https://evil.com/moc.elgoog… (copies 28 chars)".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Hidden text-direction marks".to_string()));
        assert!(!texts.iter().any(|t| t.contains('\u{202e}')));
    }

    #[test]
    fn key_hints_read_like_shortcuts() {
        let key = |ctrl, letter| Key { ctrl, letter };
        assert_eq!(key_hint(key(true, 's')), "Ctrl+S");
        assert_eq!(key_hint(key(false, 'a')), "A");
    }
}
