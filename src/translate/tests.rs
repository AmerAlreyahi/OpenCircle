//! Synthetic game panels with a text-free ground truth: erase error, block
//! grouping, render bounds, and the full-frame budget (ignored bench).

use super::draw::{FONTS, parse_marked};
use super::*;
use cosmic_text::Align;

type Rgba = Vec<u8>;
const WHITE: [u8; 3] = [242, 238, 228];
const GOLD: [u8; 3] = [232, 190, 92];

#[derive(Clone, Copy, PartialEq)]
enum Fx {
    None,
    Shadow,
    Outline,
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed;
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Multi-octave value noise in 0..1.
fn noise(x: f32, y: f32, scale: f32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut f, mut norm) = (0.0, 1.0, 1.0 / scale, 0.0);
    for o in 0..4 {
        let (fx, fy) = (x * f, y * f);
        let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
        let s = seed + o;
        let a = hash(ix, iy, s) + (hash(ix + 1, iy, s) - hash(ix, iy, s)) * tx;
        let b = hash(ix, iy + 1, s) + (hash(ix + 1, iy + 1, s) - hash(ix, iy + 1, s)) * tx;
        sum += (a + (b - a) * ty) * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// Paints `f(x, y)` with 2x2 supersampling, so edges are anti-aliased like a game's.
fn paint(w: usize, h: usize, f: impl Fn(f32, f32) -> [f32; 3]) -> Rgba {
    let mut img = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut c = [0f32; 3];
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let s = f(x as f32 + sx, y as f32 + sy);
                (0..3).for_each(|k| c[k] += s[k] / 4.0);
            }
            let i = (y * w + x) * 4;
            for k in 0..3 {
                img[i + k] = c[k].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    img
}

/// Sky, mountains, ground, dark tree silhouettes and grain: `h` = scene height.
fn art(x: f32, y: f32, h: f32) -> [f32; 3] {
    let horizon = h * 0.55 - h * 0.25 * noise(x, 0.0, 200.0, 3);
    let ground = h * 0.75;
    let t = y / h;
    let mut c = if y < horizon {
        [110.0 + 135.0 * t, 170.0 + 45.0 * t, 235.0 - 65.0 * t]
    } else if y < ground {
        [70.0, 110.0, 80.0]
    } else {
        [60.0, 105.0, 55.0]
    };
    let i = (x / 45.0).floor() as i32;
    for k in i - 1..=i + 1 {
        let cx = k as f32 * 45.0 + 15.0 * hash(k, 1, 7);
        let (th, base) = (h * (0.35 + 0.3 * hash(k, 2, 7)), ground + 4.0);
        if y < base && y > base - th && (x - cx).abs() < 18.0 * (y - base + th) / th {
            c = [30.0, 60.0, 40.0];
        }
    }
    if y > horizon {
        let g = (noise(x, y, 8.0, 5) - 0.5) * 20.0;
        c = c.map(|v| v + g);
    }
    c
}

/// Alpha-blends `col` over the rect.
fn panel(img: &mut Rgba, w: usize, r: [usize; 4], col: [u8; 3], alpha: f32) {
    for y in r[1]..r[1] + r[3] {
        for x in r[0]..r[0] + r[2] {
            let i = (y * w + x) * 4;
            for k in 0..3 {
                img[i + k] =
                    (img[i + k] as f32 * (1.0 - alpha) + col[k] as f32 * alpha).round() as u8;
            }
        }
    }
}

/// Draws English text (`[[...]]` in gold); `anchor` 0 = left, 0.5 = centre.
/// Returns the OCR-like line box.
#[allow(clippy::too_many_arguments)]
fn text(
    img: &mut Rgba,
    dims: (usize, usize),
    (x, y, anchor): (f32, f32, f32),
    s: &str,
    size: f32,
    weight: u16,
    color: [u8; 3],
    fx: Fx,
) -> Line {
    let mut f = FONTS.lock().unwrap();
    let spans = parse_marked(s, color, GOLD);
    let m = cosmic_text::Metrics::new(size, size * 1.25);
    let buf = f.layout(&spans, false, m, weight, None, Align::Left);
    let (w, h) = draw::extent(&buf);
    let x = x - w * anchor;
    let (xi, yi) = (x.round() as i32, y.round() as i32);
    match fx {
        Fx::Shadow => f.draw(&buf, img, dims, (xi + 2, yi + 2), Some([0, 0, 0, 200])),
        Fx::Outline => {
            for (dx, dy) in [
                (-2, 0),
                (2, 0),
                (0, -2),
                (0, 2),
                (-1, -1),
                (1, 1),
                (-1, 1),
                (1, -1),
            ] {
                f.draw(&buf, img, dims, (xi + dx, yi + dy), Some([0, 0, 0, 255]));
            }
        }
        Fx::None => {}
    }
    f.draw(&buf, img, dims, (xi, yi), None);
    Line {
        bounds: [x, y, w, h],
        text: s.replace("[[", "").replace("]]", ""),
    }
}

struct Case {
    name: &'static str,
    dims: (usize, usize),
    gt: Rgba,
    img: Rgba,
    lines: Vec<Line>,
}

type Spec<'a> = (f32, f32, &'a str, f32, u16, [u8; 3], Fx);

fn case(name: &'static str, gt: Rgba, dims: (usize, usize), texts: &[Spec]) -> Case {
    let mut img = gt.clone();
    let lines = texts
        .iter()
        .map(|&(x, y, s, size, wt, col, fx)| {
            text(&mut img, dims, (x, y, 0.0), s, size, wt, col, fx)
        })
        .collect();
    Case {
        name,
        dims,
        gt,
        img,
        lines,
    }
}

/// The four background kinds the erase must handle.
fn cases() -> Vec<Case> {
    let (w, h) = (640, 160);
    // two flat panels split by a 1 px gold divider at x = 330
    let flat = paint(w, h, |x, _| match x {
        x if (330.0..331.0).contains(&x) => [200.0, 160.0, 80.0],
        x if x < 330.0 => [24.0, 28.0, 40.0],
        _ => [48.0, 36.0, 30.0],
    });
    let gradient = paint(w, h, |x, y| {
        [40.0 + 0.15 * x, 70.0 - 0.05 * x + 0.3 * y, 120.0 + 0.2 * y]
    });
    let textured = paint(w, h, |x, y| {
        let n = (noise(x, y, 40.0, 5) - 0.5) * 50.0 + (hash(x as i32, y as i32, 9) - 0.5) * 12.0;
        [52.0 + n, 46.0 + n, 42.0 + n]
    });
    let busy = paint(w, h, |x, y| art(x, y, h as f32));
    let mut translucent = busy.clone();
    panel(&mut translucent, w, [0, 30, w, h - 30], [8, 10, 20], 0.7);
    let line = "Speak to the blacksmith near the gate";
    vec![
        case(
            "flat",
            flat,
            (w, h),
            &[(40.0, 60.0, line, 28.0, 500, WHITE, Fx::None)],
        ),
        case(
            "gradient",
            gradient,
            (w, h),
            &[(40.0, 60.0, line, 28.0, 500, WHITE, Fx::Shadow)],
        ),
        case(
            "textured",
            textured,
            (w, h),
            &[(40.0, 60.0, line, 28.0, 500, WHITE, Fx::Shadow)],
        ),
        case(
            "translucent_art",
            translucent,
            (w, h),
            &[
                (40.0, 45.0, "ELDRIN", 34.0, 700, GOLD, Fx::None),
                (
                    40.0,
                    95.0,
                    "Bring the [[Sunstone]] to the old shrine",
                    28.0,
                    500,
                    WHITE,
                    Fx::None,
                ),
            ],
        ),
        // subtitle with a black outline straight over high-contrast art
        case(
            "outline_art",
            busy,
            (w, h),
            &[(40.0, 60.0, line, 30.0, 500, WHITE, Fx::Outline)],
        ),
    ]
}

/// Error vs the ground truth (0-255 per channel) over the text footprint
/// (pixels the text changed) and the padded line boxes:
/// (footprint MAE, footprint smudge, box MAE). Smudge = the error after a
/// 5x5 mean, so zero-mean grain cancels and blotches (a smeared edge, a dark
/// patch) remain: the part of the error the eye reads as a smudge.
fn errors(c: &Case, out: &Rgba) -> (f32, f32, f32) {
    let (w, h) = c.dims;
    let err = |i: usize, k: usize| out[i * 4 + k] as f32 - c.gt[i * 4 + k] as f32;
    let (mut fp, mut sm, mut nfp, mut bx, mut nbx) = (0.0, 0.0, 0, 0.0, 0);
    for l in &c.lines {
        let [x, y, lw, lh] = l.bounds.map(|v| v as usize);
        for py in y.saturating_sub(3)..(y + lh + 3).min(h) {
            for px in x.saturating_sub(3)..(x + lw + 3).min(w) {
                let i = py * w + px;
                let e = (0..3).map(|k| err(i, k).abs()).sum::<f32>() / 3.0;
                bx += e;
                nbx += 1;
                if (0..3).all(|k| c.img[i * 4 + k].abs_diff(c.gt[i * 4 + k]) <= 2) {
                    continue;
                }
                let (mut acc, mut n) = ([0f32; 3], 0.0);
                for qy in py.saturating_sub(2)..(py + 3).min(h) {
                    for qx in px.saturating_sub(2)..(px + 3).min(w) {
                        (0..3).for_each(|k| acc[k] += err(qy * w + qx, k));
                        n += 1.0;
                    }
                }
                fp += e;
                sm += acc.iter().map(|a| (a / n).abs()).sum::<f32>() / 3.0;
                nfp += 1;
            }
        }
    }
    let nfp = nfp.max(1) as f32;
    (fp / nfp, sm / nfp, bx / nbx.max(1) as f32)
}

fn erased(c: &Case) -> Rgba {
    let mut out = c.img.clone();
    for l in &c.lines {
        erase::erase_line(&mut out, c.dims.0, c.dims.1, l.bounds);
    }
    out
}

/// What a box over the text would give: each line box filled with the mean
/// colour just outside it.
fn box_filled(c: &Case) -> Rgba {
    let mut out = c.img.clone();
    let w = c.dims.0;
    for l in &c.lines {
        let [x, y, lw, lh] = l.bounds.map(|v| v as usize);
        let (x0, y0, x1, y1) = (x - 4, y - 4, x + lw + 4, y + lh + 4);
        let ring: Vec<usize> = (y0..y1)
            .flat_map(|py| (x0..x1).map(move |px| (px, py)))
            .filter(|&(px, py)| px == x0 || py == y0 || px == x1 - 1 || py == y1 - 1)
            .map(|(px, py)| (py * w + px) * 4)
            .collect();
        let mean = [0, 1, 2].map(|k| {
            (ring.iter().map(|&i| out[i + k] as u32).sum::<u32>() / ring.len() as u32) as u8
        });
        for py in y0 + 1..y1 - 1 {
            for px in x0 + 1..x1 - 1 {
                out[(py * w + px) * 4..][..3].copy_from_slice(&mean);
            }
        }
    }
    out
}

/// Erase error per background kind, limits as (footprint MAE, smudge, box
/// MAE), measured (this fill / the spike's fill / a box fill):
/// - flat, gradient: linear, so the fill must be near exact (0.0 / 0.5 / 7-9).
/// - textured: grain inside a hole cannot be predicted; the limit is about
///   the grain's deviation (3.4 / 3.4 / 5.6), below a box fill's smudge.
/// - translucent_art (the spike's weak spot: trees behind a translucent
///   panel): edges must continue, not smear (smudge 2.8 / 3.9 / 9.1); the
///   limit fails the spike's level.
/// - outline_art (hardest: an outlined subtitle straight over jagged,
///   high-contrast trees, 10-14 px holes): (24 / 39 / 48); the limit fails
///   the spike's level and a box fill. Learned inpainting is the upgrade.
#[test]
fn erase_error_on_synthetic_panels() {
    let limits = [
        ("flat", 1.5, 1.0, 0.5),
        ("gradient", 1.5, 1.0, 0.5),
        ("textured", 4.5, 2.5, 2.5),
        ("translucent_art", 4.0, 3.3, 1.5),
        ("outline_art", 30.0, 25.0, 15.0),
    ];
    for (c, (name, lim_fp, lim_sm, lim_box)) in cases().iter().zip(limits) {
        assert_eq!(c.name, name);
        let (fp, sm, bx) = errors(c, &erased(c));
        let (bfp, bsm, bbx) = errors(c, &box_filled(c));
        eprintln!("{name}: erase {fp:.2} / {sm:.2} / {bx:.2}, box {bfp:.2} / {bsm:.2} / {bbx:.2}");
        let ok = fp <= lim_fp && sm <= lim_sm && bx <= lim_box;
        assert!(ok, "{name}: erase error {fp:.2} / {sm:.2} / {bx:.2}");
        let caught = bfp > lim_fp || bsm > lim_sm || bbx > lim_box;
        assert!(caught, "{name}: limits would not catch a box fill");
    }
}

#[test]
fn erase_keeps_a_frame_line_and_reads_the_style() {
    let cs = cases();
    let read =
        |c: &Case, l: usize| erase::erase_line(&mut c.img.clone(), 640, 160, c.lines[l].bounds).0;
    let out = erased(&cs[0]);
    // the gold divider at x = 330 runs through the line and must survive
    for y in 50..110 {
        let i = (y * 640 + 330) * 4;
        assert!(
            out[i] > 170 && out[i + 2] < 110,
            "divider lost at y={y}: {:?}",
            &out[i..i + 3]
        );
    }
    let s = read(&cs[0], 0);
    assert!(
        s.color.iter().all(|&v| v > 220),
        "fill colour {:?}",
        s.color
    );
    assert!(
        s.shadow.is_none() && s.accent.is_none() && s.ink[3] - s.ink[1] > 18,
        "{s:?}"
    );
    assert!(
        read(&cs[1], 0).shadow.is_some_and(|(_, offset)| offset),
        "drop shadow"
    );
    assert!(read(&cs[3], 1).accent.is_some(), "Sunstone is an accent");
    assert!(
        read(&cs[3], 0).shadow.is_none(),
        "trees behind the name are not its shadow"
    );
    assert!(
        read(&cs[4], 0)
            .shadow
            .is_some_and(|(c, offset)| !offset && c[0] < 40),
        "outline"
    );
}

fn line(text: &str, x: f32, y: f32, w: f32, h: f32) -> Line {
    Line {
        bounds: [x, y, w, h],
        text: text.into(),
    }
}

#[test]
fn wrapped_paragraphs_join_list_items_do_not() {
    let l = [
        line(
            "Bring the Sunstone to the old shrine before nightfall, and",
            0.0,
            0.0,
            700.0,
            36.0,
        ),
        line("do not stray from the path.", 0.0, 44.0, 340.0, 36.0),
        line("Find the old map", 0.0, 120.0, 200.0, 26.0),
        line("Speak to the blacksmith", 0.0, 160.0, 280.0, 26.0),
        // centred two-line subtitle
        line(
            "I never thought I would see this place",
            700.0,
            400.0,
            520.0,
            40.0,
        ),
        line("again after all these years.", 780.0, 444.0, 360.0, 40.0),
    ];
    let b = group_blocks(&l);
    let sizes: Vec<usize> = b.iter().map(|b| b.lines.len()).collect();
    assert_eq!(sizes, vec![2, 1, 1, 2]);
    assert_eq!(
        b[0].text,
        "Bring the Sunstone to the old shrine before nightfall, and do not stray from the path."
    );
    assert_eq!(b[0].bounds, [0.0, 0.0, 700.0, 80.0]);
    assert!(centred(&b[3], 1920.0) && !centred(&b[0], 1920.0));
}

#[test]
fn render_replaces_text_and_stays_inside_its_blocks() {
    let c = &cases()[3];
    let blocks = group_blocks(&c.lines);
    let mut out = c.img.clone();
    let tr = [
        "إلدرين".to_string(),
        "أحضر [[حجر الشمس]] إلى المعبد القديم".to_string(),
    ];
    render(&mut out, 640, 160, &blocks, &tr, true);
    let w = c.dims.0;
    // both blocks share one left column, so RTL text aligns to the column's
    // right edge: changes stay in the block rows, between the column's left
    // edge minus one em of growth and its right edge
    let right = blocks
        .iter()
        .map(|b| b.bounds[0] + b.bounds[2])
        .fold(0.0, f32::max);
    let inside = |px: usize, py: usize, pad: f32| {
        let (px, py) = (px as f32, py as f32);
        blocks.iter().any(|b| {
            let [x, y, _, bh] = b.bounds;
            px >= x - 36.0 - pad && px < right + pad && py >= y - pad && py < y + bh + pad
        })
    };
    let mut gold = 0;
    for (i, (a, b)) in out.chunks(4).zip(c.img.chunks(4)).enumerate() {
        let (px, py) = (i % w, i / w);
        if a != b {
            assert!(
                inside(px, py, 4.0),
                "changed outside the blocks at {px},{py}"
            );
        }
        if inside(px, py, 0.0) && a[0] > 200 && a[1] > 160 && a[2] < 120 {
            gold += 1;
        }
    }
    assert!(gold > 100, "accent words drawn in gold: {gold}");
    // nothing drawn for a block without a translation
    let mut same = c.img.clone();
    render(&mut same, 640, 160, &blocks, &[String::new()], true);
    assert!(same == c.img);
}

pub(super) fn save_png(path: &std::path::Path, (w, h): (usize, usize), rgba: &[u8]) {
    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut e = png::Encoder::new(f, w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// 1920x1080 frame, 20 lines: erase + redraw time, and before/after PNGs in
/// the temp dir. Run: cargo test --release bench_full_frame -- --ignored --nocapture
#[test]
#[ignore]
fn bench_full_frame() {
    let dir = std::env::temp_dir().join("opencircle-translate");
    std::fs::create_dir_all(&dir).unwrap();
    for c in cases() {
        let out = erased(&c);
        for (tag, img) in [("truth", &c.gt), ("before", &c.img), ("erased", &out)] {
            save_png(&dir.join(format!("{}_{tag}.png", c.name)), c.dims, img);
        }
    }

    let dims = (1920, 1080);
    let (img, lines) = game_frame();
    let blocks = group_blocks(&lines);
    let tr: Vec<String> = blocks
        .iter()
        .map(|b| ar(&b.text).unwrap().to_string())
        .collect();
    let mut warm = img.clone();
    render(&mut warm, 1920, 1080, &blocks, &tr, true); // font load + glyph cache
    let (mut times, mut erase_ms) = (Vec::new(), Vec::new());
    let mut out = img.clone();
    for _ in 0..5 {
        let mut e = img.clone();
        let t0 = std::time::Instant::now();
        for l in &lines {
            erase::erase_line(&mut e, 1920, 1080, l.bounds);
        }
        erase_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        out = img.clone();
        let t0 = std::time::Instant::now();
        render(&mut out, 1920, 1080, &blocks, &tr, true);
        times.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    erase_ms.sort_by(f64::total_cmp);
    eprintln!(
        "{} lines, {} blocks: render {:.1} ms median (min {:.1}), of which erase {:.1} ms",
        lines.len(),
        blocks.len(),
        times[2],
        times[0],
        erase_ms[2]
    );
    save_png(&dir.join("frame_before.png"), dims, &img);
    save_png(&dir.join("frame_after.png"), dims, &out);
    eprintln!("PNGs in {}", dir.display());
    assert!(times[2] < 50.0, "budget: {:.1} ms", times[2]);
}

/// A synthetic 1920x1080 game frame (dialogue box, quest list, menu, prompt,
/// outlined subtitle, settings rows) and its OCR-like lines.
pub(crate) fn game_frame() -> (Rgba, Vec<Line>) {
    let dims = (1920, 1080);
    let mut img = paint(dims.0, dims.1, |x, y| art(x, y, 1080.0));
    panel(&mut img, 1920, [160, 790, 1600, 240], [8, 10, 20], 0.7);
    panel(&mut img, 1920, [40, 200, 360, 420], [20, 22, 30], 0.92);
    for x in 0..540 {
        panel(
            &mut img,
            1920,
            [1380 + x, 120, 1, 220],
            [0, 0, 0],
            0.6 * x as f32 / 540.0,
        );
    }
    panel(&mut img, 1920, [1300, 420, 560, 140], [30, 20, 40], 0.85);
    let mut lines = Vec::new();
    let mut t = |at: (f32, f32, f32), s: &str, size: f32, wt: u16, col: [u8; 3], fx: Fx| {
        lines.push(text(&mut img, dims, at, s, size, wt, col, fx));
    };
    t((205.0, 808.0, 0.0), "ELDRIN", 34.0, 700, GOLD, Fx::None);
    t(
        (205.0, 868.0, 0.0),
        "Bring the [[Sunstone]] to the old shrine before nightfall, and",
        30.0,
        500,
        WHITE,
        Fx::None,
    );
    t(
        (205.0, 912.0, 0.0),
        "do not stray from the path. The forest is not safe after dark.",
        30.0,
        500,
        WHITE,
        Fx::None,
    );
    t(
        (1500.0, 140.0, 0.0),
        "THE LOST CROWN",
        28.0,
        700,
        GOLD,
        Fx::Shadow,
    );
    for (k, s) in [
        "Find the old map",
        "Speak to the blacksmith",
        "Reach the northern gate",
    ]
    .iter()
    .enumerate()
    {
        t(
            (1500.0, 196.0 + 40.0 * k as f32, 0.0),
            s,
            22.0,
            500,
            WHITE,
            Fx::Shadow,
        );
    }
    for (k, s) in [
        "Continue",
        "New Game",
        "Load Game",
        "Settings",
        "Credits",
        "Quit",
    ]
    .iter()
    .enumerate()
    {
        t(
            (80.0, 230.0 + 60.0 * k as f32, 0.0),
            s,
            26.0,
            500,
            WHITE,
            Fx::None,
        );
    }
    t(
        (960.0, 600.0, 0.5),
        "Press E to interact",
        28.0,
        500,
        WHITE,
        Fx::Shadow,
    );
    t(
        (960.0, 680.0, 0.5),
        "I never thought I would see this place",
        32.0,
        500,
        WHITE,
        Fx::Outline,
    );
    t(
        (960.0, 724.0, 0.5),
        "again after all these years.",
        32.0,
        500,
        WHITE,
        Fx::Outline,
    );
    for (k, (a, b)) in [("Difficulty", "Normal"), ("Subtitles", "On")]
        .iter()
        .enumerate()
    {
        let y = 450.0 + 50.0 * k as f32;
        t((1340.0, y, 0.0), a, 26.0, 500, WHITE, Fx::None);
        t((1820.0, y, 1.0), b, 26.0, 500, GOLD, Fx::None);
    }
    (img, lines)
}

/// Arabic for the game frame's texts (grouped as `group_blocks` does).
pub(super) fn ar(en: &str) -> Option<&'static str> {
    Some(match en {
        "ELDRIN" => "إلدرين",
        "THE LOST CROWN" => "التاج المفقود",
        "Find the old map" => "اعثر على الخريطة القديمة",
        "Speak to the blacksmith" => "تحدث إلى الحداد",
        "Reach the northern gate" => "اذهب إلى البوابة الشمالية",
        "Continue" => "متابعة",
        "New Game" => "لعبة جديدة",
        "Load Game" => "تحميل لعبة",
        "Settings" => "الإعدادات",
        "Credits" => "الشكر",
        "Quit" => "خروج",
        "Press E to interact" => "اضغط E للتفاعل",
        "Difficulty" => "الصعوبة",
        "Normal" => "عادي",
        "Subtitles" => "الترجمة",
        "On" => "مفعّلة",
        s if s.starts_with("Bring") => {
            "أحضر [[حجر الشمس]] إلى المعبد القديم قبل حلول الليل، ولا تبتعد عن الطريق. الغابة ليست آمنة بعد حلول الظلام."
        }
        s if s.starts_with("I never") => {
            "لم أظن أبدا أنني سأرى هذا المكان مرة أخرى بعد كل هذه السنين."
        }
        _ => return None,
    })
}

/// Source languages with the real models: downloads (on first run) the
/// Latin, Arabic and Cyrillic recognizers and the French → English pair,
/// reads samples drawn with our fonts (Cyrillic: the system's), checks
/// "auto" moves off English on Arabic text, translates the French. Sizes,
/// times and outputs are printed:
/// `cargo test --release translate::tests::languages_end_to_end -- --ignored --nocapture`
#[test]
#[ignore]
fn languages_end_to_end() {
    use crate::ocr::{self, Read, models};
    use std::time::Instant;
    let index = |id: &str| models::RECOGNIZERS.iter().position(|r| r.id == id).unwrap();
    for id in ["latin", "arabic", "cyrillic"] {
        let r = models::RECOGNIZERS[index(id)];
        let files = models::ocr_files(r);
        let missing = models::missing_bytes(&files);
        let t = Instant::now();
        models::ensure(&files, |_, _| {}).unwrap();
        println!(
            "{id} recognizer: {} + dict {} bytes; downloaded {missing} bytes in {:.1?}",
            r.model.size,
            r.dict.size,
            t.elapsed()
        );
    }
    let pair = crate::mt::pair("fr", "en").unwrap();
    let missing = pair.missing_bytes();
    let t = Instant::now();
    pair.ensure(|_, _| {}).unwrap();
    println!(
        "{}: downloaded {missing} bytes in {:.1?}",
        pair.label(),
        t.elapsed()
    );

    let sample = |lines: &[&str]| -> (Rgba, (usize, usize)) {
        let dims = (900, 60 * lines.len() + 40);
        let mut img = [24u8, 28, 36, 255].repeat(dims.0 * dims.1);
        for (i, l) in lines.iter().enumerate() {
            let at = (30.0, 20.0 + 60.0 * i as f32, 0.0);
            text(&mut img, dims, at, l, 30.0, 500, WHITE, Fx::None);
        }
        (img, dims)
    };
    let t = Instant::now();
    let mut engine = ocr::Engine::load(&ocr::models_dir()).unwrap();
    println!("OCR engine load {:.1?}", t.elapsed());
    let mut read = |name: &str, img: &Rgba, dims: (usize, usize), how: Read| {
        let t = Instant::now();
        let got = engine
            .recognize(img, dims.0 as u32, dims.1 as u32, how)
            .unwrap();
        let took = t.elapsed();
        let n: usize = got.iter().map(|b| b.text.chars().count()).sum();
        let mean = got
            .iter()
            .map(|b| b.confidence * b.text.chars().count() as f32)
            .sum::<f32>()
            / n.max(1) as f32;
        let texts: Vec<String> = got.into_iter().map(|b| b.text).collect();
        println!("{name}: {texts:?}, confidence {mean:.2}, {took:.0?}");
        (texts, mean)
    };
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    // the detector may split a line in two: compare the joined text
    let same = |got: &[String], want: &[&str]| squash(&got.join(" ")) == squash(&want.join(" "));

    let fr = [
        "Nouvelle partie",
        "Paramètres du jeu",
        "Voulez-vous quitter ?",
    ];
    let ar = ["القائمة الرئيسية", "ابدأ لعبة جديدة"];
    let ru = ["Новая игра", "Настройки"];
    let (fr_img, fr_dims) = sample(&fr);
    let (ar_img, ar_dims) = sample(&ar);
    let (ru_img, ru_dims) = sample(&ru);
    let (got_fr, _) = read(
        "latin on French",
        &fr_img,
        fr_dims,
        Read::One(index("latin")),
    );
    let (got_en, en_on_fr) = read("en on French", &fr_img, fr_dims, Read::One(0));
    let (got_ar, _) = read(
        "arabic on Arabic",
        &ar_img,
        ar_dims,
        Read::One(index("arabic")),
    );
    let (_, en_on_ar) = read("en on Arabic", &ar_img, ar_dims, Read::One(0));
    let (got_ru, _) = read(
        "cyrillic on Russian",
        &ru_img,
        ru_dims,
        Read::One(index("cyrillic")),
    );
    let (_, en_on_ru) = read("en on Russian", &ru_img, ru_dims, Read::One(0));
    // auto: English first; on Arabic it tries the others and keeps Arabic
    let (auto_ar, _) = read("auto on Arabic", &ar_img, ar_dims, Read::Auto);
    let (auto_ar2, _) = read(
        "auto on Arabic again (Arabic first now)",
        &ar_img,
        ar_dims,
        Read::Auto,
    );
    let (auto_fr, _) = read(
        "auto on French (back off Arabic)",
        &fr_img,
        fr_dims,
        Read::Auto,
    );
    assert!(same(&got_fr, &fr), "latin reads French with its accents");
    assert!(same(&got_ar, &ar), "arabic reads Arabic");
    assert!(same(&got_ru, &ru), "cyrillic reads Russian");
    assert!(
        same(&auto_ar, &ar) && same(&auto_ar2, &ar),
        "auto found Arabic"
    );
    assert!(!auto_fr.is_empty() && auto_fr.iter().all(|t| t.is_ascii() || t.contains('è')));
    println!("English model, mean confidence: on Arabic {en_on_ar:.2}, on Russian {en_on_ru:.2}");
    println!("en on French reads {got_en:?} at {en_on_fr:.2}");

    let t = Instant::now();
    let mut tr = crate::mt::Translator::load(pair, &ocr::models_dir()).unwrap();
    println!("fr->en load {:.1?}", t.elapsed());
    let t = Instant::now();
    let lines = [
        got_fr[..got_fr.len() - 2].join(" "),
        got_fr[got_fr.len() - 2].clone(),
        got_fr[got_fr.len() - 1].clone(),
    ];
    let out = tr.translate(&lines).unwrap();
    println!("fr->en {:.0?}: {lines:?} -> {out:?}", t.elapsed());
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|t| !t.is_empty() && t.is_ascii()), "{out:?}");
}

/// Real screenshots (not in CI, needs the models in `ocr::models_dir()`):
/// OCR and offline EN -> AR in process; OCR lines and translations are
/// cached as JSON in the output folder, so a re-render takes a second.
/// `OC_REAL=a.png,b.png OC_REAL_OUT=dir cargo test --release real_frames -- --ignored --nocapture`
#[test]
#[ignore]
fn real_frames() {
    use super::pipeline::{Cache, Deps, translate_frame};
    use crate::ai::AiError;
    use crate::ocr::{self, OcrError, TextBlock};
    use std::cell::RefCell;
    use std::collections::HashMap;
    let out = std::path::PathBuf::from(std::env::var("OC_REAL_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let mt_path = out.join("mt.json");
    let mt_cache: RefCell<HashMap<String, String>> = RefCell::new(
        std::fs::read(&mt_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default(),
    );
    let pair = crate::mt::pair("en", "ar").unwrap();
    let mt = RefCell::new(None);
    struct Real<'a> {
        found: Vec<TextBlock>,
        cache: &'a RefCell<HashMap<String, String>>,
        mt: &'a RefCell<Option<crate::mt::Translator>>,
        pair: &'static crate::mt::Pair,
    }
    impl Deps for Real<'_> {
        fn recognize(&self, _: &[u8], _: u32, _: u32) -> Result<Vec<TextBlock>, OcrError> {
            Ok(self.found.clone())
        }
        fn translate(&self, texts: &[String], _: &str) -> Result<Vec<String>, AiError> {
            super::pipeline::by_sentence(texts, |todo| {
                let missing: Vec<String> = (todo.iter())
                    .filter(|t| !self.cache.borrow().contains_key(*t))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    let mut mt = self.mt.borrow_mut();
                    let tr = mt.get_or_insert_with(|| {
                        crate::mt::Translator::load(self.pair, &ocr::models_dir()).unwrap()
                    });
                    for (k, v) in missing.iter().zip(tr.translate(&missing).unwrap()) {
                        self.cache.borrow_mut().insert(k.clone(), v);
                    }
                }
                Ok(todo
                    .iter()
                    .map(|t| self.cache.borrow()[t].clone())
                    .collect())
            })
        }
    }
    let mut engine = None;
    for p in std::env::var("OC_REAL").unwrap().split(',') {
        let f = crate::capture::from_png(p.as_ref()).unwrap();
        let name = std::path::Path::new(p)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let ocr_path = out.join(format!("{name}.ocr.json"));
        let found: Vec<TextBlock> = match std::fs::read(&ocr_path) {
            Ok(b) => serde_json::from_slice(&b).unwrap(),
            Err(_) => {
                let e =
                    engine.get_or_insert_with(|| ocr::Engine::load(&ocr::models_dir()).unwrap());
                let got = e
                    .recognize(&f.rgba, f.width, f.height, ocr::Read::One(0))
                    .unwrap();
                std::fs::write(&ocr_path, serde_json::to_vec_pretty(&got).unwrap()).unwrap();
                got
            }
        };
        let deps = Real {
            found,
            cache: &mt_cache,
            mt: &mt,
            pair,
        };
        let r =
            translate_frame(&f.rgba, f.width, f.height, "ar", &Cache::new(4096), &deps).unwrap();
        eprintln!("{name}: {:?}", r.timings);
        for b in &r.blocks {
            eprintln!(
                "  {:?} {:?} -> {:?}",
                b.bounds.map(|v| v as i32),
                b.original,
                b.translation
            );
        }
        save_png(
            &out.join(format!("{name}_after.png")),
            (f.width as usize, f.height as usize),
            &r.rgba,
        );
    }
    std::fs::write(
        &mt_path,
        serde_json::to_vec_pretty(&*mt_cache.borrow()).unwrap(),
    )
    .unwrap();
}

/// Crops of real screenshots (Edge on Wikipedia, Windows at 168 %, ClearType)
/// with the lines the real OCR found in them, from the full-screen demo that
/// went wrong: `fixtures/` (cut by a script from the demo's PNGs).
mod real {
    use super::*;
    use crate::translate::pipeline::{by_sentence, cut_off, to_line};

    fn image(png: &[u8]) -> (Rgba, (usize, usize)) {
        let mut d = png::Decoder::new(std::io::Cursor::new(png));
        d.set_transformations(png::Transformations::normalize_to_color8());
        let mut r = d.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        let rgba = buf[..info.buffer_size()]
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        (rgba, (info.width as usize, info.height as usize))
    }

    /// The OCR lines of fixture `name`: x, y, w, h (OCR box) and text.
    fn ocr(name: &str) -> Vec<crate::ocr::TextBlock> {
        let all: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/lines.json")).unwrap();
        all[name]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| crate::ocr::TextBlock {
                bounds: [0, 1, 2, 3].map(|k| l[k].as_f64().unwrap() as f32),
                text: l[4].as_str().unwrap().into(),
                confidence: 0.9,
            })
            .collect()
    }

    fn lines(name: &str, (w, h): (usize, usize)) -> Vec<Line> {
        (ocr(name).into_iter())
            .filter(|b| !cut_off(b, h as u32))
            .map(|b| to_line(b, w as u32, h as u32))
            .collect()
    }

    /// Arabic about as long as `en` (what MT gives back).
    fn arabic(en: &str) -> String {
        let words = [
            "لعبة",
            "فيديو",
            "على",
            "الإنترنت",
            "وحدة",
            "التحكم",
            "الحاسوب",
        ];
        let mut s = String::new();
        for w in words.iter().cycle() {
            if s.chars().count() + 3 >= en.chars().count() {
                break;
            }
            s = format!("{s} {w}").trim().to_string();
        }
        s
    }

    fn changed(a: &[u8], b: &[u8], w: usize) -> Vec<(usize, usize)> {
        (a.chunks(4).zip(b.chunks(4)).enumerate())
            .filter(|(_, (p, q))| p != q)
            .map(|(i, _)| (i % w, i / w))
            .collect()
    }

    fn inside((x, y): (usize, usize), b: [f32; 4], pad: f32) -> bool {
        let (x, y) = (x as f32, y as f32);
        x >= b[0] - pad && x < b[0] + b[2] + pad && y >= b[1] - pad && y < b[1] + b[3] + pad
    }

    /// Each block alone: its redraw never covers another block's text, nor
    /// anything but plain background outside its own box (the infobox's
    /// lists, its frame, a picture). Before: Arabic paragraphs were aligned
    /// to a far "column" edge across the infobox, and a few words came out
    /// giant over their neighbours.
    #[test]
    fn each_redraw_stays_off_its_neighbours() {
        let (img, (w, h)) = image(include_bytes!("fixtures/page.png"));
        let boxes: Vec<[f32; 4]> = ocr("page").iter().map(|b| b.bounds).collect();
        let blocks = group_blocks(&lines("page", (w, h)));
        let blank = |x: usize, y: usize| {
            let p = |x: usize, y: usize| &img[(y * w + x) * 4..(y * w + x) * 4 + 3];
            (x.saturating_sub(2)..(x + 3).min(w)).all(|qx| {
                (y.saturating_sub(2)..(y + 3).min(h))
                    .all(|qy| (0..3).all(|k| p(qx, qy)[k].abs_diff(p(x, y)[k]) <= 8))
            })
        };
        let mut drawn = 0;
        for (i, b) in blocks.iter().enumerate() {
            let mut tr = vec![String::new(); blocks.len()];
            tr[i] = arabic(&b.text);
            let mut out = img.clone();
            render(&mut out, w as u32, h as u32, &blocks, &tr, true);
            let px = changed(&img, &out, w);
            drawn += !px.is_empty() as usize;
            let bad: Vec<_> = (px.iter())
                .filter(|&&p| !inside(p, b.bounds, 3.0))
                .filter(|&&p| {
                    let other = boxes.iter().any(|&o| inside(p, o, 0.0));
                    other || !blank(p.0, p.1)
                })
                .collect();
            assert!(
                bad.len() * 200 <= px.len(),
                "{:?}: {} of {} changed pixels off its box, e.g. {:?}",
                b.text,
                bad.len(),
                px.len(),
                bad.first()
            );
        }
        assert!(
            drawn * 10 >= blocks.len() * 8,
            "most blocks are translated: {drawn} of {}",
            blocks.len()
        );
    }

    /// The page's columns read interleaved (row by row): a wrapped nav item
    /// and a wrapped paragraph still group; nothing groups across columns or
    /// joins a list of items. Before: only the last block could continue,
    /// so the paragraph came apart line by line.
    #[test]
    fn interleaved_columns_still_group() {
        let blocks = group_blocks(&lines("frame", (1716, 1271)));
        let find = |start: &str| blocks.iter().find(|b| b.text.starts_with(start)).unwrap();
        assert_eq!(find("Collecting and").text, "Collecting and preservation");
        let para = find("A video game,");
        assert!(
            para.lines.len() >= 8,
            "{} lines: {:?}",
            para.lines.len(),
            para.text
        );
        assert!(para.text.ends_with("livestreaming."), "{:?}", para.text);
        for b in &blocks {
            let [x, _, w, _] = b.lines[0].bounds;
            for l in &b.lines {
                let [lx, _, lw, _] = l.bounds;
                assert!(lx < x + w && x < lx + lw, "across columns: {:?}", b.text);
            }
            if b.lines.len() > 1 {
                assert!(!b.text.contains('·'), "a list joined: {:?}", b.text);
            }
        }
    }

    /// Blue links between black words: the black is text of its own, not an
    /// outline (before: redrawn bold blue with a black outline), and the
    /// colour is the link's blue; a radio button's label keeps its size.
    #[test]
    fn link_lines_are_not_outlined() {
        let (img, (w, h)) = image(include_bytes!("fixtures/page.png"));
        let ls = lines("page", (w, h));
        let style = |start: &str| {
            let l = ls.iter().find(|l| l.text.starts_with(start)).unwrap();
            erase::erase_line(&mut img.clone(), w, h, l.bounds).0
        };
        for start in [
            "(which includes LAN",
            "Arcade games (highest",
            "Action (Platformer",
        ] {
            let s = style(start);
            assert!(s.shadow.is_none(), "{start}: {s:?}");
            assert!(s.color[2] > s.color[0] + 100, "{start}: link blue {s:?}");
        }
    }

    /// Text right under a photo: the erase stays off the photo and the
    /// caption is translated in its colour (before: the photo's bottom rows
    /// were taken for the caption's background and smeared into it).
    #[test]
    fn a_caption_under_a_photo_leaves_the_photo_alone() {
        let (img, (w, h)) = image(include_bytes!("fixtures/caption.png"));
        let blocks = group_blocks(&lines("caption", (w, h)));
        let tr: Vec<String> = blocks.iter().map(|b| arabic(&b.text)).collect();
        let mut out = img.clone();
        let drawn = render(&mut out, w as u32, h as u32, &blocks, &tr, true);
        // the photo: rows 0..64 of the crop
        let px = changed(&img, &out, w);
        let on_photo = px
            .iter()
            .filter(|p| p.1 < 64 && p.0 > 15 && p.0 < 420)
            .count();
        assert_eq!(on_photo, 0, "photo pixels changed");
        assert_eq!(drawn.drawn, blocks.len(), "{drawn:?}");
        // drawn dark on the light caption panel, not white
        let dark = px.iter().filter(|&&(x, y)| {
            let i = (y * w + x) * 4;
            out[i..i + 3].iter().map(|&v| v as u32).sum::<u32>() < 400
        });
        assert!(dark.count() > 200);
    }

    /// Specks: after the erase no glyph pixel is left in the lines' boxes
    /// (white page: anything under luma 200 is a remnant). Before: the tops
    /// of ascenders drawn in ClearType's fringe colours (h, l, f) stayed.
    #[test]
    fn erased_body_text_leaves_no_specks() {
        let (img, (w, h)) = image(include_bytes!("fixtures/specks.png"));
        let ls = lines("specks", (w, h));
        assert_eq!(ls.len(), 2);
        let mut out = img.clone();
        for l in &ls {
            erase::erase_line(&mut out, w, h, l.bounds);
        }
        let count = |img: &[u8]| {
            (0..w * h)
                .filter(|&i| ls.iter().any(|l| inside((i % w, i / w), l.bounds, 0.0)))
                .filter(|&i| {
                    let p = &img[i * 4..i * 4 + 3];
                    (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) < 200.0
                })
                .count()
        };
        let (before, after) = (count(&img), count(&out));
        eprintln!("glyph pixels: {before} before the erase, {after} after");
        assert!(before > 1000 && after == 0, "{after} speck pixels left");
    }

    /// Marian drops or merges sentences of a long input: every sentence goes
    /// alone and each text gets back exactly its own translation.
    #[test]
    fn offline_mt_gets_one_sentence_at_a_time() {
        let texts = [
            "The first game was Pong. It came out in 1972! Was it fun? Yes.".to_string(),
            "No end mark here".to_string(),
            String::new(),
        ];
        // a translator that keeps only the first sentence of what it gets
        let mut calls = Vec::new();
        let out = by_sentence(&texts, |t| {
            calls.push(t.to_vec());
            Ok(t.iter()
                .map(|s| format!("<{}>", s.split(". ").next().unwrap()))
                .collect())
        })
        .unwrap();
        assert_eq!(calls.len(), 1, "one call");
        assert_eq!(
            out,
            [
                "<The first game was Pong.> <It came out in 1972!> <Was it fun?> <Yes.>",
                "<No end mark here>",
                ""
            ]
        );
        let short = by_sentence(&texts, |t| Ok(t[1..].to_vec()));
        assert!(short.is_err(), "a lost sentence is an error, not a shift");
    }

    /// A line the frame's edge cuts through is left alone.
    #[test]
    fn lines_cut_by_the_frame_edge_are_skipped() {
        let tb = |y: f32| crate::ocr::TextBlock {
            bounds: [10.0, y, 100.0, 20.0],
            text: "x".into(),
            confidence: 0.9,
        };
        assert!(cut_off(&tb(0.0), 200) && cut_off(&tb(180.0), 200));
        assert!(!cut_off(&tb(5.0), 200) && !cut_off(&tb(170.0), 200));
    }
}

/// The busy-background threshold sits between the game panels the erase
/// handles (flat, gradient, grain, art behind a translucent panel, an
/// outlined subtitle on art) and text straight on a photo-like busy
/// background, which `render` leaves as it was (and reports).
#[test]
fn busy_background_threshold() {
    for c in cases() {
        let worst = (c.lines.iter())
            .map(|l| erase::erase_line(&mut c.img.clone(), c.dims.0, c.dims.1, l.bounds).0)
            .map(|s| s.residual)
            .fold(0.0, f32::max);
        eprintln!("{}: residual {worst:.1}", c.name);
        assert!(worst < BUSY, "{}: {worst}", c.name);
    }
    let (w, h) = (640, 160);
    // fine, high-contrast detail (foliage in a photo)
    let photo = paint(w, h, |x, y| {
        let n = noise(x, y, 4.0, 11);
        [
            40.0 + 200.0 * n,
            220.0 - 160.0 * n,
            60.0 + 150.0 * noise(x, y, 3.0, 12),
        ]
    });
    let c = case(
        "photo",
        photo,
        (w, h),
        &[(
            40.0,
            60.0,
            "Speak to the blacksmith",
            28.0,
            500,
            WHITE,
            Fx::None,
        )],
    );
    let s = erase::erase_line(&mut c.img.clone(), w, h, c.lines[0].bounds).0;
    eprintln!("photo: residual {:.1}", s.residual);
    assert!(s.residual > BUSY);
    let blocks = group_blocks(&c.lines);
    let mut out = c.img.clone();
    let drawn = render(
        &mut out,
        w as u32,
        h as u32,
        &blocks,
        &["تحدث".into()],
        true,
    );
    assert_eq!((drawn.drawn, drawn.busy), (0, 1));
    assert!(out == c.img, "left as it was");
}
