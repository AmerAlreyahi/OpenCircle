//! The overlay drawn frame after frame as its window draws it: each frame
//! redrawn only where it differs from the one before (the compositor's damage,
//! `iced_tiny_skia::window::compositor::present`). Times every widget
//! animation (`animation_frame_times`, by hand in release) and checks that a
//! moving widget redraws only around itself.

use super::app::tests::{drag, open_app, settle_bar};
use super::app::{App, GLIDE};
use std::time::{Duration, Instant};

use iced::window::{self};
use iced::{Element, Point, Rectangle, Size, Vector};

use crate::actions::{self, lens};
use crate::ocr::{self, TextBlock};

use super::app::message::Message;
use super::app::word;
use crate::ui::kit::motion;
use crate::ui::kit::panel_frame as card;
use crate::ui::panel;
use crate::ui::screens::{onboarding, settings};

use iced::advanced::graphics::{Viewport, damage};
use iced::{Color, Theme};
use iced_runtime::user_interface::{Cache, UserInterface};

/// One frame: how long it took (view, layout, draw, raster), the rectangles
/// redrawn, and the pixels after it (RGBA).
pub struct Drawn {
    pub took: Duration,
    /// Of it: the view built and laid out, and its events (scroll) handled;
    /// then drawing it into layers (`took - layout - raster`); then
    /// rasterizing the damage.
    pub layout: Duration,
    pub raster: Duration,
    pub damage: Vec<Rectangle>,
    pub rgba: Vec<u8>,
    /// When its widgets want the next frame (a canvas still moving).
    pub next: window::RedrawRequest,
}

/// Frames `0..=frames` of `app`, frame `i` at 16 ms * `i` after now; `step`
/// moves the app on before each (frame 0 is drawn whole and not returned).
/// `pixels`: keep each frame's pixels (else `rgba` is empty).
pub fn frames(
    app: &mut App,
    frames: u32,
    pixels: bool,
    step: impl FnMut(&mut App, u32, Instant),
) -> Vec<Drawn> {
    fn view(app: &App) -> Element<'_, Message> {
        app.view(app.overlay)
    }
    let (size, k) = (app.o.screen, app.o.scale);
    let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
    drawn(app, size, k, view, frames, pixels, step, still)
}

#[allow(clippy::too_many_arguments)]
/// `frames` of any window: `view` of `state`, `size` logical px at scale `k`.
pub fn drawn<S, M>(
    state: &mut S,
    size: Size,
    k: f32,
    view: for<'a> fn(&'a S) -> Element<'a, M>,
    frames: u32,
    pixels: bool,
    mut step: impl FnMut(&mut S, u32, Instant),
    mut input: impl FnMut(u32) -> (iced::mouse::Cursor, Vec<iced::Event>),
) -> Vec<Drawn> {
    let (w, h) = (
        (size.width * k).round() as u32,
        (size.height * k).round() as u32,
    );
    let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
    let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
    let mut mask = tiny_skia::Mask::new(w, h).unwrap();
    let viewport = Viewport::with_physical_size(Size::new(w, h), k);
    let (mut cache, mut last) = (Cache::default(), None::<Vec<iced_tiny_skia::Layer>>);
    let start = Instant::now();
    let mut out = Vec::new();
    for i in 0..=frames {
        let now = start + Duration::from_millis(16 * u64::from(i));
        step(state, i, now);
        let began = Instant::now();
        let mut ui = UserInterface::build(view(state), size, cache, &mut renderer);
        let (cursor, mut events) = input(i);
        events.push(iced::Event::Window(window::Event::RedrawRequested(now)));
        let mut clipboard = iced::advanced::clipboard::Null;
        let (state, _) = ui.update(
            &events,
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut Vec::new(),
        );
        let next = match state {
            iced_runtime::user_interface::State::Updated { redraw_request, .. } => redraw_request,
            iced_runtime::user_interface::State::Outdated => window::RedrawRequest::NextFrame,
        };
        let layout = began.elapsed();
        let style = iced::advanced::renderer::Style {
            text_color: Color::WHITE,
        };
        ui.draw(&mut renderer, &Theme::Dark, &style, cursor);
        cache = ui.into_cache();
        let layers = renderer.layers().to_vec();
        let all = Rectangle::with_size(size);
        let changed = match &last {
            Some(last) => iced_tiny_skia::Layer::frame_damage(last, &layers),
            None => vec![all],
        };
        let changed = damage::group(changed, all);
        let rastering = Instant::now();
        if !changed.is_empty() {
            renderer.draw(
                &mut pixmap.as_mut(),
                &mut mask,
                &viewport,
                &changed,
                Color::BLACK,
            );
        }
        let took = began.elapsed();
        last = Some(layers);
        if i > 0 {
            out.push(Drawn {
                took,
                layout,
                raster: rastering.elapsed(),
                damage: changed,
                next,
                rgba: if pixels {
                    // iced draws B, G, R, A
                    (pixmap.data().as_chunks::<4>().0.iter())
                        .flat_map(|p| [p[2], p[1], p[0], p[3]])
                        .collect()
                } else {
                    Vec::new()
                },
            });
        }
    }
    out
}

/// Total work allowed for the opening's 16 frames in the CI test: their 16
/// beats of the frame clock (`motion::FRAME`).
const BUDGET: Duration = Duration::from_millis(16 * 16);

/// The bar let go near the top on frame 0: it glides there.
fn dock_glide(app: &mut App, i: u32, now: Instant) {
    if i == 0 {
        let _ = app.update(Message::BarGrab);
        let at = app.bar_at();
        let _ = app.update(Message::BarMoved(at));
        let up = Vector::new(0.0, app.o.screen.height * 0.7);
        let _ = app.update(Message::BarMoved(at - up));
        let _ = app.update(Message::BarReleased);
    }
    let _ = app.update(Message::Frame(now));
}

/// An open overlay with a result card, open and still.
fn card_app(w: u32, h: u32) -> App {
    let mut app = open_app(w, h, 1.0);
    let (a, b) = (Point::new(100.0, 100.0), Point::new(400.0, 300.0));
    drag(&mut app, a, b, false);
    let _ = app.update(Message::Run(&lens::ACTION));
    settle_bar(&mut app);
    app
}

/// Frame times of every widget animation at 1920x1080 and 3440x1440 (run in
/// release: `cargo test --release animation_frame_times -- --ignored
/// --nocapture`).
#[test]
#[ignore = "timing: run by hand, in release"]
fn animation_frame_times() {
    let report = |name: &str, w: u32, h: u32, t: Vec<Drawn>| {
        let ms: Vec<f64> = t.iter().map(|d| d.took.as_secs_f64() * 1e3).collect();
        let mean = ms.iter().sum::<f64>() / ms.len() as f64;
        let worst = ms.iter().cloned().fold(0.0, f64::max);
        let area: f32 = t
            .iter()
            .flat_map(|d| d.damage.iter().map(|r| r.width * r.height))
            .sum::<f32>()
            / (t.len() as f32 * (w * h) as f32);
        let raster = t.iter().map(|d| d.raster.as_secs_f64() * 1e3).sum::<f64>() / ms.len() as f64;
        let rects = t.iter().map(|d| d.damage.len()).sum::<usize>() as f32 / t.len() as f32;
        println!(
            "{name:<15} {w}x{h}: mean {mean:5.1} ms ({:5.1} fps), worst {worst:5.1} ms, \
             raster {raster:5.1} ms, redrawn {:3.0}% of the screen in {rects:.1} rects",
            1e3 / mean,
            area * 100.0
        );
    };
    for (w, h) in [(1920, 1080), (3440, 1440)] {
        // the opening: the bar slides up (the edge glow is still; the window
        // fades in as a whole: the OS's work)
        let mut app = open_app(w, h, 1.0);
        app.o.appear = None;
        let t = frames(&mut app, 16, false, |app, _, now| {
            let _ = app.update(Message::Frame(now));
        });
        report("appear", w, h, t);
        let mut app = open_app(w, h, 1.0);
        settle_bar(&mut app);
        report(
            "bar dock glide",
            w,
            h,
            frames(&mut app, 16, false, dock_glide),
        );
        // the bar held and dragged up
        let mut app = open_app(w, h, 1.0);
        settle_bar(&mut app);
        let t = frames(&mut app, 16, false, |app, i, _| {
            if i == 0 {
                let _ = app.update(Message::BarGrab);
                let at = app.bar_at();
                let _ = app.update(Message::BarMoved(at));
            }
            let at = Point::new(app.bar_at().x + 10.0, h as f32 * 0.8 - 30.0 * i as f32);
            let _ = app.update(Message::BarMoved(at));
        });
        report("bar drag", w, h, t);
        // Edit: the bar turns into the editing toolbar
        let mut app = open_app(w, h, 1.0);
        let (a, b) = (Point::new(200.0, 150.0), Point::new(700.0, 500.0));
        drag(&mut app, a, b, false);
        settle_bar(&mut app);
        let t = frames(&mut app, 16, false, |app, i, now| {
            if i == 0 {
                let _ = app.update(Message::Run(&actions::edit::ACTION));
            }
            let _ = app.update(Message::Frame(now));
        });
        report("bar->editor", w, h, t);
        // a result card grows open
        let mut app = open_app(w, h, 1.0);
        let (a, b) = (Point::new(100.0, 100.0), Point::new(400.0, 300.0));
        drag(&mut app, a, b, false);
        settle_bar(&mut app);
        let t = frames(&mut app, 16, false, |app, i, now| {
            if i == 0 {
                let _ = app.update(Message::Run(&lens::ACTION));
            }
            let _ = app.update(Message::Frame(now));
        });
        report("card open", w, h, t);
        // the result card dragged by its grip, and resized by its inner edge
        for (name, how) in [
            ("card drag", card::Drag::Move),
            ("card resize", card::Drag::Resize),
        ] {
            let mut app = card_app(w, h);
            let id = app.o.panel.as_ref().expect("a card").0;
            let t = frames(&mut app, 16, false, |app, i, _| {
                if i == 0 {
                    let grab = card::Event::Grab(how);
                    let _ = app.update(Message::Panel(id, panel::Msg::Card(grab)));
                    let _ = app.update(Message::CardMoved(Point::new(1100.0, 200.0)));
                }
                let p = Point::new(1100.0 - 20.0 * i as f32, 200.0 + 5.0 * i as f32);
                let _ = app.update(Message::CardMoved(p));
            });
            report(name, w, h, t);
        }
    }
}

/// While the bar glides over the frozen frame (dim, selection, edge glow),
/// each frame redraws only round the bar's old and new place, and what it
/// draws is exactly the whole frame drawn afresh (no trail, no black box).
/// The glide follows the clock: eased out, there at `GLIDE`.
#[test]
fn a_gliding_bar_redraws_only_round_itself() {
    let mut app = open_app(1280, 720, 1.0);
    let (a, b) = (Point::new(1000.0, 300.0), Point::new(1200.0, 500.0));
    drag(&mut app, a, b, false); // a selection: the dim has a hole
    settle_bar(&mut app);
    let mut boxes = Vec::new();
    let drawn = frames(&mut app, 16, true, |app, i, now| {
        dock_glide(app, i, now);
        boxes.push(Rectangle::new(app.bar_at(), app.dock_size()));
    });
    let ys: Vec<f32> = boxes.iter().map(|r| r.y).collect();
    assert!(ys.windows(2).all(|w| w[1] <= w[0]), "one way: {ys:?}");
    // 16 ms frames: eased out (the first steps longest), there by GLIDE
    let steps: Vec<f32> = ys.windows(2).map(|w| w[0] - w[1]).collect();
    assert!(steps[1] > steps[4] && steps[4] > steps[8], "{steps:?}");
    let there = (GLIDE.as_millis() / 16) as usize + 1;
    assert_eq!(boxes[there].y, boxes[16].y, "{ys:?}");
    assert!(boxes[there - 2].y > boxes[16].y, "{ys:?}");
    for (i, frame) in drawn.iter().enumerate() {
        // frame i + 1 went from boxes[i] to boxes[i + 1]
        let around = boxes[i].union(&boxes[i + 1]).expand(12.0);
        for r in &frame.damage {
            assert!(
                r.x >= around.x - 0.5
                    && r.y >= around.y - 0.5
                    && r.x + r.width <= around.x + around.width + 0.5
                    && r.y + r.height <= around.y + around.height + 0.5,
                "frame {}: redrew {r:?} outside {around:?}",
                i + 1
            );
        }
    }
    let last = drawn.last().unwrap();
    // the same frame drawn whole (a fresh window: no damage yet)
    let whole = frames(&mut app, 1, true, |_, _, _| {}).remove(0).rgba;
    let off = (last.rgba.iter().zip(&whole)).filter(|(a, b)| a.abs_diff(**b) > 2);
    assert_eq!(off.count(), 0, "the redrawn frame is the frame");
}

/// The frozen frame as the overlay draws it (`overlay::Frozen`, blitted with
/// nothing cleared under it) is pixel for pixel the frame, at 125 and 150 %
/// too.
#[test]
fn the_frozen_frame_is_the_frame() {
    use crate::ui::overlay::Frozen;
    use iced::widget::{canvas, image};
    struct S(image::Handle, f32, bool);
    fn view(s: &S) -> Element<'_, ()> {
        match s.2 {
            true => canvas(Frozen {
                frame: &s.0,
                scale: s.1,
            })
            .width(iced::Fill)
            .height(iced::Fill)
            .into(),
            false => image(s.0.clone())
                .width(iced::Fill)
                .height(iced::Fill)
                .content_fit(iced::ContentFit::Fill)
                .filter_method(image::FilterMethod::Nearest)
                .into(),
        }
    }
    for scale in [1.0, 1.25, 1.5] {
        let (w, h) = (1700, 900);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i * 7) as u8, (i % w) as u8, (i / w) as u8, 255])
            .collect();
        let frame = image::Handle::from_rgba(w, h, rgba);
        let size = Size::new(w as f32 / scale, h as f32 / scale);
        let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
        let draw = |frozen| {
            let mut s = S(frame.clone(), scale, frozen);
            drawn(&mut s, size, scale, view, 1, true, |_, _, _| {}, still)
                .remove(0)
                .rgba
        };
        let (frozen, whole) = (draw(true), draw(false));
        let off = (frozen.iter().zip(&whole)).filter(|(a, b)| a != b).count();
        assert_eq!(off, 0, "at {scale}");
    }
}

/// The opening redraws only what moves: the bar rising and the hint coming
/// down. Never the frame under the dim (the window fades in as a whole,
/// `windows::fade`), never the screen edge's static glow after its first frame
/// (the left and right bands stay untouched), and never a layer's whole bounds
/// when a widget gets or leaves a layer of its own (the hint while it slides).
/// At 1080p at most 10 % of the screen per frame; and what it draws is
/// exactly the frame drawn afresh.
#[test]
fn the_opening_redraws_only_what_moves() {
    let (w, h) = (1920, 1080);
    let mut app = open_app(w, h, 1.0);
    app.o.appear = None;
    let drawn = frames(&mut app, 16, true, |app, _, now| {
        let _ = app.update(Message::Frame(now));
    });
    // the edge's left and right bands (no widget moves there)
    let sides = [
        Rectangle::new(Point::new(0.0, 0.0), Size::new(80.0, h as f32)),
        Rectangle::new(Point::new(w as f32 - 80.0, 0.0), Size::new(80.0, h as f32)),
    ];
    // above the bar, below the hint
    let middle = Rectangle::new(Point::new(250.0, 250.0), Size::new(1420.0, 550.0));
    for (i, frame) in drawn.iter().enumerate() {
        let area: f32 = frame.damage.iter().map(|r| r.width * r.height).sum();
        assert!(
            area <= 0.1 * (w * h) as f32,
            "frame {}: redrew {:.0} % of the screen: {:?}",
            i + 1,
            100.0 * area / (w * h) as f32,
            frame.damage
        );
        assert!(
            frame
                .damage
                .iter()
                .all(|r| r.intersection(&middle).is_none()),
            "frame {}: redrew the still middle: {:?}",
            i + 1,
            frame.damage
        );
        assert!(
            frame
                .damage
                .iter()
                .all(|r| sides.iter().all(|s| r.intersection(s).is_none())),
            "frame {}: redrew the edge glow: {:?}",
            i + 1,
            frame.damage
        );
    }
    assert!(!app.appearing(), "over within the frames");
    let last = drawn.last().unwrap();
    let whole = frames(&mut app, 1, true, |_, _, _| {}).remove(0).rgba;
    let off = (last.rgba.iter().zip(&whole)).filter(|(a, b)| a.abs_diff(**b) > 2);
    assert_eq!(off.count(), 0, "the redrawn frame is the frame");
}

/// The CI budget (#59) for the opening at 1080p: not a wall-clock p95 (noisy
/// on shared runners) but the least total work over three runs of its 16
/// frames, and the mean share of the screen redrawn. Windows OCR's two reads
/// land during it, as they do from the capture (#62: the quick read about
/// 0.1 s in, the closer one at the end), and their handling on the UI thread
/// counts too (#134). Fails if the opening redraws more than 10 % of the
/// screen per frame (3 % now that the edge glow is drawn once; 21 % before,
/// when it drew back; 42 % before #134, about 100 % before #23), or if its
/// work grows past its 16 beats of 16 ms (release; see the printed numbers).
/// Run: `cargo test --release the_opening_stays -- --nocapture`.
#[test]
#[cfg_attr(debug_assertions, ignore = "timing: CI runs it in release")]
fn the_opening_stays_within_its_frame_budget() {
    let (w, h) = (1920, 1080);
    let whole = Rectangle::with_size(Size::new(w as f32, h as f32));
    // a page of text: 90 lines of 80 characters
    let page = |words: &str| {
        let blocks = (0..90)
            .map(|i| TextBlock {
                text: format!("{words} {i} ").repeat(8),
                bounds: [40.0, 20.0 + 11.5 * i as f32, 1200.0, 10.0],
                confidence: 1.0,
            })
            .collect();
        word::lines(blocks, [0, 0], 1.0)
    };
    let mut best = Duration::MAX;
    let mut share = 0.0;
    for _ in 0..3 {
        let mut app = open_app(w, h, 1.0);
        app.o.appear = None;
        app.o.text_by = Some(ocr::Source::System);
        let mut read = Duration::ZERO;
        let drawn = frames(&mut app, 16, false, |app, i, now| {
            let _ = app.update(Message::Frame(now));
            if i == 6 || i == 13 {
                let lines = Ok(page(if i == 6 { "quick" } else { "closer" }));
                let t = Instant::now();
                let _ = app.update(Message::ScreenText(app.opening, whole, lines));
                read += t.elapsed();
            }
        });
        assert_eq!(app.o.screen_text.lines().len(), 90, "both reads landed");
        best = best.min(drawn.iter().map(|d| d.took).sum::<Duration>() + read);
        let area: f32 = (drawn.iter())
            .flat_map(|d| d.damage.iter().map(|r| r.width * r.height))
            .sum();
        share = area / (drawn.len() as f32 * (w * h) as f32);
    }
    println!(
        "opening: {best:?} for 16 frames, {:.0} % redrawn",
        share * 100.0
    );
    assert!(
        share <= 0.1,
        "redrew {:.0} % of the screen per frame",
        share * 100.0
    );
    assert!(
        best <= BUDGET,
        "16 opening frames took {best:?}, over {BUDGET:?}"
    );
}

/// The opening blits the frozen screen's tiles and the edge glow's strips
/// (`iced_tiny_skia`'s `Pipeline::blit`) instead of running tiny-skia's
/// per-pixel pipeline over them, at 100 % and 125 %: under 1 % of the screen
/// a frame goes through the pipeline (a bar icon moving by fractions of a
/// pixel). #134: the tiles, placed a quarter pixel in against float error,
/// all went through it: about 20 ms of each opening frame on the 4-core VM,
/// so its frames came 30-45 ms apart.
#[test]
fn the_opening_blits_its_images() {
    for (w, h, scale) in [(1920, 1080, 1.0), (1700, 900, 1.25)] {
        let mut app = open_app(w, h, scale);
        app.o.appear = None;
        let before = iced_tiny_skia::PIPELINE_PIXELS.get();
        let drawn = frames(&mut app, 16, false, |app, _, now| {
            let _ = app.update(Message::Frame(now));
        });
        let piped = (iced_tiny_skia::PIPELINE_PIXELS.get() - before) as f32;
        let share = piped / ((drawn.len() + 1) as f32 * (w * h) as f32);
        assert!(
            share < 0.01,
            "{:.0} % of the screen a frame through the pipeline at {scale}",
            share * 100.0
        );
    }
}

/// Once the opening, the bar and a card have settled, nothing asks for a
/// frame: no frame messages, no canvas wanting a redraw (the edge glow at
/// rest), and a frame drawn anyway redraws nothing.
#[test]
fn a_still_overlay_asks_for_no_frames() {
    // The edge glow doesn't breathe in 0.1.2 (`edge::breathes`), so motion
    // stays on everywhere: the still overlay must cost nothing.
    motion::set_reduced(false);
    let mut app = open_app(1280, 720, 1.0);
    app.o.appear = None;
    let (a, b) = (Point::new(300.0, 200.0), Point::new(600.0, 400.0));
    let drawn = frames(&mut app, 40, false, |app, i, now| {
        if i == 1 {
            drag(app, a, b, false);
        }
        if i == 2 {
            let _ = app.update(Message::Run(&lens::ACTION));
        }
        let _ = app.update(Message::Frame(now));
    });
    assert!(app.o.panel.is_some(), "a card is up");
    assert!(!app.animating(), "no frame messages");
    let last = drawn.last().unwrap();
    assert_eq!(
        last.next,
        window::RedrawRequest::Wait,
        "no widget wants one"
    );
    assert!(last.damage.is_empty(), "{:?}", last.damage);
}

/// The edge glow's breath (owner, 2026-10-27) redraws only the edge's strips,
/// when its level changes (`edge::LEVELS`), only after the opening: never the middle of the
/// screen, and a few percent of the screen per frame at most. (Windows only:
/// Linux's glow stays still, #142.)
#[test]
#[cfg(windows)]
#[ignore = "the breath is off in 0.1.2 (edge::breathes); it returns with the GPU renderer in 0.1.4"]
fn the_breath_redraws_only_the_edge() {
    let (w, h) = (1920, 1080);
    motion::set_reduced(false);
    let mut app = open_app(w, h, 1.0);
    app.o.appear = None;
    let drawn = frames(&mut app, 80, false, |app, _, now| {
        let _ = app.update(Message::Frame(now));
    });
    // the breath starts at the opening's end (frame 13); before it, no edge work
    let middle = Rectangle::new(Point::new(260.0, 260.0), Size::new(1400.0, 540.0));
    let mut steps = 0;
    for (i, frame) in drawn.iter().enumerate().skip(16) {
        assert!(
            frame
                .damage
                .iter()
                .all(|r| r.intersection(&middle).is_none()),
            "frame {}: redrew the middle: {:?}",
            i + 1,
            frame.damage
        );
        steps += usize::from(!frame.damage.is_empty());
    }
    assert!(
        (4..=8).contains(&steps), // levels 1..=6 of the rise (`edge::LEVELS`)
        "{steps} breath steps in 64 frames"
    );
    let area: f32 = (drawn.iter().skip(16))
        .flat_map(|d| d.damage.iter().map(|r| r.width * r.height))
        .sum();
    let share = area / (64.0 * (w * h) as f32);
    println!(
        "breath: {steps} steps, {:.1} % of the screen a frame",
        share * 100.0
    );
    assert!(share < 0.08, "{:.1} %", share * 100.0);
}

/// The breath's steps timed (run in release:
/// `cargo test --release breath_frame_times -- --ignored --nocapture`).
#[test]
#[cfg(windows)] // (Linux: no breath, #142)
#[ignore = "timing: run by hand, in release"]
fn breath_frame_times() {
    for (w, h, k) in [(1920, 1080, 1.0), (1920, 1080, 1.5), (3840, 2160, 2.5)] {
        motion::set_reduced(false);
        let mut app = open_app(w, h, k);
        app.o.appear = None;
        let drawn = frames(&mut app, 120, false, |app, _, now| {
            let _ = app.update(Message::Frame(now));
        });
        let before = iced_tiny_skia::PIPELINE_PIXELS.get();
        let steps: Vec<Drawn> = (drawn.into_iter().skip(20))
            .filter(|d| !d.damage.is_empty())
            .collect();
        let piped = iced_tiny_skia::PIPELINE_PIXELS.get() - before;
        report("breath step", app.o.screen, k, &steps);
        println!("  {} steps, {piped} pipeline px", steps.len());
    }
}

/// Every open goes by `motion`: the opening's clock, a result card growing
/// from `OPEN_SCALE` in `OPEN` eased out; with the OS's animations off, all
/// of it there at once.
#[test]
fn widgets_open_by_the_design_motion_or_at_once() {
    let opened = |reduced: bool| {
        motion::set_reduced(reduced);
        let mut app = open_app(1280, 720, 1.0);
        app.o.appear = None;
        let (a, b) = (Point::new(300.0, 200.0), Point::new(600.0, 400.0));
        drag(&mut app, a, b, false);
        let _ = app.update(Message::Run(&lens::ACTION));
        let start = Instant::now();
        let steps: Vec<(f32, f32)> = [0, 50, 100, 209, 210]
            .into_iter()
            .map(|ms| {
                let _ = app.update(Message::Frame(start + Duration::from_millis(ms)));
                (app.appeared(), app.card_grown())
            })
            .collect();
        motion::set_reduced(false);
        steps
    };
    let steps = opened(false);
    let ease = |ms: f32| motion::Curve::Out.ease(ms / motion::OPEN.as_millis() as f32);
    for ((appeared, grown), ms) in steps.iter().zip([0.0, 50.0, 100.0, 209.0, 210.0]) {
        assert!((appeared - ease(ms)).abs() < 1e-4, "{steps:?}");
        assert!((grown - ease(ms)).abs() < 1e-4, "{steps:?}");
    }
    assert_eq!(steps[4], (1.0, 1.0));
    assert!(opened(true).iter().all(|&s| s == (1.0, 1.0)), "at once");
}

/// An image that lands pixel for pixel is drawn by the renderer's own copy
/// and blend (`iced_tiny_skia` patch, #23): what tiny-skia's pipeline draws
/// (to rounding), and a redraw of part of it (fractional edges) leaves the
/// frame exactly as it was.
#[test]
fn a_pixel_for_pixel_image_is_drawn_as_tiny_skia_draws_it() {
    use iced::widget::{container, image};
    let (iw, ih) = (64u32, 40u32);
    // every alpha, clear and solid among them
    let rgba: Vec<u8> = (0..iw * ih)
        .flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 3) as u8, (i * 5) as u8])
        .collect();
    let picture = image(image::Handle::from_rgba(iw, ih, rgba.clone()))
        .width(iw as f32)
        .height(ih as f32)
        .filter_method(image::FilterMethod::Nearest);
    let view: Element<'_, Message> = container(picture).padding([7, 10]).into();
    let damage = Rectangle::new(Point::new(20.3, 9.6), Size::new(30.45, 20.5));
    let (drawn, again) = crate::ui::kit::testing::shot::render_twice(view, 100, 60, Some(damage));
    assert_eq!(drawn, again, "a part redrawn is the same");
    // tiny-skia's own: the background, the image drawn over it
    let [r, g, b, a] = crate::ui::kit::themes::c().bg.into_rgba8();
    let mut reference = tiny_skia::Pixmap::new(100, 60).unwrap();
    reference.fill(tiny_skia::Color::from_rgba8(r, g, b, a));
    let mut picture = tiny_skia::Pixmap::new(iw, ih).unwrap();
    for (px, p) in picture.pixels_mut().iter_mut().zip(rgba.as_chunks::<4>().0) {
        let [r, g, b, a] = *p;
        *px = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Nearest,
        ..Default::default()
    };
    let at = tiny_skia::Transform::identity();
    reference.draw_pixmap(10, 7, picture.as_ref(), &paint, at, None);
    let off = (drawn.iter().zip(reference.data())).filter(|(a, b)| a.abs_diff(**b) > 1);
    assert_eq!(off.count(), 0);
}

/// A Settings window as drawn, `size` logical px at scale `k`.
fn settings_view(s: &settings::State) -> Element<'_, settings::Message> {
    settings::view(s, false)
}

fn settings_at(section: settings::Section) -> settings::State {
    crate::ui::kit::text::load(); // the bundled fonts, as the app has them
    let mut s = settings::State::new(
        crate::config::Config::default(),
        vec![],
        None,
        Box::new(crate::secrets::MemoryStore::default()),
    );
    let _ = settings::update(&mut s, settings::Message::Section(section));
    s
}

/// One line per motion: mean / worst frame, where the time goes, how much is redrawn.
fn report(name: &str, size: Size, k: f32, t: &[Drawn]) {
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let n = t.len() as f64;
    let mean = t.iter().map(|d| ms(d.took)).sum::<f64>() / n;
    let worst = t.iter().map(|d| ms(d.took)).fold(0.0, f64::max);
    let mut sorted: Vec<f64> = t.iter().map(|d| ms(d.took)).collect();
    sorted.sort_by(f64::total_cmp);
    let p95 = sorted[((sorted.len() as f64 * 0.95).ceil() as usize).clamp(1, sorted.len()) - 1];
    let layout = t.iter().map(|d| ms(d.layout)).sum::<f64>() / n;
    let raster = t.iter().map(|d| ms(d.raster)).sum::<f64>() / n;
    let px = (size.width * k) * (size.height * k);
    let area = t
        .iter()
        .flat_map(|d| d.damage.iter().map(|r| r.width * k * r.height * k))
        .sum::<f32>()
        / (t.len() as f32 * px);
    println!(
        "{name:<22} {}x{} @{k}: mean {mean:5.1} ms, p95 {p95:5.1}, worst {worst:5.1} ms | layout {layout:4.1}, \
         draw {:4.1}, raster {raster:4.1} ms | redrawn {:3.0}%",
        (size.width * k) as u32,
        (size.height * k) as u32,
        mean - layout - raster,
        area * 100.0
    );
}

/// Settings' page switch and the Appearance page's scroll, timed at the
/// design's size and maximised on a 3840x2160 screen at 200 and 250 % (run in release:
/// `cargo test --release settings_frame_times -- --ignored --nocapture`).
#[test]
#[ignore = "timing: run by hand, in release"]
fn settings_frame_times() {
    use settings::{Message as M, Section};
    // 3840x2160 at 200 % and 250 % (the owner's 4K laptop, 120 Hz: 8.3 ms a
    // frame): the window at its size, and maximised (the work area)
    for (k, size) in [
        (2.0, settings::WINDOW_SIZE),
        (2.0, Size::new(1920.0, 1032.0)),
        (2.5, settings::WINDOW_SIZE),
        (2.5, Size::new(1536.0, 816.0)),
    ] {
        // General -> Appearance on frame 1, then the glide's frames (real time)
        let mut s = settings_at(Section::General);
        std::thread::sleep(Duration::from_millis(300));
        let _ = settings::update(&mut s, M::Tick);
        let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
        let t = drawn(
            &mut s,
            size,
            k,
            settings_view,
            12,
            false,
            |s, i, _| {
                if i == 1 {
                    let _ = settings::update(s, M::Section(Section::Appearance));
                }
                let _ = settings::update(s, M::Tick);
            },
            still,
        );
        report("page switch", size, k, &t);
        // the Appearance page at rest, scrolled 40 px a frame
        let mut s = settings_at(Section::Appearance);
        std::thread::sleep(Duration::from_millis(300));
        let _ = settings::update(&mut s, M::Tick);
        let at = Point::new(size.width * 0.6, size.height * 0.5);
        let wheel = |i: u32| {
            let e = iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -40.0 },
            };
            let moved = iced::mouse::Event::CursorMoved { position: at };
            let events = match i {
                0 => vec![iced::Event::Mouse(moved)],
                _ => vec![iced::Event::Mouse(e)],
            };
            (iced::mouse::Cursor::Available(at), events)
        };
        let t = drawn(
            &mut s,
            size,
            k,
            settings_view,
            12,
            false,
            |_, _, _| {},
            wheel,
        );
        report("appearance scroll", size, k, &t);
    }
}

/// The frames after a Settings page switch: their damage (logical px).
fn switch_damage(to: settings::Section) -> Vec<Vec<Rectangle>> {
    use settings::Message as M;
    let mut s = settings_at(settings::Section::General);
    std::thread::sleep(Duration::from_millis(250));
    let _ = settings::update(&mut s, M::Tick);
    let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
    let t = drawn(
        &mut s,
        settings::WINDOW_SIZE,
        1.0,
        settings_view,
        4,
        false,
        |s, i, _| {
            if i == 1 {
                let _ = settings::update(s, M::Section(to));
            }
            let _ = settings::update(s, M::Tick);
        },
        still,
    );
    // after the switch's own frame (the new page, drawn whole)
    t.into_iter().skip(1).map(|d| d.damage).collect()
}

/// The sidebar's column (its highlight glides there).
const SIDEBAR_RIGHT: f32 = crate::ui::kit::tokens::size::SIDEBAR + 2.0;

/// A page switch moves only the sidebar's highlight: the page itself switches
/// at once (its rise redrew the whole window every frame on the CPU renderer).
#[test]
fn a_settings_page_switch_redraws_only_the_sidebar_glide() {
    let frames = switch_damage(settings::Section::Privacy);
    assert!(frames.iter().any(|d| !d.is_empty()), "the glide redraws");
    for r in frames.iter().flatten() {
        assert!(r.x + r.width <= SIDEBAR_RIGHT, "{r:?} outside the sidebar");
    }
}

/// The preview player's fixture clip (`scripts/make-preview-fixture.py`).
const CLIP: &[u8] = include_bytes!("../../tests/fixtures/previews/tiny.webp");

/// The settings page `section` with its previews playing the fixture clip,
/// a frame of it (300 ms each) further at every drawn frame: their damage.
fn clip_damage(section: settings::Section) -> Vec<Vec<Rectangle>> {
    use settings::Message as M;
    let mut s = settings_at(section);
    std::thread::sleep(Duration::from_millis(250));
    let _ = settings::update(&mut s, M::Tick); // the page's glide is over
    let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
    let clip = CLIP;
    let t = drawn(
        &mut s,
        settings::WINDOW_SIZE,
        1.0,
        settings_view,
        4,
        false,
        |s, i, _| {
            s.play_clip(clip, Duration::from_millis(300 * u64::from(i)));
            let _ = settings::update(s, M::Tick);
        },
        still,
    );
    t.into_iter().skip(1).map(|d| d.damage).collect()
}

/// A playing feature preview redraws only its stage's clip.
#[test]
fn a_playing_preview_redraws_only_its_stage() {
    let frames = clip_damage(settings::Section::Ai);
    // the clip's place: past the sidebar, under the preview's heading and
    // above the page's rows
    let stage = |r: &Rectangle| r.x > SIDEBAR_RIGHT && r.y >= 150.0 && r.y + r.height <= 460.0;
    assert!(frames.iter().all(|d| !d.is_empty()), "the clip plays");
    for r in frames.iter().flatten() {
        assert!(stage(r), "{r:?} outside the stage");
    }
}

/// Replay on a preview resting on its last frame: drawn over it, the first
/// frame leaves nothing of the last on screen (as drawn whole).
#[test]
fn a_replayed_preview_leaves_no_ghost_of_its_last_frame() {
    let size = settings::WINDOW_SIZE;
    let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
    let clip = CLIP;
    let mut s = settings_at(settings::Section::Ai);
    // at rest on the last frame
    s.play_clip(clip, Duration::from_secs(9));
    let _ = settings::update(&mut s, settings::Message::Tick);
    let drawn_on = drawn(
        &mut s,
        size,
        1.0,
        settings_view,
        1,
        true,
        |s, i, _| {
            if i == 1 {
                s.play_clip(clip, Duration::ZERO);
            }
        },
        still,
    );
    let fresh = drawn(
        &mut s,
        size,
        1.0,
        settings_view,
        1,
        true,
        |_, _, _| {},
        still,
    );
    let (a, b) = (&drawn_on.last().unwrap().rgba, &fresh.last().unwrap().rgba);
    let w = size.width as usize;
    let page = SIDEBAR_RIGHT as usize;
    let off = (0..size.height as usize)
        .flat_map(|y| (page..w).map(move |x| (y * w + x) * 4))
        .filter(|&i| (a[i..i + 4].iter().zip(&b[i..i + 4])).any(|(x, y)| x.abs_diff(*y) > 24))
        .count();
    assert_eq!(off, 0, "pixels left over from the last frame");
}

/// The onboarding window as drawn.
fn onboarding_view(s: &onboarding::State) -> Element<'_, onboarding::Message> {
    onboarding::view(s)
}

/// The frames after Next on the first step (drawn in real time).
fn next_frames(k: f32, pixels: bool, frames: u32) -> Vec<Drawn> {
    let mut s = onboarding::State::new(
        crate::config::Config::default(),
        None,
        Box::new(crate::secrets::MemoryStore::default()),
    );
    std::thread::sleep(Duration::from_millis(250));
    let _ = onboarding::update(&mut s, onboarding::Message::Tick);
    let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
    drawn(
        &mut s,
        onboarding::WINDOW_SIZE,
        k,
        onboarding_view,
        frames,
        pixels,
        |s, i, _| {
            if i == 1 {
                let _ = onboarding::update(s, onboarding::Message::Next);
            }
            std::thread::sleep(Duration::from_millis(16)); // the motion runs on the real clock
            let _ = onboarding::update(s, onboarding::Message::Tick);
        },
        still,
    )
}

/// Onboarding's step change at the fixed window size, scale 2 and 2.5 (a 4K
/// screen at 200 / 250 %); run in release:
/// `cargo test --release onboarding_frame_times -- --ignored --nocapture`.
#[test]
#[ignore = "timing: run by hand, in release"]
fn onboarding_frame_times() {
    for k in [2.0, 2.5] {
        let t = next_frames(k, false, 14);
        report("onboarding step", onboarding::WINDOW_SIZE, k, &t);
    }
}

/// A step change moves only the step track's thumb: the page switches at once
/// (its rise redrew the whole window every frame on the CPU renderer).
#[test]
fn an_onboarding_step_change_redraws_only_the_track() {
    let frames: Vec<_> = next_frames(1.0, false, 5)
        .into_iter()
        .skip(1)
        .map(|d| d.damage)
        .collect();
    assert!(frames.iter().any(|d| !d.is_empty()), "the thumb glides");
    // the track sits above the page: below it nothing is redrawn
    for r in frames.iter().flatten() {
        assert!(r.y + r.height <= 120.0, "{r:?} below the track");
    }
}

/// The zoom's drawing (`iced_tiny_skia` patch, #89): the frozen frame scaled
/// and placed at fractions of a pixel (`overlay::Zoomed`), faded over black
/// by the plain dim or not, and the dim's solid rectangles over it, are what
/// tiny-skia's own pipeline draws.
#[test]
fn a_zoomed_frame_and_its_dim_are_drawn_as_tiny_skia_draws_them() {
    use crate::ui::overlay::{View, Zoomed};
    use iced::widget::canvas;
    struct Dim;
    const DIM_AT: Rectangle = Rectangle {
        x: 13.0,
        y: 7.0,
        width: 51.0,
        height: 31.0,
    };
    impl canvas::Program<()> for Dim {
        type State = ();
        fn draw(
            &self,
            _: &(),
            r: &iced::Renderer,
            _: &Theme,
            b: Rectangle,
            _: iced::mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            let mut f = canvas::Frame::new(r, b.size());
            let dim = Color::from_rgba(0.0, 0.0, 0.0, 0.3);
            f.fill_rectangle(DIM_AT.position(), DIM_AT.size(), dim);
            vec![f.into_geometry()]
        }
    }
    struct S(iced::widget::image::Handle, View, f32);
    fn view(s: &S) -> Element<'_, ()> {
        let zoomed = canvas(Zoomed {
            picture: &s.0,
            view: s.1,
            dim: s.2,
        });
        let dim = canvas(Dim).width(iced::Fill).height(iced::Fill);
        iced::widget::stack![zoomed.width(iced::Fill).height(iced::Fill), dim].into()
    }
    // red = blue: the renderer's BGRA and the reference's RGBA agree
    let (iw, ih) = (90u32, 60u32);
    let rgba: Vec<u8> = (0..iw * ih)
        .flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 7) as u8, 255])
        .collect();
    let (size, k) = (Size::new(80.0, 50.0), 1.5);
    let at_rest = View::new(iw, ih, size);
    let v = at_rest.lerp(&at_rest.zoomed(Point::new(30.0, 20.0), 4.0), 0.37);
    for faded in [0.0, 0.4] {
        let handle = iced::widget::image::Handle::from_rgba(iw, ih, rgba.clone());
        let mut state = S(handle, v, faded);
        let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
        let drawn = drawn(&mut state, size, k, view, 1, true, |_, _, _| {}, still);
        // tiny-skia's own: black, the picture scaled, the dim filled over it
        let (w, h) = ((size.width * k) as u32, (size.height * k) as u32);
        let mut reference = tiny_skia::Pixmap::new(w, h).unwrap();
        reference.fill(tiny_skia::Color::BLACK);
        let size_px = tiny_skia::IntSize::from_wh(iw, ih).unwrap();
        let picture = tiny_skia::Pixmap::from_vec(rgba.clone(), size_px).unwrap();
        let on = v.frame_on_screen();
        let (sx, sy) = (on.width * k / iw as f32, on.height * k / ih as f32);
        let at = tiny_skia::Transform::from_row(sx, 0.0, 0.0, sy, on.x * k, on.y * k);
        let paint = tiny_skia::PixmapPaint {
            quality: tiny_skia::FilterQuality::Nearest,
            opacity: 1.0 - faded,
            ..Default::default()
        };
        reference.draw_pixmap(0, 0, picture.as_ref(), &paint, at, None);
        let mut fill = tiny_skia::Paint::default();
        fill.set_color(tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, 0.3).unwrap());
        fill.anti_alias = false;
        let r = DIM_AT * k;
        let rect = tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height).unwrap();
        let path = tiny_skia::PathBuilder::from_rect(rect);
        let rule = tiny_skia::FillRule::Winding;
        reference.fill_path(&path, &fill, rule, tiny_skia::Transform::identity(), None);
        let (got, want) = (&drawn[0].rgba, reference.data());
        let off: Vec<usize> = (0..(w * h) as usize)
            .filter(|&i| (0..4).any(|c| got[i * 4 + c].abs_diff(want[i * 4 + c]) > 1))
            .collect();
        let xy: Vec<_> = (off.iter().take(5))
            .map(|i| (i % w as usize, i / w as usize))
            .collect();
        let px = |i: usize| (&got[i * 4..i * 4 + 4], &want[i * 4..i * 4 + 4]);
        let first = off.first().map(|&i| px(i));
        let last = off.last().map(|&i| (i % w as usize, i / w as usize, px(i)));
        assert!(
            off.is_empty(),
            "faded {faded}: {} pixels differ, first {xy:?} {first:?}, last {last:?}",
            off.len()
        );
    }
}

/// The wheel zoom of the frozen screen: 4 notches in (one every 3 frames, so
/// each retargets mid-motion), then 4 out; and the zoomed view panned (held
/// middle button, 30 px a frame); at 1920x1080 @100 % and 3840x2160 @200 and
/// 250 % (run in release: `cargo test --release zoom_frame_times -- --ignored
/// --nocapture --test-threads=1`). Each of their frames redraws the whole
/// screen; the budget at 4K 120 Hz is 8.3 ms.
#[test]
#[ignore = "timing: run by hand, in release"]
fn zoom_frame_times() {
    use crate::ui::overlay::Event;
    for (w, h, k) in [(1920, 1080, 1.0), (3840, 2160, 2.0), (3840, 2160, 2.5)] {
        let mut app = open_app(w, h, k);
        settle_bar(&mut app);
        let at = Point::new(app.o.screen.width * 0.3, app.o.screen.height * 0.4);
        let t = frames(&mut app, 36, false, |app, i, now| {
            if i % 3 == 0 && i < 24 {
                let notches = if i < 12 { 1.0 } else { -1.0 };
                let _ = app.on_overlay(Event::Zoom { at, notches });
            }
            let _ = app.update(Message::Frame(now));
        });
        let moving: Vec<Drawn> = t.into_iter().filter(|d| !d.damage.is_empty()).collect();
        report("zoom", app.o.screen, k, &moving);
        // 2x, then a pan
        motion::set_reduced(true);
        let _ = app.on_overlay(Event::Zoom { at, notches: 3.0 });
        motion::set_reduced(false);
        let _ = app.on_overlay(Event::PanStart(at));
        let t = frames(&mut app, 16, false, |app, i, _| {
            let to = at + Vector::new(30.0 * i as f32, 12.0 * i as f32);
            let _ = app.on_overlay(Event::PanMove(to));
        });
        report("pan", app.o.screen, k, &t);
    }
}

/// What `across_screens` does with the box.
#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    /// Draws it.
    Stroke,
    /// Moves it by its middle.
    Move,
    /// Resizes it by its bottom-right corner (on the other screen).
    Resize,
}

/// Each window's frames (the starting screen's, then the other screen's) of
/// a stroke drawn from the starting screen (`w` x `h` at `k`) onto `side`,
/// then of the box it made, moved or resized (`gesture`).
fn across_screens(
    (w, h, k): (u32, u32, f32),
    side: super::platform::monitor::Place,
    gesture: Gesture,
) -> [Vec<Drawn>; 2] {
    use super::app::screens::Screen;
    use crate::ui::overlay::Event;
    fn other(app: &App) -> Element<'_, Message> {
        app.view(app.o.screens[0].window)
    }
    let stroke = |app: &mut App, i: u32| {
        let s = app.o.screen;
        // from 70 % across the starting screen on into the other
        let x = s.width * (0.7 + 0.02 * i as f32);
        let at = Point::new(x, s.height * (0.5 + 0.1 * (i as f32 * 0.4).sin()));
        let e = match i {
            0 => Event::Press { at, shift: false },
            _ => Event::Move(at),
        };
        let _ = app.on_overlay(e);
    };
    let from = std::cell::Cell::new(Point::ORIGIN);
    let step = |app: &mut App, i: u32| {
        if gesture == Gesture::Stroke {
            return stroke(app, i);
        }
        if i == 0 {
            (0..=30).for_each(|i| stroke(app, i));
            let _ = app.on_overlay(Event::Release);
            settle_bar(app);
            let r = app
                .o
                .view
                .rect_to_screen(app.o.selection.expect("a selection").area);
            from.set(match gesture {
                Gesture::Resize => crate::ui::overlay::corner(r, 2),
                _ => r.center(),
            });
        }
        let by = match gesture {
            Gesture::Resize => Vector::new(-4.0, 3.0),
            _ => Vector::new(12.0, 5.0),
        };
        let at = from.get() + by * i as f32;
        let e = match i {
            0 => Event::Press { at, shift: false },
            _ => Event::Move(at),
        };
        let _ = app.on_overlay(e);
    };
    let make = || {
        let mut app = open_app(w, h, k);
        settle_bar(&mut app);
        let (pw, ph) = side.px;
        let rgba = vec![90; (pw * ph * 4) as usize];
        let frame = iced::widget::image::Handle::from_rgba(pw, ph, rgba);
        let start = app.o.place();
        let id = window::Id::unique();
        app.o.screens.push(Screen::new(id, start, side, frame));
        app
    };
    let mut app = make();
    let first = frames(&mut app, 30, false, |app, i, _| step(app, i));
    let mut app = make();
    let still = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
    let size = side.size();
    let on = |app: &mut App, i, _| step(app, i);
    let second = drawn(&mut app, size, side.scale, other, 30, false, on, still);
    [first, second]
}

/// A stroke drawn across two screens, then its box moved: each window
/// redraws only round it. Before: the dim and the stroke were one drawing,
/// and every pointer move redrew both windows whole (102-153 % of each).
#[test]
fn a_stroke_across_screens_redraws_only_round_itself() {
    use super::platform::monitor::Place;
    let side = Place {
        origin: (1920, 0),
        px: (1920, 1080),
        scale: 1.5,
    };
    let screens = [Size::new(1920.0, 1080.0), side.size()];
    for gesture in [Gesture::Stroke, Gesture::Move, Gesture::Resize] {
        let windows = across_screens((1920, 1080, 1.0), side, gesture);
        for (t, size) in windows.iter().zip(screens) {
            // (frame 0 of the move holds the whole stroke and its release)
            for (i, d) in t.iter().enumerate().skip(1) {
                let area: f32 = d.damage.iter().map(|r| r.width * r.height).sum();
                let whole = size.width * size.height;
                assert!(
                    area < 0.75 * whole,
                    "frame {i}: {:.0} % redrawn",
                    100.0 * area / whole
                );
            }
        }
    }
}

/// A box across two screens resized (gate 0.1.2: the other screen's frames
/// p95 30-32 ms against 25): each window redraws only the tiles where the
/// box's edges moved, not every tile along it (a tile drew the whole box, so
/// a change anywhere redrew all of them: 29 % of the starting screen and
/// 35 % of the other a frame), and draws what a fresh frame draws.
#[test]
fn a_box_resized_across_screens_redraws_only_its_moving_edges() {
    use super::platform::monitor::Place;
    let side = Place {
        origin: (1920, 0),
        px: (1920, 1080),
        scale: 1.5,
    };
    let screens = [Size::new(1920.0, 1080.0), side.size()];
    let windows = across_screens((1920, 1080, 1.0), side, Gesture::Resize);
    for (t, size) in windows.iter().zip(screens) {
        let area: f32 = (t.iter().skip(1))
            .flat_map(|d| d.damage.iter().map(|r| r.width * r.height))
            .sum();
        let share = area / ((t.len() - 1) as f32 * size.width * size.height);
        assert!(share < 0.2, "{:.0} % a frame", share * 100.0);
    }
    // the last frame, drawn bit by bit, is the frame drawn afresh
    let mut app = open_app(1280, 720, 1.0);
    settle_bar(&mut app);
    drag(
        &mut app,
        Point::new(100.0, 150.0),
        Point::new(1100.0, 600.0),
        false,
    );
    settle_bar(&mut app);
    app.o.morph = None;
    let r = app
        .o
        .view
        .rect_to_screen(app.o.selection.expect("a box").area);
    let corner = crate::ui::overlay::corner(r, 0);
    let t = frames(&mut app, 12, true, |app, i, _| {
        let at = corner + Vector::new(9.0, 7.0) * i as f32;
        let e = match i {
            0 => crate::ui::overlay::Event::Press { at, shift: false },
            _ => crate::ui::overlay::Event::Move(at),
        };
        let _ = app.on_overlay(e);
    });
    let now = app
        .o
        .view
        .rect_to_screen(app.o.selection.expect("a box").area);
    assert!(
        app.o.grab.is_some() && now != r,
        "resized: {r:?} -> {now:?}"
    );
    let fresh = frames(&mut app, 1, true, |_, _, _| {});
    assert!(t.last().unwrap().rgba == fresh[0].rgba, "as drawn afresh");
}

/// A box resized by a corner redraws round its edges, never its middle (VM,
/// a box across two screens resized on one: the starting screen's frames p95
/// 23-31 ms against 25, as its four brackets were one path and every frame
/// redrew the whole box, 46 % of the screen).
#[test]
fn a_resized_box_redraws_its_edges_not_its_middle() {
    let mut app = open_app(1280, 720, 1.0);
    settle_bar(&mut app);
    let (a, b) = (Point::new(100.0, 150.0), Point::new(1100.0, 600.0));
    drag(&mut app, a, b, false);
    settle_bar(&mut app);
    app.o.morph = None;
    let r = app
        .o
        .view
        .rect_to_screen(app.o.selection.expect("a box").area);
    let middle = r.center();
    let t = frames(&mut app, 12, false, |app, i, _| {
        let at = crate::ui::overlay::corner(r, 2) + Vector::new(-4.0, 3.0) * i as f32;
        let e = match i {
            0 => crate::ui::overlay::Event::Press { at, shift: false },
            _ => crate::ui::overlay::Event::Move(at),
        };
        let _ = app.on_overlay(e);
    });
    let now = app
        .o
        .view
        .rect_to_screen(app.o.selection.expect("a box").area);
    assert!(now != r, "resized: {r:?} -> {now:?}");
    for (i, d) in t.iter().enumerate() {
        let hit = d.damage.iter().find(|d| d.contains(middle));
        assert!(hit.is_none(), "frame {i} redrew the middle: {hit:?}");
    }
}

/// The AI chat card dragged in the overlay by its header (picked up), at the
/// VM's screen and a 4K one at 250 % (run in release:
/// `cargo test --release chat_drag_frame_times -- --ignored --nocapture`).
#[test]
#[ignore = "timing: run by hand, in release"]
fn chat_drag_frame_times() {
    for (w, h, k) in [(1716, 1271, 1.0), (3840, 2160, 2.5)] {
        let mut app = open_app(w, h, k);
        let (a, b) = (Point::new(100.0, 100.0), Point::new(400.0, 300.0));
        drag(&mut app, a, b, false);
        let _ = app.update(Message::AskInput("what is this?".into()));
        let _ = app.update(Message::AskSubmit);
        settle_bar(&mut app);
        let id = app.o.panel.as_ref().expect("a chat").0;
        let t = frames(&mut app, 30, false, |app, i, _| {
            if i == 0 {
                let grab = card::Event::Grab(card::Drag::Move);
                let _ = app.update(Message::Panel(id, panel::Msg::Card(grab)));
                let _ = app.update(Message::CardMoved(Point::new(600.0, 300.0)));
            }
            let p = Point::new(600.0 - 12.0 * i as f32, 300.0 + 4.0 * i as f32);
            let _ = app.update(Message::CardMoved(p));
        });
        report("chat drag", app.o.screen, k, &t);
    }
}

/// Every window the app changed for draws with the one that draws (the
/// vendored iced_winit, `Window::stale`): on Windows a window draws on
/// WM_PAINT, which only the topmost window needing it gets while input keeps
/// coming, and a box drawn across two screens moved on the other screen only
/// after the drag (VM, 1 s drag at 60 Hz: 2 frames there against 60 on the
/// starting screen). Needs a real event loop to time; this keeps the fix in.
#[test]
fn other_windows_draw_with_the_one_that_draws() {
    // a Windows checkout has CRLF line ends
    let shell = include_str!("../../vendor/iced_winit/src/lib.rs").replace("\r\n", "\n");
    assert!(shell.contains("redraws.push(other.raw.id())"));
    assert!(shell.contains("if let Some(window_id) = redraws.pop()"));
    // ... and only then: a WM_PAINT asked for as well drew each window a
    // second time with nothing new, a whole-window copy (VM: 35 of 61 draws
    // of the other screen in a resize)
    assert!(
        shell
            .contains("} else {\n                                    window.raw.request_redraw();")
    );
    assert!(shell.contains("if window.raw.is_visible() == Some(false) {\n                                        window.raw.request_redraw();"));
    let cargo = include_str!("../../Cargo.toml");
    assert!(cargo.contains("iced_winit = { path = \"vendor/iced_winit\" }"));
}

/// The bench of it at real sizes (run in release:
/// `cargo test --release stroke_across_screens_frame_times -- --ignored --nocapture`).
#[test]
#[ignore = "timing: run by hand, in release"]
fn stroke_across_screens_frame_times() {
    use super::platform::monitor::Place;
    let place = |origin, px, scale| Place { origin, px, scale };
    for (start, side) in [
        ((1920, 1080, 1.0), place((1920, 0), (1920, 1080), 1.5)),
        ((3440, 1440, 1.0), place((3440, 0), (1920, 1080), 1.0)),
        ((3840, 2160, 2.5), place((3840, 0), (1920, 1080), 1.0)),
    ] {
        let k = start.2;
        let size = Size::new(start.0 as f32 / k, start.1 as f32 / k);
        for (what, gesture) in [
            ("stroke", Gesture::Stroke),
            ("box move", Gesture::Move),
            ("box resize", Gesture::Resize),
        ] {
            let [a, b] = across_screens(start, side, gesture);
            report(&format!("{what}, starting screen"), size, k, &a);
            report(
                &format!("{what}, other screen"),
                side.size(),
                side.scale,
                &b,
            );
        }
    }
}

/// The snip's three drags.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SnipDrag {
    /// An area picked by hand (the loupe beside the pointer).
    Pick,
    /// The live area's bottom-right handle, the toolbar docking along.
    Resize,
    /// The live area moved from its middle.
    Move,
}

/// `n` frames of a `drag` over a `w` x `h` snip, one pointer move each, and
/// the app after them; the window under the pointer is (200, 150, 600 x 400),
/// the live area starts there.
fn snip_drag(w: u32, h: u32, drag: SnipDrag, n: u32, pixels: bool) -> (Vec<Drawn>, App) {
    use super::app::tests::snipping_on;
    use crate::editor::Message as E;
    use crate::ui::overlay::Event;
    let window = Rectangle::new(Point::new(200.0, 150.0), Size::new(600.0, 400.0));
    let mut app = snipping_on(w, h, window);
    if drag != SnipDrag::Pick {
        app.o.labs = crate::labs::Labs::all_on();
        app.select(window);
        let crop = app.crop().unwrap();
        let route = super::snip::route(crate::config::SnipAfter::Editor);
        let _ = app.snip_routed(crop, true, route);
    }
    let (from, by) = match drag {
        SnipDrag::Pick => (Point::new(120.0, 100.0), Vector::new(14.0, 9.0)),
        SnipDrag::Resize => (Point::new(800.0, 550.0), Vector::new(9.0, 6.0)),
        SnipDrag::Move => (Point::new(400.0, 300.0), Vector::new(7.0, 4.0)),
    };
    let t = frames(&mut app, n, pixels, |app, i, now| {
        let at = from + by * i as f32;
        if drag == SnipDrag::Pick {
            let e = match i {
                0 => Event::Press { at, shift: false },
                _ => Event::Move(at),
            };
            let _ = app.on_overlay(e);
            return;
        }
        let _ = app.on_editor(match i {
            0 => E::Press(at),
            _ => E::Move(at),
        });
        let _ = app.update(Message::Frame(now)); // the toolbar docks along
    });
    (t, app)
}

/// The snip's drags at the VM's screen, 1280x720 and the wide desktop (run
/// in release: `cargo test --release snip_frame_times -- --ignored --nocapture`).
#[test]
#[ignore = "timing: run by hand, in release"]
fn snip_frame_times() {
    for (w, h) in [(1716, 1271), (1280, 720), (3438, 1280)] {
        for drag in [SnipDrag::Pick, SnipDrag::Resize, SnipDrag::Move] {
            let (t, app) = snip_drag(w, h, drag, 60, false);
            report(&format!("snip {drag:?}"), app.o.screen, 1.0, &t);
        }
    }
}

/// Snip's drags (0.1.3 gate: frames p95 under 16 ms on the VM, were 30 / 27 ms)
/// redraw round what moves and draw what a fresh frame draws. Before: the
/// loupe's label redrew the screen from it to the corner (a canvas text's
/// damage reached its infinite box), and the live area painted and redrew all
/// of itself on every move (51 % and 40-68 % of the screen a frame).
#[test]
fn a_snips_drags_redraw_round_what_moves() {
    for drag in [SnipDrag::Pick, SnipDrag::Resize, SnipDrag::Move] {
        let (t, app) = snip_drag(1716, 1271, drag, 30, false);
        let whole = app.o.screen.width * app.o.screen.height;
        let area: f32 = (t.iter().skip(1))
            .flat_map(|d| d.damage.iter().map(|r| r.width * r.height))
            .sum();
        let share = area / ((t.len() - 1) as f32 * whole);
        assert!(share < 0.3, "{drag:?}: {:.0} % a frame", share * 100.0);
        // the middle of the box being drawn or resized is never redrawn (once
        // the pick is big: a small box's sides are one run of damage; a moved
        // area's edges cross every dim tile it covers in turn)
        let middle = match drag {
            SnipDrag::Pick => Some(Point::new(200.0, 150.0)),
            SnipDrag::Resize => Some(Point::new(500.0, 350.0)),
            SnipDrag::Move => None,
        };
        for (i, d) in t.iter().enumerate().skip(15) {
            let hit = d
                .damage
                .iter()
                .find(|r| middle.is_some_and(|m| r.contains(m)));
            assert!(
                hit.is_none(),
                "{drag:?} frame {i} redrew the middle: {hit:?}"
            );
        }
        // drawn bit by bit, the last frame is the frame drawn afresh (to the
        // rounding of a dim blended in parts or whole: 1 of 255)
        let (t, mut app) = snip_drag(1280, 720, drag, 12, true);
        let fresh = frames(&mut app, 1, true, |_, _, _| {});
        let (got, want) = (&t.last().unwrap().rgba, &fresh[0].rgba);
        let off = (got.iter().zip(want))
            .filter(|(a, b)| a.abs_diff(**b) > 1)
            .count();
        assert_eq!(off, 0, "{drag:?}: as drawn afresh");
    }
}
