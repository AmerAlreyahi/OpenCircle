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
                // a corner to the opposite corner
                _ => Point::new(w * (0.1 + 0.7 * t), h * (0.15 + 0.7 * t)),
            }
        })
        .collect()
}

/// Ms per frame of a drag of `tool` over a `w` x `h` picture shown at `zoom`: `per` pointer events
/// a frame. `setup` picks the tool's options first.
pub(super) fn frame_times(
    (w, h, zoom): (u32, u32, f32),
    tool: Tool,
    setup: impl Fn(&mut Annotator),
    per: usize,
) -> Vec<f64> {
    let mut e = Annotator::new(photo(w, h), zoom);
    e.studio = true;
    e.assist = false;
    e.update(Message::Tool(tool));
    setup(&mut e);
    let size = e.size();
    let pts = path(tool, w as f32, h as f32, 90 * per);
    let mut at = 0;
    let frames = shot::frames_of(Size::new(size.width, size.height), 92, |i, show| {
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
    frames[1..91]
        .iter()
        .map(|f| f.draw_ms + f.raster_ms)
        .collect()
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
