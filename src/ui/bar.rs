//! Floating action bar: the action registry as one row of buttons, under the selection.
use iced::widget::{column, opaque, pin, row, tooltip};
use iced::{Center, Element, Fill, Point, Rectangle, Size, Vector};

use super::widgets::{self, CONTROL, SURFACE_PADDING, TEXT_BUTTON_WIDTH};
use crate::actions::{ACTIONS, Action, Key, qr};

const SPACING: f32 = 4.0;
/// Between the bar and the selection, and the bar's minimum distance to screen edges.
pub(super) const GAP: f32 = 16.0;

/// Between the bar and the QR chip.
const CHIP_GAP: f32 = 8.0;
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

/// The bar for `selection` (logical px), kept inside `room` (the overlay minus
/// a result card and the ask bar), with the QR `chip` (see [`qr_chip`]) next
/// to it, on the side away from the selection. Too narrow a room: the buttons
/// wrap onto more rows.
pub fn view<'a, Message: Clone + 'a>(
    selection: Rectangle,
    room: Rectangle,
    chip: Option<Element<'a, Message>>,
    on_press: fn(&'static Action) -> Message,
) -> Element<'a, Message> {
    let (per_row, mut total) = fit(ACTIONS.len(), room.width - 2.0 * GAP);
    let bar_width = total.width;
    if chip.is_some() {
        total.height += CONTROL + 2.0 * SURFACE_PADDING + CHIP_GAP;
    }
    let at = place_in(selection, total, room);
    // Tooltips go on the side away from the selection.
    let tip_side = if at.y < selection.y {
        tooltip::Position::Top
    } else {
        tooltip::Position::Bottom
    };
    // The accent button is what Enter runs.
    let default = crate::actions::default_for(&crate::config::current()).id;
    let buttons = ACTIONS.chunks(per_row).map(|line| {
        let line = line.iter().map(|action| {
            let button = widgets::text_button(
                action.icon,
                action.label,
                key_hint(action.key),
                action.id == default,
                on_press(action),
            );
            widgets::with_tip(button, action.tip, tip_side).into()
        });
        row(line).spacing(SPACING).into()
    });
    let rows = column(buttons).spacing(SPACING);
    let bar = Element::from(widgets::surface(rows));
    let stacked = match chip {
        None => bar,
        Some(chip) => {
            let parts = if at.y < selection.y {
                [chip, bar]
            } else {
                [bar, chip]
            };
            column(parts)
                .spacing(CHIP_GAP)
                .width(bar_width)
                .align_x(Center)
                .into()
        }
    };
    // opaque: a press between two buttons doesn't start a new selection.
    pin(opaque(stacked)).position(at).into()
}

/// What the button shows for its key, e.g. "Ctrl C".
fn key_hint(key: Key) -> String {
    let letter = key.letter.to_ascii_uppercase();
    if key.ctrl {
        format!("Ctrl {letter}")
    } else {
        letter.to_string()
    }
}

/// How many of `buttons` text buttons (they share one width) go in a row no
/// wider than `width` (at least one), and the bar's size in such rows.
fn fit(buttons: usize, width: f32) -> (usize, Size) {
    let fits = (width - 2.0 * SURFACE_PADDING + SPACING) / (TEXT_BUTTON_WIDTH + SPACING);
    let per = (fits.floor().max(1.0) as usize).min(buttons.max(1));
    let rows = buttons.max(1).div_ceil(per);
    let (n, r) = (per as f32, rows as f32);
    let size = Size::new(
        n * TEXT_BUTTON_WIDTH + (n - 1.0) * SPACING + 2.0 * SURFACE_PADDING,
        r * CONTROL + (r - 1.0) * SPACING + 2.0 * SURFACE_PADDING,
    );
    (per, size)
}

/// `place`, inside `room` instead of the whole screen.
pub(super) fn place_in(selection: Rectangle, bar: Size, room: Rectangle) -> Point {
    let offset = Vector::new(room.x, room.y);
    place(selection - offset, bar, room.size()) + offset
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

    #[test]
    fn the_bar_keeps_out_of_a_result_card_and_wraps_when_narrow() {
        let full = fit(ACTIONS.len(), 1920.0 - 2.0 * GAP);
        assert_eq!(full.0, ACTIONS.len(), "one row on a wide screen");
        // a card on the right (40 % wide): the room is what is left of it
        let card = Rectangle::new(Point::new(1132.0, 20.0), Size::new(768.0, 1040.0));
        let room = Rectangle::new(Point::ORIGIN, Size::new(card.x - 20.0, 1080.0));
        for selection in [
            sel(100.0, 200.0, 300.0, 200.0),  // well left of the card
            sel(800.0, 200.0, 300.0, 200.0),  // right up to the card's side
            sel(1000.0, 900.0, 200.0, 100.0), // low, by the card
            sel(1300.0, 300.0, 200.0, 100.0), // under the card (it was moved there)
        ] {
            let (per, size) = fit(ACTIONS.len(), room.width - 2.0 * GAP);
            let at = place_in(selection, size, room);
            let bar = Rectangle::new(at, size);
            assert!(
                bar.intersection(&card).is_none(),
                "{selection:?} -> {bar:?}"
            );
            assert!(bar.x >= room.x + GAP - 0.01 && bar.x + bar.width <= room.x + room.width);
            assert_eq!(per, ACTIONS.len(), "7 buttons fit 1072 px");
        }
        // a wider card: the room is 800 px, the buttons wrap onto two rows
        let room = Rectangle::new(Point::ORIGIN, Size::new(800.0, 1080.0));
        let (per, size) = fit(ACTIONS.len(), room.width - 2.0 * GAP);
        assert!(per < ACTIONS.len() && size.width <= room.width - 2.0 * GAP);
        assert!(size.height > 2.0 * CONTROL);
        // a card on the left: the room starts right of it
        let left = Rectangle::new(Point::new(20.0, 20.0), Size::new(768.0, 1040.0));
        let room = Rectangle::new(Point::new(808.0, 0.0), Size::new(1112.0, 1080.0));
        let (_, size) = fit(ACTIONS.len(), room.width - 2.0 * GAP);
        let at = place_in(sel(700.0, 300.0, 300.0, 100.0), size, room);
        assert!(Rectangle::new(at, size).intersection(&left).is_none());
        // very narrow: one button a row, never wider than the room
        let (per, size) = fit(ACTIONS.len(), 150.0);
        assert_eq!(per, 1);
        assert!(size.height > 6.0 * CONTROL);
    }

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
    fn qr_chip_offers_copy_and_open_only_for_links() {
        let screen = Size::new(1200.0, 420.0);
        let selection = sel(450.0, 60.0, 300.0, 140.0);
        let link = "https://example.com/a-long-link/that/goes/on/and/on/forever";
        let with_link = || {
            let chip = qr_chip(link, (), Some(()));
            view(selection, Rectangle::with_size(screen), Some(chip), |_| ())
        };
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
        let texts = widgets::shot::texts(view(
            selection,
            Rectangle::with_size(screen),
            Some(wifi),
            |_| (),
        ));
        assert!(texts.iter().any(|t| t.starts_with("QR code: WIFI:S:cafe")));
        assert!(!texts.iter().any(|t| t.starts_with("Open link")));
        // Text-direction controls: dropped from the chip, and a warning beside it.
        let rlo = qr_chip("https://evil.com/\u{202e}moc.elgoog", (), None);
        let texts = widgets::shot::texts(view(
            selection,
            Rectangle::with_size(screen),
            Some(rlo),
            |_| (),
        ));
        assert!(
            texts.contains(&"QR code: https://evil.com/moc.elgoog… (copies 28 chars)".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Hidden text-direction marks".to_string()));
        assert!(!texts.iter().any(|t| t.contains('\u{202e}')));
        // No QR code: the bar alone.
        let texts = widgets::shot::texts(view(
            selection,
            Rectangle::with_size(screen),
            None::<Element<()>>,
            |_| (),
        ));
        assert!(!texts.iter().any(|t| t.starts_with("QR code")));
    }

    #[test]
    fn key_hints_read_like_shortcuts() {
        assert_eq!(
            key_hint(Key {
                ctrl: true,
                letter: 's'
            }),
            "Ctrl S"
        );
        assert_eq!(
            key_hint(Key {
                ctrl: false,
                letter: 'a'
            }),
            "A"
        );
    }
}
