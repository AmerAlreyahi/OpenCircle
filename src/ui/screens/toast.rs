//! The toast window's content (`Windows::toast`): the kit's pill, as big as
//! its window (`size`), with the window's corners.
use iced::Element;

use crate::ui::kit::toast::toast;
pub use crate::ui::kit::toast::{Tone, size};
use crate::ui::kit::tokens::radius;

/// The corners the window is cut to (`platform::windows::floating`).
pub const RADIUS: f32 = radius::CARD;

/// `text` on the pill, corners of `radius` (`shape::radius(RADIUS)`: square
/// where windows can't be cut).
pub fn view<'a, Message: 'a>(text: &'a str, tone: Tone, radius: f32) -> Element<'a, Message> {
    toast(text, tone, radius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::golden;

    /// Each tone, Latin and Arabic, Sapphire and Daylight, at 1x and 2x.
    #[test]
    fn every_toast_matches_its_golden_image() {
        golden::each_theme(|theme, failed| {
            for (tone, name, latin, arabic) in [
                (Tone::Ok, "ok", "Copied", "تم النسخ"),
                (
                    Tone::Error,
                    "error",
                    "Couldn't save the picture",
                    "تعذّر حفظ الصورة",
                ),
                (
                    Tone::Neutral,
                    "neutral",
                    "OpenCircle is running in the tray. Press Ctrl+Shift+Space.",
                    "يعمل OpenCircle في شريط المهام. اضغط Ctrl+Shift+Space.",
                ),
            ] {
                for (script, s) in [("latin", latin), ("arabic", arabic)] {
                    let file = format!("toast-{name}-{script}-{}", theme.id);
                    failed.extend(golden::check(
                        "screens",
                        &file,
                        size(s),
                        &[1.0, 2.0],
                        || view::<()>(s, tone, RADIUS),
                    ));
                }
            }
        });
    }

    /// The table's words, mirrored: the dot at the right of the line.
    #[test]
    fn every_toast_mirrors_in_arabic() {
        golden::each_theme(|theme, failed| {
            let _ar = crate::ui::kit::testing::arabic();
            for (tone, name, text) in [
                (Tone::Ok, "ok", crate::t!(pill_copied).to_string()),
                (Tone::Error, "error", crate::t!(pill_cant_copy).to_string()),
                (
                    Tone::Neutral,
                    "neutral",
                    crate::app_language::fmt(
                        crate::app_language::S::pill_tray,
                        &[&"Ctrl+Shift+Space"],
                    ),
                ),
            ] {
                let file = format!("toast-{name}-rtl-{}", theme.id);
                failed.extend(golden::check(
                    "screens",
                    &file,
                    size(&text),
                    &[1.0, 2.0],
                    || view::<()>(&text, tone, RADIUS),
                ));
            }
        });
    }
}
