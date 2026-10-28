//! Song search (`settings-window.js` "SONG SEARCH", Labs): the service (AudD
//! with your own token until the no-key service lands), its token, what to
//! listen to, the history, and what is sent (the design's note card).
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
pub const SONG_PRIVACY: &str = "When you press the music button, a ~10-second clip of the audio you choose is sent to AudD. Nothing is recorded at any other time.";

pub fn view(state: &State) -> Element<'_, Message> {
    let song = &state.draft.song;
    let mut rows = column![setting_row::row(
        title("Service"),
        "Free fingerprint lookups can't name a short clip, so this needs your own key",
        pill(SERVICE.name(), false, None),
    )];
    if let Some(field) = &state.song_key {
        rows = rows.push(setting_row::row(
            title("API token"),
            "Sign up at dashboard.audd.io and paste your token. Kept in your system keyring",
            field.view().map(Message::SongKey),
        ));
    }
    let (from, t) = state.thumb(song.source, |p| match p {
        Pick::SongSource(v) => Some(v),
        _ => None,
    });
    rows = rows.push(setting_row::row(
        title("Listen to"),
        match song.source {
            SongSource::System => "What this PC plays: a video, a game, a stream",
            SongSource::Microphone => "Your microphone: music playing near you",
        },
        segmented(
            &[
                (SongSource::System, "This PC"),
                (SongSource::Microphone, "Microphone"),
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
        title("History"),
        "Recent songs, kept only on this PC. Off deletes them",
        boolean(["Off", "On"], song.history, from, t, Message::SongHistory),
    ));
    rows.push(Space::new().height(space::ROW_Y))
        .push(card(sub(Role::NOTE, SONG_PRIVACY)))
        .into()
}
