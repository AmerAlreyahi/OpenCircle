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
        let (state, _) = ui.update(
            &[redraw],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut Vec::new(),
        );
        let next = match state {
            iced_runtime::user_interface::State::Updated { redraw_request, .. } => redraw_request,
            iced_runtime::user_interface::State::Outdated => window::RedrawRequest::NextFrame,
        };
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
        let up = Vector::new(0.0, app.screen.height * 0.7);
        let _ = app.update(Message::BarMoved(at - up));
        let _ = app.update(Message::BarReleased);
    }
    let _ = app.update(Message::Frame(now));
}

/// An open overlay with a Text card, open and still.
fn card_app(w: u32, h: u32) -> App {
    let mut app = open_app(w, h, 1.0);
    let (a, b) = (Point::new(100.0, 100.0), Point::new(400.0, 300.0));
    drag(&mut app, a, b, false);
    let _ = app.update(Message::Run(&text::ACTION));
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
                let _ = app.update(Message::Run(&text::ACTION));
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
            let id = app.panel.as_ref().expect("a card").0;
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
    app.appear = None;
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
        app.appear = None;
        app.text_by = Some(ocr::Source::System);
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
        assert_eq!(app.screen_text.lines().len(), 90, "both reads landed");
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
        app.appear = None;
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
    // Windows: the edge glow's slow breath is its one motion, off with reduced
    // motion (`the_breath_redraws_only_the_edge`). Linux has none (#142), so
    // motion stays on there: the still overlay must cost nothing.
    motion::set_reduced(cfg!(windows));
    let mut app = open_app(1280, 720, 1.0);
    app.appear = None;
    let (a, b) = (Point::new(300.0, 200.0), Point::new(600.0, 400.0));
    let drawn = frames(&mut app, 40, false, |app, i, now| {
        if i == 1 {
            drag(app, a, b, false);
        }
        if i == 2 {
            let _ = app.update(Message::Run(&text::ACTION));
        }
        let _ = app.update(Message::Frame(now));
    });
    assert!(app.panel.is_some(), "a card is up");
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
/// once per `border::STEP`, and only after the opening: never the middle of the
/// screen, and a few percent of the screen per frame at most. (Windows only:
/// Linux's glow stays still, #142.)
#[test]
#[cfg(windows)]
fn the_breath_redraws_only_the_edge() {
    let (w, h) = (1920, 1080);
    motion::set_reduced(false);
    let mut app = open_app(w, h, 1.0);
    app.appear = None;
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
        (8..=16).contains(&steps),
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

/// Every open goes by `motion`: the opening's clock, a result card growing
/// from `OPEN_SCALE` in `OPEN` eased out; with the OS's animations off, all
/// of it there at once.
#[test]
fn widgets_open_by_the_design_motion_or_at_once() {
    let opened = |reduced: bool| {
        motion::set_reduced(reduced);
        let mut app = open_app(1280, 720, 1.0);
        app.appear = None;
        let (a, b) = (Point::new(300.0, 200.0), Point::new(600.0, 400.0));
        drag(&mut app, a, b, false);
        let _ = app.update(Message::Run(&text::ACTION));
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
    let (drawn, again) = widgets::shot::render_twice(view, 100, 60, Some(damage));
    assert_eq!(drawn, again, "a part redrawn is the same");
    // tiny-skia's own: the background, the image drawn over it
    let [r, g, b, a] = theme::current().window.into_rgba8();
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
