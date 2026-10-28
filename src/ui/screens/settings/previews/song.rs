//! Song search: the sound levels move while it listens, then the song's name
//! shows.
use iced::widget::{container, row};
use iced::{Center, Element, Fill};

use super::super::state::Message;
use crate::ui::kit::hero::{glyph, screen, step, wave};
use crate::ui::kit::icon::Icon;
use crate::ui::kit::text::{accent, sub};
use crate::ui::kit::tokens::{Role, space};

pub const STEPS: [&str; 5] = [
    "Choose what to listen to",
    "Press Identify",
    "Listen to a short clip",
    "Find the song",
    "See the title and artist",
];

/// The levels at rest, left to right (`!h-5 !h-8 · !h-8 !h-5`).
const REST: [f32; 4] = [0.55, 1.0, 1.0, 0.55];
/// How fast the levels move while it listens (a turn a second).
const PACE: f32 = std::f32::consts::TAU;

pub fn scene(at: f32) -> Element<'static, Message> {
    let s = step(at);
    // listening (1-3 s): each level moves on its own beat
    let level = |i: usize| match s {
        1..=3 => 0.5 + 0.5 * (at * PACE * (1.0 + i as f32 * 0.37)).sin().abs(),
        _ => REST[i],
    };
    let mut levels = row![wave(level(0)), wave(level(1))]
        .spacing(space::TRACK_PAD)
        .align_y(Center);
    levels = levels.push(screen(glyph(Icon::Music)));
    levels = levels.push(wave(level(2))).push(wave(level(3)));
    let mut all = row![levels].spacing(space::GAP * 2.0).align_y(Center);
    if s >= 3 {
        all = all.push(
            row![
                sub(Role::CARD_DESC, "Identified:"),
                accent(Role::META_STRONG, "Northern Lights · The Example Band"),
            ]
            .spacing(space::TRACK_PAD)
            .align_y(Center),
        );
    }
    container(all).center(Fill).into()
}
