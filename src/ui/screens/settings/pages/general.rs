//! General (`settings-window.js` "GENERAL"): what Enter does, start with the
//! OS, search history (Labs), the save folder, updates.
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container, row};
use iced::{Element, Fill};

use super::super::legacy::{START_LABEL, shorten};
use super::super::state::{Message, Pick, State, UPDATE_PRIVACY, issue};
use crate::config::DefaultAction;
use crate::labs::Feature;
use crate::ui::kit::button::{Kind, button, label};
use crate::ui::kit::field::field;
use crate::ui::kit::segmented::{boolean, segmented};
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::error;
use crate::ui::kit::tokens::{Role, space};
use crate::ui::widgets::{Why, hint, tip};

/// The save folder's field (`w-[260px]`).
const FOLDER_WIDTH: f32 = 260.0;
const OFF_ON: [&str; 2] = ["Off", "On"];

pub fn view(state: &State) -> Element<'_, Message> {
    let d = &state.draft;
    let general = &d.general;
    let (from, glide) = state.thumb(general.default_action, |p| match p {
        Pick::Default(a) => Some(a),
        _ => None,
    });
    let actions = [
        (DefaultAction::Lens, "Search"),
        (DefaultAction::Ai, "Ask AI"),
        (DefaultAction::Text, "Text"),
        (DefaultAction::Copy, "Copy"),
    ];
    let mut rows = column![setting_row::wide(
        title("Default action"),
        "What Enter does after you select something",
        segmented(
            &actions,
            general.default_action,
            from,
            glide,
            Message::DefaultAction
        ),
    )];
    let switch = |on: bool, pick: fn(Pick) -> Option<bool>, m: fn(bool) -> Message| {
        let (from, glide) = state.thumb(on, pick);
        boolean(OFF_ON, on, from, glide, m)
    };
    rows = rows.push(setting_row::row(
        title(START_LABEL),
        "OpenCircle waits in the tray, with no window",
        switch(
            general.autostart,
            |p| match p {
                Pick::Autostart(v) => Some(v),
                _ => None,
            },
            Message::Autostart,
        ),
    ));
    // Search history is a Labs feature: its row exists only while it is on.
    if d.labs.has(Feature::History) {
        rows = rows.push(setting_row::row(
            title("Search history"),
            "Only on this PC: what you searched, a small picture, the time",
            switch(
                general.search_history,
                |p| match p {
                    Pick::History(v) => Some(v),
                    _ => None,
                },
                Message::SearchHistory,
            ),
        ));
    }
    rows = rows.push(setting_row::row(
        title("Save folder"),
        "Where Save starts. Files are OpenCircle-date-time.png",
        folder(state),
    ));
    rows = rows.push(setting_row::row(
        title("Keep OpenCircle up to date"),
        UPDATE_PRIVACY,
        switch(
            d.update.auto == Some(true),
            |p| match p {
                Pick::Update(v) => Some(v),
                _ => None,
            },
            Message::AutoUpdate,
        ),
    ));
    let status = state.update_status.as_deref();
    rows = rows.push(setting_row::row(
        title("Updates"),
        status.unwrap_or("Look for a new version now"),
        button(
            label("Check now"),
            Kind::Elevated,
            Some(Message::CheckUpdate),
        ),
    ));
    rows.into()
}

/// The folder (cut in the middle when long: the tooltip shows it whole), and
/// Browse… / Reset under it at the right.
fn folder(state: &State) -> Element<'_, Message> {
    let folder = state.draft.general.save_folder.as_str();
    let shown = match folder.is_empty() {
        true => crate::actions::save::default_folder().map_or_else(
            || "Chosen by your system".into(),
            |p| p.to_string_lossy().into_owned(),
        ),
        false => shorten(folder),
    };
    let value = container(field(
        &shown,
        None::<fn(String) -> Message>,
        Message::BrowseFolder,
    ))
    .width(FOLDER_WIDTH);
    let value = if folder.is_empty() || shorten(folder) == folder {
        tip(value, "Save folder", Position::Top)
    } else {
        hint(value, "Save folder", Why::Cut(folder), Position::Top)
    };
    let browse = button(
        label("Browse…"),
        Kind::Elevated,
        Some(Message::BrowseFolder),
    );
    let reset = button(
        label("Reset"),
        Kind::Elevated,
        (!folder.is_empty()).then_some(Message::ClearFolder),
    );
    let mut col = column![
        value,
        row![
            Space::new().width(Fill),
            tip(browse, "Choose the save folder", Position::Bottom),
            tip(reset, "Use the system's folder", Position::Bottom),
        ]
        .spacing(space::GAP),
    ]
    .spacing(space::GAP)
    .width(FOLDER_WIDTH);
    if let Some(e) = issue(&state.problems, "general.save_folder") {
        col = col.push(error(Role::ROW_DESC, e));
    }
    col.into()
}
