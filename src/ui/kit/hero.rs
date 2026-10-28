//! A feature's preview at the top of its Settings page (`settings-window.js`
//! `featureHero`, `.oc-feature-*`): the feature's name and one line, its
//! shortcut as keycaps, Hide / Show preview, and the stage: a "5 s preview"
//! chip, Replay, the scene, the step's caption and the timeline. The scene is
//! drawn live from the parts below (no clip); `at` (seconds into the preview,
//! 0..`LENGTH`) comes from the caller's clock: `LENGTH` is the last frame.
//!
//! The scene parts: `screen` (a pretend screen), `bar` (a line of pretend
//! text), `mark` (selected text), `ring` (the circling stroke), `pill` (a bar
//! pill or a toast, lit or not), `frame` (a captured area), `wave` (a sound
//! level).
use std::f32::consts::TAU;
use std::time::Duration;

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke};
use iced::widget::{Space, canvas as draw, column, container, row, text};
use iced::{Border, Center, Color, Element, Fill, Length, Point, Rectangle, Renderer, Theme};

use super::button::{Kind, button, label};
use super::icon::{Icon, icon};
use super::keycap::keycap;
use super::text::sub;
use super::themes::c;
use super::tokens::{Role, radius, size, space};

/// How long a preview plays; it then rests on its last frame.
pub const LENGTH: Duration = Duration::from_secs(5);
/// The stage (`.oc-feature-stage`: 152 px, plus room for the kit's taller
/// Replay button, so a two-row scene keeps its place).
pub const STAGE_H: f32 = 172.0;
/// The card's inside (`padding: 10px`) and the stage's (`8px`).
const PAD: f32 = 10.0;
const STAGE_PAD: f32 = 8.0;
/// The scene's widest (`max-width: 520px`).
const SCENE_W: f32 = 520.0;
/// The chip's, the caption's and the timeline's type (9 / 10 px).
const SMALL: f32 = 9.0;
const CAPTION: f32 = 10.0;
/// The timeline's track (3 px).
const TRACK: f32 = 3.0;
/// A keycap's sides round its label.
const CAP_PAD: f32 = 16.0;

/// What a preview shows.
pub struct Hero<'a, Message> {
    pub title: &'a str,
    pub desc: &'a str,
    /// A Labs feature: "Labs" after the name.
    pub lab: bool,
    /// The feature's shortcut, one label per key (empty = not set).
    pub keys: Vec<String>,
    /// The stage is shown (else only the heading and Show preview).
    pub shown: bool,
    pub toggle: Message,
    pub replay: Message,
    /// Seconds into the preview (`LENGTH` = the last frame).
    pub at: f32,
    /// The step under the scene.
    pub caption: &'a str,
    pub scene: Element<'a, Message>,
}

/// The step `at` is in: one a second, the last from 4 s on.
pub fn step(at: f32) -> usize {
    (at.max(0.0) as usize).min(4)
}

/// How far `at` is through the stretch `from..to` seconds (0..1).
pub fn between(at: f32, from: f32, to: f32) -> f32 {
    ((at - from) / (to - from)).clamp(0.0, 1.0)
}

pub fn hero<'a, Message: Clone + 'a>(h: Hero<'a, Message>) -> Element<'a, Message> {
    let mut name = row![super::text::text(Role::PAGE_TITLE, h.title)]
        .spacing(space::GAP)
        .align_y(iced::Bottom);
    if h.lab {
        name = name.push(sub(Role::CARD_DESC, "Labs"));
    }
    let keys: Element<'a, Message> = match h.keys.is_empty() {
        true => sub(Role::CARD_DESC, "Shortcut not set").into(),
        false => row(h.keys.into_iter().map(|k| {
            let w = super::text::width(&k, Role::KEYCAP.size, super::text::font(Role::KEYCAP));
            container(keycap(k)).width(w + CAP_PAD).into()
        }))
        .spacing(space::TRACK_PAD)
        .into(),
    };
    let collapse = match h.shown {
        true => "Hide preview",
        false => "Show preview",
    };
    let heading = row![
        column![name, sub(Role::CARD_DESC, h.desc)]
            .spacing(space::TRACK_GAP)
            .width(Fill),
        column![
            keys,
            button(label(collapse), Kind::Elevated, Some(h.toggle))
        ]
        .spacing(space::GAP)
        .align_x(iced::Right),
    ]
    .spacing(space::GAP * 2.0);
    let mut body = column![heading].spacing(STAGE_PAD);
    if h.shown {
        body = body.push(stage(h.at, h.caption, h.scene, h.replay));
    }
    container(body)
        .padding(PAD)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(c().card_idle.into()),
            border: Border::default()
                .rounded(radius::CARD)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

fn stage<'a, Message: Clone + 'a>(
    at: f32,
    caption: &'a str,
    scene: Element<'a, Message>,
    replay: Message,
) -> Element<'a, Message> {
    let chip = container(
        text("5 s preview")
            .size(SMALL)
            .font(super::text::font(Role::CARD_DESC))
            .color(c().accent),
    )
    .padding([2, 8])
    .style(|_| container::Style {
        background: Some(c().accent_soft.into()),
        border: Border::default().rounded(radius::FULL),
        ..container::Style::default()
    });
    let again = row![icon(Icon::Replay, size::ICON_SMALL), label("Replay")]
        .spacing(space::GAP)
        .align_y(Center);
    let top = row![
        chip,
        Space::new().width(Fill),
        button(again, Kind::Elevated, Some(replay))
    ]
    .align_y(Center);
    let length = LENGTH.as_secs_f32();
    let share = (at / length).clamp(0.0, 1.0);
    let clock = |s: String| {
        (text(s).size(SMALL))
            .font(super::text::font(Role::CARD_DESC))
            .color(c().text_muted)
    };
    let timeline = row![
        clock(format!("0:0{}", at.clamp(0.0, length) as u32)),
        track(share),
        clock(format!("0:0{}", length as u32)),
    ]
    .spacing(space::GAP + 2.0)
    .align_y(Center);
    let line = container(Space::new().width(Fill).height(1)).style(|_| container::Style {
        background: Some(c().divider.into()),
        ..container::Style::default()
    });
    container(
        column![
            top,
            container(container(scene).max_width(SCENE_W).height(Fill)).center_x(Fill),
            container(sub(Role::CARD_DESC, caption).size(CAPTION)).center_x(Fill),
            line,
            timeline,
        ]
        .spacing(space::TRACK_PAD),
    )
    .padding(STAGE_PAD)
    .width(Fill)
    .height(STAGE_H)
    .style(|_| container::Style {
        background: Some(c().track_glass.into()),
        border: Border::default()
            .rounded(radius::CARD)
            .width(1)
            .color(c().divider),
        ..container::Style::default()
    })
    .into()
}

/// The timeline's 3 px track, `share` of it `accent`.
fn track<'a, Message: 'a>(share: f32) -> Element<'a, Message> {
    let part = |s: f32| Length::FillPortion((s * 1000.0).round().max(1.0) as u16);
    let fill = container(Space::new().width(Fill).height(TRACK))
        .width(part(share))
        .style(|_| container::Style {
            background: Some(c().accent.into()),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        });
    // (an empty part is left out: iced lays `FillPortion(0)` out as if it had one)
    let rest = (share < 1.0).then(|| Space::new().width(part(1.0 - share)).height(TRACK));
    container(row![].push((share > 0.0).then_some(fill)).push(rest))
        .width(Fill)
        .height(TRACK)
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        })
        .into()
}

// ---- the scene's parts ----

/// A pretend screen round `content`: `hover` with a divider edge, radius 8.
pub fn screen<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(space::CARD)
        .style(|_| container::Style {
            background: Some(c().hover.into()),
            border: Border::default()
                .rounded(radius::MENU)
                .width(1)
                .color(c().divider),
            ..container::Style::default()
        })
        .into()
}

/// A line of pretend text, `width` long (`strong`: darker, a heading).
pub fn bar<'a, Message: 'a>(width: f32, strong: bool) -> Element<'a, Message> {
    let fill = match strong {
        true => c().divider,
        false => c().hover,
    };
    container(Space::new().width(width).height(if strong { 8 } else { 6 }))
        .style(move |_| container::Style {
            background: Some(fill.into()),
            border: Border::default().rounded(radius::KEYCAP / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// A solid block over private text (a redaction).
pub fn redact<'a, Message: 'a>(width: f32) -> Element<'a, Message> {
    container(Space::new().width(width).height(size::ICON_SMALL))
        .style(|_| container::Style {
            background: Some(c().text_sub.into()),
            border: Border::default().rounded(radius::KEYCAP / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// Selected text: `accent_soft` under it, the text cursor at its end
/// (`caret`).
pub fn mark<'a, Message: 'a>(line: Element<'a, Message>, caret: bool) -> Element<'a, Message> {
    let mut r = row![line].align_y(Center).spacing(1);
    if caret {
        r = r.push(
            container(Space::new().width(1.5).height(12)).style(|_| container::Style {
                background: Some(c().text.into()),
                ..container::Style::default()
            }),
        );
    }
    container(r)
        .padding([1, 3])
        .style(|_| container::Style {
            background: Some(c().accent_soft.into()),
            border: Border::default().rounded(radius::KEYCAP / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// A floating pill (the bar's Search in browser, a "Copied" toast): a glyph
/// and a label on `elevated` with its edge; `lit`: `accent_soft`, accent text.
pub fn pill<'a, Message: 'a>(glyph: Icon, line: &'a str, lit: bool) -> Element<'a, Message> {
    let ink = if lit { c().accent } else { c().text };
    container(
        row![
            icon(glyph, size::ICON_SMALL - 2.0).color(c().accent),
            text(line)
                .size(Role::CARD_DESC.size)
                .font(super::text::font(Role::CARD_DESC))
                .color(ink),
        ]
        .spacing(space::GAP)
        .align_y(Center),
    )
    .padding([3, 9])
    .style(move |_| container::Style {
        background: Some(if lit { c().accent_soft } else { c().elevated }.into()),
        border: Border::default()
            .rounded(radius::FULL)
            .width(1)
            .color(if lit { c().accent } else { c().elevated_border }),
        ..container::Style::default()
    })
    .into()
}

/// `glyph` in the accent, 14 px.
pub fn glyph<'a>(glyph: Icon) -> iced::widget::Text<'a> {
    icon(glyph, size::ICON_SMALL).color(c().accent)
}

/// A captured area round `content`: an `accent` edge.
pub fn frame<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding([4, 8])
        .style(|_| container::Style {
            background: Some(c().accent_soft.into()),
            border: Border::default()
                .rounded(radius::KEYCAP)
                .width(1.5)
                .color(c().accent),
            ..container::Style::default()
        })
        .into()
}

/// A sound level: a 3 px `accent` bar `level` (0..1) of 32 px tall.
pub fn wave<'a, Message: 'a>(level: f32) -> Element<'a, Message> {
    let h = 4.0 + 28.0 * level.clamp(0.0, 1.0);
    container(Space::new().width(3).height(h))
        .style(|_| container::Style {
            background: Some(c().accent.into()),
            border: Border::default().rounded(radius::FULL),
            ..container::Style::default()
        })
        .into()
}

/// The circling stroke: an `accent` dashed ellipse filling its place, drawn
/// `share` (0..1) of the way round from the top; closed, it is tinted inside.
pub fn ring<'a, Message: 'a>(share: f32) -> Element<'a, Message> {
    draw(Ring {
        share: share.clamp(0.0, 1.0),
        ink: c().accent,
    })
    .width(Fill)
    .height(Fill)
    .into()
}

struct Ring {
    share: f32,
    ink: Color,
}

impl<Message> canvas::Program<Message> for Ring {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        if self.share <= 0.0 {
            return Vec::new();
        }
        let mut frame = Frame::new(renderer, bounds.size());
        let (cx, cy) = (bounds.width / 2.0, bounds.height / 2.0);
        let (rx, ry) = (cx - 3.0, cy - 3.0);
        let at = |a: f32| Point::new(cx + rx * a.sin(), cy - ry * a.cos());
        const STEPS: usize = 72;
        let n = ((STEPS as f32 * self.share).ceil() as usize).max(1);
        let path = Path::new(|b| {
            b.move_to(at(0.0));
            for i in 1..=n {
                b.line_to(at(TAU * self.share * i as f32 / n as f32));
            }
        });
        if self.share >= 1.0 {
            frame.fill(
                &path,
                Color {
                    a: 0.12,
                    ..self.ink
                },
            );
        }
        frame.stroke(
            &path,
            Stroke {
                line_dash: LineDash {
                    segments: &[6.0, 3.0],
                    offset: 0,
                },
                ..Stroke::default().with_color(self.ink).with_width(2.0)
            },
        );
        vec![frame.into_geometry()]
    }
}
