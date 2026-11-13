//! Labs (`settings-window.js` "LABS"): one Off / On row per unfinished
//! feature, each tagged "Experimental". A switch saves at once; while it is
//! off, the feature's page, rows and shortcuts are hidden.
use crate::shared::app_language::{S, tr};
use crate::t;
use iced::widget::column;
use iced::{Center, Element};

use super::super::state::{Message, Pick, State};
use crate::labs::Feature;
use crate::shared::kit::chip::tag;
use crate::shared::kit::segmented::boolean;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::tokens::space;

pub fn view(state: &State) -> Element<'_, Message> {
    let labs = &state.draft.labs;
    column(Feature::ALL.into_iter().map(|f| {
        let on = labs.has(f);
        let (from, glide) = state.thumb(on, |p| match p {
            Pick::Lab(g, v) if g == f => Some(v),
            _ => None,
        });
        let (name, blurb) = words(f);
        let name = crate::dir_row![title(tr(name)), tag(t!(lb_tag))]
            .spacing(space::GAP)
            .align_y(Center);
        setting_row::row(
            name.into(),
            tr(blurb),
            boolean([t!(c_off), t!(c_on)], on, from, glide, move |on| {
                Message::Labs(f, on)
            }),
        )
    }))
    .into()
}

/// A Labs feature's name and line in the app's language (`Feature::title`
/// and `blurb` are the English).
fn words(f: Feature) -> (S, S) {
    match f {
        Feature::Song => (S::lb_song, S::lb_song_d),
        Feature::Pins => (S::lb_pins, S::lb_pins_d),
        Feature::Live => (S::lb_live, S::lb_live_d),
    }
}
