//! The screen-edge border (DESIGN.md "Feel"): a thin violet → cyan frame around
//! the shown monitor, solid out to the screen edge, with rounded inner corners.
//! Circle to Search: one shimmer sweep once the first frame is up, then static.
//! Snip: static. Live translate: a soft light circles the edge, for as long as
//! the layer is up.
//!
//! It is eight strip images (four corners, four edges) rendered here pixel by
//! pixel. A frame of the animation swaps only the strips whose pixels changed,
//! and the renderer redraws only those (the light touches two or three), and on
//! the colour-keyed live layer (Windows) every pixel is either clear or solid,
//! never a blend toward the key.
use std::f32::consts::{PI, TAU};
use std::time::{Duration, Instant};

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Action, Geometry};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Color, Element, Fill, Rectangle, Renderer, Theme, window};

use super::theme;

/// Line width and inner corner radius, logical px.
const WIDTH: f32 = 2.5;
const RADIUS: f32 = 14.0;
/// Animation on or off (also off when the OS asks for less motion, Windows).
// ponytail: Linux has no reduce-motion read (GNOME's enable-animations needs
// gsettings / D-Bus); read it when a Linux user asks, or flip this.
const ANIMATE: bool = true;
/// The overlay's sweep: its length, frame time and peak brightness.
const SWEEP: Duration = Duration::from_millis(700);
const SWEEP_FRAME: Duration = Duration::from_millis(16);
const SWEEP_GLOW: f32 = 0.85;
/// Live translate: one turn of the light, its frame time (20 fps) and
/// brightness. A light, not a turning gradient: a frame then changes only the
/// strips near it (each changed strip costs the renderer a pass over the
/// window's clip mask, so all eight every frame cost ~4x more CPU).
const FLOW_LOOP: Duration = Duration::from_secs(5);
const FLOW_FRAME: Duration = Duration::from_millis(50);
const FLOW_GLOW: f32 = 0.45;
/// The light's width (fraction of the perimeter, Gaussian).
const GLOW_WIDTH: f32 = 0.035;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Circle to Search: one shimmer sweep when the overlay opens.
    Sweep,
    /// Snip.
    Static,
    /// Live translate: a light circles the edge.
    Flow,
}

/// The border over a window that shows a whole monitor at `scale` (physical px
/// per logical px). `keyed`: the window's background is a colour key (no
/// partial alpha). Its state (when it started) lives as long as the widget.
pub fn view<'a, M: 'a>(mode: Mode, scale: f32, keyed: bool) -> Element<'a, M> {
    canvas::Canvas::new(Border { mode, scale, keyed })
        .width(Fill)
        .height(Fill)
        .into()
}

/// Puts a fixed colour where the border is, in a capture (`rgba`, `w` x `h`) of
/// the monitor it frames: a capture that shows the layer (X11) then never
/// changes there while the light moves, so live mode doesn't re-read the edges.
pub fn mask(rgba: &mut [u8], w: u32, h: u32, scale: f32) {
    let geo = Geo::new(w, h, scale);
    for [x0, y0, sw, sh] in geo.strips() {
        for y in y0..y0 + sh {
            for x in x0..x0 + sw {
                let i = ((y * w + x) * 4) as usize;
                if geo.coverage(x, y) > 0.0
                    && let Some(px) = rgba.get_mut(i..i + 4)
                {
                    px.copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
    }
}

struct Border {
    mode: Mode,
    scale: f32,
    keyed: bool,
}

#[derive(Default)]
pub struct State {
    /// The first frame's time, and whether the OS allows motion (read then).
    start: Option<Instant>,
    motion: bool,
    /// What `strips` show.
    drawn: Option<(Geo, Look)>,
    /// Logical bounds and pixels of each strip.
    strips: Vec<(Rectangle, Handle)>,
}

impl<M> canvas::Program<M> for Border {
    type State = State;

    /// On each frame: the look for now, re-rendered only if it changed (and a
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
        let elapsed = now.saturating_duration_since(state.start?);
        let (look, next) = look(self.mode, elapsed, state.motion);
        let px = |v: f32| (v * self.scale).round() as u32;
        let geo = Geo::new(px(bounds.width), px(bounds.height), self.scale);
        if state.drawn != Some((geo, look)) {
            let same_geo = state.drawn.is_some_and(|(g, _)| g == geo);
            let old = std::mem::take(&mut state.strips);
            state.drawn = Some((geo, look));
            state.strips = (render(geo, look, self.keyed).into_iter().enumerate())
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
        }
        next.map(|d| Action::request_redraw_at(*now + d))
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

/// What the border shows at one moment: the gradient, plus a light at `head`
/// along the edge (0..1 clockwise from the top-left corner) of strength `glow`
/// (0 = none). `mirrored`: a second light at `1 - head`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Look {
    head: f32,
    glow: f32,
    mirrored: bool,
}

/// The look `elapsed` after the first frame, and the time to the next frame
/// (`None`: static from now on).
fn look(mode: Mode, elapsed: Duration, motion: bool) -> (Look, Option<Duration>) {
    match mode {
        _ if !ANIMATE || !motion => (Look::default(), None),
        // Two lights leave the top-left corner both ways (eased out) and meet
        // at the bottom-right, fading in and out.
        Mode::Sweep if elapsed < SWEEP => {
            let t = elapsed.as_secs_f32() / SWEEP.as_secs_f32();
            let look = Look {
                head: 0.5 * (1.0 - (1.0 - t).powi(2)),
                glow: SWEEP_GLOW * (PI * t).sin(),
                mirrored: true,
            };
            (look, Some(SWEEP_FRAME))
        }
        Mode::Flow => {
            let turn = elapsed.as_millis() % FLOW_LOOP.as_millis();
            let look = Look {
                head: turn as f32 / FLOW_LOOP.as_millis() as f32,
                glow: FLOW_GLOW,
                mirrored: false,
            };
            (look, Some(FLOW_FRAME))
        }
        Mode::Sweep | Mode::Static => (Look::default(), None),
    }
}

/// The border's shape on one monitor, physical px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geo {
    w: u32,
    h: u32,
    /// Line width and inner corner radius.
    t: u32,
    r: u32,
}

impl Geo {
    fn new(w: u32, h: u32, scale: f32) -> Geo {
        let px = |v: f32| (v * scale).round() as u32;
        Geo {
            w,
            h,
            t: px(WIDTH).max(1),
            r: px(RADIUS),
        }
    }

    /// The strips (x, y, w, h): the four rounded corners (t + r square) and
    /// the four edges between them, one line wide, clockwise from the top-left.
    /// None on a screen too small for a frame.
    fn strips(self) -> Vec<[u32; 4]> {
        let (w, h, t, band) = (self.w, self.h, self.t, self.t + self.r);
        if w <= 2 * band || h <= 2 * band {
            return Vec::new();
        }
        let (across, down) = (w - 2 * band, h - 2 * band);
        vec![
            [0, 0, band, band],
            [band, 0, across, t],
            [w - band, 0, band, band],
            [w - t, band, t, down],
            [w - band, h - band, band, band],
            [band, h - t, across, t],
            [0, h - band, band, band],
            [0, band, t, down],
        ]
    }

    /// How much of pixel (x, y) the border covers (0..1): the part outside the
    /// inner rounded rectangle (signed distance at the pixel's centre).
    fn coverage(self, x: u32, y: u32) -> f32 {
        let (cx, cy) = (self.w as f32 / 2.0, self.h as f32 / 2.0);
        let (t, r) = (self.t as f32, self.r as f32);
        let qx = (x as f32 + 0.5 - cx).abs() - (cx - t) + r;
        let qy = (y as f32 + 0.5 - cy).abs() - (cy - t) + r;
        let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
        (outside + 0.5).clamp(0.0, 1.0)
    }

    /// Where pixel (x, y) is along the edge nearest to it, clockwise from the
    /// top-left corner (0..1).
    fn along(self, x: u32, y: u32) -> f32 {
        let (w, h) = (self.w as f32, self.h as f32);
        let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
        let nearest = [y, w - x, h - y, x];
        let edge = (0..4).min_by(|&a, &b| nearest[a].total_cmp(&nearest[b]));
        let s = match edge {
            Some(0) => x,
            Some(1) => w + y,
            Some(2) => w + h + (w - x),
            _ => 2.0 * w + h + (h - y),
        };
        s / (2.0 * (w + h))
    }
}

/// The colour at `u` along the edge: violet at the top-left, cyan at the
/// bottom-right, lit toward `theme::SHIMMER` near the look's light(s).
fn colour(u: f32, look: Look) -> Color {
    let k = 0.5 - 0.5 * (TAU * u).cos();
    let base = mix(theme::STROKE_START, theme::STROKE_END, k);
    if look.glow <= 0.0 {
        return base;
    }
    let to = |head: f32| {
        let d = (u - head).rem_euclid(1.0);
        d.min(1.0 - d)
    };
    let mut d = to(look.head);
    if look.mirrored {
        d = d.min(to(1.0 - look.head));
    }
    let light = (-(d / GLOW_WIDTH).powi(2)).exp();
    mix(base, theme::SHIMMER, look.glow * light)
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    let m = |a: f32, b: f32| a + (b - a) * k;
    Color::from_rgb(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// Each strip's rectangle and pixels (RGBA, straight alpha). `keyed`: every
/// pixel fully clear or fully solid.
fn render(geo: Geo, look: Look, keyed: bool) -> Vec<([u32; 4], Vec<u8>)> {
    (geo.strips().into_iter())
        .map(|[x0, y0, w, h]| {
            let mut rgba = vec![0; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let (gx, gy) = (x0 + x, y0 + y);
                    let a = match geo.coverage(gx, gy) {
                        a if keyed => f32::from(u8::from(a >= 0.5)),
                        a => a,
                    };
                    if a > 0.0 {
                        let c = colour(geo.along(gx, gy), look);
                        let i = ((y * w + x) * 4) as usize;
                        rgba[i..i + 4].copy_from_slice(&Color { a, ..c }.into_rgba8());
                    }
                }
            }
            ([x0, y0, w, h], rgba)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_follows_the_monitor_size_and_scale() {
        for (w, h, scale, t, r) in [
            (1920, 1080, 1.0, 3, 14),
            (2560, 1440, 1.25, 3, 18),
            (3440, 1440, 1.5, 4, 21),
            (3840, 2160, 2.0, 5, 28),
        ] {
            let geo = Geo::new(w, h, scale);
            assert_eq!((geo.t, geo.r), (t, r), "{w}x{h} @ {scale}");
            let strips = geo.strips();
            assert_eq!(strips.len(), 8);
            for [x, y, sw, sh] in &strips {
                assert!(x + sw <= w && y + sh <= h, "inside the screen");
            }
            // no overlap: corners + edges add up exactly
            let area: u32 = strips.iter().map(|[_, _, sw, sh]| sw * sh).sum();
            let band = t + r;
            let edges = 2 * t * (w - 2 * band) + 2 * t * (h - 2 * band);
            assert_eq!(area, 4 * band * band + edges);
        }
        // too small for a frame: nothing (no zero-sized images)
        assert!(Geo::new(30, 30, 1.0).strips().is_empty());
        assert!(Geo::new(1, 1, 2.0).strips().is_empty());
    }

    #[test]
    fn solid_to_the_edge_clear_inside_rounded_inner_corners() {
        let geo = Geo::new(1920, 1080, 1.0);
        let t = geo.t;
        for (x, y) in [(0, 0), (960, 0), (960, t - 1), (1919, 540), (960, 1079)] {
            assert_eq!(geo.coverage(x, y), 1.0, "({x}, {y})");
        }
        for (x, y) in [(960, t), (t, 540), (1919 - t, 540), (960, 540)] {
            assert_eq!(geo.coverage(x, y), 0.0, "({x}, {y})");
        }
        // the inner corner is round: its diagonal is filled past the line width
        let d = t + geo.r / 4;
        assert_eq!(geo.coverage(d, d), 1.0);
        assert_eq!(geo.coverage(1919 - d, 1079 - d), 1.0);
        assert_eq!(geo.coverage(t + geo.r, t + geo.r), 0.0);
    }

    #[test]
    fn the_strips_hold_the_whole_border_and_a_flow_frame_changes_few() {
        let (w, h) = (800, 600);
        let geo = Geo::new(w, h, 1.5);
        let strips = geo.strips();
        let inside = |x, y| {
            (strips.iter())
                .any(|[sx, sy, sw, sh]| (*sx..sx + sw).contains(&x) && (*sy..sy + sh).contains(&y))
        };
        for y in 0..h {
            for x in 0..w {
                if geo.coverage(x, y) > 0.0 {
                    assert!(inside(x, y), "border pixel ({x}, {y}) outside the strips");
                }
            }
        }
        // all strips together, at 1920x1080: under 1% of the screen
        let geo = Geo::new(1920, 1080, 1.0);
        let area: u32 = geo.strips().iter().map(|[_, _, sw, sh]| sw * sh).sum();
        assert!(area * 100 < 1920 * 1080, "{area}");

        // a live frame (50 ms on) redraws only the strips near the light
        let ms = Duration::from_millis;
        for at in (0..5000).step_by(250) {
            let a = render(geo, look(Mode::Flow, ms(at), true).0, true);
            let b = render(geo, look(Mode::Flow, ms(at) + FLOW_FRAME, true).0, true);
            let changed = a.iter().zip(&b).filter(|(a, b)| a != b).count();
            assert!((1..=3).contains(&changed), "{changed} strips at {at} ms");
        }
    }

    #[test]
    fn sweep_runs_once_flow_loops_static_stays() {
        let ms = Duration::from_millis;
        let (first, next) = look(Mode::Sweep, ms(0), true);
        assert_eq!(first.glow, 0.0, "the first frame shows no shimmer yet");
        assert_eq!(next, Some(SWEEP_FRAME));
        let (mid, _) = look(Mode::Sweep, ms(350), true);
        assert!((mid.glow - SWEEP_GLOW).abs() < 1e-3 && mid.mirrored);
        assert!((mid.head - 0.375).abs() < 1e-3, "eased: past halfway");
        assert_eq!(look(Mode::Sweep, SWEEP, true), (Look::default(), None));
        assert_eq!(look(Mode::Sweep, ms(5000), true), (Look::default(), None));

        assert_eq!(look(Mode::Flow, ms(0), true).0.head, 0.0);
        assert_eq!(look(Mode::Flow, ms(2500), true).0.head, 0.5);
        assert_eq!(look(Mode::Flow, FLOW_LOOP + ms(1250), true).0.head, 0.25);
        let hours = look(Mode::Flow, Duration::from_secs(36_000) + ms(500), true);
        let light = Look {
            head: 0.1,
            glow: FLOW_GLOW,
            mirrored: false,
        };
        assert_eq!(hours, (light, Some(FLOW_FRAME)));

        assert_eq!(look(Mode::Static, ms(0), true), (Look::default(), None));
        // reduce motion: everything static, no frames asked for
        for mode in [Mode::Sweep, Mode::Flow] {
            assert_eq!(look(mode, ms(300), false), (Look::default(), None));
        }
    }

    #[test]
    fn the_gradient_runs_violet_to_cyan_and_the_lights_brighten_it() {
        let near =
            |a: Color, b: Color| (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs() < 0.01;
        let still = Look::default();
        assert!(near(colour(0.0, still), theme::STROKE_START));
        assert!(near(colour(0.5, still), theme::STROKE_END));
        let lum = |c: Color| c.r + c.g + c.b;
        // mid-sweep: both lights, nothing away from them
        let sweep = look(Mode::Sweep, Duration::from_millis(350), true).0;
        assert!(lum(colour(0.375, sweep)) > lum(colour(0.375, still)) + 0.3);
        assert!(lum(colour(0.625, sweep)) > lum(colour(0.625, still)) + 0.3);
        assert!(near(colour(0.0, sweep), colour(0.0, still)));
        // the live light wraps around the top-left corner
        let flow = Look {
            head: 0.99,
            glow: FLOW_GLOW,
            mirrored: false,
        };
        assert!(lum(colour(0.01, flow)) > lum(colour(0.01, still)) + 0.2);
        assert!(near(colour(0.5, flow), colour(0.5, still)));
    }

    #[test]
    fn keyed_border_has_no_key_pixels_and_no_fringe() {
        let key = crate::live::COLOUR_KEY;
        let geo = Geo::new(1366, 768, 1.25); // odd scale: the corner is anti-aliased otherwise
        let sweep = look(Mode::Sweep, Duration::from_millis(350), true).0;
        for look in [Look::default(), sweep] {
            for (_, rgba) in render(geo, look, true) {
                for px in rgba.as_chunks::<4>().0 {
                    // over the key: clear (the key itself) or the border colour, nothing between
                    match px[3] {
                        0 => {}
                        255 => assert!(
                            (px[..3].iter().zip(key))
                                .map(|(a, b)| a.abs_diff(b) as u32)
                                .sum::<u32>()
                                > 200,
                            "{px:?} is close to the key"
                        ),
                        a => panic!("partial alpha {a}: a blend toward the key"),
                    }
                }
            }
        }
        // unkeyed: the corner is anti-aliased
        let soft = render(geo, Look::default(), false);
        assert!(
            soft.iter().any(|(_, rgba)| {
                (rgba.as_chunks::<4>().0.iter()).any(|p| p[3] > 0 && p[3] < 255)
            })
        );
    }

    #[test]
    fn the_mask_hides_the_moving_border_from_capture() {
        let geo = Geo::new(640, 480, 1.0);
        let a = render(geo, Look::default(), false);
        let b = render(geo, look(Mode::Flow, Duration::from_secs(1), true).0, false);
        assert_ne!(a, b, "the light changes the pixels");

        // a capture showing either frame looks the same once masked
        let shot = |strips: &[([u32; 4], Vec<u8>)]| {
            let mut rgba = vec![90u8; 640 * 480 * 4];
            for ([x0, y0, w, _], px) in strips {
                for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
                    if p[3] > 0 {
                        let (x, y) = (x0 + i as u32 % w, y0 + i as u32 / w);
                        let j = ((y * 640 + x) * 4) as usize;
                        rgba[j..j + 4].copy_from_slice(p);
                    }
                }
            }
            mask(&mut rgba, 640, 480, 1.0);
            rgba
        };
        assert_eq!(shot(&a), shot(&b));
        // and leaves the inside alone
        let mut rgba = vec![90u8; 640 * 480 * 4];
        mask(&mut rgba, 640, 480, 1.0);
        let centre = ((240 * 640 + 320) * 4) as usize;
        assert_eq!(rgba[centre..centre + 4], [90; 4]);
        assert_eq!(rgba[..4], [0, 0, 0, 255]);
    }

    /// `OPENCIRCLE_BORDER_PNG=<dir> cargo test border::tests::snapshots -- --ignored`
    /// writes the border as the canvas draws it (headless tiny-skia, 1920x1080):
    /// two live phases, the keyed layer over the key colour, the overlay's sweep
    /// at 300 ms and the static snip border.
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
        for (name, mode, keyed, ms, background) in [
            ("live-1000ms", Mode::Flow, false, 1000, grey),
            ("live-3500ms", Mode::Flow, false, 3500, grey),
            ("live-keyed-1000ms", Mode::Flow, true, 1000, key),
            ("overlay-sweep-300ms", Mode::Sweep, false, 300, grey),
            ("snip-static", Mode::Static, false, 300, grey),
        ] {
            let mut element: Element<'_, ()> = view(mode, 1.0, keyed);
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
            let path = dir.join(format!("border-{name}.png"));
            crate::actions::write_png(&path, 1920, 1080, &rgba).unwrap();
        }
    }
}
