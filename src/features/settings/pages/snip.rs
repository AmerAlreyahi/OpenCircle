//! Snip (`settings-window.js` "SNIP"; out of Labs since 0.1.3, N-41): its
//! shortcuts (Print Screen, Alt+ / Ctrl+Print Screen: the same rows as on
//! Shortcuts), a calm note when Windows keeps Print Screen for its own screen
//! capture, whether a pick is copied, the size and file it is copied and saved as,
//! the save folder (General's, the one folder), the pointer, the timer, how
//! tall a long capture may grow, and Recent captures (its switch is on
//! History).
use crate::t;
use iced::Element;
use iced::widget::column;

use super::super::state::{Message, Slot, State};
use super::{general, history, shortcuts};
use crate::shared::config::{Format, LONG_MAX, SnipAfter, SnipConfig};
use crate::shared::kit::button::Kind;
use crate::shared::kit::segmented::{boolean, segmented};
use crate::shared::kit::setting_row::{self, title};

fn off_on() -> [&'static str; 2] {
    [t!(c_off), t!(c_on)]
}

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft.snip;
    // The draft's snip choices with one changed.
    let edit = move |change: &dyn Fn(&mut SnipConfig)| {
        let mut next = d.clone();
        change(&mut next);
        Message::Snip(next)
    };
    let copies = crate::features::snip::screens::pick::copies(d.after);
    let pick = |name, desc, control| setting_row::row(title(name), desc, control);
    let snips = state.slots().filter(|slot| match slot {
        Slot::Quick(i) => state.draft.quick_actions[*i].action == "snip",
        Slot::Capture => false,
    });
    let snips: Vec<_> = snips.collect();
    // The note is for a user whose Snip key is Print Screen (Alt+A is the default).
    let on_print_screen = (snips.iter()).any(|&slot| {
        crate::shared::config::parse_shortcut(state.shortcut(slot)).ok()
            == super::super::state::print_screen()
    });
    let mut rows = column(snips.into_iter().map(|slot| shortcuts::row(state, slot)));
    if state.print_screen_taken && on_print_screen {
        rows = rows.push(pick(
            t!(s_prtsc),
            t!(s_prtsc_desc),
            crate::shared::kit::setting_row::action(
                t!(s_prtsc_settings),
                Kind::Elevated,
                Some(Message::OpenKeyboardSettings),
            ),
        ));
    }
    let recent = match d.history {
        true => t!(s_recent_on),
        false => t!(s_recent_off),
    };
    rows = rows.extend([
        pick(
            t!(s_copy),
            t!(s_copy_desc),
            boolean(off_on(), copies, copies, 1.0, move |copy| {
                let after = if copy {
                    SnipAfter::Both
                } else {
                    SnipAfter::Editor
                };
                edit(&|s| s.after = after)
            }),
        ),
        pick(
            t!(s_size),
            t!(s_size_desc),
            segmented(
                &[
                    (1024, "1024"),
                    (1568, "1568"),
                    (2048, "2048"),
                    (0, t!(s_original)),
                ],
                d.max_edge,
                d.max_edge,
                1.0,
                move |max_edge| edit(&|s| s.max_edge = max_edge),
            ),
        ),
        pick(
            t!(s_as),
            t!(s_as_desc),
            segmented(
                &[(Format::Png, t!(s_png)), (Format::Jpeg, t!(s_jpeg))],
                d.format,
                d.format,
                1.0,
                move |format| edit(&|s| s.format = format),
            ),
        ),
        pick(
            t!(s_save_size),
            t!(s_save_size_desc),
            segmented(
                &[(false, t!(s_size)), (true, t!(s_original))],
                d.save_original,
                d.save_original,
                1.0,
                move |original| edit(&|s| s.save_original = original),
            ),
        ),
        general::folder_row(state),
        pick(
            t!(s_pointer),
            t!(s_pointer_desc),
            boolean(off_on(), d.cursor, d.cursor, 1.0, move |cursor| {
                edit(&|s| s.cursor = cursor)
            }),
        ),
        pick(
            t!(s_timer),
            t!(s_timer_desc),
            segmented(
                &[
                    (0, t!(c_off)),
                    (3, t!(s_t3)),
                    (5, t!(s_t5)),
                    (10, t!(s_t10)),
                ],
                d.delay,
                d.delay,
                1.0,
                move |delay| edit(&|s| s.delay = delay),
            ),
        ),
        pick(
            t!(s_long),
            t!(s_long_desc),
            segmented(
                &LONG_MAX.map(|px| (px, px_label(px))),
                d.long_max,
                d.long_max,
                1.0,
                move |px| edit(&|s| s.long_max = px),
            ),
        ),
        history::link(t!(s_recent), recent),
    ]);
    rows.into()
}

/// A long-capture limit as the page shows it.
fn px_label(px: u32) -> &'static str {
    match px {
        5_000 => t!(s_px5),
        10_000 => t!(s_px10),
        _ => t!(s_px20),
    }
}

/// Windows' keyboard settings, where its Print Screen switch is (never
/// changed by us).
pub fn open_keyboard_settings() {
    #[cfg(windows)]
    {
        let run = crate::shared::platform::sys::open::open_system_settings(
            "ms-settings:easeofaccess-keyboard",
        );
        if let Err(e) = run {
            crate::log!("[opencircle] could not open Windows' keyboard settings: {e}");
        }
    }
}
