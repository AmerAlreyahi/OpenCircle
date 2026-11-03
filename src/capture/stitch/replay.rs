//! Replays of real long captures (frames as the stitcher got them on the
//! Windows VM, `OPENCIRCLE_LONG_DUMP`), checked against the true page.
//!
//! The pages (docs/internal tooling, a local HTML file in Edge) draw a
//! ruler down their right edge, inside the strip matching ignores
//! (`scrollbar_strip`): each device row of the document in its own colour
//! (red = row & 255, green = row >> 8, blue = 0x55). Fixed bars cover it.
//! So every row of the result says which document row it came from, and the
//! true page is: the first frame's top bar, every document row once and in
//! order, the last frame's bottom bar.
//!
//! A fixture is two files in `testdata/replay/`: `<name>.png`, every
//! distinct row of the recording once (RGB), and `<name>.txt`, the size
//! (`w h`) then one line per frame: its rows as runs `start+len` of the
//! PNG's rows. Frames that scroll share their rows, so a few hundred frames
//! cost about one page.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::{Push, Stitcher};

/// The ruler's blue: a row whose ruler pixel has it is a document row.
const RULER_B: u8 = 0x55;

struct Tape {
    w: usize,
    h: usize,
    frames: Vec<Vec<u8>>,
}

impl Tape {
    fn load(png_bytes: &[u8], runs: &str) -> Tape {
        let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        let dict: Vec<u8> = (buf.as_chunks::<3>().0.iter())
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect();
        let mut lines = runs.lines();
        let size: Vec<usize> = (lines.next().unwrap().split(' '))
            .map(|v| v.parse().unwrap())
            .collect();
        let (w, h) = (size[0], size[1]);
        assert_eq!(w, info.width as usize);
        let stride = w * 4;
        let frames = lines
            .map(|line| {
                let mut f = Vec::with_capacity(stride * h);
                for run in line.split(' ') {
                    let (start, len) = run.split_once('+').unwrap();
                    let (start, len): (usize, usize) =
                        (start.parse().unwrap(), len.parse().unwrap());
                    f.extend_from_slice(&dict[start * stride..(start + len) * stride]);
                }
                assert_eq!(f.len(), stride * h);
                f
            })
            .collect();
        Tape { w, h, frames }
    }

    /// The ruler's column: the one near the right edge most rows of the first frame have it in.
    fn ruler_x(&self) -> usize {
        let f = &self.frames[0];
        let count = |x: usize| {
            (0..self.h)
                .filter(|&y| f[(y * self.w + x) * 4 + 2] == RULER_B)
                .count()
        };
        let x = (self.w.saturating_sub(12)..self.w)
            .max_by_key(|&x| count(x))
            .unwrap();
        assert!(count(x) > self.h / 2, "no ruler in the first frame");
        x
    }
}

/// The document row a row (`w` px of RGBA) shows, by its ruler pixel at `x`.
fn code(row: &[u8], x: usize) -> Option<usize> {
    let p = &row[x * 4..x * 4 + 3];
    (p[2] == RULER_B).then(|| p[0] as usize | (p[1] as usize) << 8)
}

struct Score {
    /// Stitched height and the true page's.
    height: usize,
    truth: usize,
    /// Rows that are not the true page's row there (plus the rows missing
    /// or too many at the end).
    wrong: usize,
    frames: usize,
    /// Stitch time per frame: mean and worst.
    mean: Duration,
    worst: Duration,
}

fn replay(tape: &Tape) -> Score {
    let x = tape.ruler_x();
    let stride = tape.w * 4;
    let codes =
        |f: &[u8]| -> Vec<Option<usize>> { f.chunks_exact(stride).map(|r| code(r, x)).collect() };
    let first = codes(&tape.frames[0]);
    let top = first.iter().position(Option::is_some).unwrap();
    // expected: row k (under the top bar) is document row k + c0
    let c0 = first[top].unwrap() - top;
    let last = codes(tape.frames.last().unwrap());
    let bot = last.iter().rev().position(Option::is_some).unwrap();
    let max = (tape.frames.iter())
        .flat_map(|f| codes(f).into_iter().flatten())
        .max()
        .unwrap();
    let truth = max - c0 + 1 + bot;
    let mut st = Stitcher::new(tape.w as u32, tape.h as u32);
    let (mut all, mut worst) = (Duration::ZERO, Duration::ZERO);
    let log = std::env::var_os("OPENCIRCLE_REPLAY_LOG").is_some();
    for (i, f) in tape.frames.iter().enumerate() {
        let t = Instant::now();
        let push = st.push(f);
        let took = t.elapsed();
        (all, worst) = (all + took, worst.max(took));
        if log {
            let shown = codes(f).into_iter().flatten().next();
            eprintln!(
                "{i}: {push:?}, {} (true row at the top: {shown:?})",
                st.step
            );
        }
        let _: Push = push;
    }
    let (px, _, h) = st.finish();
    let height = h as usize;
    let want = |k: usize| (k >= top && k + bot < truth).then(|| k + c0);
    let wrong = (0..height.min(truth))
        .filter(|&k| code(&px[k * stride..(k + 1) * stride], x) != want(k))
        .count()
        + height.abs_diff(truth);
    Score {
        height,
        truth,
        wrong,
        frames: tape.frames.len(),
        mean: all / tape.frames.len() as u32,
        worst,
    }
}

macro_rules! tape {
    ($name:literal) => {
        Tape::load(
            include_bytes!(concat!("../testdata/replay/", $name, ".png")),
            include_str!(concat!("../testdata/replay/", $name, ".txt")),
        )
    };
}

/// Every recording, replayed: as tall as the true page, every row the
/// true page's. (`OPENCIRCLE_REPLAY_LOG=1` prints each frame's decision.)
#[test]
fn real_recordings_replay_to_the_true_page() {
    let mut bad = Vec::new();
    for (name, tape) in recordings() {
        let s = replay(&tape);
        eprintln!(
            "{name}: {} of {} px, {} rows wrong; {} frames, {:.1} ms a frame, worst {:.1}",
            s.height,
            s.truth,
            s.wrong,
            s.frames,
            s.mean.as_secs_f64() * 1000.0,
            s.worst.as_secs_f64() * 1000.0
        );
        if s.wrong > 0 {
            bad.push(format!(
                "{name}: {} px for {}, {} rows wrong",
                s.height, s.truth, s.wrong
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

fn recordings() -> Vec<(&'static str, Tape)> {
    vec![
        ("steps", tape!("steps")),
        ("flick", tape!("flick")),
        ("chat", tape!("chat")),
        ("smooth", tape!("smooth")),
        ("anim", tape!("anim")),
        ("dpr25", tape!("dpr25")),
    ]
}

/// A fixture from a folder of frames (`OPENCIRCLE_LONG_DUMP`'s PNGs, in
/// name order): `OPENCIRCLE_REPLAY_IN=<folder> OPENCIRCLE_REPLAY_OUT=<path
/// without extension> cargo test record_replay -- --ignored`.
#[test]
#[ignore]
fn record_replay() {
    let dir = std::env::var("OPENCIRCLE_REPLAY_IN").expect("OPENCIRCLE_REPLAY_IN=<folder>");
    let out = std::env::var("OPENCIRCLE_REPLAY_OUT").expect("OPENCIRCLE_REPLAY_OUT=<path>");
    let mut paths: Vec<_> = (std::fs::read_dir(&dir).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    paths.sort();
    // every n-th frame only (a slower grab), for a long still stretch of a
    // spinner: `OPENCIRCLE_REPLAY_EVERY=n`
    let every = std::env::var("OPENCIRCLE_REPLAY_EVERY").map_or(1, |v| v.parse().unwrap());
    let paths: Vec<_> = paths.into_iter().step_by(every).collect();
    let (mut seen, mut dict) = (HashMap::<Vec<u8>, usize>::new(), Vec::<u8>::new());
    let (mut size, mut text) = (None, String::new());
    let mut prev: Option<Vec<u8>> = None;
    for path in &paths {
        let mut reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
                .read_info()
                .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let (w, h) = (info.width as usize, info.height as usize);
        assert_eq!(*size.get_or_insert((w, h)), (w, h));
        if prev.as_ref() == Some(&buf) {
            continue;
        }
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for row in buf.chunks_exact(w * 4) {
            let rgb: Vec<u8> = (row.as_chunks::<4>().0.iter())
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            let n = seen.len();
            let i = *seen.entry(rgb.clone()).or_insert_with(|| {
                dict.extend_from_slice(&rgb);
                n
            });
            match runs.last_mut() {
                Some((s, l)) if *s + *l == i => *l += 1,
                _ => runs.push((i, 1)),
            }
        }
        let line: Vec<String> = runs.iter().map(|(s, l)| format!("{s}+{l}")).collect();
        text.push_str(&line.join(" "));
        text.push('\n');
        prev = Some(buf);
    }
    let (w, h) = size.expect("no frames");
    let mut png_out = Vec::new();
    let mut enc = png::Encoder::new(&mut png_out, w as u32, seen.len() as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_compression(png::Compression::High);
    let mut writer = enc.write_header().unwrap();
    writer.write_image_data(&dict).unwrap();
    writer.finish().unwrap();
    std::fs::write(format!("{out}.png"), png_out).unwrap();
    std::fs::write(format!("{out}.txt"), format!("{w} {h}\n{text}")).unwrap();
    eprintln!(
        "{} frames, {} distinct rows",
        text.lines().count(),
        seen.len()
    );
}

/// Cost per frame on a big area (release: `cargo test --release stitch_cost -- --ignored --nocapture`).
#[test]
#[ignore]
fn stitch_cost() {
    use super::synth;
    let (w, vh) = (1720, 1440);
    let page = synth::readme(w, 12_000, 1.0, 2);
    let b = synth::Browser::new(page, vh, 60, 0, 0);
    let pos = synth::smooth(100.0, 60, 3, 0, b.max_scroll() + 50, 3);
    let frames: Vec<_> = pos.iter().map(|&s| b.frame(s)).collect();
    let mut st = Stitcher::new(w as u32, vh as u32);
    let (mut all, mut worst) = (Duration::ZERO, Duration::ZERO);
    for f in &frames {
        let t = Instant::now();
        st.push(&f.px);
        let e = t.elapsed();
        (all, worst) = (all + e, worst.max(e));
    }
    println!(
        "{} frames to {} px: {:.1} ms a frame, worst {:.1} ms",
        frames.len(),
        st.height(),
        all.as_secs_f64() * 1000.0 / frames.len() as f64,
        worst.as_secs_f64() * 1000.0
    );
}
