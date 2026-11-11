//! Song search (`settings-window.js` "SONG SEARCH", Labs): the service (AudD
//! with your own token until the no-key service lands), its token, what to
//! listen to, the history, and what is sent (the design's note card).
use crate::t;
use iced::Element;
use iced::widget::{Space, column};

use super::super::state::{Message, Pick, State};
use crate::config::SongSource;
use crate::music::SERVICE;
use crate::ui::kit::card::card;
use crate::ui::kit::chip::pill;
use crate::ui::kit::segmented::{boolean, segmented};
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::text::sub;
use crate::ui::kit::tokens::{Role, space};

/// What song search sends, and when (also in docs/privacy.md).
pub const SONG_PRIVACY: crate::app_language::S = crate::app_language::S::song_privacy;

pub fn view(state: &State) -> Element<'_, Message> {
    let song = &state.draft.song;
    let mut rows = column![setting_row::row(
        title(t!(song_service)),
        t!(song_need_key),
        pill(SERVICE.name(), false, None),
    )];
    if let Some(field) = &state.song_key {
        rows = rows.push(setting_row::row(
            title(t!(song_token)),
            t!(song_sign_up),
            field.view().map(Message::SongKey),
        ));
    }
    let (from, t) = state.thumb(song.source, |p| match p {
        Pick::SongSource(v) => Some(v),
        _ => None,
    });
    rows = rows.push(setting_row::row(
        title(t!(song_listen)),
        match song.source {
            SongSource::System => t!(song_pc_desc),
            SongSource::Microphone => t!(song_mic_desc),
        },
        segmented(
            &[
                (SongSource::System, t!(song_pc)),
                (SongSource::Microphone, t!(song_mic)),
            ],
            song.source,
            from,
            t,
            Message::SongSource,
        ),
    ));
    let (from, t) = state.thumb(song.history, |p| match p {
        Pick::SongHistory(v) => Some(v),
        _ => None,
    });
    rows = rows.push(setting_row::row(
        title(t!(nav_history)),
        t!(song_recent),
        boolean(
            [t!(c_off), t!(c_on)],
            song.history,
            from,
            t,
            Message::SongHistory,
        ),
    ));
    rows.push(Space::new().height(space::ROW_Y))
        .push(card(sub(Role::NOTE, crate::app_language::tr(SONG_PRIVACY))))
        .into()
}
