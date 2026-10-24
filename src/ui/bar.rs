//! Floating action bar: the action registry as one row of buttons, under the selection.
use iced::widget::{column, opaque, pin, row, tooltip};
use iced::{Center, Element, Fill, Point, Rectangle, Size};

use super::widgets::{self, CONTROL, SURFACE_PADDING, TEXT_BUTTON_WIDTH};
use crate::actions::entities::shorten;
use crate::actions::{ACTIONS, Action, Key};

const SPACING: f32 = 4.0;
/// Between the bar and the selection, and the bar's minimum distance to screen edges.
pub(super) const GAP: f32 = 16.0;

/// Between the bar and the QR chip.
const CHIP_GAP: f32 = 8.0;
/// The longest QR text the chip shows, in characters.
const QR_SHOWN: usize = 36;

/// The chip for a QR code found in the selection: its text (shortened) copies it,
/// and "Open link" (only offered for an http(s) link) opens it. Neither happens
/// by itself.
pub fn qr_chip<'a, Message: Clone + 'a>(
    text: &str,
    copy: Message,
    open: Option<Message>,
) -> Element<'a, Message> {
    let mut chips = row![
        widgets::icon(crate::actions::copy::ACTION.icon),
        widgets::chip(format!("QR code: {}", shorten(text.trim(), QR_SHOWN)), copy),
    ]
    .spacing(10)
    .height(Fill)
    .align_y(Center);
    if let Some(open) = open {
        chips = chips.push(widgets::chip("Open link", open));
    }
    widgets::surface(chips)
        .height(CONTROL + 2.0 * SURFACE_PADDING)
        .padding([0.0, 14.0])
        .into()
}

/// The bar for `selection` on a `screen`-sized overlay (logical px), with the QR
/// `chip` (see [`qr_chip`]) next to it, on the side away from the selection.
pub fn view<'a, Message: Clone + 'a>(
    selection: Rectangle,
    screen: Size,
    chip: Option<Element<'a, Message>>,
    on_press: fn(&'static Action) -> Message,
) -> Element<'a, Message> {
    let mut total = size(ACTIONS.len());
    let bar_width = total.width;
    if chip.is_some() {
        total.height = 2.0 * total.height + CHIP_GAP;
    }
    let at = place(selection, total, screen);
    // Tooltips go on the side away from the selection.
    let tip_side = if at.y < selection.y {
        tooltip::Position::Top
    } else {
        tooltip::Position::Bottom
    };
    // The accent button is what Enter runs.
    let default = crate::actions::default_for(&crate::config::current()).id;
    let buttons = ACTIONS.iter().map(|action| {
        let button = widgets::text_button(
            action.icon,
            action.label,
            key_hint(action.key),
            action.id == default,
            on_press(action),
        );
        widgets::with_tip(button, action.tip, tip_side).into()
    });
    let bar = Element::from(widgets::surface(row(buttons).spacing(SPACING)));
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

/// The bar's size with `buttons` text buttons (they share one width).
fn size(buttons: usize) -> Size {
    let n = buttons as f32;
    Size::new(
        n * TEXT_BUTTON_WIDTH + (n - 1.0).max(0.0) * SPACING + 2.0 * SURFACE_PADDING,
        CONTROL + 2.0 * SURFACE_PADDING,
    )
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
    fn qr_chip_offers_copy_and_open_only_for_links() {
        let screen = Size::new(1200.0, 420.0);
        let selection = sel(450.0, 60.0, 300.0, 140.0);
        let link = "https://example.com/a-long-link/that/goes/on/and/on/forever";
        let with_link = || {
            let chip = qr_chip(link, (), Some(()));
            view(selection, screen, Some(chip), |_| ())
        };
        let texts = widgets::shot::texts(with_link());
        assert!(
            texts.contains(&"QR code: https://example.com/a-long-link/that…".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Open link".to_string()));
        let px = widgets::shot::save("bar-qr-chip", with_link(), screen);
        assert!(px.as_chunks::<4>().0.iter().any(|p| p[..] != px[..4]));

        let wifi = qr_chip("WIFI:S:cafe;T:WPA;P:secret;;", (), None);
        let texts = widgets::shot::texts(view(selection, screen, Some(wifi), |_| ()));
        assert!(texts.iter().any(|t| t.starts_with("QR code: WIFI:S:cafe")));
        assert!(!texts.contains(&"Open link".to_string()));
        // No QR code: the bar alone.
        let texts = widgets::shot::texts(view(selection, screen, None::<Element<()>>, |_| ()));
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
