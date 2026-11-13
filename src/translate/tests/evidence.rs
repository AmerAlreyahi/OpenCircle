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
    let f = crate::shared::platform::capture::from_png(path.as_ref()).unwrap();
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
    for name in ["subtitle", "subtitle2", "sign", "web", "player"] {
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

/// The Sintel subtitle of the second recording: white, a thick black
/// outline, straight on the dark film. Before: the fill was erased but the
/// outline stayed, and the hole filled from it: black Spanish silhouettes
/// around the English. Now no near-black pixels are left beside the redraw
/// (the film there is no darker than ~10), and the English is outlined.
#[test]
fn the_outline_goes_with_the_text() {
    let (img, out, (w, _), blocks, d) = run("subtitle2");
    let i = (blocks.iter().position(|b| b.text.contains("pasado"))).unwrap();
    let [bx, by, bw, bh] = blocks[i].bounds.map(|v| v as usize);
    let [ix, _, iw, _] = d.ink[i].map(|v| v as usize);
    assert!(iw > 0, "{d:?}");
    let black = |im: &[u8], xs: std::ops::Range<usize>| {
        (by..by + bh)
            .flat_map(|y| xs.clone().map(move |x| (y * w + x) * 4))
            .filter(|&k| im[k] < 6 && im[k + 1] < 6 && im[k + 2] < 6)
            .count()
    };
    let beside = |im: &[u8]| black(im, bx..ix) + black(im, ix + iw..bx + bw);
    let (before, after) = (beside(&img), beside(&out));
    assert!(
        after * 20 < before,
        "black outline pixels beside the English: {before} -> {after}"
    );
    assert!(black(&out, ix..ix + iw) > 500, "the English has no outline");
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

/// A paused video in a browser page (our own player-and-page layout around
/// the source Sintel frame, `player.png`): white subtitle with a soft dark
/// edge, the page's English all around. "auto" finds the Spanish line
/// among the English (before: one guess for the whole screen said English
/// and "the text is already in English"), only it is translated, and no
/// dark edge of the Spanish is left beside the English.
#[test]
fn a_subtitle_in_an_english_player() {
    use super::super::pipeline::{Deps, Real, Via};
    let (img, (w, h)) = image("player");
    let (blocks, en) = blocks("player", (w, h));
    let texts: Vec<String> = blocks.iter().map(|b| b.text.clone()).collect();
    let real = Real::new(lang::AUTO, Via::Offline);
    real.read(&texts.iter().map(String::as_str).collect::<Vec<_>>());
    let sub = (texts.iter().position(|t| t.contains("sangre"))).unwrap();
    let routes = real.routes(&texts, "en").unwrap();
    let got: Vec<_> = (routes.iter())
        .map(|(r, i)| (r[0].source, i.clone()))
        .collect();
    assert_eq!(got, [("es", vec![sub])], "{texts:?}");
    let tr: Vec<String> = (0..blocks.len())
        .map(|i| {
            if i == sub {
                en[i].clone()
            } else {
                String::new()
            }
        })
        .collect();
    let mut out = img.clone();
    let d = render(&mut out, w as u32, h as u32, &blocks, &tr, false);
    let [bx, by, bw, bh] = blocks[sub].bounds.map(|v| v as usize);
    let [ix, _, iw, _] = d.ink[sub].map(|v| v as usize);
    assert!(iw > 0, "{d:?}");
    // the Spanish's black edge (the film here is ~10): black pixels within 3 px
    // of its white glyphs
    let lum = |im: &[u8], x: usize, y: usize| {
        let k = (y * w + x) * 4;
        (im[k] as u32 + im[k + 1] as u32 + im[k + 2] as u32) / 3
    };
    let edge: Vec<(usize, usize)> = (by + 3..by + bh - 3)
        .flat_map(|y| {
            (bx + 3..ix)
                .chain(ix + iw..bx + bw - 3)
                .map(move |x| (x, y))
        })
        .filter(|&(x, y)| lum(&img, x, y) < 4)
        .filter(|&(x, y)| (y - 3..=y + 3).any(|v| (x - 3..=x + 3).any(|u| lum(&img, u, v) > 200)))
        .collect();
    let left = (edge.iter()).filter(|&&(x, y)| lum(&out, x, y) < 4).count();
    assert!(
        edge.len() > 300 && left * 10 < edge.len(),
        "dark Spanish edge pixels beside the English: {} -> {left}",
        edge.len()
    );
    // no white Spanish glyph left beside it either
    let white = (by..by + bh)
        .flat_map(|y| {
            (bx..ix)
                .chain(ix + iw..bx + bw)
                .map(move |x| (y * w + x) * 4)
        })
        .filter(|&k| out[k] > 200 && out[k + 1] > 200 && out[k + 2] > 200)
        .count();
    assert!(white < 20, "{white} white Spanish pixels left");
    // the page's English is untouched outside the subtitle's box
    let changed = (0..w * h)
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            !(bx..bx + bw).contains(&x) || !(by..by + bh).contains(&y)
        })
        .filter(|&i| img[i * 4..][..3] != out[i * 4..][..3])
        .count();
    assert!(
        changed < 200,
        "{changed} pixels changed outside the subtitle"
    );
}
