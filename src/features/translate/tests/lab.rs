//! Render lab (W22, #45): today's erase + redraw on the 13 game screens with
//! recorded reads and translations (`OC_LAB_ROWS`: dir of `c_<screen>.ocr.json`
//! and `c_<screen>-<target>.json`), one PNG per screen and target in `OC_LAB_OUT`.
//! `OC_LAB_IMG=.. OC_LAB_ROWS=.. OC_LAB_OUT=.. cargo test lab_render -- --ignored --nocapture`
//! Then the tests the lab's findings became.

use super::super::pipeline::to_line;
use super::super::*;
use crate::features::text_reading::TextBlock;
use std::path::PathBuf;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/translate/");

fn key(s: &str) -> String {
    (s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>())
    .to_lowercase()
}

#[test]
#[ignore]
fn lab_render() {
    let rows = PathBuf::from(std::env::var("OC_LAB_ROWS").unwrap());
    let out = PathBuf::from(std::env::var("OC_LAB_OUT").unwrap());
    // the screens at the size the reads were made on (1920x1080 `raw_<screen>.png`)
    let games = PathBuf::from(std::env::var("OC_LAB_IMG").unwrap());
    for name in screens_in(&rows) {
        if std::env::var("OC_LAB_ONLY").is_ok_and(|o| o != name) {
            continue;
        }
        let frame = std::fs::File::open(games.join(format!("raw_{name}.png"))).unwrap();
        let frame =
            crate::shared::platform::capture::decode_png(std::io::BufReader::new(frame)).unwrap();
        let (w, h) = (frame.width, frame.height);
        let img = frame.rgba;
        let ocr: Vec<TextBlock> = serde_json::from_slice(
            &std::fs::read(rows.join(format!("c_{name}.ocr.json"))).unwrap(),
        )
        .unwrap();
        let lines: Vec<_> = ocr.into_iter().map(|b| to_line(b, w, h)).collect();
        let blocks = group_blocks(&lines);
        for target in ["ar", "fr", "en"] {
            let Ok(bytes) = std::fs::read(rows.join(format!("c_{name}-{target}.json"))) else {
                continue;
            };
            let rs: Vec<([i32; 4], String, String, [f32; 4])> =
                serde_json::from_slice(&bytes).unwrap();
            let tr: Vec<String> = blocks
                .iter()
                .map(|b| {
                    (rs.iter().find(|r| key(&r.1) == key(&b.text)))
                        .map(|r| r.2.clone())
                        .unwrap_or_default()
                })
                .collect();
            let mut px = img.clone();
            let d = render(&mut px, w, h, &blocks, &tr, target == "ar");
            let todo = tr.iter().filter(|t| !t.trim().is_empty()).count();
            eprintln!(
                "{name}-{target}: {} blocks, {todo} to draw, drawn {}, busy {}, no_room {}",
                blocks.len(),
                d.drawn,
                d.busy,
                d.no_room
            );
            let path = out.join(format!("{name}-{target}.png"));
            super::save_png(&path, (w as usize, h as usize), &px);
        }
    }
}

fn screens_in(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let n = e.ok()?.file_name().into_string().ok()?;
            Some(n.strip_suffix(".ocr.json")?.strip_prefix("c_")?.to_string())
        })
        .collect();
    v.sort();
    v
}

/// The novel screen's three choice buttons (`novel_buttons.png`, a cut of the
/// 1920x1080 frame at 750,440), the French read as the app reads it, rendered
/// into `tr`. `light`: the colours inverted (a light theme's buttons).
fn novel_buttons(tr: [&str; 3], light: bool) -> (Vec<u8>, (usize, usize), Drawn) {
    let path = format!("{DIR}novel_buttons.png");
    let f = crate::shared::platform::capture::from_png(path.as_ref()).unwrap();
    let (w, h) = (f.width, f.height);
    let read = |t: &str, b: [f32; 4]| TextBlock {
        text: t.into(),
        bounds: [b[0] - 750.0, b[1] - 440.0, b[2], b[3]],
        confidence: 1.0,
    };
    let lines: Vec<_> = [
        read("Bien sür que oui.", [809.0, 455.0, 300.0, 42.0]),
        read("Je n'en suis pas sür...", [774.0, 548.0, 368.0, 41.0]),
        read("On devrait partir.", [807.0, 655.0, 305.0, 40.0]),
    ]
    .into_iter()
    .map(|b| to_line(b, w, h))
    .collect();
    let blocks = group_blocks(&lines);
    let tr = tr.map(String::from);
    let mut px = f.rgba;
    if light {
        for p in px.chunks_mut(4) {
            (0..3).for_each(|k| p[k] = 255 - p[k]);
        }
    }
    let d = render(&mut px, w, h, &blocks, &tr, true);
    (px, (w as usize, h as usize), d)
}

/// One size on screen, so one size drawn. Before: "Je n'en suis pas sür..."
/// (few round letters, a thin dense band) was drawn at 0.55 of the size of
/// "Bien sür que oui." (the width of the same word: 78 / 43 / 64 px).
#[test]
fn buttons_of_one_size_get_translations_of_one_size() {
    let (_, _, d) = novel_buttons(["بالطبع"; 3], false);
    assert_eq!(d.drawn, 3);
    let widths: Vec<f32> = d.ink.iter().map(|r| r[2]).collect();
    let lo = widths.iter().cloned().fold(f32::MAX, f32::min);
    let hi = widths.iter().cloned().fold(0.0, f32::max);
    assert!(
        hi / lo < 1.25,
        "one size on screen, so one drawn: {widths:?}"
    );
}

/// Arabic on Latin source buttons, dark and light: each render matches its
/// golden (`novel_buttons_{dark,light}_ar_expected.png`, looked at by eye;
/// `OC_BLESS=1` rewrites them), so shaping, right-to-left order, alignment and
/// the erase cannot regress unseen.
#[test]
fn arabic_on_latin_buttons_matches_its_golden_in_both_themes() {
    let tr = ["بالطبع", "أنا لستُ متأكدًا من ذلك", "يجب أن نذهب"];
    for (name, light) in [("dark", false), ("light", true)] {
        let (out, dims, d) = novel_buttons(tr, light);
        assert_eq!(d.drawn, 3, "{name}");
        let path = format!("{DIR}novel_buttons_{name}_ar_expected.png");
        if std::env::var_os("OC_BLESS").is_some() {
            super::save_png(path.as_ref(), dims, &out);
            continue;
        }
        let want = crate::shared::platform::capture::from_png(path.as_ref())
            .unwrap()
            .rgba;
        let off = (out.chunks(4).zip(want.chunks(4)))
            .filter(|(a, b)| (0..3).any(|k| a[k].abs_diff(b[k]) > 24))
            .count();
        assert!(
            off * 1000 <= dims.0 * dims.1,
            "{name}: {off} pixels differ from the golden"
        );
    }
}

/// The button frames are not text: erasing a label leaves the frame lines
/// whole, also where a translucent button's faint inner line passes the
/// threshold only in pieces (they were erased as dashes).
#[test]
fn a_buttons_broken_frame_line_stays_whole() {
    let src =
        crate::shared::platform::capture::from_png(format!("{DIR}novel_buttons.png").as_ref())
            .unwrap();
    let (out, (w, _), _) = novel_buttons(["بالطبع"; 3], false);
    let changed = (60..=70usize)
        .flat_map(|y| (0..w).map(move |x| (y * w + x) * 4))
        .filter(|&i| (0..3).any(|k| out[i + k].abs_diff(src.rgba[i + k]) > 8))
        .count();
    // (375 pixels changed before, 92 now: the rest are anti-aliasing fringes)
    assert!(changed < 120, "{changed} frame pixels erased");
}

/// A dark translucent panel's own shade beside the glyphs is no outline:
/// "Bien sûr que oui." has none to redraw (drawn as one it greyed thin Arabic
/// strokes).
#[test]
fn a_dark_panels_own_shade_is_no_outline() {
    let src =
        crate::shared::platform::capture::from_png(format!("{DIR}novel_buttons.png").as_ref())
            .unwrap();
    let mut px = src.rgba;
    let b = [59.0, 15.0, 300.0, 42.0];
    let (w, h) = (src.width as usize, src.height as usize);
    let (st, _) = super::super::erase::erase_line(&mut px, w, h, b);
    assert_eq!(st.shadow, None);
}

/// A one-line button's translation stays on one line: shrunk, then cut with
/// an ellipsis, never wrapped into a second line below its frame.
#[test]
fn a_one_line_label_never_wraps() {
    let long = "أنا لستُ متأكدًا من ذلك أبدًا ولا أعرف ماذا يجب أن أفعل الآن في هذا الموقف";
    let (_, _, d) = novel_buttons([long, "بالطبع", "بالطبع"], false);
    assert!(d.drawn >= 1);
    assert!(d.ink[0][3] < 1.6 * 42.0, "wrapped: {:?}", d.ink[0]);
}
