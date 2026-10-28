//! A dropdown: the design's language selector (`.oc-input`, as wide as a
//! settings row's control column; the value and "▾") and its menu
//! (`.oc-dropdown-menu`: the `dropdown` material, a `rim` edge, radius 14; the item under the cursor on `elevated`). The
//! design only cycles its selectors; this one opens iced's list (no fly-in:
//! the list can't animate, and its rows can't round their corners).
use iced::widget::pick_list::{self, Handle, Icon, PickList};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Border, Element, Shadow};

use super::text::{MONO, font};
use super::themes::c;
use super::tokens::{Role, radius, size};

/// The list draws its rows with the field's line and padding: 12 px text on
/// iced's 1.3 line, 7x11 px round it (32 px with the edge, as `.oc-input`).
const LINE: f32 = 1.3;
const PAD: [f32; 2] = [7.0, 11.0];

pub fn dropdown<'a, Message: Clone + 'a, T: ToString + PartialEq + Clone + 'a>(
    options: &'a [T],
    chosen: Option<T>,
    placeholder: &str,
    on_pick: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let role = Role::NOTE;
    let picker: PickList<'a, T, &'a [T], T, Message> =
        iced::widget::pick_list(options, chosen, on_pick);
    picker
        .placeholder(placeholder)
        .font(font(role))
        .text_size(role.size)
        .text_line_height(LINE)
        .padding(PAD)
        .width(size::CONTROL_COLUMN)
        .handle(Handle::Static(Icon {
            font: MONO,
            code_point: '▾',
            size: Some(role.size.into()),
            line_height: LineHeight::Relative(LINE),
            shaping: Shaping::Basic,
        }))
        .style(|_, status| pick_list::Style {
            text_color: c().text,
            placeholder_color: c().text_muted,
            handle_color: c().text,
            background: c().track.into(),
            border: Border::default()
                .rounded(radius::TRACK)
                .width(1)
                .color(match status {
                    pick_list::Status::Active => c().rim,
                    _ => c().accent,
                }),
        })
        .menu_style(|_| iced::overlay::menu::Style {
            background: c().dropdown.into(),
            border: Border::default()
                .rounded(radius::DROPDOWN)
                .width(1)
                .color(c().rim),
            text_color: c().text,
            selected_text_color: c().text,
            selected_background: c().elevated.into(),
            shadow: Shadow::default(),
        })
        .into()
}

#[cfg(test)]
mod tests {
    /// Changing a dropdown's value leaves no part of the old text on screen
    /// (#53): each change redrawn only where it differs, as the window does,
    /// equals the same view drawn whole, at 100 % and 150 %.
    #[test]
    fn a_changed_dropdown_leaves_no_ghost_text() {
        const LANGS: [&str; 6] = ["Arabic", "English", "Japanese", "العربية", "Hindi", "Wolof"];
        for scale in [1.0, 1.5] {
            let view = |i: usize| {
                (i <= 20).then(|| {
                    let field =
                        super::dropdown(&LANGS[..], Some(LANGS[i % LANGS.len()]), "", |_| ());
                    iced::widget::container(field).padding(20).into()
                })
            };
            let size = iced::Size::new(360.0, 80.0);
            let (window, fresh) = super::super::testing::shot::redrawn(view, size, scale);
            let differ = window.iter().zip(&fresh).filter(|(a, b)| a != b).count();
            assert_eq!(differ, 0, "{differ} stale bytes at {scale}x");
        }
    }
}
