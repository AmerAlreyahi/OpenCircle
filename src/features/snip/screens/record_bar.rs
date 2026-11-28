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
    /// The silent-microphone card's buttons: open OS microphone privacy settings, or dismiss.
    MicSettings,
    DismissSilent,
    /// A press on the bar's body: the window is dragged.
    Grab,
    /// The Hide group of the list (before Record and while paused): drag an area to hide, pick
    /// an app to hide and follow, show everything again.
    HideArea,
    HideApp,
    HideClear,
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
    /// True when an unmuted mic is delivering all-zero packets (shows calm notice card).
    pub mic_silent: bool,
    pub size: (u32, u32),
    pub fps: u32,
    /// Level bars (0..1) of an audio recording.
    pub levels: Vec<f32>,
    /// The countdown's number (3, 2, 1) while it runs: it replaces the Record button's words.
    pub count: Option<u8>,
    /// The words behind an error (the missing plugins' names): the Encoder card's hint.
    pub note: String,
    /// Zones hidden in the recording (R-06), and whether this desktop can name an app's window
    /// to follow (native Wayland cannot: areas only).
    pub zones: usize,
    pub can_hide_app: bool,
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
            mic_silent: false,
            size: (1920, 1080),
            fps: 30,
            levels: Vec::new(),
            count: None,
            note: String::new(),
            zones: 0,
            can_hide_app: false,
        }
    }

    /// The Hide group is in the list: a picture recording that has not started or is paused.
    pub fn hides(&self) -> bool {
        !self.audio_only && matches!(self.phase, Phase::Ready | Phase::Paused)
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
        let (items, lines) = match self.sound.is_some() {
            true => (self.mics.len() + 3, 1),
            false => (self.mics.len() + 1, 0),
        };
        match self.hides() {
            true => (
                items + 1 + usize::from(self.can_hide_app) + usize::from(self.zones > 0),
                lines + 1,
            ),
            false => (items, lines),
        }
    }

    fn shows_silent_card(&self) -> bool {
        self.mic_silent && !self.menu && matches!(self.phase, Phase::Recording | Phase::Paused)
    }

    /// The window's size: the bar, and the device list or the error/silent card under it.
    pub fn size(&self) -> Size {
        let under = match (self.menu, self.phase) {
            (true, _) => {
                let (items, lines) = self.menu_rows();
                GAP + option_list::floating_menu_height(items, lines) + HEADING * (lines + 1) as f32
            }
            (false, Phase::Error(_)) => GAP + 150.0,
            _ if self.shows_silent_card() => GAP + 128.0,
            _ => 0.0,
        };
        let width = match (self.menu, self.phase) {
            (_, Phase::Error(_)) => self.bar_width().max(CARD_W),
            _ if self.shows_silent_card() => self.bar_width().max(CARD_W),
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
            _ if self.shows_silent_card() => Some(self.silent_mic_card()),
            _ => None,
        };
        let bar = mouse_area(self.bar(corners))
            .on_press(Msg::Grab)
            .interaction(mouse::Interaction::Grab)
            .into();
        parts::hung(bar, under)
    }

    fn bar(&self, corners: f32) -> Element<'_, Msg> {
        let below = iced::widget::tooltip::Position::Bottom;
        let mut items: Vec<Element<'_, Msg>> = Vec::new();
        match self.phase {
            Phase::Ready => {
                items.push(parts::meta(self.size.0, self.size.1, self.fps_words()));
                items.push(parts::group_line());
                items.push(self.device_button());
                items.push(parts::record_button(
                    match self.count {
                        Some(3) => "3",
                        Some(2) => "2",
                        Some(1) => "1",
                        _ => t!(rec_record),
                    },
                    Msg::Record,
                ));
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
        if self.hides() {
            rows.push(option_list::separator());
            rows.push(parts::heading(t!(rec_hide)));
            let item = |label, msg| option_list::floating_item_when(label, None, false, Some(msg));
            rows.push(item(t!(rec_hide_area), Msg::HideArea));
            if self.can_hide_app {
                rows.push(item(t!(rec_hide_app), Msg::HideApp));
            }
            if self.zones > 0 {
                rows.push(item(t!(rec_hide_clear), Msg::HideClear));
            }
        }
        option_list::floating_menu_width(crate::shared::kit::tokens::widget_e::MENU_W_REC, rows)
    }

    fn error(&self, fault: Fault) -> Element<'_, Msg> {
        let (headline, hint, fix) = match fault {
            Fault::Mic => (
                t!(rec_err_mic),
                t!(rec_err_mic_d),
                Some((t!(rec_err_mic_fix), Msg::Fix)),
            ),
            Fault::Encoder => (
                t!(rec_err_encoder),
                match self.note.is_empty() {
                    true => t!(rec_err_encoder_d),
                    false => self.note.as_str(),
                },
                None,
            ),
            Fault::WriteFailed => (t!(rec_err_write), t!(rec_err_write_d), None),
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

    /// Calm notice when an unmuted microphone delivers continuous all-zero packets.
    fn silent_mic_card(&self) -> Element<'_, Msg> {
        let card = column![
            text::text(Role::ROW_TITLE, t!(rec_mic_silent_h)),
            text::text(Role::ROW_DESC, t!(rec_mic_silent_d)),
            dir::row(vec![
                regular(t!(rec_mic_settings), false, Some(Msg::MicSettings)),
                regular(t!(rec_mic_silent_dismiss), false, Some(Msg::DismissSilent)),
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
                let mut hide_ready = bar(Phase::Ready, false);
                (hide_ready.menu, hide_ready.can_hide_app) = (true, true);
                let mut hide_paused = bar(Phase::Paused, false);
                (hide_paused.menu, hide_paused.zones) = (true, 2);
                let cases = [
                    ("hide-ready", hide_ready),
                    ("hide-paused", hide_paused),
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

    /// R04-30 (owner shot 2026-11-27): the timer, the microphone's name and the size all sit on
    /// the bar's one centre line (the microphone's was a little high), in the live bar, the
    /// ready bar and the audio bar; a long device name is cut, never wider than the bar.
    #[test]
    fn r04_30_every_item_shares_the_bars_centre_line() {
        let mut long = bar(Phase::Recording, false);
        long.mics = vec!["Microphone Array (Realtek High Definition Audio) Extra Long".into()];
        let cases = [
            bar(Phase::Recording, false),
            bar(Phase::Paused, false),
            bar(Phase::Ready, false),
            bar(Phase::Recording, true),
            long,
        ];
        for b in cases {
            let size = b.size();
            let spots = shot::texts_at(b.view(RADIUS), size);
            let mid = crate::shared::kit::recording::BAR_H / 2.0;
            assert!(spots.len() >= 2, "{:?}: {spots:?}", b.phase);
            for (r, words) in &spots {
                if words.is_empty() {
                    continue;
                }
                let centre = r.y + r.height / 2.0;
                assert!(
                    (centre - mid).abs() <= 1.0,
                    "{:?}: {words:?} centred at {centre}, the bar's line is {mid}",
                    b.phase
                );
                assert!(r.x + r.width <= b.bar_width() + 0.5, "{words:?} sticks out");
            }
        }
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

    /// R06-10: the Hide group (area, app, show again) is in the list only for a picture recording
    /// that has not started or is paused; the app item only where windows can be named, the
    /// "show again" item only with a zone; and its rows send their messages.
    #[test]
    fn r06_10_the_hide_group_is_in_the_list_before_record_and_while_paused() {
        let open = |phase, audio| {
            let mut b = bar(phase, audio);
            b.menu = true;
            b
        };
        let plain = open(Phase::Recording, false);
        assert!(!plain.hides() && !open(Phase::Ready, true).hides());
        let ready = open(Phase::Ready, false);
        assert!(ready.hides() && open(Phase::Paused, false).hides());
        let h = |b: &Bar| b.size().height;
        let mut with_app = ready.clone();
        with_app.can_hide_app = true;
        let mut with_zone = with_app.clone();
        with_zone.zones = 1;
        assert!(h(&ready) > h(&plain), "the group makes the list taller");
        assert!(h(&with_app) > h(&ready) && h(&with_zone) > h(&with_app));
        let reach = |b: &Bar| -> Vec<Msg> {
            let size = b.size();
            (30..size.height as u32)
                .step_by(4)
                .flat_map(|y| {
                    shot::click(b.view(RADIUS), size, Point::new(size.width / 2.0, y as f32))
                })
                .collect()
        };
        let all = reach(&with_zone);
        for want in [Msg::HideArea, Msg::HideApp, Msg::HideClear] {
            assert!(all.contains(&want), "{want:?} not reachable");
        }
        let all = reach(&ready);
        assert!(
            all.contains(&Msg::HideArea)
                && !all.contains(&Msg::HideApp)
                && !all.contains(&Msg::HideClear)
        );
        assert!(
            !reach(&plain).contains(&Msg::HideArea),
            "not while recording"
        );
    }

    /// R06-20: a zone's outline on screen matches its golden, both themes.
    #[test]
    fn r06_20_the_zone_outline_matches_its_golden_images() {
        golden::each_theme(|theme, failed| {
            let file = format!("record-zone-outline-{}", theme.id);
            failed.extend(golden::check(
                "screens",
                &file,
                Size::new(220.0, 120.0),
                &[1.0],
                || crate::shared::kit::recording::zone_outline::<Msg>(8.0),
            ));
        });
    }

    /// R04C-3: pressing anywhere under the bar (open dropdown or card) never grabs the window,
    /// and the silent-microphone card grows the window and sends MicSettings / DismissSilent.
    #[test]
    fn r04c_3_dropdown_press_does_not_grab_and_silent_card_sends_messages() {
        let mut open = bar(Phase::Recording, false);
        open.menu = true;
        let size = open.size();
        // Sweep the entire dropdown area below the bar (y > BAR_H): Msg::Grab must NEVER fire.
        for y in ((parts::BAR_H as u32 + 4)..size.height as u32).step_by(6) {
            let got = shot::click(
                open.view(RADIUS),
                size,
                Point::new(size.width / 2.0, y as f32),
            );
            assert!(
                !got.contains(&Msg::Grab),
                "press at y={y} in dropdown must not grab the bar: {got:?}"
            );
        }

        let plain = bar(Phase::Recording, false);
        let mut silent = plain.clone();
        silent.mic_silent = true;
        assert!(
            silent.size().height > plain.size().height,
            "silent-mic card grows the window"
        );
        let ssize = silent.size();
        let mut msgs = Vec::new();
        for x in (40..ssize.width as u32).step_by(24) {
            for y in ((parts::BAR_H as u32 + 4)..ssize.height as u32).step_by(10) {
                let got = shot::click(silent.view(RADIUS), ssize, Point::new(x as f32, y as f32));
                assert!(
                    !got.contains(&Msg::Grab),
                    "press on silent card must not grab"
                );
                msgs.extend(got);
            }
        }
        assert!(
            msgs.contains(&Msg::MicSettings),
            "MicSettings button missing: {msgs:?}"
        );
        assert!(
            msgs.contains(&Msg::DismissSilent),
            "DismissSilent button missing: {msgs:?}"
        );
    }
}
