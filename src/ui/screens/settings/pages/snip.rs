//! Snip (`settings-window.js` "SNIP", Labs): what a capture does, the size
//! and file it is copied and saved as, the pointer, the timer, and Recent
//! (off = nothing kept on disk).
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, State};
use crate::config::{Format, SnipAfter, SnipConfig};
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
    column![
        pick(
            "After a capture",
            "Copy: ready to paste into an AI chat, Edit a click away",
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
                    (0, "Original")
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
            "Recent captures",
            "The last 20, on this PC only. Off deletes them",
            boolean(OFF_ON, d.history, d.history, 1.0, move |history| {
                edit(&|s| s.history = history)
            }),
        ),
    ]
    .into()
}
