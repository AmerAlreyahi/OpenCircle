//! The overlay drawn frame after frame as its window draws it: each frame
//! redrawn only where it differs from the one before (the compositor's damage,
//! `iced_tiny_skia::window::compositor::present`). Times every widget
//! animation (`animation_frame_times`, by hand in release) and checks that a
//! moving widget redraws only around itself.

use super::tests::{drag, open_app, settle_bar};
use super::*;
use iced::advanced::graphics::{Viewport, damage};
use iced::{Color, Theme};
use iced_runtime::user_interface::{Cache, UserInterface};

/// One frame: how long it took (view, layout, draw, raster), the rectangles
/// redrawn, and the pixels after it (RGBA).
pub struct Drawn {
    pub took: Duration,
    /// Of it: rasterizing the damage.
    pub raster: Duration,
    pub damage: Vec<Rectangle>,
    pub rgba: Vec<u8>,
}

/// Frames `0..=frames` of `app`, frame `i` at 16 ms * `i` after now; `step`
/// moves the app on before each (frame 0 is drawn whole and not returned).
/// `pixels`: keep each frame's pixels (else `rgba` is empty).
pub fn frames(
    app: &mut App,
    frames: u32,
    pixels: bool,
    mut step: impl FnMut(&mut App, u32, Instant),
) -> Vec<Drawn> {
    let k = app.scale;
    let (w, h) = (
        (app.screen.width * k).round() as u32,
        (app.screen.height * k).round() as u32,
    );
    let size = app.screen;
    let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
    let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
    let mut mask = tiny_skia::Mask::new(w, h).unwrap();
    let viewport = Viewport::with_physical_size(Size::new(w, h), k);
    let (mut cache, mut last) = (Cache::default(), None::<Vec<iced_tiny_skia::Layer>>);
    let start = Instant::now();
    let mut out = Vec::new();
    for i in 0..=frames {
        let now = start + Duration::from_millis(16 * u64::from(i));
        step(app, i, now);
        let began = Instant::now();
        let mut ui = UserInterface::build(app.view(app.overlay), size, cache, &mut renderer);
        let redraw = iced::Event::Window(window::Event::RedrawRequested(now));
        let cursor = iced::mouse::Cursor::Unavailable;
        let mut clipboard = iced::advanced::clipboard::Null;
        let _ = ui.update(
            &[redraw],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut Vec::new(),
        );
        let style = iced::advanced::renderer::Style {
            text_color: Color::WHITE,
        };
        ui.draw(&mut renderer, &Theme::Dark, &style, cursor);
        cache = ui.into_cache();
        let layers = renderer.layers().to_vec();
        let all = Rectangle::with_size(size);
        let changed = match &last {
            Some(last) => damage::diff(
                last,
                &layers,
                |l| vec![l.bounds],
                iced_tiny_skia::Layer::damage,
            ),
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
                raster: rastering.elapsed(),
                damage: changed,
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

/// The bar let go near the top on frame 0: it glides there.
fn dock_glide(app: &mut App, i: u32, now: Instant) {
    if i == 0 {
        let _ = app.update(Message::BarGrab);
        let at = app.bar_at();
        let _ = app.update(Message::BarMoved(at));
        let up = Vector::new(0.0, app.screen.height * 0.7);
        let _ = app.update(Message::BarMoved(at - up));
        let _ = app.update(Message::BarReleased);
    }
    let _ = app.update(Message::Frame(now));
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
        // the opening: the dim fades in, the bar slides up
        let mut app = open_app(w, h, 1.0);
        app.appear = None;
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
        // the result card dragged by its grip
        let mut app = open_app(w, h, 1.0);
        let (a, b) = (Point::new(100.0, 100.0), Point::new(400.0, 300.0));
        drag(&mut app, a, b, false);
        let _ = app.update(Message::Run(&text::ACTION));
        let id = app.panel.as_ref().expect("a card").0;
        let t = frames(&mut app, 16, false, |app, i, _| {
            if i == 0 {
                let grab = card::Event::Grab(card::Drag::Move);
                let _ = app.update(Message::Panel(id, panel::Msg::Card(grab)));
                let _ = app.update(Message::CardMoved(Point::new(1500.0, 200.0)));
            }
            let p = Point::new(1500.0 - 20.0 * i as f32, 200.0 + 5.0 * i as f32);
            let _ = app.update(Message::CardMoved(p));
        });
        report("card drag", w, h, t);
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

/// The frame drawn as tiles is pixel for pixel the frame, at 125 % too (no
/// seam where two tiles meet).
#[test]
fn the_tiled_frame_is_the_frame() {
    for scale in [1.0, 1.25, 1.5] {
        let mut app = open_app(1700, 900, scale);
        let tiled = frames(&mut app, 1, true, |_, _, _| {}).remove(0).rgba;
        assert!(app.tiles.as_ref().is_some_and(|(_, t)| t.len() == 7 * 4));
        app.tiles = None; // the frame as one image
        let whole = frames(&mut app, 1, true, |_, _, _| {}).remove(0).rgba;
        let off = (tiled.iter().zip(&whole)).filter(|(a, b)| a != b).count();
        assert_eq!(off, 0, "at {scale}");
    }
}
