//! Frames of the 0.1.3 recordings (`tests/fixtures/translate`, sources and
//! attribution in its README), cut to the text, with the lines PP-OCR read
//! on the whole frame and fixed English (what offline MT gives back, by
//! hand): a film subtitle, a forest sign, a Wikipedia page (a game's
//! settings: `game.rs`, synthetic). Each render must match its golden (`<name>_expected.png`, looked at
//! by eye); `OC_BLESS=1` rewrites them.

use super::*;
use std::collections::HashMap;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/translate/");

fn image(name: &str) -> (Rgba, (usize, usize)) {
    let path = format!("{DIR}{name}.png");
    let f = crate::capture::from_png(path.as_ref()).unwrap();
    (f.rgba, (f.width as usize, f.height as usize))
}

/// The fixture's blocks and their English ("" = left as it is).
fn blocks(name: &str, (w, h): (usize, usize)) -> (Vec<Block>, Vec<String>) {
    let all: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/translate/lines.json")).unwrap();
    let lines: Vec<Line> = (all[name].as_array().unwrap().iter())
        .map(|l| crate::ocr::TextBlock {
            bounds: [0, 1, 2, 3].map(|k| l[k].as_f64().unwrap() as f32),
            text: l[4].as_str().unwrap().into(),
            confidence: 0.9,
        })
        .map(|b| super::super::pipeline::to_line(b, w as u32, h as u32))
        .collect();
    let tr: HashMap<String, HashMap<String, String>> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/translate/translations.json"
    ))
    .unwrap();
    let blocks = group_blocks(&lines);
    let t = (blocks.iter())
        .map(|b| tr[name].get(&b.text).cloned().unwrap_or_default())
        .collect();
    (blocks, t)
}

fn run(name: &str) -> (Rgba, Rgba, (usize, usize), Vec<Block>, Drawn) {
    let (img, dims) = image(name);
    let (blocks, tr) = blocks(name, dims);
    let mut out = img.clone();
    let d = render(&mut out, dims.0 as u32, dims.1 as u32, &blocks, &tr, false);
    (img, out, dims, blocks, d)
}

#[test]
fn every_case_matches_its_golden() {
    for name in ["subtitle", "sign", "web"] {
        let (_, out, dims, _, _) = run(name);
        if std::env::var_os("OC_BLESS").is_some() {
            save_png(format!("{DIR}{name}_expected.png").as_ref(), dims, &out);
            continue;
        }
        let (want, _) = image(&format!("{name}_expected"));
        let off = (out.chunks(4).zip(want.chunks(4)))
            .filter(|(a, b)| (0..3).any(|k| a[k].abs_diff(b[k]) > 24))
            .count();
        assert!(
            off * 1000 <= dims.0 * dims.1,
            "{name}: {off} pixels differ from the golden"
        );
    }
}

/// The Sintel subtitle: erased whole (no grey glyphs left), the English at
/// the subtitle's size, the progress bar under it untouched.
#[test]
fn the_film_subtitle_is_replaced_at_its_size() {
    let (img, out, (w, _), blocks, d) = run("subtitle");
    let i = (blocks.iter().position(|b| b.text.contains("pasado"))).unwrap();
    let [bx, by, bw, bh] = blocks[i].bounds;
    let [ix, _, iw, ih] = d.ink[i];
    // the line box is the 60 px glyphs padded (1.3): the redraw's line about as tall
    assert!(
        ih >= 0.85 * bh / 1.3,
        "redraw {:?}, line box {bh}",
        d.ink[i]
    );
    // no grey fill of the Spanish left beside the English
    let grey = |x: usize, y: usize| {
        let p = &out[(y * w + x) * 4..][..3];
        p[0] > 60 && p[0].abs_diff(p[2]) < 6 && p[0].abs_diff(p[1]) < 6
    };
    let left = (by as usize..(by + bh) as usize - 14)
        .flat_map(|y| {
            (bx as usize..ix as usize)
                .chain((ix + iw) as usize..(bx + bw) as usize)
                .map(move |x| (x, y))
        })
        .filter(|&(x, y)| grey(x, y))
        .count();
    assert!(left < 40, "{left} grey Spanish pixels left");
    // the progress bar (frame row 915, row 155 here) keeps its pixels, but
    // where the English's descenders reach down over it
    let changed = (0..w)
        .filter(|&x| (x as f32) < ix || x as f32 >= ix + iw)
        .filter(|&x| img[(155 * w + x) * 4..][..3] != out[(155 * w + x) * 4..][..3])
        .count();
    assert_eq!(changed, 0, "the progress bar changed");
}

/// The forest sign: each line one block (the detector split them at word
/// gaps), every line drawn, the three body lines at one size.
#[test]
fn the_sign_translates_whole_lines() {
    let (_, _, _, blocks, d) = run("sign");
    let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
    for want in [
        "REFUGIO DEL BOSQUE",
        "El sendero está abierto todo el año.",
        "Siga el río hasta el puente de piedra.",
        "Lleve agua y respete la naturaleza.",
        "Horario: de nueve a seis.",
    ] {
        assert!(texts.contains(&want), "{want:?} not one block: {texts:?}");
    }
    assert_eq!((d.busy, d.no_room), (0, 0), "{d:?}");
    let body: Vec<f32> = (blocks.iter().zip(&d.ink))
        .filter(|(b, _)| b.text.ends_with('.') && !b.text.starts_with("Horario"))
        .map(|(_, r)| r[3])
        .collect();
    let (lo, hi) = (body.iter()).fold((f32::MAX, 0f32), |(a, b), &h| (a.min(h), b.max(h)));
    assert!(body.len() == 3 && hi - lo <= 0.05 * hi, "{body:?}");
}

/// Wikipedia: a wrapped contents entry is one block ("Revoluciones de la" |
/// "Luna"), and the infobox title "Luna" is replaced whole.
#[test]
fn web_entries_join_and_titles_are_whole() {
    let (_, _, _, blocks, d) = run("web");
    assert!(blocks.iter().any(|b| b.text == "Revoluciones de la Luna"));
    let title = (blocks.iter().zip(&d.ink))
        .filter(|(b, _)| b.text == "Luna")
        .map(|(_, r)| r[2])
        .fold(0f32, f32::max);
    assert!(title > 60.0, "the title's redraw is {title} px wide");
}
