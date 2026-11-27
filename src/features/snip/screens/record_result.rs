//! S2, the corner thumbnail after a recording stops (approved design, style E):
//! a small picture with the duration chip and a thin timer line; on hover the
//! four actions Copy, Save, Edit in OpenCircle Studio (accent), Open with (the
//! system's own chooser). It slides away after `SLIDE` and waits while the
//! pointer is on it. A recording with no picture (audio only) is the small
//! waveform chip. A press and drag takes the file out (the App routes it).
#![allow(dead_code)] // wired into the App in this branch's last commit
use std::time::Duration;

use iced::widget::{image, mouse_area};
use iced::{Element, Size, mouse};

use crate::features::snip::record::clock;
use crate::shared::kit::icon::Icon;
use crate::shared::kit::recording as parts;
use crate::shared::kit::tooltip::tip;
use crate::t;

/// The video thumbnail (`.thumb`) and the audio chip (`.achip`).
pub const THUMB: Size = Size::new(236.0, 140.0);
pub const CHIP: Size = Size::new(268.0, 60.0);
pub const RADIUS: f32 = 16.0;
pub const CHIP_RADIUS: f32 = 18.0;
/// How long it stays before it slides away.
/// lucide "copy".
const COPY: char = '\u{e0a2}';
pub const SLIDE: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    Copy,
    Save,
    /// Edit in OpenCircle Studio.
    Edit,
    /// The system's chooser.
    OpenWith,
    Hover(bool),
    /// A press on the picture: a drag takes the file out.
    DragOut,
}

#[derive(Debug, Clone)]
pub struct Thumb {
    pub audio_only: bool,
    pub duration: Duration,
    /// The first frame (none: the design's placeholder gradient is not drawn, a flat tile is).
    pub poster: Option<image::Handle>,
    /// The waveform's bars (0..1) of an audio recording.
    pub wave: Vec<f32>,
    pub hover: bool,
    /// Time shown so far; the timer line is what is left of `SLIDE`.
    pub shown: Duration,
}

impl Thumb {
    pub fn new(audio_only: bool, duration: Duration) -> Thumb {
        Thumb {
            audio_only,
            duration,
            poster: None,
            wave: Vec::new(),
            hover: false,
            shown: Duration::ZERO,
        }
    }

    pub fn size(&self) -> Size {
        match self.audio_only {
            true => CHIP,
            false => THUMB,
        }
    }

    pub fn radius(&self) -> f32 {
        match self.audio_only {
            true => CHIP_RADIUS,
            false => RADIUS,
        }
    }

    /// `dt` passed: the timer runs unless the pointer is on it. `true`: it has slid away.
    pub fn tick(&mut self, dt: Duration) -> bool {
        if !self.hover {
            self.shown += dt;
        }
        self.shown >= SLIDE
    }

    /// What is left of the timer line, 0..1.
    pub fn left(&self) -> f32 {
        (1.0 - self.shown.as_secs_f32() / SLIDE.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn update(&mut self, msg: Msg) {
        if let Msg::Hover(on) = msg {
            self.hover = on;
        }
    }

    pub fn view(&self, corners: f32) -> Element<'_, Msg> {
        let body = match self.audio_only {
            true => self.chip(corners),
            false => self.video(corners),
        };
        mouse_area(body)
            .on_enter(Msg::Hover(true))
            .on_exit(Msg::Hover(false))
            .into()
    }

    fn actions(&self) -> Element<'_, Msg> {
        let below = iced::widget::tooltip::Position::Bottom;
        let words = [
            (COPY, t!(rec_copy), false, Msg::Copy),
            (Icon::Download.glyph(), t!(rec_save), false, Msg::Save),
            (Icon::Picture.glyph(), t!(rec_edit), true, Msg::Edit),
            (
                Icon::External.glyph(),
                t!(rec_open_with),
                false,
                Msg::OpenWith,
            ),
        ];
        let items = words
            .into_iter()
            .map(|(glyph, words, primary, msg)| {
                tip(parts::action(glyph, primary, msg), words, below)
            })
            .collect();
        parts::actions(items)
    }

    fn video(&self, corners: f32) -> Element<'_, Msg> {
        let chip = parts::duration(Icon::Video, &clock::short(self.duration));
        let actions = self.hover.then(|| self.actions());
        let picture = parts::thumb(corners, self.poster.as_ref(), chip, actions, self.left());
        mouse_area(picture)
            .on_press(Msg::DragOut)
            .interaction(mouse::Interaction::Grab)
            .into()
    }

    fn chip(&self, corners: f32) -> Element<'_, Msg> {
        let end = match self.hover {
            true => self.actions(),
            false => parts::time(clock::short(self.duration)),
        };
        let chip = parts::chip(
            corners,
            parts::wave(&self.wave, self.wave.len() * 3 / 4),
            end,
        );
        mouse_area(chip)
            .on_press(Msg::DragOut)
            .interaction(mouse::Interaction::Grab)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::kit::testing::{arabic, golden, shot};
    use iced::Point;

    fn video() -> Thumb {
        let mut t = Thumb::new(false, Duration::from_secs(12));
        // a 2x2 poster: its colours stand for the first frame
        let px = [
            30u8, 58, 138, 255, 124, 58, 237, 255, 124, 58, 237, 255, 236, 72, 153, 255,
        ];
        t.poster = Some(image::Handle::from_rgba(2, 2, px.to_vec()));
        t.shown = Duration::from_secs(3);
        t
    }

    fn audio() -> Thumb {
        let mut t = Thumb::new(true, Duration::from_secs(12));
        t.wave = [
            6, 10, 16, 22, 12, 8, 18, 24, 14, 9, 20, 26, 12, 7, 15, 22, 10, 6, 13, 19, 8, 5, 11,
            17, 7, 4, 9, 14, 6, 4,
        ]
        .iter()
        .map(|h| *h as f32 / 28.0)
        .collect();
        t
    }

    /// R02-7: the thumbnail and the audio chip match the approved design
    /// (video, video on hover, audio, audio on hover; English and Arabic).
    #[test]
    fn r02_7_the_thumbnail_matches_its_golden_images() {
        golden::each_theme(|theme, failed| {
            for rtl in [false, true] {
                let _ar = rtl.then(arabic);
                let mut v_hover = video();
                v_hover.hover = true;
                let mut a_hover = audio();
                a_hover.hover = true;
                let cases = [
                    ("video", video()),
                    ("video-hover", v_hover),
                    ("audio", audio()),
                    ("audio-hover", a_hover),
                ];
                let ar = if rtl { "-ar" } else { "" };
                for (name, t) in cases {
                    let file = format!("record-thumb-{name}{ar}-{}", theme.id);
                    failed.extend(golden::check("screens", &file, t.size(), &[1.0], || {
                        t.view(t.radius())
                    }));
                }
            }
        });
    }

    /// R02-8: it slides away after `SLIDE`, and the timer waits while the pointer is on it.
    #[test]
    fn r02_8_it_slides_away_unless_hovered() {
        let mut t = video();
        t.shown = Duration::ZERO;
        t.update(Msg::Hover(true));
        assert!(!t.tick(SLIDE * 3), "hovered: stays");
        assert_eq!(t.left(), 1.0);
        t.update(Msg::Hover(false));
        assert!(!t.tick(SLIDE / 2));
        assert!((t.left() - 0.5).abs() < 0.01);
        assert!(t.tick(SLIDE / 2), "gone after SLIDE");
    }

    /// R02-9: the four hover actions send Copy, Save, Edit and Open with.
    #[test]
    fn r02_9_the_hover_actions_send_their_messages() {
        let mut t = video();
        t.hover = true;
        let size = t.size();
        let all: Vec<Msg> = (0..size.width as u32)
            .step_by(2)
            .flat_map(|x| shot::click(t.view(RADIUS), size, Point::new(x as f32, 24.0)))
            .collect();
        for want in [Msg::Copy, Msg::Save, Msg::Edit, Msg::OpenWith] {
            assert!(all.contains(&want), "{want:?}: {all:?}");
        }
        // no actions until hovered
        t.hover = false;
        let none: Vec<Msg> = (0..size.width as u32)
            .step_by(2)
            .flat_map(|x| shot::click(t.view(RADIUS), size, Point::new(x as f32, 24.0)))
            .filter(|m| !matches!(m, Msg::DragOut | Msg::Hover(_)))
            .collect();
        assert!(none.is_empty(), "{none:?}");
    }

    /// R02-10: audio only is the small chip, a video the thumbnail.
    #[test]
    fn r02_10_audio_is_the_small_chip() {
        assert_eq!(audio().size(), CHIP);
        assert_eq!(video().size(), THUMB);
        assert!(audio().size().height < video().size().height);
    }
}
