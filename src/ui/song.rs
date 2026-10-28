//! Song search on the overlay (its button is in the overlay's bottom bar):
//! the "Listening… N s" pill with Cancel, then the result card (cover, title,
//! artist, album · year, links opened only on click). The work is `crate::music`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::widget::{column, container, image, opaque, row};
use iced::{Center, Element, Fill, Point, Rectangle, Size, Task};

use super::{background, widgets};
use crate::config;
use crate::history::song as history;
use crate::music::{self, LISTEN_SECS, Song};
use crate::ui::kit::panel_frame as card;

/// Lucide "music" (in the subset font).
pub const MUSIC: char = '\u{e125}';
const COVER: f32 = 88.0;
/// The result card's height: header + cover row + links (+ a line per recent song).
const CARD_HEIGHT: f32 = 300.0;
const RECENT_LINE: f32 = 22.0;
/// Recent songs shown under a result (history on).
const RECENT: usize = 3;

#[derive(Debug, Clone)]
pub enum Msg {
    /// Seconds heard so far.
    Heard(u32),
    /// Done listening; the clip is on its way to the service.
    Sent,
    Done(Result<Song, music::Error>, Vec<history::Entry>),
    Cover(image::Handle),
    Cancel,
    Retry,
    Open(String),
    /// From the card's frame (only its × does something: the card stays put).
    Card(card::Event),
}

/// What the App does after a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    None,
    /// Drop the session (Cancel, Close).
    Close,
    /// Start a new search.
    Retry,
    /// Open this link in the browser (the user clicked it).
    Open(String),
}

#[derive(Debug)]
enum State {
    Listening(u32),
    Looking,
    Found(Song, Option<image::Handle>),
    Failed(String),
}

/// One search, from listening to its card.
#[derive(Debug)]
pub struct Session {
    state: State,
    recent: Vec<history::Entry>,
    /// Set on Cancel or drop: stops the capture at once.
    cancel: Arc<AtomicBool>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Session {
    /// Starts listening on a thread (the audio device is open only until it's done).
    pub fn start() -> (Session, Task<Msg>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let work = background(move |tx| {
            let send = |m| {
                let _ = tx.unbounded_send(m);
            };
            let cfg = config::current().song;
            let found =
                music::listen(cfg.source, &stop, &mut |s| send(Msg::Heard(s))).and_then(|clip| {
                    send(Msg::Sent);
                    music::identify(&clip) // the clip is dropped here, never stored
                });
            let recent = history::path()
                .map(|p| history::after_search(&p, cfg.history, found.as_ref().ok()))
                .unwrap_or_default();
            let cover = found.as_ref().ok().and_then(|s| s.cover.clone());
            send(Msg::Done(found, recent));
            if let Some((w, h, rgba)) = cover.as_deref().and_then(music::cover) {
                send(Msg::Cover(image::Handle::from_rgba(w, h, rgba)));
            }
        });
        let session = Session {
            state: State::Listening(0),
            recent: Vec::new(),
            cancel,
        };
        (session, work)
    }

    pub fn update(&mut self, msg: Msg) -> Outcome {
        match msg {
            Msg::Heard(s) => {
                if let State::Listening(_) = self.state {
                    self.state = State::Listening(s);
                }
            }
            Msg::Sent => self.state = State::Looking,
            Msg::Done(Ok(song), recent) => {
                self.recent = recent;
                self.state = State::Found(song, None);
            }
            Msg::Done(Err(music::Error::Cancelled), _) => return Outcome::Close,
            Msg::Done(Err(e), recent) => {
                self.recent = recent;
                self.state = State::Failed(e.to_string());
            }
            Msg::Cover(handle) => {
                if let State::Found(_, cover) = &mut self.state {
                    *cover = Some(handle);
                }
            }
            Msg::Cancel => return Outcome::Close,
            Msg::Retry => return Outcome::Retry,
            Msg::Open(url) => return Outcome::Open(url),
            Msg::Card(card::Event::Close) => return Outcome::Close,
            Msg::Card(_) => {}
        }
        Outcome::None
    }

    /// Over the frozen frame, at the bottom centre where the music button was:
    /// the listening pill, then the result card (the shared floating card, so it
    /// looks like Ask AI's). The App drops any result card first, so nothing
    /// overlaps it.
    pub fn view(&self, screen: Size) -> Element<'_, Msg> {
        let (body, footer): (Element<'_, Msg>, Option<Element<'_, Msg>>) = match &self.state {
            State::Listening(heard) => {
                return pill(status(format!(
                    "Listening… {} s",
                    LISTEN_SECS.saturating_sub(*heard)
                )));
            }
            State::Looking => return pill(status("Identifying…".to_string())),
            State::Found(song, cover) => (self.result(song, cover.as_ref()), None),
            State::Failed(why) => {
                let retry =
                    widgets::text_button(MUSIC, "Try again", String::new(), true, Msg::Retry);
                (widgets::body(why.as_str()).into(), Some(retry.into()))
            }
        };
        let header = card::Header {
            glyph: MUSIC,
            title: "Song search",
            chip: None,
        };
        super::panel::card(
            header,
            body,
            footer,
            self.card_at(screen),
            screen,
            false,
            Msg::Card,
        )
    }

    /// Bottom centre, `card::SIDE_WIDTH` wide (narrower screens: inset by `card::GAP`).
    fn card_at(&self, screen: Size) -> Rectangle {
        let extra = match &self.state {
            State::Found(..) => self.recent.len().min(RECENT) as f32 * RECENT_LINE,
            _ => 0.0,
        };
        let width = card::SIDE_WIDTH
            .min(screen.width - 2.0 * card::GAP)
            .max(0.0);
        let height = (CARD_HEIGHT + extra).min(screen.height - 2.0 * card::GAP);
        Rectangle::new(
            Point::new(
                (screen.width - width) / 2.0,
                screen.height - card::GAP - height,
            ),
            Size::new(width, height),
        )
    }

    fn result<'a>(&'a self, song: &'a Song, cover: Option<&image::Handle>) -> Element<'a, Msg> {
        let art: Element<'a, Msg> = match cover {
            Some(h) => image(h.clone()).width(COVER).height(COVER).into(),
            None => widgets::surface(
                container(widgets::icon(MUSIC).size(32))
                    .center(COVER - 2.0 * widgets::SURFACE_PADDING),
            )
            .into(),
        };
        let mut words = column![
            widgets::heading(song.title.as_str()),
            widgets::body(song.artist.as_str())
        ]
        .spacing(4);
        let detail = song.detail();
        if !detail.is_empty() {
            words = words.push(widgets::caption(detail));
        }
        let top = row![art, words.width(Fill)].spacing(16);
        let links = row(song
            .links
            .iter()
            .map(|(label, url)| widgets::chip(*label, Msg::Open(url.clone())).into()))
        .spacing(8);
        let mut body = column![top, links].spacing(16);
        let recent: Vec<_> = self
            .recent
            .iter()
            .filter(|e| e.title != song.title || e.artist != song.artist)
            .take(RECENT)
            .collect();
        if !recent.is_empty() {
            body = body.push(widgets::caption("Recent"));
            for e in recent {
                body = body.push(widgets::caption(format!("{} · {}", e.title, e.artist)));
            }
        }
        body.into()
    }
}

/// The bottom centre, like the music button it replaces (opaque: a press on
/// it never starts a selection).
fn pill<'a>(content: Element<'a, Msg>) -> Element<'a, Msg> {
    container(opaque(content))
        .center_x(Fill)
        .align_bottom(Fill)
        .padding(24)
        .into()
}

/// The listening / identifying pill: a note, the text, Cancel.
fn status<'a>(label: String) -> Element<'a, Msg> {
    let cancel = widgets::text_button(widgets::CLOSE, "Cancel", String::new(), false, Msg::Cancel);
    widgets::surface(
        row![
            widgets::icon(MUSIC),
            widgets::body(label).width(150),
            cancel
        ]
        .spacing(12)
        .padding([0, 10])
        .align_y(Center),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    const SCREEN: Size = Size::new(1280.0, 720.0);

    fn session(state: State) -> Session {
        Session {
            state,
            recent: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn song() -> Song {
        Song {
            title: "Wallpaper".into(),
            artist: "Kevin MacLeod".into(),
            album: "Royalty Free".into(),
            year: "2014".into(),
            cover: None,
            links: vec![
                ("Spotify", "https://open.spotify.com/track/x".into()),
                (
                    "YouTube",
                    "https://www.youtube.com/results?search_query=a".into(),
                ),
            ],
        }
    }

    #[test]
    fn the_music_glyph_is_in_the_font() {
        let script = include_str!("../../assets/lucide/subset.sh");
        assert!(
            script.contains(&format!("U+{:04X}", MUSIC as u32)),
            "glyph in the font"
        );
    }

    #[test]
    fn listening_counts_down_then_shows_the_song() {
        let mut s = session(State::Listening(0));
        let texts = widgets::shot::texts(s.view(SCREEN));
        assert!(texts.iter().any(|t| t == "Listening… 10 s"), "{texts:?}");
        s.update(Msg::Heard(2));
        let texts = widgets::shot::texts(s.view(SCREEN));
        assert!(texts.iter().any(|t| t == "Listening… 8 s"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Cancel"));
        widgets::shot::save("song-listening", s.view(SCREEN), SCREEN);
        s.update(Msg::Sent);
        assert!(widgets::shot::texts(s.view(SCREEN)).contains(&"Identifying…".to_string()));
        let recent = vec![history::Entry {
            title: "Carefree".into(),
            artist: "Kevin MacLeod".into(),
        }];
        assert_eq!(s.update(Msg::Done(Ok(song()), recent)), Outcome::None);
        let texts = widgets::shot::texts(s.view(SCREEN));
        for t in [
            "Wallpaper",
            "Kevin MacLeod",
            "Royalty Free · 2014",
            "Spotify",
            "YouTube",
        ] {
            assert!(texts.iter().any(|x| x == t), "{t} in {texts:?}");
        }
        assert!(
            texts.iter().any(|t| t == "Carefree · Kevin MacLeod"),
            "recent"
        );
        widgets::shot::save("song-card", s.view(SCREEN), SCREEN);
        // A late Heard doesn't undo the result.
        s.update(Msg::Heard(9));
        assert!(matches!(s.state, State::Found(..)));
        // Links open only on click, through the App (lens::open_link).
        let url = "https://open.spotify.com/track/x".to_string();
        assert_eq!(s.update(Msg::Open(url.clone())), Outcome::Open(url));
    }

    #[test]
    fn errors_show_with_try_again_and_cancel_stops_the_capture() {
        let mut s = session(State::Listening(3));
        s.update(Msg::Done(Err(music::Error::Silent), Vec::new()));
        let texts = widgets::shot::texts(s.view(SCREEN));
        assert!(
            texts.iter().any(|t| t.starts_with("No sound is playing")),
            "{texts:?}"
        );
        assert!(texts.iter().any(|t| t == "Try again"));
        widgets::shot::save("song-error", s.view(SCREEN), SCREEN);
        assert_eq!(s.update(Msg::Retry), Outcome::Retry);
        assert_eq!(s.update(Msg::Cancel), Outcome::Close);
        let flag = s.cancel.clone();
        drop(s);
        assert!(
            flag.load(Ordering::Relaxed),
            "dropping the session stops listening"
        );
        // A cancelled search just closes.
        let mut s = session(State::Listening(1));
        assert_eq!(
            s.update(Msg::Done(Err(music::Error::Cancelled), Vec::new())),
            Outcome::Close
        );
    }
}
