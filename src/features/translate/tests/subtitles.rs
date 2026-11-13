//! Big outlined subtitles (a film's, a game's): white or yellow, a dark
//! outline, over a dark noisy picture. Before: the erase found a sliver of
//! the glyphs, the Spanish stayed and the English came out at 8 px.

use super::*;

const W: usize = 1600;
const H: usize = 220;
const TR: &str = "This leaf has a dark past.";

fn dark() -> Rgba {
    paint(W, H, |x, y| {
        let n = noise(x, y, 6.0, 9);
        [18.0 + 50.0 * n, 12.0 + 35.0 * n, 20.0 + 45.0 * n]
    })
}

fn luma(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// The subtitle centred at `size` px; its OCR-like line box.
fn subtitle(img: &mut Rgba, size: f32, color: [u8; 3]) -> Line {
    let y = (H as f32 - size * 1.25) / 2.0;
    let mut l = text(
        img,
        (W, H),
        (W as f32 / 2.0, y, 0.5),
        "Esta hoja tiene un pasado oscuro.",
        size,
        500,
        color,
        Fx::Outline,
    );
    // the outline reaches 2 px past the glyphs
    l.bounds = [
        l.bounds[0] - 4.0,
        l.bounds[1],
        l.bounds[2] + 8.0,
        l.bounds[3],
    ];
    l
}

/// Pixels at least `min` bright inside `r`, and the rows they span.
fn bright(img: &[u8], r: [f32; 4], min: f32) -> (usize, Option<(usize, usize)>) {
    let (mut n, mut ys) = (0, None::<(usize, usize)>);
    for y in r[1].max(0.0) as usize..((r[1] + r[3]).max(0.0) as usize).min(H) {
        for x in r[0].max(0.0) as usize..((r[0] + r[2]).max(0.0) as usize).min(W) {
            if luma(&img[(y * W + x) * 4..][..3]) >= min {
                n += 1;
                ys = Some(ys.map_or((y, y), |(a, b)| (a.min(y), b.max(y))));
            }
        }
    }
    (n, ys)
}

/// (before, after, the line, what render did); `bar` = a player's progress
/// bar through the box, under the glyphs.
fn run(size: f32, color: [u8; 3], bar: bool) -> (Rgba, Rgba, Line, Drawn) {
    let mut img = dark();
    let line = subtitle(&mut img, size, color);
    if bar {
        let y = (line.bounds[1] + line.bounds[3] - 3.0) as usize;
        panel(&mut img, W, [20, y, W - 40, 4], [200, 200, 200], 1.0);
    }
    let blocks = group_blocks(std::slice::from_ref(&line));
    let mut out = img.clone();
    let d = render(&mut out, W as u32, H as u32, &blocks, &[TR.into()], false);
    (img, out, line, d)
}

#[test]
fn outlined_text_is_erased_and_redrawn_at_its_size() {
    for size in [40.0, 60.0, 80.0] {
        let (img, out, line, d) = run(size, [245, 245, 245], false);
        assert_eq!(d.drawn, 1, "{size}: {d:?}");
        // the original's glyphs and the redraw's, from their bright pixels
        let (_, Some((t0, b0))) = bright(&img, line.bounds, 200.0) else {
            panic!("no text drawn")
        };
        let (_, Some((t1, b1))) = bright(&out, d.ink[0], 200.0) else {
            panic!("{size}: nothing bright drawn")
        };
        let (h0, h1) = ((b0 - t0) as f32, (b1 - t1) as f32);
        assert!(
            h1 >= 0.85 * h0,
            "{size}: redraw {h1} px tall, original {h0}"
        );
        // nothing of the Spanish is left beside the English
        let [x, y, w, h] = line.bounds;
        let [ix, _, iw, _] = d.ink[0];
        let left = bright(&out, [x, y, ix - x, h], 120.0).0
            + bright(&out, [ix + iw, y, x + w - ix - iw, h], 120.0).0;
        assert_eq!(left, 0, "{size}: Spanish left beside the redraw");
        // the outline comes back: near-black pixels hug the white
        let all = bright(&out, d.ink[0], 0.0).0;
        let black = all - bright(&out, d.ink[0], 12.0).0;
        let lit = bright(&out, d.ink[0], 200.0).0;
        assert!(
            black * 4 > lit,
            "{size}: no outline ({black} black, {lit} lit)"
        );
    }
}

#[test]
fn a_yellow_subtitle_stays_yellow() {
    let (_, out, _, d) = run(60.0, [240, 214, 60], false);
    assert_eq!(d.drawn, 1, "{d:?}");
    let [x, y, w, h] = d.ink[0].map(|v| v as usize);
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for yy in y..(y + h).min(H) {
        for xx in x..(x + w).min(W) {
            let p = &out[(yy * W + xx) * 4..][..3];
            if p[0] > 180 && p[1] > 160 {
                (0..3).for_each(|k| sum[k] += p[k] as u64);
                n += 1;
            }
        }
    }
    let c = sum.map(|s| s / n.max(1));
    assert!(n > 500 && c[2] + 80 < c[1], "{n} pixels, mean {c:?}");
}

#[test]
fn a_bar_through_the_box_survives() {
    let (img, out, line, d) = run(60.0, [245, 245, 245], true);
    assert_eq!(d.drawn, 1, "{d:?}");
    let y = (line.bounds[1] + line.bounds[3] - 3.0) as usize + 1;
    let changed = (20..W - 20)
        .filter(|&x| {
            let i = (y * W + x) * 4;
            img[i..i + 3] != out[i..i + 3]
        })
        .count();
    assert!(changed * 50 < W, "the bar changed in {changed} columns");
}
