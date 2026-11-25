//! Try it (step 3, F6): the user's own shortcuts as key caps; a row ticks
//! "Tried" when its key is pressed. Nothing is sent and nothing opens: the
//! global shortcuts are held off on this step (`State::trying`).
use crate::shared::app_language::tr;
use crate::t;
use iced::widget::{Space, column, container, row};
use iced::{Center, Element, Fill};

use super::super::flow::{Message, State, TRY_ROWS};
use crate::features::settings;
use crate::shared::kit::divider::divider;
use crate::shared::kit::icon::{Icon, icon};
use crate::shared::kit::keycap::keycap;
use crate::shared::kit::text::{muted, paragraph, sub, text};
use crate::shared::kit::tokens::{Role, fixed, space};

/// The key caps' column.
const KEYS: f32 = 190.0;
/// A key's cap: its label at about 7 px a letter (mono 11) and 10 px each side.
const KEY_PAD: f32 = 20.0;
const KEY_LETTER: f32 = 7.0;
/// The status column.
const STATUS: f32 = 110.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let mut rows = column![
        text(Role::ROW_TITLE, t!(o_try_title)),
        paragraph(sub(Role::ROW_DESC, t!(o_try_desc))),
        Space::new().height(space::GAP),
    ]
    .spacing(space::GAP / 2.0);
    for (i, r) in TRY_ROWS.iter().enumerate() {
        let caps = row(settings::key_labels(&state.try_key(i))
            .into_iter()
            .map(|k| {
                let w = KEY_PAD + KEY_LETTER * k.chars().count() as f32;
                container(keycap(k)).width(w).into()
            }))
        .spacing(space::GAP / 2.0);
        let status: Element<'_, Message> = match state.tried(i) {
            true => row![
                icon(Icon::Check, 14.0).color(fixed::SUCCESS),
                text(Role::ROW_DESC, t!(o_tried)).color(fixed::SUCCESS)
            ]
            .spacing(space::GAP / 2.0)
            .align_y(Center)
            .into(),
            false => muted(Role::ROW_DESC, t!(o_try_press)).into(),
        };
        let line = crate::dir_row![
            container(caps).width(KEYS),
            column![
                text(Role::ROW_TITLE, tr(r.title)),
                muted(Role::ROW_DESC, tr(r.desc))
            ]
            .width(Fill),
            container(status)
                .width(STATUS)
                .align_x(crate::shared::kit::dir::end())
        ]
        .align_y(Center)
        .spacing(space::GAP);
        rows = rows
            .push(container(line).padding([space::GAP, 0.0]))
            .push(divider());
    }
    rows.into()
}
