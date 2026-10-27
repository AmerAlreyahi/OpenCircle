//! The screen edge (DESIGN.md "Feel", config `[look]`): per mode (overlay,
//! live translate, snip) the design's Active Look edge, a soft inset glow
//! inside the screen edges (`box-shadow: inset 0 0 22px c1, inset 0 0 54px c2`
//! of the design's `.effect-ambient-glow`), in the active look's two colours
//! (`theme::edge_glow`); or nothing.
//!
//! Static (owner, 2026-10-27): no moving colours, no draw-back, no travel. The
//! edge is four strip images made once per size and look, on the first frame;
//! every later frame draws the same images and so redraws nothing. The
//! overlay's own fade-in (`windows::fade`) moves it as a whole.
//! The one motion is the design's `ripplePulse`, a barely-there breath of the
//! overlay's glow once its opening is over (owner, 2026-10-27): 4 s, the
//! blurs going 22/54 px -> 38/85 px -> 22/54 px, redrawn as the edge strips
//! ONLY (never the screen), `STEP` apart (12.5 fps), from a cached depth map so
//! a step is a table lookup per pixel. Live translate and snip stay still, and
//! so does the overlay when the OS asks for reduced motion.
//! On the colour-keyed live layer (Windows) there is no partial alpha: a thin
//! solid line in c1 stands in for the glow.
use std::time::{Duration, Instant};

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Action, Geometry};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Color, Element, Fill, Rectangle, Renderer, Theme, window};

pub use crate::config::EdgeMode as Mode;
use crate::config::{self, EdgeStyle, Look};

use super::theme;

/// The screen's corner radius in the overlay, logical px (the design's 14).
const RADIUS: f32 = 14.0;
/// The design's two blurs (c1's layer, c2's), logical px; a CSS blur of `b`
/// is a Gaussian of sigma `b / 2`.
const BLUR: [f32; 2] = [22.0, 54.0];
/// The blurs at the breath's peak (the design's 50 % keyframe).
const BLUR_PEAK: [f32; 2] = [38.0, 85.0];
/// The breath: its length and the gap between redraws.
const BREATH: Duration = Duration::from_secs(4);
const STEP: Duration = Duration::from_millis(80);
/// How far inward the glow reaches at its peak, logical px: past this the
/// outer layer at full strength is under half a level of alpha.
const REACH: f32 = 102.0;

/// The edge over a window that shows a whole monitor at `scale` (physical px
/// per logical px), in the active look. `keyed`: the window's background is a
/// colour key (no partial alpha).
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

pub fn view<'a, M: 'a>(mode: Mode, scale: f32, keyed: bool, breathing: Breathe) -> Element<'a, M> {
    canvas::Canvas::new(Border {
        mode,
        scale,
        keyed,
        breathing,
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
    let Some((geo, table)) = edge(&config::look(), Mode::Live, w, h, scale, false, 0.0) else {
        return Vec::new();
    };
    let strips = geo.strips();
    for &[x0, y0, sw, sh] in &strips {
        for y in y0..y0 + sh {
            for x in x0..x0 + sw {
                let i = ((y * w + x) * 4) as usize;
                if table[geo.index(x, y)][3] > 0
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
/// the screen were `w / scale` logical px wide: Settings' previews.
pub fn paint(rgba: &mut [u8], w: u32, h: u32, scale: f32, look: &Look, mode: Mode) {
    let Some((geo, table)) = edge(look, mode, w, h, scale, false, 0.0) else {
        return;
    };
    for ([x0, y0, sw, _], px) in render(&depths(geo), &table) {
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
    breathing: Breathe,
    /// Tests: this look instead of the config's.
    look: Option<Look>,
}

#[derive(Default)]
pub struct State {
    /// What `strips` show.
    drawn: Option<(Geo, Vec<[u8; 4]>)>,
    /// Each strip's depth map (`depths`), kept while the geometry is.
    depths: Option<(Geo, Vec<Depths>)>,
    /// When the breath began (the first frame that was allowed to).
    since: Option<Instant>,
    /// Logical bounds and pixels of each strip.
    strips: Vec<(Rectangle, Handle)>,
}

impl<M> canvas::Program<M> for Border {
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
        let mut breathing = self.breathing != Breathe::Off
            && self.mode == Mode::Overlay
            && !super::motion::reduced();
        if breathing && self.breathing == Breathe::Paused && state.drawn.is_some() {
            return None;
        }
        breathing &= self.breathing == Breathe::On;
        let since = *state.since.get_or_insert(*now);
        if !breathing {
            state.since = None;
        }
        let step = (now.saturating_duration_since(since).as_millis() / STEP.as_millis()) as u32;
        let breath = if breathing { breath(step) } else { 0.0 };
        let px = |v: f32| (v * self.scale).round() as u32;
        let look = self.look.unwrap_or_else(config::look);
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
            .then(|| Action::request_redraw_at(since + STEP * (step + 1)));
        if state.drawn == want {
            return next;
        }
        if let Some((geo, table)) = &want {
            if state.depths.as_ref().is_none_or(|(g, _)| g != geo) {
                state.depths = Some((*geo, depths(*geo)));
            }
            let depths = &state.depths.as_ref()?.1;
            state.strips = (render(depths, table).into_iter())
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
        for (at, handle) in &state.strips {
            let image = canvas::Image::new(handle.clone())
                .filter_method(FilterMethod::Nearest)
                .snap(true);
            frame.draw_image(*at, image);
        }
        vec![frame.into_geometry()]
    }
}

/// The edge's shape on one monitor, physical px: the screen's size, the band
/// it lives in (`t` deep: the glow's reach, or the line's width) and the
/// screen's corner radius `r`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geo {
    w: u32,
    h: u32,
    t: u32,
    r: u32,
}

/// How far into the breath (0..1: 0 = the design's 22/54 px blurs, 1 = 38/85)
/// step `step` of the 4 s cycle is: eased, there and back.
fn breath(step: u32) -> f32 {
    let t = STEP.as_secs_f32() * step as f32 / BREATH.as_secs_f32();
    0.5 - 0.5 * (std::f32::consts::TAU * t).cos()
}

/// The edge `mode` shows in `look` on a `w` x `h` screen at `scale`: its shape
/// and its RGBA (straight alpha) per whole px of depth from the screen edge,
/// the last entry clear (`None`: no edge). `keyed`: a solid line, no blending.
/// `breath`: see `breath()`.
fn edge(
    look: &Look,
    mode: Mode,
    w: u32,
    h: u32,
    scale: f32,
    keyed: bool,
    breath: f32,
) -> Option<(Geo, Vec<[u8; 4]>)> {
    let e = look.edge(mode);
    let strength = f32::from(look.glow) / 100.0;
    if e.style == EdgeStyle::None || strength <= 0.0 {
        return None;
    }
    let px = |v: f32| (v * scale).round() as u32;
    let [c1, c2] = theme::edge_glow(look);
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
        let sigma: Vec<f32> = (BLUR.iter().zip(BLUR_PEAK))
            .map(|(b, peak)| (b + (peak - b) * breath) / 2.0 * scale)
            .collect();
        let table = (0..=t)
            .map(|i| {
                let d = i as f32 + 0.5;
                let tail = |s: f32| 0.5 * erfc(d / (s * std::f32::consts::SQRT_2));
                let a1 = c1.a * strength * tail(sigma[0]);
                let a2 = c2.a * strength * tail(sigma[1]);
                // c1's layer over c2's
                let a = a1 + a2 * (1.0 - a1);
                if i == t || a < 0.5 / 255.0 {
                    return [0; 4];
                }
                let mix = |u: f32, v: f32| (u * a1 + v * a2 * (1.0 - a1)) / a;
                let [r, g, b, _] =
                    Color::from_rgb(mix(c1.r, c2.r), mix(c1.g, c2.g), mix(c1.b, c2.b)).into_rgba8();
                [r, g, b, (a * 255.0).round() as u8]
            })
            .collect();
        (t, table)
    };
    let geo = Geo {
        w,
        h,
        t,
        r: px(RADIUS),
    };
    Some((geo, table))
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
    /// The four strips (x, y, w, h): top and bottom full width, left and right
    /// between them, each as deep as the band or the corner radius (all a lit
    /// pixel can be). None on a screen too small for an edge.
    fn strips(self) -> Vec<[u32; 4]> {
        let (w, h, band) = (self.w, self.h, self.t.max(self.r));
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

    /// Which table entry pixel (x, y) takes: its centre's depth inside the
    /// screen's rounded rectangle (0 in a square corner outside it), capped at
    /// the table's clear last entry.
    fn index(self, x: u32, y: u32) -> usize {
        let (cx, cy) = (self.w as f32 / 2.0, self.h as f32 / 2.0);
        let r = self.r as f32;
        let qx = (x as f32 + 0.5 - cx).abs() - (cx - r);
        let qy = (y as f32 + 0.5 - cy).abs() - (cy - r);
        let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
        ((-outside).max(0.0) as usize).min(self.t as usize)
    }
}

/// A strip's rectangle and, per pixel, its table entry (`Geo::index`).
type Depths = ([u32; 4], Vec<u16>);

/// Every strip's depth map: made once per geometry, so that a breath step is
/// a lookup per pixel and not a distance per pixel.
fn depths(geo: Geo) -> Vec<Depths> {
    (geo.strips().into_iter())
        .map(|rect @ [x0, y0, w, h]| {
            let at = |i: u32| geo.index(x0 + i % w, y0 + i / w) as u16;
            (rect, (0..w * h).map(at).collect())
        })
        .collect()
}

/// Each strip's rectangle and pixels (RGBA, straight alpha).
fn render(depths: &[Depths], table: &[[u8; 4]]) -> Vec<([u32; 4], Vec<u8>)> {
    (depths.iter())
        .map(|(rect, at)| {
            let mut rgba = vec![0; at.len() * 4];
            for (chunk, &i) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(at) {
                *chunk = table[usize::from(i)];
            }
            (*rect, rgba)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColourSet, LookConfig, LookTheme, PRESETS};
    use std::time::{Duration, Instant};

    fn spectrum() -> Look {
        LookConfig::default().active()
    }

    fn rgba8(c: Color) -> [u8; 4] {
        c.into_rgba8()
    }

    /// The whole edge as one `w` x `h` image (clear where it is not).
    fn full(geo: Geo, table: &[[u8; 4]]) -> Vec<u8> {
        let mut out = vec![0; (geo.w * geo.h * 4) as usize];
        for ([x0, y0, sw, _], px) in render(&depths(geo), table) {
            for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
                let (x, y) = (x0 + i as u32 % sw, y0 + i as u32 / sw);
                let j = ((y * geo.w + x) * 4) as usize;
                out[j..j + 4].copy_from_slice(p);
            }
        }
        out
    }

    #[test]
    fn the_glow_is_the_designs_inset_shadow_in_the_looks_two_colours() {
        // c1 (22 px blur) over c2 (54 px): strongest at the screen edge, about
        // half a layer's alpha there (a CSS blur), gone past the reach
        let (_, table) = edge(&spectrum(), Mode::Overlay, 1920, 1080, 1.0, false, 0.0).unwrap();
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
    fn every_look_has_its_design_colours() {
        // how far a table colour is from a design colour (rgb)
        let apart = |a: [u8; 4], c: Color| {
            let b = rgba8(c);
            (0..3).map(|i| u32::from(a[i].abs_diff(b[i]))).sum::<u32>()
        };
        for palette in [
            ColourSet::Spectrum,
            ColourSet::Aurora,
            ColourSet::Sunset,
            ColourSet::Ocean,
            ColourSet::Neon,
            ColourSet::Mono,
            ColourSet::TwoColour,
        ] {
            let look = Look {
                palette,
                ..spectrum()
            };
            let [c1, c2] = theme::edge_glow(&look);
            let (_, table) = edge(&look, Mode::Overlay, 1920, 1080, 1.0, false, 0.0).unwrap();
            if palette == ColourSet::Mono {
                continue; // both white
            }
            // c1 leads at the edge, c2 alone further in
            let (e, d) = (table[0], table[50]);
            assert!(
                apart(e, c1) < apart(e, c2),
                "{palette:?} at the edge: {e:?}"
            );
            assert!(apart(d, c2) < 12, "{palette:?} further in: {d:?}");
        }
        // the default look is the design's Spectrum
        let [c1, c2] = theme::edge_glow(&spectrum());
        assert_eq!(rgba8(c1), [155, 123, 255, 107]);
        assert_eq!(rgba8(c2), [255, 107, 94, 66]);
    }

    #[test]
    fn none_and_zero_strength_draw_nothing() {
        let mut look = spectrum();
        look.overlay.style = EdgeStyle::None;
        assert!(edge(&look, Mode::Overlay, 1920, 1080, 1.0, false, 0.0).is_none());
        assert!(edge(&look, Mode::Live, 1920, 1080, 1.0, false, 0.0).is_some());
        let calm = LookTheme::Calm.look(&look);
        assert!(edge(&calm, Mode::Overlay, 1920, 1080, 1.0, false, 0.0).is_none());
        let grey = vec![60u8; 192 * 108 * 4];
        let mut b = grey.clone();
        paint(&mut b, 192, 108, 0.1, &look, Mode::Overlay);
        assert_eq!(b, grey);
    }

    /// Runs the canvas over frames at `ms` after the first: the strips after
    /// each and whether it asked for another frame.
    fn frames(border: &Border, ms: &[u64]) -> Vec<(Vec<(Rectangle, Handle)>, bool)> {
        let bounds = Rectangle::with_size(iced::Size::new(1920.0, 1080.0));
        let mut state = State::default();
        let start = Instant::now();
        (ms.iter())
            .map(|&ms| {
                let at = start + Duration::from_millis(ms);
                let event = canvas::Event::Window(window::Event::RedrawRequested(at));
                let action = <Border as canvas::Program<()>>::update(
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

    fn border(mode: Mode, look: Look, breathing: Breathe) -> Border {
        Border {
            mode,
            scale: 1.0,
            keyed: false,
            breathing,
            look: Some(look),
        }
    }

    /// The owner's rule: no moving colours, no draw-back, no travel. Everywhere
    /// but the overlay after its opening, frames 0.2 s and 60 s after the
    /// first hold the very same images, and none asks for another frame.
    #[test]
    fn the_edge_is_still_but_for_the_overlays_breath() {
        for look in PRESETS.iter().map(|(_, l)| *l) {
            for mode in [Mode::Overlay, Mode::Live, Mode::Snip] {
                // the opening (not breathing yet), live, snip, reduced motion
                let mut cases = vec![(border(mode, look, Breathe::Off), false)];
                cases.push((border(mode, look, Breathe::On), mode == Mode::Overlay));
                for (case, breathes) in cases {
                    let reduced = [false, true];
                    for reduced in reduced {
                        super::super::motion::set_reduced(reduced);
                        let seen = frames(&case, &[0, 16, 200, 2000, 60_000]);
                        let moves = breathes && !reduced;
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
        super::super::motion::set_reduced(false);
    }

    /// The overlay's breath is the design's `ripplePulse`: 4 s, the blurs
    /// 22/54 px -> 38/85 px at 2 s -> back, in steps of `STEP`; only the edge's
    /// strips (never the screen), and it asks for the next step, not every frame.
    #[test]
    fn the_overlay_breathes_slowly_in_its_strips_only() {
        super::super::motion::set_reduced(false);
        let look = spectrum();
        let b = border(Mode::Overlay, look, Breathe::On);
        let seen = frames(&b, &[0, 1000, 2000, 3000, 4000, 4010]);
        let alpha_at = |p: f32, d: usize| {
            edge(&look, Mode::Overlay, 1920, 1080, 1.0, false, p)
                .unwrap()
                .1[d][3]
        };
        // peak: deeper than at rest, the same at 4 s as at 0
        assert!(alpha_at(1.0, 40) > alpha_at(0.0, 40) + 2);
        assert!(seen[2].0 != seen[0].0 && seen[1].0 != seen[0].0);
        assert!(seen.iter().all(|f| f.1), "asks for the next step");
        assert!(seen[5].0 == seen[4].0, "10 ms later: the same step");
        assert!((breath(0) - 0.0).abs() < 1e-6 && (breath(25) - 1.0).abs() < 1e-5);
        assert!((breath(50) - 0.0).abs() < 1e-5);
        // never more than the four strips, all inside the edge's band
        let mut state = State::default();
        let bounds = Rectangle::with_size(iced::Size::new(1920.0, 1080.0));
        let event = canvas::Event::Window(window::Event::RedrawRequested(Instant::now()));
        let _ = <Border as canvas::Program<()>>::update(
            &b,
            &mut state,
            &event,
            bounds,
            Cursor::Unavailable,
        );
        assert_eq!(state.strips.len(), 4);
        let area: f32 = state.strips.iter().map(|(r, _)| r.width * r.height).sum();
        assert!(area < 0.3 * 1920.0 * 1080.0, "{area}");
        // reduced motion: still
        super::super::motion::set_reduced(true);
        let seen = frames(&b, &[0, 2000]);
        assert!(seen[0].0 == seen[1].0 && !seen[1].1);
        super::super::motion::set_reduced(false);
    }

    #[test]
    fn geometry_follows_the_monitor_size_and_scale() {
        for (w, h, scale, t, r) in [
            (1920, 1080, 1.0, 102, 14),
            (2560, 1440, 1.25, 128, 18),
            (3840, 2160, 2.0, 204, 28),
        ] {
            let (geo, table) = edge(&spectrum(), Mode::Overlay, w, h, scale, false, 0.0).unwrap();
            assert_eq!((geo.t, geo.r), (t, r), "{w}x{h} @ {scale}");
            assert_eq!(table.len(), t as usize + 1);
            let strips = geo.strips();
            assert_eq!(strips.len(), 4);
            for [x, y, sw, sh] in &strips {
                assert!(x + sw <= w && y + sh <= h, "inside the screen");
            }
            // every lit pixel is in a strip, and the middle is clear
            let img = full(geo, &table);
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
        // too small for an edge: nothing (no zero-sized images)
        let (small, _) = edge(&spectrum(), Mode::Overlay, 30, 30, 1.0, false, 0.0).unwrap();
        assert!(small.strips().is_empty());
    }

    /// The live layer (colour-keyed on Windows) shows a thin solid line at the
    /// edge's thickness: every pixel clear or solid, none near the key.
    #[test]
    fn the_keyed_edge_is_a_thin_solid_line_away_from_the_key() {
        let key = crate::live::COLOUR_KEY;
        let mut magenta = spectrum();
        magenta.palette = ColourSet::TwoColour;
        magenta.colours = [config::Rgb(key), config::Rgb([250, 10, 240])];
        let looks = PRESETS.iter().map(|(_, l)| *l).chain([magenta]);
        for look in looks.filter(|l| l.glow > 0) {
            let (geo, table) = edge(&look, Mode::Live, 1366, 768, 1.25, true, 0.0).unwrap();
            let want = (f32::from(look.live.thickness) * 1.25).round() as u32;
            assert_eq!(geo.t, want);
            let img = full(geo, &table);
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

    /// The default look's edge at 1080p, pixel for pixel
    /// (`tests/fixtures/edge-default-1080p.png`, clear in the middle). A change
    /// to the edge on purpose: `OPENCIRCLE_UPDATE_GOLDEN=1 cargo test border::`.
    #[test]
    fn the_default_edge_at_1080p_matches_its_golden_image() {
        let (geo, table) = edge(&spectrum(), Mode::Overlay, 1920, 1080, 1.0, false, 0.0).unwrap();
        let img = full(geo, &table);
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/edge-default-1080p.png"
        );
        if std::env::var_os("OPENCIRCLE_UPDATE_GOLDEN").is_some() {
            crate::actions::write_png(std::path::Path::new(path), 1920, 1080, &img).unwrap();
        }
        let png = png::Decoder::new(std::io::Cursor::new(std::fs::read(path).unwrap()));
        let mut reader = png.read_info().unwrap();
        let mut want = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut want).unwrap();
        assert_eq!((info.width, info.height), (1920, 1080));
        assert!(
            img == want[..img.len()],
            "the edge differs from its golden image"
        );
    }
}
