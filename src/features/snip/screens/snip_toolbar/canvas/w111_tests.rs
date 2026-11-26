//! W111: every drawing tool follows the pointer. The lab (`w111_tool_frame_times`, by hand, in
//! release) times each frame of a scripted drag the way a window runs it: the pointer events of
//! the frame handled (`Annotator::update`, one message each), then the canvas built, laid out,
//! drawn and rastered where it changed.
use iced::{Point, Size};

use super::{ScreenMap, canvas};
use crate::features::snip::annotate::blur::{Kind, Style};
use crate::features::snip::annotate::{Annotator, Image, Message, Tool};
use crate::shared::kit::testing::shot;

/// A picture with detail (a flat one hides what costs): text-like bars on a gradient.
pub(super) fn photo(width: u32, height: u32) -> Image {
    let rgba = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 24;
            match (y / 9) % 3 == 0 && (x / 5) % 4 != 0 {
                true => [40, 44, 58, 255],
                false => [
                    (200 + x * 40 / width) as u8,
                    (200 + y * 40 / height) as u8,
                    (230 + n % 8) as u8,
                    255,
                ],
            }
        })
        .collect();
    Image {
        width,
        height,
        rgba,
    }
}

/// A scripted drag: where the pointer is at each of `n` steps, in image px.
pub(super) fn path(tool: Tool, w: f32, h: f32, n: usize) -> Vec<Point> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            match tool {
                // a wave across the picture (a pen stroke long enough to matter)
                Tool::Pen | Tool::Highlighter => {
                    Point::new(w * (0.05 + 0.9 * t), h * (0.5 + 0.3 * (t * 14.0).sin()))
                }
                // a text line's height, left to right (the brush)
                Tool::Blur => Point::new(w * (0.1 + 0.8 * t), h * 0.3),
                // a lens grown from its centre to 300 image px (W113: the corner-to-corner drag made
                // a lens as big as the picture, 4K: p95 160 ms, which nobody draws)
                Tool::Magnifier => Point::new(w * 0.5 + 300.0 * t, h * 0.4),
                // a corner to the opposite corner
                _ => Point::new(w * (0.1 + 0.7 * t), h * (0.15 + 0.7 * t)),
            }
        })
        .collect()
}

/// What the Studio's stage does for a picture (W113: W111's numbers lacked it): shown 1:1 or magnified
/// it draws nearest (`crisp`); a big picture shown smaller draws from a copy of about the shown width.
fn stage(e: &mut Annotator) {
    let iced::widget::image::Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = &e.base
    else {
        return;
    };
    let want = (*width as f32 * e.zoom).round().max(1.0) as u32;
    e.crisp = true;
    if want * 5 < *width * 4 {
        let src = ::image::ImageBuffer::<::image::Rgba<u8>, &[u8]>::from_raw(
            *width,
            *height,
            pixels.as_ref(),
        )
        .unwrap();
        let th = (*height as f32 * want as f32 / *width as f32)
            .round()
            .max(1.0) as u32;
        let small = ::image::imageops::thumbnail(&src, want, th);
        e.display = Some(iced::widget::image::Handle::from_rgba(
            want,
            th,
            small.into_raw(),
        ));
    }
}

/// Ms per frame of a drag of `tool` over a `w` x `h` picture shown at `zoom`: `per` pointer events
/// a frame. `setup` picks the tool's options first.
pub(super) fn frame_times(
    dims: (u32, u32, f32),
    tool: Tool,
    setup: impl Fn(&mut Annotator),
    per: usize,
) -> Vec<f64> {
    (frames(dims, tool, setup, per).iter())
        .map(|f| f.draw_ms + f.raster_ms)
        .collect()
}

/// The pointer-driven frames of the drag, with their draw / raster split and damage.
pub(super) fn frames(
    (w, h, zoom): (u32, u32, f32),
    tool: Tool,
    setup: impl Fn(&mut Annotator),
    per: usize,
) -> Vec<shot::Frame> {
    let mut e = Annotator::new(photo(w, h), zoom);
    e.studio = true;
    e.assist = false;
    stage(&mut e);
    e.update(Message::Tool(tool));
    setup(&mut e);
    let size = e.size();
    let pts = path(tool, w as f32, h as f32, 90 * per);
    let mut at = 0;
    let mut frames = shot::frames_of(Size::new(size.width, size.height), 92, |i, show| {
        match i {
            0 => {}
            1..=90 => {
                for _ in 0..per {
                    e.update(match at {
                        0 => Message::Press(pts[0]),
                        _ => Message::Move(pts[at]),
                    });
                    at += 1;
                }
            }
            _ => {
                e.update(Message::Release);
            }
        }
        show(canvas(&e, 0.0, ScreenMap::ONE));
    });
    // (the update time is inside `draw_ms`: `frames_of` times the step and the view together)
    frames.drain(1..91).collect()
}

/// Where a 4K pen frame goes: the live layer's paint alone, at the end of the drag.
#[test]
#[ignore = "timing: run by hand, in release"]
fn w111_live_layer_paint_time() {
    let mut e = Annotator::new(photo(3840, 2160), 0.5);
    e.update(Message::Tool(Tool::Pen));
    let pts = path(Tool::Pen, 3840.0, 2160.0, 90);
    e.update(Message::Press(pts[0]));
    for p in &pts[1..] {
        e.update(Message::Move(*p));
    }
    let t = std::time::Instant::now();
    for i in 0..10 {
        e.update(Message::Move(pts[90] + iced::Vector::new(i as f32, 0.0)));
        let _ = e.live_layer();
    }
    println!(
        "live layer paint: {:.1} ms",
        t.elapsed().as_secs_f64() * 100.0
    );
}

fn stats(mut v: Vec<f64>) -> String {
    v.sort_by(f64::total_cmp);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    format!(
        "{mean:5.1} {:5.1} {:5.1}",
        v[v.len() * 95 / 100],
        v[v.len() - 1]
    )
}

/// Every tool's frame times (ms: mean, p95, max) on 1080p and 4K. `cargo test --release w111_tool_frame_times --
/// --ignored --nocapture` (Xvfb is not needed: the renderer is the CPU one).
#[test]
#[ignore = "timing: run by hand, in release"]
fn w111_tool_frame_times() {
    let hide = |kind, style| {
        move |e: &mut Annotator| {
            e.update(Message::BlurShape(kind));
            e.update(Message::BlurStyle(style));
        }
    };
    #[allow(clippy::type_complexity)]
    let tools: Vec<(&str, Tool, Box<dyn Fn(&mut Annotator)>)> = vec![
        ("pen", Tool::Pen, Box::new(|_| {})),
        ("highlighter", Tool::Highlighter, Box::new(|_| {})),
        ("rect", Tool::Rect, Box::new(|_| {})),
        ("ellipse", Tool::Ellipse, Box::new(|_| {})),
        ("line", Tool::Line, Box::new(|_| {})),
        ("arrow", Tool::Arrow, Box::new(|_| {})),
        ("step", Tool::Step, Box::new(|_| {})),
        ("redact", Tool::Redact, Box::new(|_| {})),
        ("spotlight", Tool::Spotlight, Box::new(|_| {})),
        ("magnifier", Tool::Magnifier, Box::new(|_| {})),
        (
            "hide brush frost",
            Tool::Blur,
            Box::new(hide(Kind::Brush, Style::Frost)),
        ),
        (
            "hide brush pixelate",
            Tool::Blur,
            Box::new(hide(Kind::Brush, Style::Pixelate)),
        ),
        (
            "hide brush soft",
            Tool::Blur,
            Box::new(hide(Kind::Brush, Style::Soft)),
        ),
        (
            "hide brush blur",
            Tool::Blur,
            Box::new(hide(Kind::Brush, Style::Blur)),
        ),
        (
            "hide rect frost",
            Tool::Blur,
            Box::new(hide(Kind::Rect, Style::Frost)),
        ),
        (
            "hide ellipse frosted",
            Tool::Blur,
            Box::new(hide(Kind::Ellipse, Style::Frosted)),
        ),
        (
            "hide rect progressive",
            Tool::Blur,
            Box::new(hide(Kind::Rect, Style::Progressive)),
        ),
        ("crop", Tool::Crop, Box::new(|_| {})),
    ];
    for (w, h, zoom, name) in [(1920, 1080, 1.0, "1080p"), (3840, 2160, 0.5, "4K")] {
        println!("--- {name}");
        for (label, tool, setup) in &tools {
            for per in [1, 4] {
                let t = frame_times((w, h, zoom), *tool, setup, per);
                println!("{label:<22} x{per}  {}", stats(t));
            }
        }
    }
}

/// W113: where a 4K frame goes: draw (update, view, layout, draw) and raster (the damage), mean ms,
/// and the damaged area as a share of one 1080p screen.
#[test]
#[ignore = "timing: run by hand, in release"]
fn w113_frame_split() {
    for (label, tool) in [
        ("pen", Tool::Pen),
        ("rect", Tool::Rect),
        ("step", Tool::Step),
    ] {
        let f = frames((3840, 2160, 0.5), tool, |_| {}, 1);
        let n = f.len() as f64;
        let area = |f: &shot::Frame| {
            f.damage
                .iter()
                .map(|r| f64::from(r.width * r.height))
                .sum::<f64>()
        };
        println!(
            "{label:<6} draw {:5.1}  raster {:5.1} (finding the damage {:5.1})  damage {:4.0}% of 1920x1080",
            f.iter().map(|f| f.draw_ms).sum::<f64>() / n,
            f.iter().map(|f| f.raster_ms).sum::<f64>() / n,
            f.iter().map(|f| f.damage_ms).sum::<f64>() / n,
            f.iter().map(area).sum::<f64>() / n / (1920.0 * 1080.0) * 100.0,
        );
    }
}

fn run_magnifier_frames(
    dims: (u32, u32, f32),
    stage_size: Size,
    setup: impl Fn(&mut Annotator),
    mut act: impl FnMut(usize, &mut Annotator),
    n: usize,
) -> Vec<shot::Frame> {
    let (w, h, zoom) = dims;
    let mut e = Annotator::new(photo(w, h), zoom);
    e.studio = true;
    e.assist = false;
    setup(&mut e);
    stage(&mut e);
    if zoom > 1.0 {
        e.viewport = Some(iced::Rectangle::new(
            Point::new(
                w as f32 * 0.5 * zoom - stage_size.width * 0.5,
                h as f32 * 0.4 * zoom - stage_size.height * 0.5,
            ),
            stage_size,
        ));
    }
    let mut frames = shot::frames_of(stage_size, n + 2, |i, show| {
        if (1..=n).contains(&i) {
            act(i - 1, &mut e);
        } else if i == n + 1 {
            e.update(Message::Release);
        }
        show(canvas(&e, 0.0, ScreenMap::ONE));
    });
    frames.drain(1..=n).collect()
}

/// W117: real Studio Magnifier frame times (ms: mean, p95, max) on a 4K picture:
/// large lens, zoomed in, drawing, dragging (plain and over Hide+Rect), and resizing.
#[test]
#[ignore = "timing: run by hand, in release"]
fn w117_magnifier_frame_times() {
    let n = 60usize;
    let report = |label: &str, f: Vec<shot::Frame>| {
        let total: Vec<f64> = f.iter().map(|x| x.draw_ms + x.raster_ms).collect();
        let draw: Vec<f64> = f.iter().map(|x| x.draw_ms).collect();
        let raster: Vec<f64> = f.iter().map(|x| x.raster_ms).collect();
        println!(
            "{label:<34} total [{}]  draw [{}]  raster [{}]",
            stats(total),
            stats(draw),
            stats(raster)
        );
    };

    report(
        "4K 0.5x draw r=300",
        run_magnifier_frames(
            (3840, 2160, 0.5),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Magnifier));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(1920.0 + 300.0 * t, 864.0);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 0.5x draw r=600",
        run_magnifier_frames(
            (3840, 2160, 0.5),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Magnifier));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(1920.0 + 600.0 * t, 1080.0);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 0.5x move r=500",
        run_magnifier_frames(
            (3840, 2160, 0.5),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Magnifier));
                e.update(Message::Press(Point::new(1200.0, 1080.0)));
                e.update(Message::Move(Point::new(1700.0, 1080.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Select));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(1200.0 + 1200.0 * t, 1080.0);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 0.5x move r=500 over Hide+Rect",
        run_magnifier_frames(
            (3840, 2160, 0.5),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Blur));
                e.update(Message::BlurShape(Kind::Rect));
                e.update(Message::BlurStyle(Style::Soft));
                e.update(Message::Press(Point::new(1100.0, 700.0)));
                e.update(Message::Move(Point::new(2600.0, 1400.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Rect));
                e.update(Message::Press(Point::new(1400.0, 800.0)));
                e.update(Message::Move(Point::new(2200.0, 1300.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Magnifier));
                e.update(Message::Press(Point::new(1200.0, 1080.0)));
                e.update(Message::Move(Point::new(1700.0, 1080.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Select));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(1200.0 + 1200.0 * t, 1080.0);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 0.5x resize r=300->600",
        run_magnifier_frames(
            (3840, 2160, 0.5),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Magnifier));
                e.update(Message::Press(Point::new(1920.0, 1080.0)));
                e.update(Message::Move(Point::new(2220.0, 1080.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Select));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(2220.0 + 300.0 * t, 1380.0 + 300.0 * t);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 2.0x draw r=350",
        run_magnifier_frames(
            (3840, 2160, 2.0),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Magnifier));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(1920.0 + 350.0 * t, 864.0);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 2.0x move r=300",
        run_magnifier_frames(
            (3840, 2160, 2.0),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Magnifier));
                e.update(Message::Press(Point::new(1700.0, 864.0)));
                e.update(Message::Move(Point::new(2000.0, 864.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Select));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(1700.0 + 440.0 * t, 864.0);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 2.0x resize r=200->400",
        run_magnifier_frames(
            (3840, 2160, 2.0),
            Size::new(1920.0, 1080.0),
            |e| {
                e.update(Message::Tool(Tool::Magnifier));
                e.update(Message::Press(Point::new(1920.0, 864.0)));
                e.update(Message::Move(Point::new(2120.0, 864.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Select));
            },
            |i, e| {
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(2120.0 + 200.0 * t, 1064.0 + 200.0 * t);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
    report(
        "4K 2.0x on-screen move r=300",
        run_magnifier_frames(
            (3840, 2160, 2.0),
            Size::new(1920.0, 1080.0),
            |e| {
                e.viewport = Some(iced::Rectangle::with_size(Size::new(1920.0, 1080.0)));
                e.update(Message::Tool(Tool::Magnifier));
                e.update(Message::Press(Point::new(360.0, 270.0)));
                e.update(Message::Move(Point::new(660.0, 270.0)));
                e.update(Message::Release);
                e.update(Message::Tool(Tool::Select));
            },
            |i, e| {
                e.viewport = Some(iced::Rectangle::with_size(Size::new(1920.0, 1080.0)));
                let t = i as f32 / (n - 1) as f32;
                let p = Point::new(360.0 + 240.0 * t, 270.0);
                e.update(if i == 0 {
                    Message::Press(p)
                } else {
                    Message::Move(p)
                });
            },
            n,
        ),
    );
}
