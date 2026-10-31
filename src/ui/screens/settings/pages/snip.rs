//! Snip (`settings-window.js` "SNIP"; out of Labs since 0.1.3, N-41): its
//! shortcuts (Print Screen, Alt+ / Ctrl+Print Screen: the same rows as on
//! Shortcuts), a calm note when Windows keeps Print Screen for its own screen
//! capture, what a capture does, the size and file it is copied and saved as,
//! the save folder (General's, the one folder), the pointer, the timer, how
//! tall a long capture may grow, and Recent captures (its switch is on
//! History).
use iced::widget::{Space, column, row};
use iced::{Element, Fill};

use super::super::state::{Message, Slot, State};
use super::{general, history, shortcuts};
use crate::config::{Format, LONG_MAX, SnipAfter, SnipConfig};
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::segmented::{boolean, segmented};
use crate::ui::kit::setting_row::{self, title};

const OFF_ON: [&str; 2] = ["Off", "On"];

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft.snip;
    // The draft's snip choices with one changed.
    let edit = move |change: &dyn Fn(&mut SnipConfig)| {
        let mut next = d.clone();
        change(&mut next);
        Message::Snip(next)
    };
    let pick = |name, desc, control| setting_row::row(title(name), desc, control);
    let snips = state.slots().filter(|slot| match slot {
        Slot::Quick(i) => state.draft.quick_actions[*i].action == "snip",
        Slot::Capture => false,
    });
    let mut rows = column(snips.map(|slot| shortcuts::row(state, slot)));
    if state.print_screen_taken {
        rows = rows.push(pick(
            "Print Screen key",
            "Windows opens its own screen capture with it. To snip with Print Screen, \
             turn off \"Use the Print screen key to open screen capture\" in Windows",
            row![
                Space::new().width(Fill),
                button(
                    label("Open Windows settings"),
                    Kind::Elevated,
                    Some(Message::OpenKeyboardSettings),
                )
            ]
            .into(),
        ));
    }
    let recent = match d.history {
        true => "On: the last 20, on this PC only. Kept and cleared in History",
        false => "Off: nothing is kept. Turn it on in History",
    };
    rows = rows.extend([
        pick(
            "After a capture",
            "Copy: ready to paste into an AI chat, Snip a click away",
            segmented(
                &[
                    (SnipAfter::Copy, "Copy"),
                    (SnipAfter::Editor, "Editor"),
                    (SnipAfter::Both, "Both"),
                ],
                d.after,
                d.after,
                1.0,
                move |after| edit(&|s| s.after = after),
            ),
        ),
        pick(
            "Image size",
            "Long edge for Copy and Save; AI chats shrink bigger ones anyway",
            segmented(
                &[
                    (1024, "1024"),
                    (1568, "1568"),
                    (2048, "2048"),
                    (0, "Original"),
                ],
                d.max_edge,
                d.max_edge,
                1.0,
                move |max_edge| edit(&|s| s.max_edge = max_edge),
            ),
        ),
        pick(
            "Save as",
            "The file Save writes",
            segmented(
                &[(Format::Png, "PNG"), (Format::Jpeg, "JPEG")],
                d.format,
                d.format,
                1.0,
                move |format| edit(&|s| s.format = format),
            ),
        ),
        pick(
            "Save size",
            "Copy always uses the image size",
            segmented(
                &[(false, "Image size"), (true, "Original")],
                d.save_original,
                d.save_original,
                1.0,
                move |original| edit(&|s| s.save_original = original),
            ),
        ),
        general::folder_row(state),
        pick(
            "Mouse pointer",
            "Draw the pointer into the capture",
            boolean(OFF_ON, d.cursor, d.cursor, 1.0, move |cursor| {
                edit(&|s| s.cursor = cursor)
            }),
        ),
        pick(
            "Timer",
            "Wait before a snip captures, so you can open a menu first",
            segmented(
                &[(0, "Off"), (3, "3 s"), (5, "5 s"), (10, "10 s")],
                d.delay,
                d.delay,
                1.0,
                move |delay| edit(&|s| s.delay = delay),
            ),
        ),
        pick(
            "Long capture",
            "How tall a long capture may grow; taller takes more memory",
            segmented(
                &LONG_MAX.map(|px| (px, px_label(px))),
                d.long_max,
                d.long_max,
                1.0,
                move |px| edit(&|s| s.long_max = px),
            ),
        ),
        history::link("Recent captures", recent),
    ]);
    rows.into()
}

/// A long-capture limit as the page shows it.
fn px_label(px: u32) -> &'static str {
    match px {
        5_000 => "5,000 px",
        10_000 => "10,000 px",
        _ => "20,000 px",
    }
}

/// Windows' keyboard settings, where its Print Screen switch is (never
/// changed by us).
pub fn open_keyboard_settings() {
    #[cfg(windows)]
    {
        let run = crate::sys::open::open_system_settings("ms-settings:easeofaccess-keyboard");
        if let Err(e) = run {
            crate::log!("[opencircle] could not open Windows' keyboard settings: {e}");
        }
    }
}
