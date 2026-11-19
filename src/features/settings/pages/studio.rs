//! Studio (STUDIO_V3_SPEC section 8, boards 8a / 8b): OpenCircle Studio's settings in
//! the design's order (General, Saving and export, Copy and paste, Editing, View,
//! Shortcuts), kit rows only. The values live in `[studio]` (`StudioPrefs`); the two
//! panel switches are `snip.studio`, the picture size is `snip.max_edge`, the Snip
//! shortcut and Reset shortcuts are the Shortcuts page's own rows.
use crate::t;
use iced::Element;
use iced::widget::{column, container};
use iced::{Fill, widget::tooltip::Position};

use super::super::state::{Message, Slot, State};
use super::{general, shortcuts};
use crate::shared::config::{
    EditorConfig, Format, STUDIO_QUALITY, STUDIO_RECENT, SnipConfig, StudioBackdrop, StudioOpenIn,
    StudioPrefs, StudioWheel, StudioZoom,
};
use crate::shared::kit::button::Kind;
use crate::shared::kit::field::field;
use crate::shared::kit::segmented::{boolean, segmented};
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::tooltip::tip;

fn off_on() -> [&'static str; 2] {
    [t!(c_off), t!(c_on)]
}

pub fn view(state: &State) -> Element<'_, Message> {
    let p = &state.draft.studio;
    let sn = &state.draft.snip;
    let edit = move |change: &dyn Fn(&mut StudioPrefs)| {
        let mut next = p.clone();
        change(&mut next);
        Message::Studio(next)
    };
    let edit_snip = move |change: &dyn Fn(&mut SnipConfig)| {
        let mut next = sn.clone();
        change(&mut next);
        Message::Snip(next)
    };
    let row = |name, desc, control| setting_row::row(title(name), desc, control);
    let switch = move |name, desc, on: bool, set: fn(&mut StudioPrefs, bool)| {
        row(
            name,
            desc,
            boolean(off_on(), on, on, 1.0, move |v| edit(&|s| set(s, v))),
        )
    };
    let name = container(field(
        &p.file_name,
        Some(move |v: String| edit(&|s| s.file_name = v.replace(['/', '\\'], ""))),
        Message::Studio(p.clone()),
    ))
    .width(Fill);
    let tool_styles_set = state.draft.editor
        != EditorConfig {
            line_assist: state.draft.editor.line_assist,
            ..Default::default()
        };
    let (tool_desc, tool_action) = match tool_styles_set {
        true => (t!(ss_tool_styles_d), Some(Message::ResetToolStyles)),
        false => (t!(ss_tool_styles_done), None),
    };
    let snips = state.slots().filter(|slot| match slot {
        Slot::Quick(i) => state.draft.quick_actions[*i].action == "snip",
        Slot::Capture => false,
    });
    let format_row = |label, desc, now: Format, set: fn(&mut StudioPrefs, Format)| {
        row(
            label,
            desc,
            segmented(
                &[(Format::Png, t!(s_png)), (Format::Jpeg, t!(s_jpeg))],
                now,
                now,
                1.0,
                move |f| edit(&|s| set(s, f)),
            ),
        )
    };

    let mut rows = column![
        // General
        row(
            t!(ss_open_in),
            t!(ss_open_in_d),
            segmented(
                &[
                    (StudioOpenIn::Quick, t!(ss_quick)),
                    (StudioOpenIn::Studio, t!(ss_studio)),
                ],
                p.open_in,
                p.open_in,
                1.0,
                move |v| edit(&|s| s.open_in = v),
            ),
        ),
        switch(t!(ss_reopen), t!(ss_reopen_d), p.reopen_last, |s, v| s
            .reopen_last =
            v),
        switch(
            t!(ss_ask_unsaved),
            t!(ss_ask_unsaved_d),
            p.ask_unsaved,
            |s, v| s.ask_unsaved = v
        ),
        switch(t!(ss_keep), t!(ss_keep_d), p.keep_unsaved, |s, v| s
            .keep_unsaved =
            v),
        row(
            t!(ss_recent),
            t!(ss_recent_d),
            segmented(
                &STUDIO_RECENT.map(|n| (n, n_label(n))),
                p.recent_files,
                p.recent_files,
                1.0,
                move |v| edit(&|s| s.recent_files = v),
            ),
        ),
        // Saving and export
        row(
            t!(ss_folder),
            t!(ss_folder_d),
            general::folder_control(
                state,
                &p.folder,
                "studio.folder",
                Message::BrowseStudioFolder,
                Message::ClearStudioFolder,
            ),
        ),
        row(
            t!(ss_name),
            t!(ss_name_d),
            tip(name, t!(ss_name), Position::Top)
        ),
        format_row(t!(ss_format), t!(ss_format_d), p.format, |s, f| s.format =
            f),
        row(
            t!(ss_quality),
            t!(ss_quality_d),
            segmented(
                &STUDIO_QUALITY.map(|q| (q, n_label(q))),
                p.quality,
                p.quality,
                1.0,
                move |v| edit(&|s| s.quality = v),
            ),
        ),
        row(
            t!(ss_size),
            t!(ss_size_d),
            segmented(
                &[
                    (1024, "1024"),
                    (1568, "1568"),
                    (2048, "2048"),
                    (0, t!(s_original)),
                ],
                sn.max_edge,
                sn.max_edge,
                1.0,
                move |v| edit_snip(&|s| s.max_edge = v),
            ),
        ),
        switch(t!(ss_ask_where), t!(ss_ask_where_d), p.ask_where, |s, v| {
            s.ask_where = v
        }),
        switch(t!(ss_copy_path), t!(ss_copy_path_d), p.copy_path, |s, v| {
            s.copy_path = v
        }),
        // Copy and paste
        switch(t!(ss_ctrlc), t!(ss_ctrlc_d), p.ctrl_c_picture, |s, v| s
            .ctrl_c_picture =
            v),
        switch(
            t!(ss_paste_obj),
            t!(ss_paste_obj_d),
            p.paste_movable,
            |s, v| s.paste_movable = v
        ),
        switch(t!(ss_grow), t!(ss_grow_d), p.grow_canvas, |s, v| s
            .grow_canvas =
            v),
        format_row(t!(ss_drag), t!(ss_drag_d), p.drag_type, |s, f| s
            .drag_type =
            f),
        // Editing
        row(
            t!(ss_tool_styles),
            tool_desc,
            setting_row::action(t!(c_reset), Kind::Elevated, tool_action),
        ),
        switch(t!(ss_snap), t!(ss_snap_d), p.snap, |s, v| s.snap = v),
        switch(t!(ss_grid), t!(ss_grid_d), p.pixel_grid, |s, v| s
            .pixel_grid =
            v),
        row(
            t!(ss_bg),
            t!(ss_bg_d),
            segmented(
                &[
                    (StudioBackdrop::Theme, t!(ss_bg_theme)),
                    (StudioBackdrop::Checkerboard, t!(ss_bg_check)),
                    (StudioBackdrop::Dark, t!(ss_bg_dark)),
                ],
                p.background,
                p.background,
                1.0,
                move |v| edit(&|s| s.background = v),
            ),
        ),
        // View
        row(
            t!(ss_zoom),
            t!(ss_zoom_d),
            segmented(
                &[
                    (StudioZoom::Fit, t!(ss_zoom_fit)),
                    (StudioZoom::Actual, t!(ss_zoom_100)),
                ],
                p.zoom_on_open,
                p.zoom_on_open,
                1.0,
                move |v| edit(&|s| s.zoom_on_open = v),
            ),
        ),
        row(
            t!(ss_wheel),
            t!(ss_wheel_d),
            segmented(
                &[
                    (StudioWheel::Scroll, t!(ss_wheel_scroll)),
                    (StudioWheel::Zoom, t!(ss_wheel_zoom)),
                ],
                p.wheel,
                p.wheel,
                1.0,
                move |v| edit(&|s| s.wheel = v),
            ),
        ),
        row(
            t!(ss_left),
            t!(ss_left_d),
            boolean(
                off_on(),
                sn.studio.library,
                sn.studio.library,
                1.0,
                move |v| { edit_snip(&|s| s.studio.library = v) }
            ),
        ),
        row(
            t!(ss_right),
            t!(ss_right_d),
            boolean(
                off_on(),
                sn.studio.options,
                sn.studio.options,
                1.0,
                move |v| { edit_snip(&|s| s.studio.options = v) }
            ),
        ),
    ];
    // Shortcuts: the Snip key (the Studio's New snip), then Reset shortcuts
    for slot in snips {
        rows = rows.push(shortcuts::row(state, slot));
    }
    rows.push(shortcuts::reset(state)).into()
}

/// A number as the segment shows it ("10", "85 %" reads as the number alone).
fn n_label(n: u8) -> &'static str {
    match n {
        0 => t!(c_off),
        10 => "10",
        20 => "20",
        70 => "70 %",
        85 => "85 %",
        _ => "95 %",
    }
}
