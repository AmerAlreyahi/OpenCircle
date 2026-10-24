//! The screen edge (DESIGN.md "Feel", config `[look]`): per mode (overlay,
//! live translate, snip) a soft glow fading inward, a thin line with rounded
//! inner corners, or nothing, in the active look's colours (`theme::along`).
//!
//! Motion: the overlay's activation glow (a wide glow from the first frame
//! that draws back to the edge, the overlay's opening `overlay::APPEAR`) or, without it, a fade-in (glow) / one
//! shimmer sweep (thin); snip the same when its edge animates; live translate a
//! slow flow of the colours round the edge (glow) or a light circling it (thin).
//! Off when the look says so or the OS asks for less motion (Windows).
//!
//! Cheap: the edge is four strip images. Their geometry (where each pixel is
//! along the edge and how deep inside the band) is computed once per size and
//! style (`Cache`); a frame is then two small tables (colour round the edge,
//! alpha per depth) applied to it, and only strips whose pixels changed get a
//! new image (the renderer redraws only those). On the colour-keyed live layer
//! (Windows) every pixel is clear or solid: the glow is an ordered dither,
//! never a blend toward the key colour.
use std::f32::consts::PI;
use std::time::{Duration, Instant};

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Action, Geometry};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Color, Element, Fill, Rectangle, Renderer, Theme, window};

pub use crate::config::EdgeMode as Mode;
use crate::config::{self, EdgeStyle, Look};

use super::theme;

/// The thin line's inner corner radius, logical px.
const RADIUS: f32 = 14.0;
/// A glow reaches this many times its thickness inward.
const GLOW_REACH: f32 = 8.0;
/// Animation on or off (also off when the OS asks for less motion, Windows).
// ponytail: Linux has no reduce-motion read (GNOME's enable-animations needs
// gsettings / D-Bus); read it when a Linux user asks, or flip this.
const ANIMATE: bool = true;
/// Overlay / snip: the thin line's sweep, the glow's fade-in, their frame time
/// and the sweep's peak brightness.
const SWEEP: Duration = Duration::from_millis(700);
const FADE: Duration = Duration::from_millis(450);
const FRAME: Duration = Duration::from_millis(16);
const SWEEP_GLOW: f32 = 0.85;
/// The overlay's activation glow: how far it reaches (logical px), how long it
/// takes to draw back to the edge (the overlay's opening, with its dim and
/// bars) and its frame time.
const ACT_REACH: f32 = 150.0;
const ACTIVATION: Duration = super::overlay::APPEAR;
const ACT_FRAME: Duration = Duration::from_millis(16);
/// Live, thin: one turn of the light, its frame time (20 fps) and brightness.
/// A light, not a turning gradient: a frame changes only the strips near it.
const FLOW_LOOP: Duration = Duration::from_secs(5);
const FLOW_FRAME: Duration = Duration::from_millis(50);
const FLOW_GLOW: f32 = 0.45;
/// The light's width (fraction of the perimeter, Gaussian).
const LIGHT_WIDTH: f32 = 0.035;
/// Live, glow: the colours flow once round the edge in this long, a frame
/// every `DRIFT_FRAME` (5 fps: each frame redraws all four strips, the CPU
/// budget; a slow loop keeps each step small).
const DRIFT_LOOP: Duration = Duration::from_secs(20);
const DRIFT_FRAME: Duration = Duration::from_millis(200);
/// Colours per turn of the edge.
const LUT: usize = 512;

/// The edge over a window that shows a whole monitor at `scale` (physical px
/// per logical px), in the active look. `keyed`: the window's background is a
/// colour key (no partial alpha). Its state (when it started) lives as long
/// as the widget.
pub fn view<'a, M: 'a>(mode: Mode, scale: f32, keyed: bool) -> Element<'a, M> {
    canvas::Canvas::new(Border {
        mode,
        scale,
        keyed,
        look: None,
        opening: None,
    })
    .width(Fill)
    .height(Fill)
    .into()
}

/// The overlay's edge, its activation glow `opening` of the way through
/// (0..1, linear; the overlay's opening steps, `overlay::opening_step`)
/// rather than by the clock, so it keeps step with the dim and the bars.
pub fn opening<'a, M: 'a>(scale: f32, opening: f32) -> Element<'a, M> {
    canvas::Canvas::new(Border {
        mode: Mode::Overlay,
        scale,
        keyed: false,
        look: None,
        opening: Some(opening),
    })
    .width(Fill)
    .height(Fill)
    .into()
}

/// Puts a fixed colour where the live edge is, in a capture (`rgba`, `w` x `h`)
/// of the monitor it frames: a capture that shows the layer (X11) then never
/// changes there while the edge moves, so live mode doesn't re-read the edges.
pub fn mask(rgba: &mut [u8], w: u32, h: u32, scale: f32) {
    let Some(spec) = Spec::of(config::look().live) else {
        return;
    };
    let geo = Geo::new(w, h, scale, spec);
    for [x0, y0, sw, sh] in geo.strips() {
        for y in y0..y0 + sh {
            for x in x0..x0 + sw {
                let i = ((y * w + x) * 4) as usize;
                if geo.level(x, y) > 0
                    && let Some(px) = rgba.get_mut(i..i + 4)
                {
                    px.copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
    }
}

/// `mode`'s edge in `look` at rest, laid over `rgba` (`w` x `h`, straight
/// alpha) as if the screen were `w / scale` logical px wide: Settings' previews.
pub fn paint(rgba: &mut [u8], w: u32, h: u32, scale: f32, look: &Look, mode: Mode) {
    let Some(spec) = Spec::of(look.edge(mode)) else {
        return;
    };
    let cache = Cache::build(Geo::new(w, h, scale, spec));
    let paint = Paint::of(look, spec.style);
    for ([x0, y0, sw, _], px) in render(&cache, &paint, Frame::REST, false) {
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

struct Border {
    mode: Mode,
    scale: f32,
    keyed: bool,
    /// Tests: this look instead of the config's.
    look: Option<Look>,
    /// The overlay's opening, 0..1 (`opening`): the activation's time.
    opening: Option<f32>,
}

#[derive(Default)]
pub struct State {
    /// The first frame's time, and whether the OS allows motion (read then).
    start: Option<Instant>,
    motion: bool,
    cache: Option<Cache>,
    /// What `strips` show.
    drawn: Option<(Geo, Frame, Paint)>,
    /// Logical bounds and pixels of each strip.
    strips: Vec<(Rectangle, Handle)>,
}

impl<M> canvas::Program<M> for Border {
    type State = State;

    /// On each frame: the edge for now, re-rendered only if it changed (and a
    /// strip's image replaced only if its pixels did), and the next frame's
    /// time while it moves.
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
        if state.start.is_none() {
            state.start = Some(*now);
            state.motion = motion_allowed();
        }
        let clock = now.saturating_duration_since(state.start?);
        let elapsed = match self.opening {
            Some(t) if t < 1.0 => ACTIVATION.mul_f32(t),
            Some(_) => clock.max(ACTIVATION),
            None => clock,
        };
        let look = self.look.unwrap_or_else(config::look);
        let (spec, frame, next) = plan(self.mode, &look, elapsed, state.motion);
        let redraw = next.map(|d| Action::request_redraw_at(*now + d));
        let Some(spec) = spec.filter(|_| frame.alpha > 0.0) else {
            state.strips.clear();
            state.drawn = None;
            return redraw;
        };
        let px = |v: f32| (v * self.scale).round() as u32;
        let geo = Geo::new(px(bounds.width), px(bounds.height), self.scale, spec);
        if state.cache.as_ref().is_none_or(|c| c.geo != geo) {
            state.cache = Some(Cache::build(geo));
        }
        let paint = Paint::of(&look, spec.style);
        let key = (geo, frame, paint);
        if state.drawn.as_ref() == Some(&key) {
            return redraw;
        }
        let same_geo = state.drawn.as_ref().is_some_and(|(g, ..)| *g == geo);
        let old = std::mem::take(&mut state.strips);
        let cache = state.cache.as_ref()?;
        state.strips = (render(cache, &key.2, frame, self.keyed)
            .into_iter()
            .enumerate())
        .map(|(i, ([x, y, w, h], rgba))| match old.get(i) {
            Some((at, handle @ Handle::Rgba { pixels, .. }))
                if same_geo && pixels[..] == rgba[..] =>
            {
                (*at, handle.clone())
            }
            _ => {
                // + a quarter pixel: the canvas hands image bounds on
                // with float error and tiny-skia truncates them to
                // whole pixels (a strip at x = 1917 landed on 1916).
                let at = iced::Point::new(x as f32 + 0.25, y as f32 + 0.25);
                let at = Rectangle::new(at, iced::Size::new(w as f32, h as f32));
                (at * (1.0 / self.scale), Handle::from_rgba(w, h, rgba))
            }
        })
        .collect();
        state.drawn = Some(key);
        redraw
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
        for (at, handle) in &state.strips {
            let image = canvas::Image::new(handle.clone())
                .filter_method(FilterMethod::Nearest)
                .snap(true);
            frame.draw_image(*at, image);
        }
        vec![frame.into_geometry()]
    }
}

/// The OS's "show animations" setting (Windows); elsewhere motion is on.
#[cfg(windows)]
fn motion_allowed() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut on = windows::core::BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL into `on`.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut windows::core::BOOL as *mut std::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || on.as_bool()
}

#[cfg(not(windows))]
fn motion_allowed() -> bool {
    true
}

/// The edge's shape in logical px: a glow `inset` deep, or a line `inset`
/// wide; inner corners rounded by `rad`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spec {
    style: EdgeStyle,
    inset: f32,
    rad: f32,
}

impl Spec {
    /// A mode's edge at rest (`None`: no edge).
    fn of(edge: config::Edge) -> Option<Spec> {
        let t = f32::from(edge.thickness);
        match edge.style {
            EdgeStyle::None => None,
            EdgeStyle::Thin => Some(Spec {
                style: EdgeStyle::Thin,
                inset: t,
                rad: RADIUS,
            }),
            EdgeStyle::Glow => Some(Spec::glow(t * GLOW_REACH)),
        }
    }

    fn glow(reach: f32) -> Spec {
        Spec {
            style: EdgeStyle::Glow,
            inset: reach,
            rad: reach / 4.0,
        }
    }
}

/// What the edge shows at one moment: its colours turned `shift` round the
/// edge, `alpha` of its strength, a glow reaching `reach` of its depth, plus a
/// light at `head` along the edge (0..1 clockwise from the top-left corner) of
/// strength `light` (0 = none); `mirrored`: a second light at `1 - head`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    shift: f32,
    alpha: f32,
    reach: f32,
    head: f32,
    light: f32,
    mirrored: bool,
}

impl Frame {
    const REST: Frame = Frame {
        shift: 0.0,
        alpha: 1.0,
        reach: 1.0,
        head: 0.0,
        light: 0.0,
        mirrored: false,
    };
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The edge `elapsed` after the first frame: its shape (`None`: none), the
/// frame, and the time to the next frame (`None`: still from now on).
fn plan(
    mode: Mode,
    look: &Look,
    elapsed: Duration,
    motion: bool,
) -> (Option<Spec>, Frame, Option<Duration>) {
    let edge = look.edge(mode);
    let rest = Spec::of(edge);
    let moving = ANIMATE && motion;
    let secs = |d: Duration| d.as_secs_f32();
    let mut after = elapsed;
    if mode == Mode::Overlay && look.activation_glow && moving {
        if elapsed < ACTIVATION {
            // A wide glow from the first frame draws back to the edge's own
            // glow (or away: a thin line / no edge follows), eased out.
            let t = secs(elapsed) / secs(ACTIVATION);
            let end = match rest {
                Some(s) if s.style == EdgeStyle::Glow => s.inset / ACT_REACH,
                _ => 0.0,
            };
            let back = ease(t);
            let frame = Frame {
                shift: 0.2 * (back - 1.0),
                alpha: 1.0,
                reach: 1.0 + (end - 1.0) * back,
                ..Frame::REST
            };
            return (Some(Spec::glow(ACT_REACH)), frame, Some(ACT_FRAME));
        }
        after = elapsed - ACTIVATION;
    }
    let Some(spec) = rest else {
        return (None, Frame::REST, None);
    };
    let still = (Some(spec), Frame::REST, None);
    if !moving || !edge.animate {
        return still;
    }
    match (mode, spec.style) {
        // Two lights leave the top-left corner both ways (eased out) and meet
        // at the bottom-right, fading in and out.
        (Mode::Overlay | Mode::Snip, EdgeStyle::Thin) if after < SWEEP => {
            let t = secs(after) / secs(SWEEP);
            let frame = Frame {
                head: 0.5 * (1.0 - (1.0 - t).powi(2)),
                light: SWEEP_GLOW * (PI * t).sin(),
                mirrored: true,
                ..Frame::REST
            };
            (Some(spec), frame, Some(FRAME))
        }
        // The overlay right after its activation glow is already there.
        (Mode::Overlay, EdgeStyle::Glow) if after != elapsed => still,
        // A glow grows in from the edge, its colours settling into place.
        (Mode::Overlay | Mode::Snip, EdgeStyle::Glow) if after < FADE => {
            let t = ease(secs(after) / secs(FADE));
            let frame = Frame {
                shift: 0.12 * (t - 1.0),
                alpha: t,
                reach: 0.4 + 0.6 * t,
                ..Frame::REST
            };
            (Some(spec), frame, Some(FRAME))
        }
        (Mode::Live, EdgeStyle::Thin) => {
            let turn = elapsed.as_millis() % FLOW_LOOP.as_millis();
            let frame = Frame {
                head: turn as f32 / FLOW_LOOP.as_millis() as f32,
                light: FLOW_GLOW,
                ..Frame::REST
            };
            (Some(spec), frame, Some(FLOW_FRAME))
        }
        (Mode::Live, _) => {
            // Whole frames only: the same moment always draws the same.
            let frames = DRIFT_LOOP.as_millis() / DRIFT_FRAME.as_millis();
            let n = elapsed.as_millis() / DRIFT_FRAME.as_millis() % frames;
            let frame = Frame {
                shift: n as f32 / frames as f32,
                ..Frame::REST
            };
            (Some(spec), frame, Some(DRIFT_FRAME))
        }
        _ => still,
    }
}

/// What colours a frame is drawn in: the look's edge colours and, for a glow,
/// its strength (0..1).
#[derive(Debug, Clone, PartialEq)]
struct Paint {
    stops: Vec<Color>,
    peak: f32,
}

impl Paint {
    fn of(look: &Look, style: EdgeStyle) -> Paint {
        Paint {
            stops: theme::edge_colours(look),
            peak: match style {
                EdgeStyle::Glow => f32::from(look.glow) / 100.0,
                _ => 1.0,
            },
        }
    }
}

/// The edge's shape on one monitor, physical px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geo {
    w: u32,
    h: u32,
    glow: bool,
    /// Line width / glow depth, and inner corner radius.
    t: u32,
    r: u32,
}

impl Geo {
    fn new(w: u32, h: u32, scale: f32, spec: Spec) -> Geo {
        let px = |v: f32| (v * scale).round() as u32;
        Geo {
            w,
            h,
            glow: spec.style == EdgeStyle::Glow,
            t: px(spec.inset).max(1),
            r: px(spec.rad),
        }
    }

    /// The four strips (x, y, w, h): top and bottom full width, left and right
    /// between them, each `t + r` deep (all a pixel inside the band can be).
    /// None on a screen too small for an edge.
    fn strips(self) -> Vec<[u32; 4]> {
        let (w, h, band) = (self.w, self.h, self.t + self.r);
        if w <= 2 * band || h <= 2 * band {
            return Vec::new();
        }
        let down = h - 2 * band;
        vec![
            [0, 0, w, band],
            [w - band, band, band, down],
            [0, h - band, w, band],
            [0, band, band, down],
        ]
    }

    /// How far pixel (x, y)'s centre is outside the inner rounded rectangle
    /// (`t` in from the screen edge, corners rounded by `r`); negative inside.
    fn outside(self, x: u32, y: u32) -> f32 {
        let (cx, cy) = (self.w as f32 / 2.0, self.h as f32 / 2.0);
        let (t, r) = (self.t as f32, self.r as f32);
        let qx = (x as f32 + 0.5 - cx).abs() - (cx - t) + r;
        let qy = (y as f32 + 0.5 - cy).abs() - (cy - t) + r;
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
    }

    /// How strongly the edge covers pixel (x, y), 0..255: a line's coverage, a
    /// glow's nearness to the screen edge (255 at it, 0 at its inner end).
    fn level(self, x: u32, y: u32) -> u8 {
        let out = self.outside(x, y);
        let v = if self.glow {
            if out <= 0.0 {
                0.0
            } else {
                (out / self.t as f32).min(1.0)
            }
        } else {
            (out + 0.5).clamp(0.0, 1.0)
        };
        (v * 255.0).round() as u8
    }

    /// Where pixel (x, y) is along the edge, clockwise from the top-left
    /// corner (0..1): where the ray from the screen's centre through it leaves
    /// the screen (smooth round the corners, unlike the nearest edge).
    fn along(self, x: u32, y: u32) -> f32 {
        let (w, h) = (self.w as f32, self.h as f32);
        let (dx, dy) = (x as f32 + 0.5 - w / 2.0, y as f32 + 0.5 - h / 2.0);
        let (kx, ky) = (w / 2.0 / dx.abs().max(1e-6), h / 2.0 / dy.abs().max(1e-6));
        let k = kx.min(ky);
        let (qx, qy) = (w / 2.0 + dx * k, h / 2.0 + dy * k);
        let s = match (kx < ky, dx > 0.0, dy > 0.0) {
            (false, _, false) => qx,                    // top
            (true, true, _) => w + qy,                  // right
            (false, _, true) => w + h + (w - qx),       // bottom
            (true, false, _) => 2.0 * w + h + (h - qy), // left
        };
        (s / (2.0 * (w + h))).rem_euclid(1.0)
    }
}

/// One strip's rectangle and, per pixel, its colour index round the edge and its level.
type Strip = ([u32; 4], Vec<(u16, u8)>);

struct Cache {
    geo: Geo,
    strips: Vec<Strip>,
}

impl Cache {
    fn build(geo: Geo) -> Cache {
        let strips = (geo.strips().into_iter())
            .map(|[x0, y0, w, h]| {
                let px = (y0..y0 + h)
                    .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        let at = (geo.along(x, y) * LUT as f32) as usize % LUT;
                        (at as u16, geo.level(x, y))
                    })
                    .collect();
                ([x0, y0, w, h], px)
            })
            .collect();
        Cache { geo, strips }
    }
}

/// The colours round the edge for `frame` (`LUT` steps), lit toward
/// `theme::SHIMMER` near its light(s); over a colour key (`keyed`), kept
/// clear of the key.
fn colours(paint: &Paint, frame: Frame, keyed: bool) -> Vec<[u8; 3]> {
    let to = |u: f32, head: f32| {
        let d = (u - head).rem_euclid(1.0);
        d.min(1.0 - d)
    };
    (0..LUT)
        .map(|i| {
            let u = i as f32 / LUT as f32;
            let mut c = theme::along(&paint.stops, u - frame.shift);
            if frame.light > 0.0 {
                let mut d = to(u, frame.head);
                if frame.mirrored {
                    d = d.min(to(u, 1.0 - frame.head));
                }
                let light = frame.light * (-(d / LIGHT_WIDTH).powi(2)).exp();
                c = mix(c, theme::SHIMMER, light);
            }
            let [r, g, b, _] = c.into_rgba8();
            if keyed { off_key([r, g, b]) } else { [r, g, b] }
        })
        .collect()
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

fn mix(a: Color, b: Color, k: f32) -> Color {
    let m = |a: f32, b: f32| a + (b - a) * k;
    Color::from_rgb(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// Alpha (0..255) per level for `frame`: a line's coverage, a glow's soft
/// fall-off from the screen edge to `reach` of its depth.
fn alphas(glow: bool, paint: &Paint, frame: Frame) -> [u8; 256] {
    let mut out = [0; 256];
    for (level, a) in out.iter_mut().enumerate() {
        let v = level as f32 / 255.0;
        let strength = if glow {
            // depth into the band, 0 at the screen edge, 1 at its inner end
            let depth = 1.0 - v;
            let x = if frame.reach > 0.0 {
                depth / frame.reach
            } else {
                1.0
            };
            if level == 0 || x >= 1.0 {
                0.0
            } else {
                paint.peak * (1.0 - x).powi(2)
            }
        } else {
            v
        };
        *a = (strength * frame.alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// 4x4 ordered dither thresholds (1..255): a keyed glow's pixel is solid where
/// its alpha beats the threshold.
const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Each strip's rectangle and pixels (RGBA, straight alpha). `keyed`: every
/// pixel fully clear or fully solid.
fn render(cache: &Cache, paint: &Paint, frame: Frame, keyed: bool) -> Vec<([u32; 4], Vec<u8>)> {
    let lut = colours(paint, frame, keyed);
    let alpha = alphas(cache.geo.glow, paint, frame);
    (cache.strips.iter())
        .map(|&([x0, y0, w, h], ref px)| {
            let mut rgba = vec![0; (w * h * 4) as usize];
            for (i, &(at, level)) in px.iter().enumerate() {
                let mut a = alpha[level as usize];
                if keyed {
                    let (x, y) = (x0 + i as u32 % w, y0 + i as u32 / w);
                    let threshold = BAYER[(y % 4 * 4 + x % 4) as usize] * 16 + 8;
                    a = if a > threshold { 255 } else { 0 };
                }
                if a > 0 {
                    let [r, g, b] = lut[at as usize];
                    rgba[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, a]);
                }
            }
            ([x0, y0, w, h], rgba)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColourSet, LookConfig, LookTheme, PRESETS};

    fn spectrum() -> Look {
        LookConfig::default().active()
    }

    fn aurora() -> Look {
        LookTheme::Aurora.look(&spectrum())
    }

    fn thin(t: f32) -> Spec {
        Spec::of(config::Edge {
            style: EdgeStyle::Thin,
            thickness: t as u8,
            animate: false,
        })
        .unwrap()
    }

    fn area(strips: &[[u32; 4]]) -> u32 {
        strips.iter().map(|[_, _, sw, sh]| sw * sh).sum()
    }

    #[test]
    fn geometry_follows_the_monitor_size_and_scale() {
        for (w, h, scale, t, r) in [
            (1920, 1080, 1.0, 3, 14),
            (2560, 1440, 1.25, 4, 18),
            (3440, 1440, 1.5, 5, 21),
            (3840, 2160, 2.0, 6, 28),
        ] {
            let geo = Geo::new(w, h, scale, thin(3.0));
            assert_eq!((geo.t, geo.r), (t, r), "{w}x{h} @ {scale}");
            let strips = geo.strips();
            assert_eq!(strips.len(), 4);
            for [x, y, sw, sh] in &strips {
                assert!(x + sw <= w && y + sh <= h, "inside the screen");
            }
            // no overlap: top and bottom full width, the sides between
            let band = t + r;
            assert_eq!(area(&strips), 2 * w * band + 2 * band * (h - 2 * band));
        }
        // too small for an edge: nothing (no zero-sized images)
        assert!(Geo::new(30, 30, 1.0, thin(3.0)).strips().is_empty());
        assert!(Geo::new(1, 1, 2.0, thin(3.0)).strips().is_empty());
        // the glow: 8x its thickness deep, a quarter of that as corner radius
        let glow = Geo::new(1920, 1080, 1.0, Spec::glow(48.0));
        assert_eq!((glow.t, glow.r, glow.glow), (48, 12, true));
    }

    #[test]
    fn a_line_is_solid_to_the_edge_with_rounded_inner_corners() {
        let geo = Geo::new(1920, 1080, 1.0, thin(3.0));
        let t = geo.t;
        for (x, y) in [(0, 0), (960, 0), (960, t - 1), (1919, 540), (960, 1079)] {
            assert_eq!(geo.level(x, y), 255, "({x}, {y})");
        }
        for (x, y) in [(960, t), (t, 540), (1919 - t, 540), (960, 540)] {
            assert_eq!(geo.level(x, y), 0, "({x}, {y})");
        }
        let d = t + geo.r / 4;
        assert_eq!(geo.level(d, d), 255);
        assert_eq!(geo.level(1919 - d, 1079 - d), 255);
        assert_eq!(geo.level(t + geo.r, t + geo.r), 0);
    }

    #[test]
    fn a_glow_fades_inward_and_the_strips_hold_it_all() {
        let geo = Geo::new(800, 600, 1.0, Spec::glow(48.0));
        let paint = Paint::of(&spectrum(), EdgeStyle::Glow);
        let alpha = alphas(true, &paint, Frame::REST);
        let at = |x, y| alpha[geo.level(x, y) as usize];
        // strongest at the edge, weaker inward, gone past the reach
        let column: Vec<u8> = [0, 8, 16, 32, 47, 48, 80].map(|y| at(400, y)).to_vec();
        assert!(column.windows(2).all(|p| p[0] >= p[1]), "{column:?}");
        assert!(column[0] >= 190 && column[3] < column[1], "{column:?}");
        assert_eq!(column[5..], [0, 0]);
        // the glow's strength scales its peak
        let half = Paint { peak: 0.4, ..paint };
        assert!(alphas(true, &half, Frame::REST)[255] < 110);
        // every lit pixel is inside a strip
        let strips = geo.strips();
        let inside = |x, y| {
            (strips.iter())
                .any(|[sx, sy, sw, sh]| (*sx..sx + sw).contains(&x) && (*sy..sy + sh).contains(&y))
        };
        for y in 0..600 {
            for x in 0..800 {
                if geo.level(x, y) > 0 {
                    assert!(inside(x, y), "lit pixel ({x}, {y}) outside the strips");
                }
            }
        }
        // all four strips together at 1920x1080, a 48 px glow: under 20% of the screen
        let big = Geo::new(1920, 1080, 1.0, Spec::glow(48.0));
        assert!(area(&big.strips()) * 100 < 20 * 1920 * 1080);
    }

    #[test]
    fn the_cache_is_per_size_and_style_and_the_palette_only_recolours() {
        let a = Geo::new(640, 360, 1.0, Spec::glow(48.0));
        let cache = Cache::build(a);
        assert_eq!(cache.geo, a);
        // the same size and style: the same geometry (the view keeps it)
        assert_eq!(Geo::new(640, 360, 1.0, Spec::glow(48.0)), a);
        // another size, scale or style: another geometry (rebuilt)
        for other in [
            Geo::new(800, 360, 1.0, Spec::glow(48.0)),
            Geo::new(640, 360, 1.5, Spec::glow(48.0)),
            Geo::new(640, 360, 1.0, Spec::glow(36.0)),
            Geo::new(640, 360, 1.0, thin(3.0)),
        ] {
            assert_ne!(other, a);
        }
        // another palette: the same cache, other pixels
        let spectrum = render(
            &cache,
            &Paint::of(&spectrum(), EdgeStyle::Glow),
            Frame::REST,
            false,
        );
        let aurora = render(
            &cache,
            &Paint::of(&aurora(), EdgeStyle::Glow),
            Frame::REST,
            false,
        );
        assert_ne!(spectrum, aurora);
        assert_eq!(spectrum.len(), 4);
        // and the colours go round the edge: top-left violet, the right side warm
        let top = &spectrum[0].1;
        let px = |x: usize| &top[x * 4..x * 4 + 4];
        assert!(px(2)[2] > px(2)[1], "violet near the top-left: {:?}", px(2));
        let ([_, _, band, _], right) = &spectrum[1];
        let edge = (*band as usize - 1) * 4; // the first row's pixel at the screen edge
        let px = &right[edge..edge + 4];
        assert!(px[3] > 0 && px[0] > px[2], "warm on the right: {px:?}");
    }

    #[test]
    fn overlay_activation_starts_wide_and_eases_back_to_the_edge() {
        let look = spectrum();
        let ms = Duration::from_millis;
        let (spec, first, next) = plan(Mode::Overlay, &look, ms(0), true);
        assert_eq!((first.alpha, first.reach), (1.0, 1.0), "frame 1 is t = 0");
        assert_eq!((spec.unwrap().inset, next), (ACT_REACH, Some(ACT_FRAME)));
        // the reach only shrinks, and is the edge's own by the end
        let reach = |t: u64| plan(Mode::Overlay, &look, ms(t), true).1.reach;
        let steps: Vec<f32> = (0..=ACTIVATION.as_millis() as u64 - 1)
            .step_by(10)
            .map(reach)
            .collect();
        assert!(steps.windows(2).all(|w| w[1] <= w[0]), "{steps:?}");
        let last = reach(ACTIVATION.as_millis() as u64 - 1);
        assert!((last - 48.0 / ACT_REACH).abs() < 0.01, "{last}");
        // then the edge's own glow, still; the reach matched, so no jump
        let (spec, rest, next) = plan(Mode::Overlay, &look, ACTIVATION + ms(1), true);
        assert_eq!((spec.unwrap().inset, rest, next), (48.0, Frame::REST, None));
        // no activation glow: the glow grows in, then rests
        let quiet = Look {
            activation_glow: false,
            ..look
        };
        let (spec, grow, _) = plan(Mode::Overlay, &quiet, ms(100), true);
        assert_eq!(spec.unwrap().inset, 48.0);
        assert!(grow.alpha > 0.0 && grow.alpha < 1.0);
        assert_eq!(plan(Mode::Overlay, &quiet, FADE, true).1, Frame::REST);
        // reduce motion: at rest at once
        let (spec, still, next) = plan(Mode::Overlay, &look, ms(0), false);
        assert_eq!(
            (spec.unwrap().inset, still, next),
            (48.0, Frame::REST, None)
        );
    }

    #[test]
    fn thin_sweeps_once_live_moves_snip_stays() {
        let ms = Duration::from_millis;
        let look = aurora();
        assert!(!look.activation_glow);
        let (_, first, next) = plan(Mode::Overlay, &look, ms(0), true);
        assert_eq!((first.light, next), (0.0, Some(FRAME)));
        let (_, mid, _) = plan(Mode::Overlay, &look, ms(350), true);
        assert!((mid.light - SWEEP_GLOW).abs() < 1e-3 && mid.mirrored);
        assert!((mid.head - 0.375).abs() < 1e-3, "eased: past halfway");
        assert_eq!(plan(Mode::Overlay, &look, SWEEP, true).2, None);
        // live thin: a light goes round every FLOW_LOOP, 20 fps
        assert_eq!(plan(Mode::Live, &look, ms(2500), true).1.head, 0.5);
        let hours = plan(
            Mode::Live,
            &look,
            Duration::from_secs(36_000) + ms(500),
            true,
        );
        assert!((hours.1.head - 0.1).abs() < 1e-6 && hours.2 == Some(FLOW_FRAME));
        // live glow: the colours drift, at most 20 fps, the same moment the same frame
        let glow = spectrum();
        let (_, a, next) = plan(Mode::Live, &glow, ms(1000), true);
        assert!(next.unwrap() >= Duration::from_millis(50));
        assert!(a.shift > 0.0 && a.shift < 0.2);
        assert_eq!(plan(Mode::Live, &glow, ms(1010), true).1, a);
        let (_, wrapped, _) = plan(Mode::Live, &glow, DRIFT_LOOP + ms(1000), true);
        assert_eq!(wrapped, a);
        // snip: thin and still by default; none = nothing
        assert_eq!(plan(Mode::Snip, &glow, ms(0), true).2, None);
        let mut none = glow;
        none.snip.style = EdgeStyle::None;
        assert_eq!(plan(Mode::Snip, &none, ms(0), true).0, None);
        // reduce motion: nothing moves
        for mode in [Mode::Overlay, Mode::Live] {
            assert_eq!(plan(mode, &look, ms(300), false).2, None);
        }
    }

    #[test]
    fn a_live_frame_changes_few_strips_for_a_light_and_all_for_a_drift() {
        let ms = Duration::from_millis;
        let changed = |look: &Look, at: u64, step: Duration| {
            let (spec, a, _) = plan(Mode::Live, look, ms(at), true);
            let (_, b, _) = plan(Mode::Live, look, ms(at) + step, true);
            let geo = Geo::new(1920, 1080, 1.0, spec.unwrap());
            let cache = Cache::build(geo);
            let paint = Paint::of(look, geo_style(geo));
            let (a, b) = (
                render(&cache, &paint, a, true),
                render(&cache, &paint, b, true),
            );
            a.iter().zip(&b).filter(|(a, b)| a != b).count()
        };
        for at in (0..5000).step_by(500) {
            let n = changed(&aurora(), at, FLOW_FRAME);
            assert!((1..=2).contains(&n), "{n} strips at {at} ms");
        }
        assert_eq!(changed(&spectrum(), 1000, DRIFT_FRAME), 4);
    }

    fn geo_style(geo: Geo) -> EdgeStyle {
        if geo.glow {
            EdgeStyle::Glow
        } else {
            EdgeStyle::Thin
        }
    }

    #[test]
    fn the_aurora_line_runs_violet_to_cyan_and_the_lights_brighten_it() {
        let paint = Paint::of(&aurora(), EdgeStyle::Thin);
        let still = colours(&paint, Frame::REST, false);
        let near = |a: [u8; 3], b: Color| {
            let [r, g, bb, _] = b.into_rgba8();
            (a[0].abs_diff(r) as u32 + a[1].abs_diff(g) as u32 + a[2].abs_diff(bb) as u32) < 6
        };
        assert!(near(still[0], theme::STROKE_START));
        assert!(near(still[LUT / 2], theme::STROKE_END));
        let lum = |c: [u8; 3]| c.iter().map(|&v| v as u32).sum::<u32>();
        let sweep = plan(Mode::Overlay, &aurora(), Duration::from_millis(350), true).1;
        let lit = colours(&paint, sweep, false);
        let i = |u: f32| (u * LUT as f32) as usize;
        assert!(lum(lit[i(0.375)]) > lum(still[i(0.375)]) + 60);
        assert!(lum(lit[i(0.625)]) > lum(still[i(0.625)]) + 60);
        assert_eq!(lit[0], still[0]);
    }

    #[test]
    fn keyed_edges_have_no_key_pixels_and_no_partial_alpha() {
        let key = crate::live::COLOUR_KEY;
        let ms = Duration::from_millis;
        // every preset, glow and line, over the key colour
        let mut looks: Vec<Look> = PRESETS.iter().map(|(_, l)| *l).collect();
        let mut magenta = spectrum();
        magenta.palette = ColourSet::TwoColour;
        magenta.colours = [config::Rgb(key), config::Rgb([250, 10, 240])];
        looks.push(magenta);
        for look in looks {
            for (spec, frame) in [
                (
                    Spec::of(look.live),
                    plan(Mode::Live, &look, ms(700), true).1,
                ),
                (Some(Spec::glow(48.0)), Frame::REST),
                (Some(thin(3.0)), Frame::REST),
            ] {
                let Some(spec) = spec else { continue };
                let cache = Cache::build(Geo::new(1366, 768, 1.25, spec));
                let paint = Paint::of(&look, spec.style);
                let mut solid = 0;
                for (_, rgba) in render(&cache, &paint, frame, true) {
                    for px in rgba.as_chunks::<4>().0 {
                        match px[3] {
                            0 => {}
                            255 => {
                                solid += 1;
                                let d: u32 = (px[..3].iter().zip(key))
                                    .map(|(a, b)| a.abs_diff(b) as u32)
                                    .sum();
                                assert!(d > 200, "{px:?} is close to the key");
                            }
                            a => panic!("partial alpha {a}: a blend toward the key"),
                        }
                    }
                }
                if look.glow > 0 || spec.style == EdgeStyle::Thin {
                    assert!(solid > 0, "{:?} drew nothing", look.palette);
                }
            }
        }
        // unkeyed: soft
        let cache = Cache::build(Geo::new(640, 360, 1.0, Spec::glow(48.0)));
        let soft = render(
            &cache,
            &Paint::of(&spectrum(), EdgeStyle::Glow),
            Frame::REST,
            false,
        );
        assert!(
            soft.iter().any(|(_, rgba)| {
                (rgba.as_chunks::<4>().0.iter()).any(|p| p[3] > 0 && p[3] < 255)
            })
        );
    }

    #[test]
    fn the_preview_paints_the_look_over_a_frame() {
        let (w, h) = (192, 108);
        let grey = vec![60u8; (w * h * 4) as usize];
        let mut a = grey.clone();
        paint(&mut a, w, h, 0.1, &spectrum(), Mode::Overlay);
        assert_ne!(a, grey);
        let centre = ((54 * w + 96) * 4) as usize;
        assert_eq!(a[centre..centre + 3], [60, 60, 60], "the middle stays");
        let mut none = spectrum();
        none.overlay.style = EdgeStyle::None;
        let mut b = grey.clone();
        paint(&mut b, w, h, 0.1, &none, Mode::Overlay);
        assert_eq!(b, grey);
    }

    /// `cargo test --release border::tests::frame_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "timing, release only"]
    fn frame_cost() {
        let look = spectrum();
        let cache = Cache::build(Geo::new(1920, 1080, 1.0, Spec::of(look.live).unwrap()));
        let paint = Paint::of(&look, EdgeStyle::Glow);
        let start = Instant::now();
        for i in 0..50 {
            let frame = Frame {
                shift: i as f32 / 150.0,
                ..Frame::REST
            };
            std::hint::black_box(render(&cache, &paint, frame, false));
        }
        println!("render: {:?} per frame", start.elapsed() / 50);
    }

    /// `OPENCIRCLE_BORDER_PNG=<dir> cargo test border::tests::snapshots -- --ignored`
    /// writes the edge as the canvas draws it (headless tiny-skia, 1920x1080).
    #[test]
    #[ignore = "writes PNGs to look at"]
    fn snapshots() {
        use iced::advanced::renderer::{Headless, Style};
        use iced::advanced::widget::Tree;
        use iced::advanced::{Layout, Shell, clipboard, layout};
        let dir = std::path::PathBuf::from(
            std::env::var("OPENCIRCLE_BORDER_PNG").expect("OPENCIRCLE_BORDER_PNG=<dir>"),
        );
        let size = iced::Size::new(1920.0, 1080.0);
        let screen = Rectangle::with_size(size);
        let [r, g, b] = crate::live::COLOUR_KEY;
        let (grey, key) = (Color::from_rgb8(40, 42, 54), Color::from_rgb8(r, g, b));
        let calm = LookTheme::Calm.look(&spectrum());
        let neon = LookTheme::Neon.look(&spectrum());
        let sunset = LookTheme::Sunset.look(&spectrum());
        for (name, look, mode, keyed, ms, background) in [
            (
                "overlay-activation-150ms",
                spectrum(),
                Mode::Overlay,
                false,
                150,
                grey,
            ),
            (
                "overlay-activation-400ms",
                spectrum(),
                Mode::Overlay,
                false,
                400,
                grey,
            ),
            (
                "overlay-glow-rest",
                spectrum(),
                Mode::Overlay,
                false,
                2000,
                grey,
            ),
            (
                "live-glow-1000ms",
                spectrum(),
                Mode::Live,
                false,
                1000,
                grey,
            ),
            (
                "live-glow-5000ms",
                spectrum(),
                Mode::Live,
                false,
                5000,
                grey,
            ),
            ("live-glow-keyed", spectrum(), Mode::Live, true, 1000, key),
            (
                "live-aurora-1000ms",
                aurora(),
                Mode::Live,
                false,
                1000,
                grey,
            ),
            (
                "overlay-aurora-sweep-300ms",
                aurora(),
                Mode::Overlay,
                false,
                300,
                grey,
            ),
            ("overlay-sunset", sunset, Mode::Overlay, false, 2000, grey),
            ("overlay-neon", neon, Mode::Overlay, false, 2000, grey),
            ("overlay-calm", calm, Mode::Overlay, false, 2000, grey),
            ("snip-thin", spectrum(), Mode::Snip, false, 300, grey),
        ] {
            let mut element: Element<'_, ()> = canvas::Canvas::new(Border {
                mode,
                scale: 1.0,
                keyed,
                look: Some(look),
                opening: None,
            })
            .width(Fill)
            .height(Fill)
            .into();
            let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut tree = Tree::new(element.as_widget());
            let limits = layout::Limits::new(iced::Size::ZERO, size);
            let node = element
                .as_widget_mut()
                .layout(&mut tree, &renderer, &limits);
            let start = Instant::now();
            for at in [start, start + Duration::from_millis(ms)] {
                let event = canvas::Event::Window(window::Event::RedrawRequested(at));
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                element.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    Cursor::Unavailable,
                    &renderer,
                    &mut clipboard::Null,
                    &mut shell,
                    &screen,
                );
            }
            element.as_widget().draw(
                &tree,
                &mut renderer,
                &Theme::Dark,
                &Style::default(),
                Layout::new(&node),
                Cursor::Unavailable,
                &screen,
            );
            let rgba = renderer.screenshot(iced::Size::new(1920, 1080), 1.0, background);
            let path = dir.join(format!("edge-{name}.png"));
            crate::actions::write_png(&path, 1920, 1080, &rgba).unwrap();
        }
    }
}
