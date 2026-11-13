//! A game's settings dialog, drawn here (no game's art in the repo): a
//! textured gradient panel, Spanish labels in four sizes, drop-down fields
//! with their own fill and text, a label and its field read as one line,
//! and buttons with small dim labels. The defects of the OpenTTD recording
//! (0.1.3): values' boxes reach past their fields, so the field became the
//! "text" and was wiped into the panel; "Nombres de municipios: Inglés" was
//! one line over panel and field; button labels came back small and dim.

use super::*;
use std::collections::HashMap;

const W: usize = 1000;
const H: usize = 460;
const FIELD: [u8; 3] = [250, 173, 48];
const INK: [u8; 3] = [24, 18, 12];
const DIM: [u8; 3] = [70, 50, 28];

/// A drop-down field: its fill, a 1 px dark frame, the arrow box at its end.
fn field(img: &mut Rgba, r: [usize; 4]) {
    panel(img, W, r, [80, 52, 18], 1.0);
    panel(img, W, [r[0] + 1, r[1] + 1, r[2] - 2, r[3] - 2], FIELD, 1.0);
    let a = [r[0] + r[2] - r[3], r[1] + 1, r[3] - 1, r[3] - 2];
    panel(img, W, a, [214, 140, 30], 1.0);
}

/// OCR-like boxes become line boxes as the pipeline pads them.
fn ocr(l: Line) -> Line {
    let b = crate::features::text_reading::TextBlock {
        bounds: l.bounds,
        text: l.text,
        confidence: 0.9,
    };
    super::super::pipeline::to_line(b, W as u32, H as u32)
}

/// The frame, its lines, and the field rectangles by value text.
pub(super) fn frame() -> (Rgba, Vec<Line>, HashMap<&'static str, [usize; 4]>) {
    let mut img = paint(W, H, |x, y| {
        let n = noise(x, y, 5.0, 21) - 0.5;
        let g = y / H as f32 * 24.0;
        [
            150.0 - g + 22.0 * n,
            120.0 - g + 16.0 * n,
            72.0 - g + 9.0 * n,
        ]
    });
    let dims = (W, H);
    let mut lines = Vec::new();
    let mut fields = HashMap::new();
    let put = |img: &mut Rgba, x, y, anchor, s, size, wt, col, fx| {
        text(img, dims, (x, y, anchor), s, size, wt, col, fx)
    };
    let title = put(
        &mut img,
        500.0,
        14.0,
        0.5,
        "Generación de mundo",
        28.0,
        600,
        WHITE,
        Fx::Shadow,
    );
    lines.push(ocr(title));
    // label, then its field with the value (18 px)
    let rows = [
        ("Ríos:", "Medio"),
        ("Variedad:", "Ninguna"),
        ("Bordes del mapa:", "Aleatorio"),
    ];
    for (k, (label, value)) in rows.into_iter().enumerate() {
        let y = 80 + 40 * k;
        let l = put(
            &mut img,
            30.0,
            y as f32,
            0.0,
            label,
            18.0,
            500,
            INK,
            Fx::None,
        );
        lines.push(ocr(l));
        let r = [220, y, 300, 24];
        field(&mut img, r);
        let v = put(
            &mut img,
            228.0,
            y as f32 + 0.5,
            0.0,
            value,
            18.0,
            600,
            INK,
            Fx::None,
        );
        lines.push(ocr(v));
        fields.insert(value, r);
    }
    // a label and its field's value read as ONE line (22 px)
    let y = 210;
    let a = put(
        &mut img,
        30.0,
        y as f32,
        0.0,
        "Nombres de municipios:",
        22.0,
        500,
        INK,
        Fx::None,
    );
    let r = [a.bounds[0] as usize + a.bounds[2] as usize + 8, y, 260, 28];
    field(&mut img, r);
    let b = put(
        &mut img,
        r[0] as f32 + 8.0,
        y as f32 + 0.25,
        0.0,
        "Inglés",
        22.0,
        600,
        INK,
        Fx::None,
    );
    fields.insert("Inglés", r);
    let [x0, y0] = [a.bounds[0], a.bounds[1].min(b.bounds[1])];
    let x1 = b.bounds[0] + b.bounds[2];
    let y1 = (a.bounds[1] + a.bounds[3]).max(b.bounds[1] + b.bounds[3]);
    lines.push(ocr(Line {
        bounds: [x0, y0, x1 - x0, y1 - y0],
        text: "Nombres de municipios: Inglés".into(),
    }));
    // a small label (14 px) beside a field
    let y = 260;
    let l = put(
        &mut img,
        30.0,
        y as f32,
        0.0,
        "Nivel del mar:",
        14.0,
        500,
        INK,
        Fx::None,
    );
    lines.push(ocr(l));
    let r = [220, y, 200, 19];
    field(&mut img, r);
    let v = put(
        &mut img,
        226.0,
        y as f32 + 0.75,
        0.0,
        "Muy bajo",
        14.0,
        600,
        INK,
        Fx::None,
    );
    lines.push(ocr(v));
    fields.insert("Muy bajo", r);
    // buttons with small dim labels (13 px)
    for (k, label) in ["Configuración de IA", "Configuración de NewGRF"]
        .into_iter()
        .enumerate()
    {
        let r = [30 + 320 * k, 330, 300, 34];
        panel(&mut img, W, r, [80, 52, 18], 1.0);
        panel(
            &mut img,
            W,
            [r[0] + 2, r[1] + 2, r[2] - 4, r[3] - 4],
            FIELD,
            1.0,
        );
        let cx = (r[0] + r[2] / 2) as f32;
        let l = put(&mut img, cx, 339.0, 0.5, label, 13.0, 500, DIM, Fx::None);
        lines.push(ocr(l));
    }
    (img, lines, fields)
}

fn english() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("Generación de mundo", "World generation"),
        ("Ríos:", "Rivers:"),
        ("Medio", "Medium"),
        ("Variedad:", "Variety:"),
        ("Ninguna", "None"),
        ("Bordes del mapa:", "Map edges:"),
        ("Aleatorio", "Random"),
        ("Nombres de municipios:", "Town names:"),
        ("Inglés", "English"),
        ("Nombres de municipios: Inglés", "Town names: English"),
        ("Nivel del mar:", "Sea level:"),
        ("Muy bajo", "Very low"),
        ("Configuración de IA", "AI settings"),
        ("Configuración de NewGRF", "NewGRF settings"),
    ])
}

pub(super) fn run() -> (
    Rgba,
    Rgba,
    Vec<Block>,
    Drawn,
    HashMap<&'static str, [usize; 4]>,
) {
    let (img, lines, fields) = frame();
    let lines = super::super::split_fields(&img, W, H, &lines);
    let blocks = group_blocks(&lines);
    let en = english();
    let tr: Vec<String> = (blocks.iter())
        .map(|b| {
            en.get(b.text.as_str())
                .map(|t| t.to_string())
                .unwrap_or_default()
        })
        .collect();
    let mut out = img.clone();
    let d = render(&mut out, W as u32, H as u32, &blocks, &tr, false);
    (img, out, blocks, d, fields)
}

fn count(img: &[u8], r: [usize; 4], f: impl Fn(&[u8]) -> bool) -> usize {
    (r[1]..r[1] + r[3])
        .flat_map(|y| (r[0]..r[0] + r[2]).map(move |x| (y * W + x) * 4))
        .filter(|&i| f(&img[i..i + 3]))
        .count()
}

fn orange(p: &[u8]) -> bool {
    p[0] > 220 && (150..200).contains(&p[1]) && p[2] < 90
}

/// Every value is translated inside its own field, and the field keeps its
/// fill (no panel brown poured into it, no smear over its frame).
#[test]
fn values_are_translated_inside_their_fields() {
    let (img, out, blocks, d, fields) = run();
    for (value, r) in &fields {
        let i = (blocks.iter().position(|b| b.text == *value))
            .unwrap_or_else(|| panic!("{value:?} is no block of its own"));
        let [ix, iy, iw, ih] = d.ink[i];
        assert!(iw > 0.0, "{value:?} not drawn: {d:?}");
        let [fx, fy, fw, fh] = r.map(|v| v as f32);
        assert!(
            ix >= fx && ix + iw <= fx + fw && iy + ih / 2.0 > fy && iy + ih / 2.0 < fy + fh,
            "{value:?} drawn at {:?}, field {r:?}",
            d.ink[i]
        );
        // the field's fill stays (the new text covers about as much as the old)
        let inner = [r[0] + 2, r[1] + 2, r[2] - 4 - r[3], r[3] - 4];
        let (before, after) = (count(&img, inner, orange), count(&out, inner, orange));
        assert!(
            after * 100 >= before * 93,
            "{value:?}: fill {before} -> {after}"
        );
        // its frame is untouched
        let top = [r[0], r[1], r[2], 1];
        assert_eq!(
            count(&img, top, |_| true),
            count(&out, top, |p| p == &[80, 52, 18][..])
        );
    }
}

/// A label and its field's value read as one line are cut at the field:
/// the label is drawn on the panel, the value in the field.
#[test]
fn a_label_and_its_value_split_at_the_field() {
    let (_, _, blocks, d, fields) = run();
    let r = fields["Inglés"];
    let label = (blocks.iter()).position(|b| b.text == "Nombres de municipios:");
    let label = label.expect("the line is split at the field");
    let [lx, _, lw, _] = d.ink[label];
    assert!(
        lw > 0.0 && lx + lw <= r[0] as f32,
        "label at {:?}",
        d.ink[label]
    );
}

/// Small dim button labels come back at their size and in their colour.
#[test]
fn button_labels_keep_size_and_colour() {
    let (img, out, blocks, d, _) = run();
    for name in ["Configuración de IA", "Configuración de NewGRF"] {
        let i = blocks.iter().position(|b| b.text == name).unwrap();
        let [x, y, w, h] = blocks[i].bounds.map(|v| v as usize);
        let dark = |p: &[u8]| p[0] < 120 && p[2] < 70;
        let rows = |im: &[u8]| {
            let ys: Vec<usize> = (y..y + h)
                .filter(|&yy| count(im, [x, yy, w, 1], dark) > 0)
                .collect();
            ys.last().map_or(0, |b| b + 1 - ys[0])
        };
        let (h0, h1) = (rows(&img), rows(&out));
        assert!(d.ink[i][2] > 0.0, "{name} not drawn");
        assert!(h1 as f32 >= 0.85 * h0 as f32, "{name}: {h0} -> {h1} rows");
        // the darkest strokes: about the original's colour, not grey or faint
        let mut v: Vec<[u8; 3]> = (y..y + h)
            .flat_map(|yy| (x..x + w).map(move |xx| (yy * W + xx) * 4))
            .map(|i| [out[i], out[i + 1], out[i + 2]])
            .filter(|p| dark(p))
            .collect();
        v.sort_by_key(|p| p[0] as u32 + p[1] as u32);
        let core = v[v.len() / 10];
        assert!(
            (0..3).all(|k| core[k].abs_diff(DIM[k]) < 45),
            "{name}: strokes {core:?}, label {DIM:?}"
        );
    }
}

/// The whole dialog against its golden (`tests/fixtures/translate/game_expected.png`,
/// looked at by eye; `OC_BLESS=1` rewrites it).
#[test]
fn the_dialog_matches_its_golden() {
    let (_, out, _, _, _) = run();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/translate/game_expected.png"
    );
    if std::env::var_os("OC_BLESS").is_some() {
        save_png(path.as_ref(), (W, H), &out);
        return;
    }
    let want = crate::shared::platform::capture::from_png(path.as_ref())
        .unwrap()
        .rgba;
    let off = (out.chunks(4).zip(want.chunks(4)))
        .filter(|(a, b)| (0..3).any(|k| a[k].abs_diff(b[k]) > 24))
        .count();
    assert!(off * 1000 <= W * H, "{off} pixels differ from the golden");
}
