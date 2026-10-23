//! Floating action bar: the action registry as one row of buttons, under the selection.
use iced::widget::{opaque, pin, row, tooltip};
use iced::{Element, Point, Rectangle, Size};

use super::widgets::{self, CONTROL, SURFACE_PADDING, TEXT_BUTTON_WIDTH};
use crate::actions::{ACTIONS, Action, Key};

const SPACING: f32 = 4.0;
/// Between the bar and the selection, and the bar's minimum distance to screen edges.
pub(super) const GAP: f32 = 16.0;

/// The bar for `selection` on a `screen`-sized overlay (logical px).
pub fn view<'a, Message: Clone + 'a>(
    selection: Rectangle,
    screen: Size,
    on_press: fn(&'static Action) -> Message,
) -> Element<'a, Message> {
    let at = place(selection, size(ACTIONS.len()), screen);
    // Tooltips go on the side away from the selection.
    let tip_side = if at.y < selection.y {
        tooltip::Position::Top
    } else {
        tooltip::Position::Bottom
    };
    let buttons = ACTIONS.iter().enumerate().map(|(i, action)| {
        let button = widgets::text_button(
            action.icon,
            action.label,
            key_hint(action.key),
            i == 0,
            on_press(action),
        );
        widgets::with_tip(button, action.tip, tip_side).into()
    });
    let bar = widgets::surface(row(buttons).spacing(SPACING));
    // opaque: a press between two buttons doesn't start a new selection.
    pin(opaque(bar)).position(at).into()
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
