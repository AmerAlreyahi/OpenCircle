//! A dropdown: the design's language selector (`.oc-input w-[190px]`, the
//! value and "▾") and its menu (`.oc-dropdown-menu`: the `dropdown` material,
//! a `rim` edge, radius 14; the item under the cursor on `elevated`). The
//! design only cycles its selectors; this one opens iced's list (no fly-in:
//! the list can't animate, and its rows can't round their corners).
use iced::widget::pick_list::{self, Handle, Icon, PickList};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Border, Element, Shadow};

use super::text::{MONO, font};
use super::themes::c;
use super::tokens::{Role, radius};

/// `w-[190px]`.
pub const WIDTH: f32 = 190.0;
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
        .width(WIDTH)
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
