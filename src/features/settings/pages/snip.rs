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

use super::super::state::{Message, Section, Slot, State};
use super::{history, shortcuts};
use crate::shared::config::{Format, LONG_MAX, SnipAfter, SnipConfig, StudioOpenIn};
use crate::shared::kit::button::Kind;
use crate::shared::kit::choice_card::{Choice, Mark, choice_cards};
use crate::shared::kit::section::section;
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
    let mut capture: Vec<Element<'_, Message>> = snips
        .into_iter()
        .map(|slot| shortcuts::row(state, slot))
        .collect();
    if state.print_screen_taken && on_print_screen {
        capture.push(pick(
            t!(s_prtsc),
            t!(s_prtsc_desc),
            crate::shared::kit::setting_row::action(
                t!(s_prtsc_settings),
                Kind::Elevated,
                Some(Message::OpenKeyboardSettings),
            ),
        ));
    }
    // After a capture (owner, 2026-11-26): the frozen `studio.open_in`, moved here from the Studio page
    let opens = [
        (
            StudioOpenIn::Quick,
            Mark::Glyph('\u{e151}'),
            t!(ss_quick),
            t!(sn_widget_d),
        ),
        (
            StudioOpenIn::Studio,
            Mark::Glyph('\u{e416}'),
            t!(nav_studio),
            t!(ss_studio_d),
        ),
    ];
    let cards: Vec<_> = (opens.iter())
        .map(|&(_, mark, title_, desc)| Choice {
            mark,
            title: title_,
            desc,
        })
        .collect();
    let at = opens
        .iter()
        .position(|o| o.0 == state.draft.studio.open_in)
        .unwrap_or(0);
    capture.push(setting_row::block(
        title(t!(ss_after)),
        t!(ss_after_d),
        choice_cards(&cards, at, move |i| Message::StudioOpenIn(opens[i].0)),
    ));
    let recent = match d.history {
        true => t!(s_recent_on),
        false => t!(s_recent_off),
    };
    capture.extend([
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
            t!(s_pointer),
            t!(s_pointer_desc),
            boolean(t!(s_pointer), off_on(), d.cursor, move |cursor| {
                edit(&|s| s.cursor = cursor)
            }),
        ),
        pick(
            t!(s_copy),
            t!(s_copy_desc),
            boolean(t!(s_copy), off_on(), copies, move |copy| {
                let after = if copy {
                    SnipAfter::Both
                } else {
                    SnipAfter::Editor
                };
                edit(&|s| s.after = after)
            }),
        ),
    ]);
    let picture = vec![
        pick(
            t!(s_size),
            t!(s_size_desc),
            segmented(
                &crate::shared::config::size_choices(),
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
        // F11: the Save folder lives in General only
        pick(
            t!(ss_folder),
            t!(ss_folder_d),
            setting_row::action(
                t!(ss_folder_open),
                crate::shared::kit::button::Kind::Elevated,
                Some(Message::Section(Section::General)),
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
    ];
    column![
        section(Some(t!(sn_capture)), None, || capture),
        section(Some(t!(sn_picture)), None, || picture),
    ]
    .spacing(crate::shared::kit::tokens::settings::SECTION_GAP)
    .into()
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
