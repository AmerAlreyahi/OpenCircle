//! R1, the recording bar (approved design, style E): a pill at the top centre of
//! the recorded screen with the red dot and the timer, Pause / Resume, Stop,
//! the bin, the microphone button (mute on a click of its mic, the device list
//! on its name), and the capture size and fps; in an audio recording the level
//! bars instead of the size. Ready (before the first frame) shows the size, the
//! microphone, a red Record and a quiet x. It is dragged by its body and
//! remembered for the session (`Bar::spot`). Its error is the one calm error card
//! under it. Placed through `windows::floating`, out of captures like every
//! floating widget of ours.
use std::time::Duration;

use iced::widget::{column, mouse_area};
use iced::{Element, Point, Size, mouse};

use crate::features::snip::record::clock;
use crate::features::snip::record::state::Fault;
use crate::shared::kit::button::regular;
use crate::shared::kit::error_card::{self, ErrorCard};
use crate::shared::kit::icon::Icon;
use crate::shared::kit::option_list;
use crate::shared::kit::tokens::Role;
use crate::shared::kit::tooltip::tip;
use crate::shared::kit::{dir, icon_button, recording as parts, text};
use crate::t;

/// The bar's corners (`.rec`: 26).
pub const RADIUS: f32 = 26.0;
/// The bar's width by what it shows, so it does not breathe as the time counts.
const W_VIDEO: f32 = 540.0;
const W_AUDIO: f32 = 480.0;
const W_PAUSED: f32 = 76.0;
const W_READY: f32 = 500.0;
/// The error card's width and the gap under the bar.
const CARD_W: f32 = 420.0;
const GAP: f32 = parts::GAP;
/// Space between the keep card's parts.
const PAD: f32 = 8.0;
/// A group heading of the device list.
const HEADING: f32 = 30.0;
/// The distance from the screen's top edge (the design: 14 px).
const TOP: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before the first frame: Record / x.
    Ready,
    Recording,
    Paused,
    /// Stopping: flushing the file.
    Saving,
    Error(Fault),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Record,
    Cancel,
    Pause,
    Resume,
    Stop,
    Discard,
    /// The device name: open / close the list.
    Menu,
    /// A microphone of the list (`None`: off).
    Mic(Option<usize>),
    /// Sound from the PC on / off.
    Sound(bool),
    /// The error card's fix (record without a microphone) and Try again.
    Fix,
    Retry,
    /// The keep card: Keep (Discard is the bin's).
    Keep,
    /// A press on the bar's body: the window is dragged.
    Grab,
}

/// What the bar shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub phase: Phase,
    pub audio_only: bool,
    pub elapsed: Duration,
    pub muted: bool,
    /// The microphones and the one in use (`None`: off); the PC sound's name and whether it is on.
    pub mics: Vec<String>,
    pub mic: Option<usize>,
    pub sound: Option<String>,
    pub sound_on: bool,
    pub menu: bool,
    /// Esc was pressed: the Keep / Discard card hangs under the bar.
    pub ask: bool,
    pub size: (u32, u32),
    pub fps: u32,
    /// Level bars (0..1) of an audio recording.
    pub levels: Vec<f32>,
}

impl Bar {
    pub fn new(audio_only: bool) -> Bar {
        Bar {
            phase: Phase::Ready,
            audio_only,
            elapsed: Duration::ZERO,
            muted: false,
            mics: Vec::new(),
            mic: None,
            sound: None,
            sound_on: false,
            menu: false,
            ask: false,
            size: (1920, 1080),
            fps: 30,
            levels: Vec::new(),
        }
    }

    /// The device button's words.
    fn fps_words(&self) -> String {
        format!("{} {}", self.fps, t!(rec_fps))
    }

    fn device_label(&self) -> String {
        match (self.muted, self.mic.and_then(|i| self.mics.get(i))) {
            (true, _) => t!(rec_mic_muted).to_string(),
            (false, Some(name)) => name.clone(),
            (false, None) => t!(rec_mic).to_string(),
        }
    }

    fn bar_width(&self) -> f32 {
        match (self.phase, self.audio_only) {
            (Phase::Ready, _) => W_READY,
            (Phase::Paused, true) => W_AUDIO + W_PAUSED,
            (Phase::Paused, false) => W_VIDEO + W_PAUSED,
            (_, true) => W_AUDIO,
            (_, false) => W_VIDEO,
        }
    }

    fn menu_rows(&self) -> (usize, usize) {
        match self.sound.is_some() {
            true => (self.mics.len() + 3, 1),
            false => (self.mics.len() + 1, 0),
        }
    }

    /// The window's size: the bar, and the device list or the error card under it.
    pub fn size(&self) -> Size {
        let under = match (self.menu, self.phase) {
            (true, _) => {
                let (items, lines) = self.menu_rows();
                GAP + option_list::floating_menu_height(items, lines) + HEADING * (lines + 1) as f32
            }
            (false, Phase::Error(_)) => GAP + 150.0,
            _ if self.ask => GAP + 100.0,
            _ => 0.0,
        };
        let width = match (self.menu, self.phase) {
            (_, Phase::Error(_)) => self.bar_width().max(CARD_W),
            _ => self.bar_width(),
        };
        Size::new(width, parts::BAR_H + under)
    }

    /// Where the window goes by default: the top centre of `screen` (logical
    /// origin and size), `TOP` px down.
    pub fn default_spot(&self, screen: iced::Rectangle) -> Point {
        let width = self.size().width;
        Point::new(screen.x + (screen.width - width) / 2.0, screen.y + TOP)
    }

    pub fn view(&self, corners: f32) -> Element<'_, Msg> {
        let under = match (self.menu, self.phase) {
            (true, _) => Some(self.devices()),
            (false, Phase::Error(fault)) => Some(self.error(fault)),
            _ => None,
        };
        let under = match (self.ask, under) {
            (true, _) => Some(self.keep_card()),
            (false, under) => under,
        };
        mouse_area(parts::hung(self.bar(corners), under))
            .on_press(Msg::Grab)
            .interaction(mouse::Interaction::Grab)
            .into()
    }

    fn bar(&self, corners: f32) -> Element<'_, Msg> {
        let below = iced::widget::tooltip::Position::Bottom;
        let mut items: Vec<Element<'_, Msg>> = Vec::new();
        match self.phase {
            Phase::Ready => {
                items.push(parts::meta(self.size.0, self.size.1, self.fps_words()));
                items.push(parts::group_line());
                items.push(self.device_button());
                items.push(parts::record_button(t!(rec_record), Msg::Record));
                items.push(tip(
                    icon_button::quiet_close(false, Some(Msg::Cancel)),
                    t!(rec_cancel),
                    below,
                ));
            }
            _ => {
                let paused = matches!(self.phase, Phase::Paused | Phase::Error(_));
                let live = matches!(self.phase, Phase::Recording | Phase::Paused);
                items.push(parts::dot(paused));
                items.push(parts::timer(&clock::format(self.elapsed)));
                if self.phase == Phase::Paused {
                    items.push(parts::paused_tag(t!(rec_paused)));
                }
                items.push(parts::group_line());
                items.push(match self.phase {
                    Phase::Paused => tip(
                        icon_button::pill_icon(
                            Icon::Play.glyph(),
                            icon_button::State {
                                active: true,
                                focused: false,
                            },
                            Some(Msg::Resume),
                        ),
                        t!(rec_resume),
                        below,
                    ),
                    _ => tip(
                        icon_button::tool_icon(
                            Icon::Pause.glyph(),
                            icon_button::State::default(),
                            (self.phase == Phase::Recording).then_some(Msg::Pause),
                        ),
                        t!(rec_pause),
                        below,
                    ),
                });
                items.push(tip(
                    parts::stop(live.then_some(Msg::Stop)),
                    t!(rec_stop),
                    below,
                ));
                items.push(tip(
                    icon_button::tool_icon(
                        Icon::Trash.glyph(),
                        icon_button::State::default(),
                        live.then_some(Msg::Discard),
                    ),
                    t!(rec_discard),
                    below,
                ));
                items.push(parts::group_line());
                items.push(self.device_button());
                items.push(parts::group_line());
                items.push(match self.audio_only {
                    true => parts::levels(&self.levels),
                    false => parts::meta(self.size.0, self.size.1, self.fps_words()),
                });
            }
        }
        parts::bar(corners, self.bar_width(), items)
    }

    fn device_button(&self) -> Element<'_, Msg> {
        tip(
            parts::device(self.muted, &self.device_label(), self.menu, Msg::Menu),
            t!(rec_mic_pick),
            iced::widget::tooltip::Position::Bottom,
        )
    }

    /// The list under the bar (`devmenu`): the microphones and Off, then the PC's sound.
    fn devices(&self) -> Element<'_, Msg> {
        let mut rows: Vec<Element<'_, Msg>> = vec![parts::heading(t!(rec_mic))];
        for (i, name) in self.mics.iter().enumerate() {
            let on = self.mic == Some(i) && !self.muted;
            rows.push(option_list::floating_item(
                name,
                on,
                None,
                false,
                on,
                Msg::Mic(Some(i)),
            ));
        }
        let off = self.mic.is_none() || self.muted;
        rows.push(option_list::floating_item(
            t!(rec_off),
            off,
            None,
            false,
            off,
            Msg::Mic(None),
        ));
        if let Some(sound) = &self.sound {
            rows.push(option_list::separator());
            rows.push(parts::heading(t!(rec_sound)));
            rows.push(option_list::floating_item(
                sound,
                self.sound_on,
                None,
                false,
                self.sound_on,
                Msg::Sound(true),
            ));
            rows.push(option_list::floating_item(
                t!(rec_off),
                !self.sound_on,
                None,
                false,
                !self.sound_on,
                Msg::Sound(false),
            ));
        }
        option_list::floating_menu_width(280.0, rows)
    }

    fn error(&self, fault: Fault) -> Element<'_, Msg> {
        let (headline, hint, fix) = match fault {
            Fault::Mic => (
                t!(rec_err_mic),
                t!(rec_err_mic_d),
                Some((t!(rec_err_mic_fix), Msg::Fix)),
            ),
            _ => (t!(rec_err_refused), t!(rec_err_refused_d), None),
        };
        let card = error_card::error_card(ErrorCard {
            headline: headline.into(),
            hint: hint.into(),
            detail: String::new(),
            width: CARD_W,
            open: false,
            on_details: Msg::Retry,
            fix,
            retry: Some(Msg::Retry),
            words: error_card::Words {
                show: "",
                hide: "",
                retry: t!(rec_try_again),
            },
        });
        parts::card_under(CARD_W, card)
    }

    /// Esc while recording: Keep or Discard.
    fn keep_card(&self) -> Element<'_, Msg> {
        let card = column![
            text::text(Role::ROW_TITLE, t!(rec_keep_h)),
            text::text(
                Role::ROW_DESC,
                t!(rec_keep_d).replace("{}", &clock::format(self.elapsed))
            ),
            dir::row(vec![
                regular(t!(rec_keep), false, Some(Msg::Keep)),
                regular(t!(rec_discard_btn), false, Some(Msg::Discard)),
            ])
            .spacing(PAD),
        ]
        .spacing(PAD);
        parts::card_under(CARD_W, card.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::{arabic, golden, shot};
    use iced::Rectangle;

    fn bar(phase: Phase, audio: bool) -> Bar {
        let mut b = Bar::new(audio);
        b.phase = phase;
        b.elapsed = Duration::from_secs(12);
        b.mics = vec!["Realtek Microphone Array".into(), "USB Headset Mic".into()];
        b.mic = Some(0);
        b.sound = Some("Speakers (Realtek)".into());
        b.sound_on = true;
        b.levels = vec![0.3, 0.6, 0.9, 0.5, 0.7, 0.4, 0.2];
        b
    }

    /// R02-1: the bar is drawn by the approved design in every state, both
    /// themes, English and Arabic (goldens under tests/fixtures/screens).
    #[test]
    fn r02_1_the_bar_matches_its_golden_images() {
        golden::each_theme(|theme, failed| {
            for rtl in [false, true] {
                let _ar = rtl.then(arabic);
                let mut muted = bar(Phase::Recording, false);
                muted.muted = true;
                let mut open = bar(Phase::Recording, false);
                open.menu = true;
                let cases = [
                    ("ready", bar(Phase::Ready, false)),
                    ("recording", bar(Phase::Recording, false)),
                    ("paused", bar(Phase::Paused, false)),
                    ("audio", bar(Phase::Recording, true)),
                    ("muted", muted),
                    ("devices", open),
                    ("error-mic", bar(Phase::Error(Fault::Mic), false)),
                    ("error-refused", bar(Phase::Error(Fault::Refused), false)),
                ];
                let ar = if rtl { "-ar" } else { "" };
                for (name, b) in cases {
                    let file = format!("record-bar-{name}{ar}-{}", theme.id);
                    failed.extend(golden::check("screens", &file, b.size(), &[1.0], || {
                        b.view(RADIUS)
                    }));
                }
            }
        });
    }

    /// R02-2: Pause, Stop, the bin, the mic and Resume send their messages.
    #[test]
    fn r02_2_the_buttons_send_their_messages() {
        let rec = bar(Phase::Recording, false);
        let hit = |b: &Bar, x: f32| shot::click(b.view(RADIUS), b.size(), Point::new(x, 26.0));
        let all: Vec<Msg> = (0..rec.size().width as u32)
            .step_by(3)
            .flat_map(|x| hit(&rec, x as f32))
            .collect();
        for want in [Msg::Pause, Msg::Stop, Msg::Discard, Msg::Menu] {
            assert!(all.contains(&want), "{want:?} not reachable: {all:?}");
        }
        let paused = bar(Phase::Paused, false);
        let all: Vec<Msg> = (0..paused.size().width as u32)
            .step_by(3)
            .flat_map(|x| hit(&paused, x as f32))
            .collect();
        assert!(all.contains(&Msg::Resume) && !all.contains(&Msg::Pause));
        let ready = bar(Phase::Ready, false);
        let all: Vec<Msg> = (0..ready.size().width as u32)
            .step_by(3)
            .flat_map(|x| hit(&ready, x as f32))
            .collect();
        assert!(all.contains(&Msg::Record) && all.contains(&Msg::Cancel));
    }

    /// R02-3: the bar starts at the top centre of the recorded screen, and a
    /// second screen's origin moves it with the screen.
    #[test]
    fn r02_3_the_bar_opens_at_the_top_centre_of_its_screen() {
        let b = bar(Phase::Recording, false);
        let screen = Rectangle::new(Point::new(0.0, 0.0), Size::new(1920.0, 1080.0));
        let at = b.default_spot(screen);
        assert_eq!(at.y, TOP);
        assert!((at.x + b.size().width / 2.0 - 960.0).abs() < 0.01);
        let second = Rectangle::new(Point::new(1920.0, 0.0), Size::new(1280.0, 720.0));
        assert!((b.default_spot(second).x + b.size().width / 2.0 - 2560.0).abs() < 0.01);
    }

    /// R02-4: a press on the bar's body starts the window drag.
    #[test]
    fn r02_4_a_press_on_the_body_grabs_the_bar() {
        let b = bar(Phase::Recording, false);
        // the far end of the bar is its quiet readout: nothing there but the body
        let got = shot::click(
            b.view(RADIUS),
            b.size(),
            Point::new(b.size().width - 4.0, 26.0),
        );
        assert!(got.contains(&Msg::Grab), "{got:?}");
    }

    /// R02-5: the device button says what is on: the name, "Mic muted", or the
    /// plain word when nothing is chosen.
    #[test]
    fn r02_5_the_device_button_says_what_is_on() {
        let mut b = bar(Phase::Recording, false);
        assert_eq!(b.device_label(), "Realtek Microphone Array");
        b.muted = true;
        assert_eq!(b.device_label(), "Mic muted");
        b.muted = false;
        b.mic = None;
        assert_eq!(b.device_label(), "Microphone");
    }

    /// R02-6: the device list and the error card grow the window under the bar;
    /// an audio recording's bar is narrower than a video's (no size readout).
    #[test]
    fn r02_6_the_window_grows_for_the_list_and_the_card() {
        let plain = bar(Phase::Recording, false);
        let mut open = plain.clone();
        open.menu = true;
        assert!(open.size().height > plain.size().height + 100.0);
        assert!(bar(Phase::Error(Fault::Mic), false).size().height > plain.size().height + 80.0);
        assert!(bar(Phase::Recording, true).size().width < plain.size().width);
        assert!(bar(Phase::Paused, false).size().width > plain.size().width);
    }
}
