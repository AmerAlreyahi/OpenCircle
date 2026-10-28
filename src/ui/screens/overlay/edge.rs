//! The screen edge (DESIGN.md "Feel", config `[look]`): per mode (overlay,
//! live translate, snip) the design's Active Look (`kit::looks`), drawn by its
//! kind (`newdesing/core/theme-context.js`, `ui-kit.css` `.effect-*`), in its
//! two colours c1 (near the edge) and c2 (further in); or nothing:
//! - Glow: the soft inset glow inside the screen edges (`box-shadow: inset 0 0
//!   22px c1, inset 0 0 54px c2` of `.effect-ambient-glow`, radius 14);
//! - Horizon: that glow along the top and bottom edges only, fading out
//!   towards the left and right ends (the look's `horizon` gradient);
//! - Hud: the glow plus four viewfinder corner brackets 12 px in (30 px, a
//!   2.5 px line in c1, outer radius 8, an 8 px shadow in c2);
//! - Bezel: a 1.5 px line in c1 over an 8 px band of c2 and a 28 px inset
//!   glow of c1 (`.effect-bezel-frame`); it never breathes.
//!
//! The user's own two colours draw a Glow.
//!
//! Static (owner, 2026-10-27): no moving colours, no draw-back, no travel. The
//! edge is a few strip images made once per size and look, on the first frame;
//! every later frame draws the same images and so redraws nothing. The
//! overlay's own fade-in (`windows::fade`) moves it as a whole.
//! The one motion is the design's `ripplePulse`, a barely-there breath of the
//! overlay's glow once its opening is over (owner, 2026-10-27): 4 s, the
//! blurs going 22/54 px -> 38/85 px -> 22/54 px, redrawn as the edge strips
//! ONLY (never the screen), from a cached depth map so a step is a table
//! lookup per pixel, and only when it shows: the breath moves no pixel by
//! more than 2 of 255 a step (`LEVELS`), so it is redrawn when its level
//! changes, 24 times in 4 s (it was every `STEP`, 50: each redraws about 30 %
//! of the screen, ~11 ms of the VM's CPU, 14 % of a core while still). Live
//! translate and snip stay still, and so does the overlay when the OS asks for
//! reduced motion, and on Linux (`breathes`).
//! On the colour-keyed live layer (Windows) there is no partial alpha: a thin
//! solid line in c1 stands in for any look.
use std::time::{Duration, Instant};

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Action, Geometry};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Color, Element, Fill, Rectangle, Renderer, Theme, window};

pub use crate::config::EdgeMode as Mode;
use crate::config::{self, EdgeStyle, LookConfig};
use crate::ui::kit::looks::Kind;
use crate::ui::theme;

/// The screen's corner radius in the overlay, logical px (the design's 14).
const RADIUS: f32 = 14.0;
/// The design's two blurs (c1's layer, c2's), logical px; a CSS blur of `b`
/// is a Gaussian of sigma `b / 2`.
const BLUR: [f32; 2] = [22.0, 54.0];
/// The blurs at the breath's peak (the design's 50 % keyframe).
const BLUR_PEAK: [f32; 2] = [38.0, 85.0];
/// The bezel: its line, its band and its glow's blur, logical px.
const BEZEL: [f32; 3] = [1.5, 8.0, 28.0];
/// The HUD brackets: inset, size, line, outer radius, shadow blur; logical px.
const HUD: [f32; 5] = [12.0, 30.0, 2.5, 8.0, 8.0];
/// The breath: its length and the gap between redraws.
const BREATH: Duration = Duration::from_secs(4);
const STEP: Duration = Duration::from_millis(80);
/// The breath's levels from rest to peak: the whole breath changes a pixel by
/// 17-18 of 255 at most (premultiplied), so a level is at most 2 of them.
const LEVELS: f32 = 12.0;
/// How far inward the glow reaches at its peak, logical px: past this the
/// outer layer at full strength is under half a level of alpha.
const REACH: f32 = 102.0;

/// Whether the overlay's glow breathes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breathe {
    /// Not (yet): the opening is on, or the edge is not the overlay's.
    Off,
    /// It does, but another motion is on (the bar gliding, a card opening):
    /// the edge keeps what it shows and draws nothing meanwhile.
    Paused,
    On,
}

/// Whether the overlay's glow may breathe at all: not when the OS asks for
/// reduced motion, and not off Windows (#142): there the 12.5 fps redraw goes
/// through our CPU renderer and a whole-window present each step (~9 % of a
/// core on the Linux VM); the owner's rule is near 0 % while the overlay is
/// still. Windows measured 0.10 %. Linux keeps the static glow.
fn breathes() -> bool {
    cfg!(windows) && !crate::ui::kit::motion::reduced()
}

/// The edge over a window that shows a whole monitor at `scale` (physical px
/// per logical px), in the active look. `keyed`: the window's background is a
/// colour key (no partial alpha). `opacity`: how much of it shows (the glow
/// fading between screens, `app::screens::Glow`): the same strips drawn
/// fainter, never made again; none at 0.
pub fn view<'a, M: 'a>(
    mode: Mode,
    scale: f32,
    keyed: bool,
    breathing: Breathe,
    opacity: f32,
) -> Element<'a, M> {
    canvas::Canvas::new(EdgeCanvas {
        mode,
        scale,
        keyed,
        breathing,
        opacity,
        look: None,
    })
    .width(Fill)
    .height(Fill)
    .into()
}

/// Puts a fixed colour where the live edge is, in a capture (`rgba`, `w` x `h`)
/// of the monitor it frames: a capture that shows the layer (X11) then never
/// changes there, so live mode doesn't re-read the edges.
/// Returns the strips (x, y, w, h) it may have blanked.
pub fn mask(rgba: &mut [u8], w: u32, h: u32, scale: f32) -> Vec<[u32; 4]> {
    let cfg = config::look_config();
    let Some((geo, paint)) = edge(&cfg, Mode::Live, w, h, scale, false, 0.0) else {
        return Vec::new();
    };
    let strips = geo.strips();
    for &[x0, y0, sw, sh] in &strips {
        for y in y0..y0 + sh {
            for x in x0..x0 + sw {
                let i = ((y * w + x) * 4) as usize;
                if paint.table[geo.index(x, y)][3] > 0
                    && let Some(px) = rgba.get_mut(i..i + 4)
                {
                    px.copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
    }
    strips
}

/// `mode`'s edge in `look`, laid over `rgba` (`w` x `h`, straight alpha) as if
/// the screen were `w / scale` logical px wide (the tests' pictures).
#[cfg(test)]
pub fn paint(rgba: &mut [u8], w: u32, h: u32, scale: f32, look: &LookConfig, mode: Mode) {
    let Some((geo, paint)) = edge(look, mode, w, h, scale, false, 0.0) else {
        return;
    };
    for ([x0, y0, sw, _], px) in render(&depths(geo), &paint) {
        for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (x0 + i as u32 % sw, y0 + i as u32 / sw);
            let j = ((y * w + x) * 4) as usize;
            let a = f32::from(p[3]) / 255.0;
            for c in 0..3 {
                let under = f32::from(rgba[j + c]);
                rgba[j + c] = (under + (f32::from(p[c]) - under) * a).round() as u8;
            }
        }
    }
}

struct EdgeCanvas {
    mode: Mode,
    scale: f32,
    keyed: bool,
    breathing: Breathe,
    opacity: f32,
    /// Tests: this look instead of the config's.
    look: Option<LookConfig>,
}

#[derive(Default)]
pub struct State {
    /// What `strips` show.
    drawn: Option<(Geo, Paint)>,
    /// Each strip's depth map (`depths`), kept while the geometry is.
    depths: Option<(Geo, Vec<Strip>)>,
    /// When the breath began (the first frame that was allowed to).
    since: Option<Instant>,
    /// Logical bounds and pixels of each strip.
    strips: Vec<(Rectangle, Handle)>,
}

impl<M> canvas::Program<M> for EdgeCanvas {
    type State = State;

    /// On each frame: the edge, rebuilt only if its size, look or breath step
    /// changed (the first frame, in practice), and the next step's time while
    /// it breathes.
    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Option<Action<M>> {
        let canvas::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let look = self.look.unwrap_or_else(config::look_config);
        let mut breathing = self.breathing != Breathe::Off
            && self.mode == Mode::Overlay
            && design(&look).0 != Kind::Bezel
            && breathes();
        if breathing && self.breathing == Breathe::Paused && state.drawn.is_some() {
            return None;
        }
        breathing &= self.breathing == Breathe::On;
        let since = *state.since.get_or_insert(*now);
        if !breathing {
            state.since = None;
        }
        let step = (now.saturating_duration_since(since).as_millis() / STEP.as_millis()) as u32;
        let breath = if breathing { level(step) } else { 0.0 };
        let px = |v: f32| (v * self.scale).round() as u32;
        let want = edge(
            &look,
            self.mode,
            px(bounds.width),
            px(bounds.height),
            self.scale,
            self.keyed,
            breath,
        );
        let next = (breathing && want.is_some())
            .then(|| Action::request_redraw_at(since + STEP * next_step(step)));
        if state.drawn == want {
            return next;
        }
        if let Some((geo, paint)) = &want {
            if state.depths.as_ref().is_none_or(|(g, _)| g != geo) {
                state.depths = Some((*geo, depths(*geo)));
            }
            let depths = &state.depths.as_ref()?.1;
            state.strips = (render(depths, paint).into_iter())
                .map(|([x, y, w, h], rgba)| {
                    // + a quarter pixel: the canvas hands image bounds on
                    // with float error and tiny-skia truncates them to
                    // whole pixels (a strip at x = 1917 landed on 1916).
                    let at = iced::Point::new(x as f32 + 0.25, y as f32 + 0.25);
                    let at = Rectangle::new(at, iced::Size::new(w as f32, h as f32));
                    (at * (1.0 / self.scale), Handle::from_rgba(w, h, rgba))
                })
                .collect();
        } else {
            state.strips.clear();
        }
        state.drawn = want;
        next
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for (at, handle) in state.strips.iter().filter(|_| self.opacity > 0.0) {
            let image = canvas::Image::new(handle.clone())
                .filter_method(FilterMethod::Nearest)
                .opacity(self.opacity.min(1.0))
                .snap(true);
            frame.draw_image(*at, image);
        }
        vec![frame.into_geometry()]
    }
}

/// The edge's shape on one monitor, physical px: the screen's size, the band
/// it lives in (`t` deep: the glow's reach, or the line's width), the
/// screen's corner radius `r` and the look's kind (which strips, which marks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geo {
    w: u32,
    h: u32,
    t: u32,
    r: u32,
    kind: Kind,
    /// Physical px per logical px, in thousandths (the HUD brackets' size).
    milli: u32,
}

/// What an edge draws besides its shape: its RGBA (straight alpha) per whole
/// px of depth from the screen edge, the last entry clear; and the HUD's
/// bracket and bracket-shadow colours (straight alpha; clear otherwise).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Paint {
    table: Vec<[u8; 4]>,
    marks: [[u8; 4]; 2],
}

/// How far into the breath (0..1: 0 = the design's 22/54 px blurs, 1 = 38/85)
/// step `step` of the 4 s cycle is: eased, there and back.
fn breath(step: u32) -> f32 {
    let t = STEP.as_secs_f32() * step as f32 / BREATH.as_secs_f32();
    0.5 - 0.5 * (std::f32::consts::TAU * t).cos()
}

/// `breath(step)` rounded to one of the `LEVELS`: what step `step` draws.
fn level(step: u32) -> f32 {
    (breath(step) * LEVELS).round() / LEVELS
}

/// The first step after `step` that draws another level (the next redraw).
fn next_step(step: u32) -> u32 {
    let cycle = (BREATH.as_millis() / STEP.as_millis()) as u32;
    let now = level(step);
    (step + 1..=step + cycle)
        .find(|&s| level(s) != now)
        .unwrap_or(step + 1)
}

/// The stored look's kind and edge colours c1, c2 (alpha included): a design
/// look's own; the user's own two as a glow.
fn design(look: &LookConfig) -> (Kind, [Color; 2]) {
    match theme::kit_look(look.theme) {
        Some(l) => (l.kind, l.glow),
        None => (Kind::Glow, theme::edge_glow(&look.active())),
    }
}

fn rgb(c: Color) -> [f32; 3] {
    [c.r, c.g, c.b]
}

/// Layers (colour, alpha), bottom first, as one straight-alpha pixel; clear
/// under half a level of alpha.
fn over(layers: &[([f32; 3], f32)]) -> [u8; 4] {
    let (mut c, mut a) = ([0.0; 3], 0.0);
    for &(top, at) in layers {
        if a <= 0.0 {
            (c, a) = (top, at);
            continue;
        }
        let both = at + a * (1.0 - at);
        if both > 0.0 {
            c = std::array::from_fn(|i| (top[i] * at + c[i] * a * (1.0 - at)) / both);
        }
        a = both;
    }
    if a < 0.5 / 255.0 {
        return [0; 4];
    }
    let u8 = |v: f32| (v * 255.0).round() as u8;
    [u8(c[0]), u8(c[1]), u8(c[2]), u8(a)]
}

/// The edge `mode` shows in `look` on a `w` x `h` screen at `scale`: its shape
/// and paint (`None`: no edge). `keyed`: a solid line, no blending.
/// `breath`: see `breath()`.
fn edge(
    look: &LookConfig,
    mode: Mode,
    w: u32,
    h: u32,
    scale: f32,
    keyed: bool,
    breath: f32,
) -> Option<(Geo, Paint)> {
    let active = look.active();
    let e = active.edge(mode);
    let strength = f32::from(active.glow) / 100.0;
    if e.style == EdgeStyle::None || strength <= 0.0 {
        return None;
    }
    let px = |v: f32| (v * scale).round() as u32;
    let (kind, [c1, c2]) = design(look);
    let kind = if keyed { Kind::Glow } else { kind };
    let mut marks = [[0; 4]; 2];
    let (t, table) = if keyed {
        let t = px(f32::from(e.thickness)).max(1);
        let [r, g, b, _] = c1.into_rgba8();
        let [r, g, b] = off_key([r, g, b]);
        let table = (0..=t)
            .map(|i| if i < t { [r, g, b, 255] } else { [0; 4] })
            .collect();
        (t, table)
    } else {
        let t = px(REACH).max(1);
        // a CSS inset blur `b` at depth `d` (px from where it starts)
        let tail = |d: f32, b: f32| 0.5 * erfc(d / (b / 2.0 * scale * std::f32::consts::SQRT_2));
        let (a1, a2) = (c1.a * strength, c2.a * strength);
        let table = (0..=t)
            .map(|i| {
                if i == t {
                    return [0; 4];
                }
                let d = i as f32 + 0.5;
                if kind == Kind::Bezel {
                    let [line, band, blur] = BEZEL.map(|v| v * scale);
                    let i = i as f32;
                    let cover =
                        |from: f32, to: f32| (to.min(i + 1.0) - from.max(i)).clamp(0.0, 1.0);
                    return over(&[
                        (rgb(c1), a1 * tail((d - line).max(0.0), blur / scale)),
                        (rgb(c2), a2 * cover(line, line + band)),
                        (rgb(c1), a1 * cover(0.0, line)),
                    ]);
                }
                let blur = |n: usize| BLUR[n] + (BLUR_PEAK[n] - BLUR[n]) * breath;
                // c1's layer over c2's
                over(&[
                    (rgb(c2), a2 * tail(d, blur(1))),
                    (rgb(c1), a1 * tail(d, blur(0))),
                ])
            })
            .collect();
        if kind == Kind::Hud {
            let alpha = |c: Color, a: f32| {
                let [r, g, b, _] = c.into_rgba8();
                [r, g, b, (a * 255.0).round() as u8]
            };
            marks = [alpha(c1, a1), alpha(c2, a2)];
        }
        (t, table)
    };
    let geo = Geo {
        w,
        h,
        t,
        r: px(RADIUS),
        kind,
        milli: (scale * 1000.0).round() as u32,
    };
    Some((geo, Paint { table, marks }))
}

/// 1 - erf(x), x >= 0 (Abramowitz & Stegun 7.1.26, error < 1.5e-7).
fn erfc(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_8 + t * (-1.453_152_1 + t * 1.061_405_4))));
    poly * (-x * x).exp()
}

/// A colour too near the live layer's key (it would be keyed out, a hole) is
/// darkened away from it.
fn off_key(c: [u8; 3]) -> [u8; 3] {
    let key = crate::live::COLOUR_KEY;
    let near: u32 = (c.iter().zip(key)).map(|(a, b)| a.abs_diff(b) as u32).sum();
    if near > 220 {
        return c;
    }
    let dark = |v: u8| (f32::from(v) * 0.55) as u8;
    [dark(c[0]), c[1], dark(c[2])]
}

impl Geo {
    /// The strips (x, y, w, h): top and bottom full width, left and right
    /// between them (not for a Horizon), each as deep as the band or the
    /// corner radius (all a lit pixel can be). None on a screen too small for
    /// an edge.
    fn strips(self) -> Vec<[u32; 4]> {
        let (w, h, band) = (self.w, self.h, self.t.max(self.r));
        if w <= 2 * band || h <= 2 * band {
            return Vec::new();
        }
        let down = h - 2 * band;
        let mut strips = vec![[0, 0, w, band], [0, h - band, w, band]];
        if self.kind != Kind::Horizon {
            strips.extend([[w - band, band, band, down], [0, band, band, down]]);
        }
        strips
    }

    /// Which table entry pixel (x, y) takes: its centre's depth inside the
    /// screen's rounded rectangle (0 in a square corner outside it; a
    /// Horizon: from the top or bottom edge), capped at the table's clear last
    /// entry.
    fn index(self, x: u32, y: u32) -> usize {
        if self.kind == Kind::Horizon {
            return (y.min(self.h - 1 - y) as usize).min(self.t as usize);
        }
        let (cx, cy) = (self.w as f32 / 2.0, self.h as f32 / 2.0);
        let r = self.r as f32;
        let qx = (x as f32 + 0.5 - cx).abs() - (cx - r);
        let qy = (y as f32 + 0.5 - cy).abs() - (cy - r);
        let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
        ((-outside).max(0.0) as usize).min(self.t as usize)
    }

    /// A Horizon's strength at column `x`, 0..=255: none at the screen's
    /// left and right ends, full from a quarter in (the look's gradient).
    fn gain(self, x: u32) -> u8 {
        let w = self.w as f32;
        let e = ((x as f32 + 0.5).min(w - x as f32 - 0.5) / (0.25 * w)).clamp(0.0, 1.0);
        (e * 255.0).round() as u8
    }

    /// The HUD's corner brackets: (x, y, line cover, shadow) per pixel they
    /// touch, covers 0..=255 (the shadow before its colour's alpha).
    fn brackets(self) -> Vec<(u32, u32, u8, u8)> {
        let scale = self.milli as f32 / 1000.0;
        let [inset, size, line, round, blur] = HUD.map(|v| v * scale);
        let sigma = blur / 2.0;
        let pad = (3.0 * sigma).ceil() as i64;
        let n = (size.ceil() as i64 + 2 * pad) as usize;
        // a rounded corner's signed distance (top-left rounded, open beyond)
        let corner = |x: f32, y: f32, r: f32| {
            if x < r && y < r {
                (r - x).hypot(r - y) - r
            } else {
                (-x).max(-y)
            }
        };
        let cover: Vec<f32> = (0..n * n)
            .map(|i| {
                let x = (i % n) as f32 + 0.5 - pad as f32;
                let y = (i / n) as f32 + 0.5 - pad as f32;
                let outer = corner(x, y, round).max(x - size).max(y - size);
                let inner = corner(x - line, y - line, (round - line).max(0.0));
                (0.5 - outer.max(-inner)).clamp(0.0, 1.0)
            })
            .collect();
        // the drop shadow: the cover blurred (a Gaussian, rows then columns)
        let kernel: Vec<f32> = (-pad..=pad)
            .map(|k| (-((k * k) as f32) / (2.0 * sigma * sigma)).exp())
            .collect();
        let sum: f32 = kernel.iter().sum();
        let blur1 = |src: &[f32], across: bool| -> Vec<f32> {
            (0..n * n)
                .map(|i| {
                    let (x, y) = ((i % n) as i64, (i / n) as i64);
                    (kernel.iter().enumerate())
                        .map(|(k, wt)| {
                            let o = k as i64 - pad;
                            let (sx, sy) = if across { (x + o, y) } else { (x, y + o) };
                            let inside = (0..n as i64).contains(&sx) && (0..n as i64).contains(&sy);
                            if inside {
                                src[(sy * n as i64 + sx) as usize] * wt
                            } else {
                                0.0
                            }
                        })
                        .sum::<f32>()
                        / sum
                })
                .collect()
        };
        let shadow = blur1(&blur1(&cover, true), false);
        let at = inset.round() as i64 - pad;
        let (w, h) = (i64::from(self.w), i64::from(self.h));
        let mut out = Vec::new();
        for (i, (&c, &s)) in cover.iter().zip(&shadow).enumerate() {
            let (c, s) = ((c * 255.0).round() as u8, (s * 255.0).round() as u8);
            if c == 0 && s == 0 {
                continue;
            }
            let (lx, ly) = (at + (i % n) as i64, at + (i / n) as i64);
            for (x, y) in [
                (lx, ly),
                (w - 1 - lx, ly),
                (lx, h - 1 - ly),
                (w - 1 - lx, h - 1 - ly),
            ] {
                if (0..w).contains(&x) && (0..h).contains(&y) {
                    out.push((x as u32, y as u32, c, s));
                }
            }
        }
        out
    }
}

/// One strip: its rectangle, per pixel its table entry (`Geo::index`), per
/// pixel its Horizon gain (empty: full), and the HUD brackets' pixels in it
/// (index, line cover, shadow).
struct Strip {
    rect: [u32; 4],
    at: Vec<u16>,
    gain: Vec<u8>,
    marks: Vec<(u32, u8, u8)>,
}

/// Every strip's depth map: made once per geometry, so that a breath step is
/// a lookup per pixel and not a distance per pixel.
fn depths(geo: Geo) -> Vec<Strip> {
    let mut strips: Vec<Strip> = (geo.strips().into_iter())
        .map(|rect @ [x0, y0, w, h]| {
            let at = |i: u32| geo.index(x0 + i % w, y0 + i / w) as u16;
            let gain = if geo.kind == Kind::Horizon {
                (0..w * h).map(|i| geo.gain(x0 + i % w)).collect()
            } else {
                Vec::new()
            };
            Strip {
                rect,
                at: (0..w * h).map(at).collect(),
                gain,
                marks: Vec::new(),
            }
        })
        .collect();
    if geo.kind == Kind::Hud {
        for (x, y, c, s) in geo.brackets() {
            let hit = (strips.iter_mut()).find(|st| {
                let [sx, sy, sw, sh] = st.rect;
                (sx..sx + sw).contains(&x) && (sy..sy + sh).contains(&y)
            });
            if let Some(st) = hit {
                let [sx, sy, sw, _] = st.rect;
                st.marks.push(((y - sy) * sw + (x - sx), c, s));
            }
        }
    }
    strips
}

/// Each strip's rectangle and pixels (RGBA, straight alpha).
fn render(strips: &[Strip], paint: &Paint) -> Vec<([u32; 4], Vec<u8>)> {
    let f = |v: u8| f32::from(v) / 255.0;
    let [line, glow] = paint.marks;
    let colour = |c: [u8; 4]| [f(c[0]), f(c[1]), f(c[2])];
    (strips.iter())
        .map(|st| {
            let mut rgba = vec![0; st.at.len() * 4];
            let px = rgba.as_chunks_mut::<4>().0;
            for (chunk, &i) in px.iter_mut().zip(&st.at) {
                *chunk = paint.table[usize::from(i)];
            }
            for (chunk, &g) in px.iter_mut().zip(&st.gain) {
                chunk[3] = ((u32::from(chunk[3]) * u32::from(g) + 127) / 255) as u8;
            }
            for &(i, c, s) in &st.marks {
                let p = &mut px[i as usize];
                *p = over(&[
                    (colour(*p), f(p[3])),
                    (colour(glow), f(s) * f(line[3]) * f(glow[3])),
                    (colour(line), f(c) * f(line[3])),
                ]);
            }
            (st.rect, rgba)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColourSet, Look, LookTheme, PRESETS};
    use crate::ui::kit::motion;
    use std::time::{Duration, Instant};

    fn spectrum() -> LookConfig {
        LookConfig::default()
    }

    /// The design look stored as `theme`.
    fn theme(theme: LookTheme) -> LookConfig {
        LookConfig {
            theme,
            ..LookConfig::default()
        }
    }

    /// The user's own look.
    fn own(look: Look) -> LookConfig {
        LookConfig {
            theme: LookTheme::Custom,
            custom: look,
        }
    }

    fn table(look: &LookConfig, breath: f32) -> Vec<[u8; 4]> {
        edge(look, Mode::Overlay, 1920, 1080, 1.0, false, breath)
            .unwrap()
            .1
            .table
    }

    /// The whole edge as one `w` x `h` image (clear where it is not).
    fn full(geo: Geo, paint: &Paint) -> Vec<u8> {
        let mut out = vec![0; (geo.w * geo.h * 4) as usize];
        for ([x0, y0, sw, _], px) in render(&depths(geo), paint) {
            for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
                let (x, y) = (x0 + i as u32 % sw, y0 + i as u32 / sw);
                let j = ((y * geo.w + x) * 4) as usize;
                out[j..j + 4].copy_from_slice(p);
            }
        }
        out
    }

    fn image(look: &LookConfig) -> Vec<u8> {
        let (geo, paint) = edge(look, Mode::Overlay, 1920, 1080, 1.0, false, 0.0).unwrap();
        full(geo, &paint)
    }

    fn at(img: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * 1920 + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn the_glow_is_the_designs_inset_shadow_in_the_looks_two_colours() {
        // c1 (22 px blur) over c2 (54 px): strongest at the screen edge, about
        // half a layer's alpha there (a CSS blur), gone past the reach
        let table = table(&spectrum(), 0.0);
        let alpha: Vec<u8> = table.iter().map(|p| p[3]).collect();
        assert!(alpha.windows(2).all(|p| p[0] >= p[1]), "{alpha:?}");
        // 0.5 * 0.42 over 0.5 * 0.26: 0.21 + 0.13 * 0.79 = 0.31
        assert!((75..=84).contains(&alpha[0]), "{alpha:?}");
        assert!(alpha[22] > 0 && alpha[22] * 4 < alpha[0], "{alpha:?}");
        assert!(alpha[65] <= 1, "{alpha:?}");
        assert_eq!(alpha.last(), Some(&0));
        // at the edge c1 shows (violet), further in c2 (coral)
        let [r, g, b, _] = table[0];
        assert!(b > r && r > g, "violet: {:?}", table[0]);
        let deep = table[40];
        assert!(deep[0] > deep[2] + 100, "coral: {deep:?}");
    }

    #[test]
    fn every_look_has_its_design_colours_and_kind() {
        // how far a table colour is from a design colour (rgb)
        let apart = |a: [u8; 4], c: Color| {
            let b = c.into_rgba8();
            (0..3).map(|i| u32::from(a[i].abs_diff(b[i]))).sum::<u32>()
        };
        // the user's own palettes and colours: a glow in c1 then c2
        for palette in [
            ColourSet::Spectrum,
            ColourSet::Aurora,
            ColourSet::Sunset,
            ColourSet::Ocean,
            ColourSet::Neon,
            ColourSet::TwoColour,
        ] {
            let look = own(Look {
                palette,
                ..spectrum().active()
            });
            let [c1, c2] = theme::edge_glow(&look.active());
            let t = table(&look, 0.0);
            let (e, d) = (t[0], t[50]);
            assert!(apart(e, c1) < apart(e, c2), "{palette:?}: {e:?}");
            assert!(apart(d, c2) < 12, "{palette:?} further in: {d:?}");
        }
        // every stored look draws its design look's kind in its glow colours
        for (stored, _) in PRESETS.iter().filter(|(t, _)| *t != LookTheme::Calm) {
            let kit = theme::kit_look(*stored).unwrap();
            let look = theme(*stored);
            assert_eq!(design(&look), (kit.kind, kit.glow), "{stored:?}");
            let (geo, _) = edge(&look, Mode::Overlay, 1920, 1080, 1.0, false, 0.0).unwrap();
            assert_eq!(geo.kind, kit.kind, "{stored:?}");
            if kit.kind != Kind::Bezel {
                let [c1, c2] = kit.glow;
                let t = table(&look, 0.0);
                assert!(apart(t[0], c1) <= apart(t[0], c2), "{stored:?}");
            }
        }
        // the four kinds are there, and the default look is the design's Spectrum
        for kind in [Kind::Glow, Kind::Hud, Kind::Bezel, Kind::Horizon] {
            assert!(PRESETS.iter().any(|(t, _)| design(&theme(*t)).0 == kind));
        }
        let [c1, c2] = design(&spectrum()).1;
        assert_eq!(c1.into_rgba8(), [155, 123, 255, 107]);
        assert_eq!(c2.into_rgba8(), [255, 107, 94, 66]);
    }

    #[test]
    fn none_and_zero_strength_draw_nothing() {
        let mut look = spectrum().active();
        look.overlay.style = EdgeStyle::None;
        let look = own(look);
        assert!(edge(&look, Mode::Overlay, 1920, 1080, 1.0, false, 0.0).is_none());
        assert!(edge(&look, Mode::Live, 1920, 1080, 1.0, false, 0.0).is_some());
        let calm = theme(LookTheme::Calm);
        assert!(edge(&calm, Mode::Overlay, 1920, 1080, 1.0, false, 0.0).is_none());
        let grey = vec![60u8; 192 * 108 * 4];
        let mut b = grey.clone();
        paint(&mut b, 192, 108, 0.1, &look, Mode::Overlay);
        assert_eq!(b, grey);
    }

    /// Runs the canvas over frames at `ms` after the first: the strips after
    /// each and whether it asked for another frame.
    fn frames(border: &EdgeCanvas, ms: &[u64]) -> Vec<(Vec<(Rectangle, Handle)>, bool)> {
        let bounds = Rectangle::with_size(iced::Size::new(1920.0, 1080.0));
        let mut state = State::default();
        let start = Instant::now();
        (ms.iter())
            .map(|&ms| {
                let at = start + Duration::from_millis(ms);
                let event = canvas::Event::Window(window::Event::RedrawRequested(at));
                let action = <EdgeCanvas as canvas::Program<()>>::update(
                    border,
                    &mut state,
                    &event,
                    bounds,
                    Cursor::Unavailable,
                );
                (state.strips.clone(), action.is_some())
            })
            .collect()
    }

    fn border(mode: Mode, look: LookConfig, breathing: Breathe) -> EdgeCanvas {
        EdgeCanvas {
            opacity: 1.0,
            mode,
            scale: 1.0,
            keyed: false,
            breathing,
            look: Some(look),
        }
    }

    /// The owner's rule: no moving colours, no draw-back, no travel. Everywhere
    /// but the overlay after its opening (and never the still Bezel), frames
    /// 0.2 s and 60 s after the first hold the very same images, and none asks
    /// for another frame.
    #[test]
    fn the_edge_is_still_but_for_the_overlays_breath() {
        for look in PRESETS.iter().map(|(t, _)| theme(*t)) {
            let still = design(&look).0 == Kind::Bezel;
            for mode in [Mode::Overlay, Mode::Live, Mode::Snip] {
                // the opening (not breathing yet), live, snip, reduced motion
                let mut cases = vec![(border(mode, look, Breathe::Off), false)];
                cases.push((border(mode, look, Breathe::On), mode == Mode::Overlay));
                for (case, breathes) in cases {
                    for reduced in [false, true] {
                        motion::set_reduced(reduced);
                        let seen = frames(&case, &[0, 16, 200, 2000, 60_000]);
                        let moves = breathes && !reduced && !still && cfg!(windows);
                        let same = seen.windows(2).all(|p| p[0].0 == p[1].0);
                        let asks = seen.iter().any(|f| f.1);
                        let has = edge(&look, mode, 1920, 1080, 1.0, false, 0.0).is_some();
                        assert_eq!(asks, moves && has, "{mode:?} asks");
                        assert_eq!(!same, moves && has, "{mode:?} moves");
                        assert_eq!(!seen[0].0.is_empty(), has, "{mode:?}");
                    }
                }
            }
        }
        motion::set_reduced(false);
    }

    /// The overlay's breath is the design's `ripplePulse`: 4 s, the blurs
    /// 22/54 px -> 38/85 px at 2 s -> back, in steps of `STEP`; only the edge's
    /// strips (never the screen), and it asks for the next step, not every frame.
    #[test]
    fn the_overlay_breathes_slowly_in_its_strips_only() {
        motion::set_reduced(false);
        for look in [spectrum(), theme(LookTheme::Hud), theme(LookTheme::Horizon)] {
            let b = border(Mode::Overlay, look, Breathe::On);
            let seen = frames(&b, &[0, 1000, 2000, 3000, 4000, 4010]);
            if !cfg!(windows) {
                // #142: Linux's glow stays still (near 0 % CPU while untouched)
                assert!(seen.iter().all(|f| !f.1 && f.0 == seen[0].0));
                continue;
            }
            let alpha_at = |p: f32, d: usize| table(&look, p)[d][3];
            // peak: deeper than at rest, the same at 4 s as at 0
            assert!(alpha_at(1.0, 40) > alpha_at(0.0, 40) + 2);
            assert!(seen[2].0 != seen[0].0 && seen[1].0 != seen[0].0);
            assert!(seen.iter().all(|f| f.1), "asks for the next step");
            assert!(seen[5].0 == seen[4].0, "10 ms later: the same step");
            // never more than the four strips, all inside the edge's band
            let strips = &seen[0].0;
            assert!(strips.len() <= 4);
            let area: f32 = strips.iter().map(|(r, _)| r.width * r.height).sum();
            assert!(area < 0.3 * 1920.0 * 1080.0, "{area}");
        }
        assert!((breath(0) - 0.0).abs() < 1e-6 && (breath(25) - 1.0).abs() < 1e-5);
        assert!((breath(50) - 0.0).abs() < 1e-5);
        // reduced motion: still
        motion::set_reduced(true);
        let seen = frames(&border(Mode::Overlay, spectrum(), Breathe::On), &[0, 2000]);
        assert!(seen[0].0 == seen[1].0 && !seen[1].1);
        motion::set_reduced(false);
    }

    /// Bug A (gate 0.1.2): the still overlay's breath asked for a redraw every
    /// `STEP`, 50 in 4 s, each about 30 % of the screen (14 % of a VM core,
    /// hidden from Task Manager's tick count). It asks only when its level
    /// changes, 24 times a cycle, and a redraw still moves no pixel by more
    /// than 2 of 255: as smooth as before, half the redraws.
    #[test]
    fn the_breath_asks_for_a_frame_only_when_it_shows() {
        let cycle = (BREATH.as_millis() / STEP.as_millis()) as u32;
        let mut drawn = vec![0];
        while *drawn.last().unwrap() < cycle {
            drawn.push(next_step(*drawn.last().unwrap()));
        }
        let redraws = drawn.iter().filter(|&&s| s > 0 && s <= cycle).count();
        assert!((20..=24).contains(&redraws), "{redraws} redraws in 4 s");
        let pm =
            |p: [u8; 4]| [p[0], p[1], p[2]].map(|c| (u32::from(c) * u32::from(p[3]) + 127) / 255);
        for scale in [1.0, 1.5, 2.5] {
            let t = |b: f32| {
                edge(&spectrum(), Mode::Overlay, 1920, 1080, scale, false, b)
                    .unwrap()
                    .1
                    .table
            };
            for pair in drawn.windows(2) {
                let (a, b) = (t(level(pair[0])), t(level(pair[1])));
                let most = (a.iter().zip(&b))
                    .flat_map(|(x, y)| {
                        let alpha = x[3].abs_diff(y[3]);
                        pm(*x)
                            .into_iter()
                            .zip(pm(*y))
                            .map(move |(p, q)| p.abs_diff(q).max(alpha.into()))
                    })
                    .max()
                    .unwrap();
                assert!(most <= 2, "steps {pair:?} at {scale}: {most} of 255");
            }
        }
        // the level of a step drawn is what it shows until the next one
        for pair in drawn.windows(2) {
            assert!((pair[0]..pair[1]).all(|s| level(s) == level(pair[0])));
        }
    }

    #[test]
    fn geometry_follows_the_monitor_size_and_scale() {
        for stored in [
            LookTheme::Spectrum,
            LookTheme::Hud,
            LookTheme::Mono,
            LookTheme::Horizon,
        ] {
            let look = theme(stored);
            for (w, h, scale, t, r) in [
                (1920, 1080, 1.0, 102, 14),
                (2560, 1440, 1.25, 128, 18),
                (3840, 2160, 2.0, 204, 28),
            ] {
                let (geo, paint) = edge(&look, Mode::Overlay, w, h, scale, false, 0.0).unwrap();
                assert_eq!((geo.t, geo.r), (t, r), "{w}x{h} @ {scale}");
                assert_eq!(paint.table.len(), t as usize + 1);
                let strips = geo.strips();
                let n = if stored == LookTheme::Horizon { 2 } else { 4 };
                assert_eq!(strips.len(), n, "{stored:?}");
                for [x, y, sw, sh] in &strips {
                    assert!(x + sw <= w && y + sh <= h, "inside the screen");
                }
                // every lit pixel is in a strip, and the middle is clear
                let img = full(geo, &paint);
                let inside = |x: u32, y: u32| {
                    (strips.iter()).any(|[sx, sy, sw, sh]| {
                        (*sx..sx + sw).contains(&x) && (*sy..sy + sh).contains(&y)
                    })
                };
                for y in (0..h).step_by(7) {
                    for x in (0..w).step_by(5) {
                        if img[((y * w + x) * 4 + 3) as usize] > 0 {
                            assert!(inside(x, y) || geo.index(x, y) == 0, "({x}, {y})");
                        }
                    }
                }
                let mid = (((h / 2) * w + w / 2) * 4 + 3) as usize;
                assert_eq!(img[mid], 0);
            }
        }
        // too small for an edge: nothing (no zero-sized images)
        let (small, _) = edge(&spectrum(), Mode::Overlay, 30, 30, 1.0, false, 0.0).unwrap();
        assert!(small.strips().is_empty());
    }

    /// Optical HUD: four 30 px brackets 12 px in, a line in c1 (white) along
    /// each bracket's two arms, the plain glow past their ends.
    #[test]
    fn the_hud_draws_four_corner_brackets_over_the_glow() {
        let hud = image(&theme(LookTheme::Hud));
        for (x, y) in [
            (13, 25),
            (1906, 25),
            (13, 1054),
            (1906, 1054),
            (25, 13),
            (25, 1066),
        ] {
            let p = at(&hud, x, y);
            assert!(p[3] > 180 && p[0] > 230 && p[2] > 230, "({x}, {y}): {p:?}");
        }
        // past a bracket's arm (and well inside it) only the glow
        let alpha = |x, y| at(&hud, x, y)[3];
        assert!(alpha(13, 80) < 150, "{}", alpha(13, 80));
        assert!(alpha(30, 30) < alpha(13, 25) / 2);
        assert_eq!(at(&hud, 13, 25), at(&hud, 1906, 1054), "symmetric");
    }

    /// Horizon Pulse: light along the top and bottom only, none at their left
    /// and right ends, nothing down the sides.
    #[test]
    fn the_horizon_lights_the_top_and_bottom_only() {
        let img = image(&theme(LookTheme::Horizon));
        assert!(at(&img, 960, 0)[3] > 80 && at(&img, 960, 1079)[3] > 80);
        assert_eq!(at(&img, 960, 0), at(&img, 960, 1079));
        assert_eq!(at(&img, 0, 0)[3], 0);
        assert!(at(&img, 100, 0)[3] < at(&img, 960, 0)[3] / 2);
        assert_eq!(at(&img, 0, 540)[3], 0);
        assert_eq!(at(&img, 1919, 540)[3], 0);
    }

    /// Stealth Frost: a crisp line in c1 at the edge, a faint band behind it,
    /// a soft glow inward; and it never breathes.
    #[test]
    fn the_bezel_is_a_line_over_a_band_and_a_soft_glow() {
        let t = table(&theme(LookTheme::Mono), 0.0);
        assert!(t[0][3] > 100, "{:?}", t[0]);
        assert!(t[1][3] < t[0][3] && t[4][3] > 20, "{t:?}");
        assert!(t[12][3] < t[4][3] && t[12][3] > 0, "{t:?}");
        assert_eq!(t[90][3], 0);
        assert_eq!(table(&theme(LookTheme::Mono), 1.0), t, "no breath");
    }

    /// The live layer (colour-keyed on Windows) shows a thin solid line at the
    /// edge's thickness, whatever the look: every pixel clear or solid, none
    /// near the key.
    #[test]
    fn the_keyed_edge_is_a_thin_solid_line_away_from_the_key() {
        let key = crate::live::COLOUR_KEY;
        let mut magenta = spectrum().active();
        magenta.palette = ColourSet::TwoColour;
        magenta.colours = [config::Rgb(key), config::Rgb([250, 10, 240])];
        let looks = PRESETS.iter().map(|(t, _)| theme(*t)).chain([own(magenta)]);
        for look in looks.filter(|l| l.active().glow > 0) {
            let (geo, paint) = edge(&look, Mode::Live, 1366, 768, 1.25, true, 0.0).unwrap();
            let want = (f32::from(look.active().live.thickness) * 1.25).round() as u32;
            assert_eq!(geo.t, want);
            let img = full(geo, &paint);
            let column: Vec<u8> = (0..60)
                .map(|y| img[((y * 1366 + 683) * 4 + 3) as usize])
                .collect();
            let lit = column.iter().take_while(|&&a| a == 255).count();
            assert_eq!(lit, want as usize, "{column:?}");
            assert!(column[lit..].iter().all(|&a| a == 0), "{column:?}");
            for px in img.as_chunks::<4>().0 {
                match px[3] {
                    0 => {}
                    255 => {
                        let d: u32 = (px[..3].iter().zip(key))
                            .map(|(a, b)| a.abs_diff(b) as u32)
                            .sum();
                        assert!(d > 200, "{px:?} is close to the key");
                    }
                    a => panic!("partial alpha {a}"),
                }
            }
        }
    }

    #[test]
    fn the_preview_paints_the_look_over_a_frame() {
        let (w, h) = (192, 108);
        let grey = vec![60u8; (w * h * 4) as usize];
        let mut a = grey.clone();
        paint(&mut a, w, h, 0.3, &spectrum(), Mode::Overlay);
        assert_ne!(a, grey);
        let centre = ((54 * w + 96) * 4) as usize;
        assert_eq!(a[centre..centre + 3], [60, 60, 60], "the middle stays");
    }

    /// Each kind's edge at 1080p, pixel for pixel (`tests/fixtures/edge-*.png`,
    /// clear in the middle): Glow (the default Spectrum), Optical HUD,
    /// Stealth Frost's bezel, Horizon Pulse. A change to the edge on purpose:
    /// `OPENCIRCLE_UPDATE_GOLDEN=1 cargo test edge::`.
    #[test]
    fn each_kinds_edge_at_1080p_matches_its_golden_image() {
        for (stored, name) in [
            (LookTheme::Spectrum, "edge-default-1080p.png"),
            (LookTheme::Hud, "edge-hud-1080p.png"),
            (LookTheme::Mono, "edge-bezel-1080p.png"),
            (LookTheme::Horizon, "edge-horizon-1080p.png"),
        ] {
            let img = image(&theme(stored));
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name);
            if std::env::var_os("OPENCIRCLE_UPDATE_GOLDEN").is_some() {
                crate::actions::write_png(&path, 1920, 1080, &img).unwrap();
            }
            let png = png::Decoder::new(std::io::Cursor::new(std::fs::read(&path).unwrap()));
            let mut reader = png.read_info().unwrap();
            let mut want = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut want).unwrap();
            assert_eq!((info.width, info.height), (1920, 1080));
            assert!(img == want[..img.len()], "{name} differs from the edge");
        }
    }
}
