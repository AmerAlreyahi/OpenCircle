//! The colour picker (N-75): the snip loupe follows the pointer (the pixel
//! grid, the crosshair, the true colour's HEX); a click copies HEX
//! (`#1E90FF`), Shift+click RGB (`rgb(30, 144, 255)`), Enter / Ctrl+C the
//! HEX where the arrows put the pointer (1 px, Shift 10 px); the last
//! `RECENT` colours sit in the toolbar, a click copies one again. They are
//! kept for this run of the app only (nothing is written).
use iced::widget::{Space, canvas, row};
use iced::{Color, Element, Fill, Point};

use super::{CLOSE, Msg};
use crate::ui::kit::chip;
use crate::ui::kit::colour;
use crate::ui::kit::divider;
use crate::ui::kit::icon_button::{self, State};
use crate::ui::kit::key_hint::Group;
use crate::ui::kit::loupe::Loupe;
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::text::sub;
use crate::ui::kit::tokens::{Role, radius, space};
use crate::ui::kit::tooltip::{Why, hint};
use crate::ui::screens::bar::geometry;

/// lucide "pipette" (the Snip toolbar's button).
pub const ICON: char = '\u{e13e}';
/// How many picked colours the toolbar keeps.
pub const RECENT: usize = 8;
const DIVIDER_H: f32 = 24.0;

/// The freeze hint while the picker starts.
pub const HINT: [Group<'static>; 4] = [
    (None, "Click copies HEX"),
    (Some("Shift"), "RGB"),
    (Some("← ↑ → ↓"), "1 px"),
    (Some("Esc"), "Close"),
];

/// `#RRGGBB`, upper case.
pub fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// `rgb(r, g, b)`, as CSS writes it.
pub fn rgb([r, g, b]: [u8; 3]) -> String {
    format!("rgb({r}, {g}, {b})")
}

/// The colour at `(x, y)` (frame px) of an RGBA frame; none off it.
pub fn colour_at(rgba: &[u8], width: u32, height: u32, (x, y): (i32, i32)) -> Option<[u8; 3]> {
    if !(0..width as i32).contains(&x) || !(0..height as i32).contains(&y) {
        return None;
    }
    let i = (y as usize * width as usize + x as usize) * 4;
    rgba.get(i..i + 3).map(|p| [p[0], p[1], p[2]])
}

/// `colour` picked: first in `recent`, once, at most `RECENT` kept.
pub fn remember(recent: &mut Vec<[u8; 3]>, colour: [u8; 3]) {
    recent.retain(|c| *c != colour);
    recent.insert(0, colour);
    recent.truncate(RECENT);
}

/// The loupe at `at` (this screen's logical px) over `frame`.
pub fn loupe<'a>(
    frame: &'a iced::widget::image::Handle,
    scale: f32,
    at: Point,
    look: (Color, Color),
) -> Element<'a, Msg> {
    let iced::widget::image::Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = frame
    else {
        return Space::new().into();
    };
    let loupe = Loupe {
        pixels,
        width: *width,
        height: *height,
        scale,
        at,
        outline: None,
        look,
        away: None,
    };
    canvas(loupe).width(Fill).height(Fill).into()
}

/// The toolbar: the colour under the pointer (`under`), the recent ones,
/// ×. `rtl`: mirrored.
pub fn toolbar<'a>(under: Option<[u8; 3]>, recent: &[[u8; 3]], rtl: bool) -> Element<'a, Msg> {
    let ar = |latin: &'a str, arabic: &'a str| if rtl { arabic } else { latin };
    let now = chip::readout(under.map_or_else(|| "#------".into(), hex), true, None);
    let mut parts: Vec<Element<'a, Msg>> = vec![now];
    match recent.is_empty() {
        true => parts.push(
            sub(
                Role::META,
                ar("Picked colours show here", "تظهر هنا الألوان المختارة"),
            )
            .into(),
        ),
        false => parts.push(colour::recent(recent, Msg::Recopy)),
    }
    let close = icon_button::square(CLOSE, State::default(), Some(Msg::Close));
    parts.extend([
        divider::vertical(DIVIDER_H),
        hint(
            close,
            ar("Close", "إغلاق"),
            Why::Keys("Esc"),
            iced::widget::tooltip::Position::Bottom,
        ),
    ]);
    if rtl {
        parts.reverse();
    }
    let line = iced::widget::Row::with_children(parts)
        .spacing(space::GAP)
        .padding([0.0, space::GAP])
        .align_y(iced::Center);
    surface(Material::Widget, radius::WINDOW, row![line])
        .padding(space::BAR_PAD)
        .height(geometry::HEIGHT)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_written_as_hex_and_rgb() {
        assert_eq!(hex([30, 144, 255]), "#1E90FF");
        assert_eq!(hex([0, 0, 0]), "#000000");
        assert_eq!(rgb([30, 144, 255]), "rgb(30, 144, 255)");
    }

    #[test]
    fn the_colour_is_read_at_its_frame_pixel() {
        // 2 x 2: red, green / blue, white
        let px = [
            255, 0, 0, 255, 0, 255, 0, 255, //
            0, 0, 255, 255, 255, 255, 255, 255,
        ];
        assert_eq!(colour_at(&px, 2, 2, (0, 0)), Some([255, 0, 0]));
        assert_eq!(colour_at(&px, 2, 2, (1, 0)), Some([0, 255, 0]));
        assert_eq!(colour_at(&px, 2, 2, (0, 1)), Some([0, 0, 255]));
        assert_eq!(colour_at(&px, 2, 2, (1, 1)), Some([255, 255, 255]));
        for off in [(-1, 0), (2, 0), (0, 2), (0, -1)] {
            assert_eq!(colour_at(&px, 2, 2, off), None, "{off:?}");
        }
    }

    #[test]
    fn recent_colours_are_newest_first_once_and_capped() {
        let mut recent = Vec::new();
        for i in 0..10u8 {
            remember(&mut recent, [i, 0, 0]);
        }
        assert_eq!(recent.len(), RECENT);
        assert_eq!(recent[0], [9, 0, 0]);
        remember(&mut recent, [5, 0, 0]);
        assert_eq!(recent[0], [5, 0, 0]);
        assert_eq!(recent.iter().filter(|c| **c == [5, 0, 0]).count(), 1);
        assert_eq!(recent.len(), RECENT);
    }
}
