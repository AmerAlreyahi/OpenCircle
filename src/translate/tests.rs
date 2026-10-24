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
        |c: &Case, l: usize| erase::erase_line(&mut c.img.clone(), 640, 160, c.lines[l].bounds);
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
