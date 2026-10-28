//! Labs (`settings-window.js` "LABS"): one Off / On row per unfinished
//! feature, each tagged "Experimental". A switch saves at once; while it is
//! off, the feature's page, rows and shortcuts are hidden.
use iced::widget::{column, row};
use iced::{Center, Element};

use super::super::state::{Message, Pick, State};
use crate::labs::Feature;
use crate::ui::kit::chip::tag;
use crate::ui::kit::segmented::boolean;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::tokens::space;

pub fn view(state: &State) -> Element<'_, Message> {
    let labs = &state.draft.labs;
    column(Feature::ALL.into_iter().map(|f| {
        let on = labs.has(f);
        let (from, glide) = state.thumb(on, |p| match p {
            Pick::Lab(g, v) if g == f => Some(v),
            _ => None,
        });
        let name = row![title(f.title()), tag("Experimental")]
            .spacing(space::GAP)
            .align_y(Center);
        setting_row::row(
            name.into(),
            f.blurb(),
            boolean(["Off", "On"], on, from, glide, move |on| {
                Message::Labs(f, on)
            }),
        )
    }))
    .into()
}
