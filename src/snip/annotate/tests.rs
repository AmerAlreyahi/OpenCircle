use super::*;
use crate::ui::screens::snip_toolbar::toolbar;
use iced::Element;
use iced::keyboard::{Key, Modifiers, key::Named};
use iced::widget::column;

/// The views of the editor's screen (`ui::screens::snip_toolbar`), as the tests
/// look at them.
impl Annotator {
    fn canvas(&self) -> Element<'_, Message> {
        crate::ui::screens::snip_toolbar::canvas::canvas(
            self,
            1.0,
            crate::ui::screens::snip_toolbar::canvas::ScreenMap::ONE,
        )
    }

    fn toolbar(&self) -> Element<'_, Message> {
        toolbar::view(self)
    }

    fn toolbar_at(&self, at: Rectangle) -> (Element<'_, Message>, Option<Element<'_, Message>>) {
        toolbar::view_at(self, at, iced::widget::tooltip::Position::Bottom)
    }

    /// The canvas above the toolbar.
    fn view(&self) -> Element<'_, Message> {
        column![self.canvas(), self.toolbar()]
            .spacing(12)
            .align_x(iced::Center)
            .into()
    }
}

const RED: [u8; 4] = [255, 59, 48, 255];

/// `w`x`h` image, pixel (x, y) = `f(x, y)`.
fn image_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    Image {
        width: w,
        height: h,
        rgba: (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| f(x, y))
            .collect(),
    }
}

/// A busy pattern (so a blur visibly changes it).
fn checker(w: u32, h: u32) -> Image {
    image_of(w, h, |x, y| {
        if (x / 3 + y / 3) % 2 == 0 {
            [250, 250, 250, 255]
        } else {
            [10, 20, 30, 255]
        }
    })
}

fn item(shape: Shape) -> Item {
    Item {
        shape,
        colour: RED,
        width: 4.0,
        outline: false,
        fill: false,
    }
}

fn doc_with(image: Image, items: Vec<Item>) -> Document {
    let mut doc = Document::new(image);
    for i in items {
        doc.exec(Edit::Insert(doc.items.len(), i));
    }
    doc
}

fn px(c: &Crop, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * c.width + x) * 4) as usize;
    c.rgba[i..i + 4].try_into().unwrap()
}

fn pt(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

// ---- smart shape -------------------------------------------------------------

/// Deterministic wobble in [-1, 1].
fn wobble(i: usize) -> f32 {
    ((i * 7919 % 101) as f32 / 50.0) - 1.0
}

fn rough_ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Point> {
    (0..=60)
        .map(|i| {
            let a = i as f32 / 60.0 * std::f32::consts::TAU;
            let r = 1.0 + 0.04 * wobble(i);
            pt(cx + rx * r * a.cos(), cy + ry * r * a.sin())
        })
        .collect()
}

fn rough_rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Point> {
    let corners = [pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1), pt(x0, y0)];
    let mut points = Vec::new();
    for (k, w) in corners.windows(2).enumerate() {
        for i in 0..15 {
            let t = i as f32 / 15.0;
            let n = 2.0 * wobble(k * 15 + i);
            points.push(pt(
                w[0].x + (w[1].x - w[0].x) * t + n,
                w[0].y + (w[1].y - w[0].y) * t + n,
            ));
        }
    }
    points.push(pt(x0 + 3.0, y0 + 2.0)); // doesn't quite close
    points
}

#[test]
fn rough_strokes_snap_to_clean_shapes() {
    let snapped = shape::snap(&rough_ellipse(200.0, 150.0, 90.0, 50.0));
    let Some(Shape::Ellipse { a, b }) = snapped else {
        panic!("ellipse: {snapped:?}")
    };
    assert!(
        (a.x - 110.0).abs() < 6.0 && (b.y - 200.0).abs() < 6.0,
        "{a:?} {b:?}"
    );

    let snapped = shape::snap(&rough_rect(50.0, 40.0, 250.0, 160.0));
    let Some(Shape::Rect { a, b }) = snapped else {
        panic!("rect: {snapped:?}")
    };
    assert!(a.distance(pt(50.0, 40.0)) < 5.0 && b.distance(pt(250.0, 160.0)) < 5.0);

    // A slightly shaky straight stroke: a line.
    let line: Vec<_> = (0..=40)
        .map(|i| pt(10.0 + 5.0 * i as f32, 20.0 + 2.5 * i as f32 + wobble(i)))
        .collect();
    let snapped = shape::snap(&line);
    assert!(
        matches!(snapped, Some(Shape::Arrow { head: false, .. })),
        "line: {snapped:?}"
    );

    // A shaft, then a short hook back from its tip: an arrow to the tip.
    let mut arrow: Vec<_> = (0..=30).map(|i| pt(20.0 + 6.0 * i as f32, 100.0)).collect();
    arrow.extend((1..=6).map(|i| pt(200.0 - 4.0 * i as f32, 100.0 - 4.0 * i as f32)));
    let snapped = shape::snap(&arrow);
    let Some(Shape::Arrow {
        a, b, head: true, ..
    }) = snapped
    else {
        panic!("arrow: {snapped:?}")
    };
    assert_eq!((a, b), (pt(20.0, 100.0), pt(200.0, 100.0)));

    // Freehand: an open arc, a zig-zag, a scribbled loop, a dot.
    let arc: Vec<_> = rough_ellipse(200.0, 150.0, 80.0, 80.0)[..30].to_vec();
    let zigzag: Vec<_> = (0..=20)
        .map(|i| pt(10.0 * i as f32, if i % 2 == 0 { 0.0 } else { 40.0 }))
        .collect();
    let scribble: Vec<_> = (0..=80)
        .map(|i| {
            let a = i as f32 / 80.0 * std::f32::consts::TAU;
            let r = 60.0 + 30.0 * (5.0 * a).sin();
            pt(200.0 + r * a.cos(), 200.0 + r * a.sin())
        })
        .collect();
    for (name, stroke) in [("arc", arc), ("zigzag", zigzag), ("scribble", scribble)] {
        assert_eq!(shape::snap(&stroke), None, "{name}");
    }
    assert_eq!(shape::snap(&[pt(1.0, 1.0)]), None);
}

// ---- render ------------------------------------------------------------------
#[test]
fn blur_changes_only_its_band() {
    let size = 16.0;
    let stroke = vec![pt(20.0, 40.0), pt(50.0, 40.0), pt(80.0, 40.0)];
    let blur = item(Shape::Blur {
        points: stroke,
        size,
        cell: 6.0,
        seed: 7,
    });
    let plain = render(&doc_with(checker(100, 80), vec![]));
    let blurred = render(&doc_with(checker(100, 80), vec![blur]));
    // A flat band: x 12..88 (square ends), y 32..48; nothing else changes.
    for y in 0..80u32 {
        for x in 0..100u32 {
            let near = (11..89).contains(&x) && (31..49).contains(&y);
            if !near {
                assert_eq!(px(&plain, x, y), px(&blurred, x, y), "({x}, {y})");
            }
        }
    }
    // Square ends, not a round blob: the band's corners are pixelated too.
    for (x, y) in [(12, 32), (87, 32), (12, 47), (87, 47)] {
        assert_ne!(px(&plain, x, y), px(&blurred, x, y), "corner ({x}, {y})");
    }
}

/// A password field: `secret` in dark text on an off-white field.
fn field(secret: &str) -> Image {
    field_in(secret, [250, 250, 250, 255], [20, 20, 30, 255])
}

/// `secret` in `fg` text on a `bg` field.
fn field_in(secret: &str, bg: [u8; 4], fg: [u8; 4]) -> Image {
    let (w, h) = (240, 40);
    let mut layer = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(
            image_of(w, h, |_, _| bg).rgba,
            tiny_skia::IntSize::from_wh(w, h).unwrap(),
        )
        .unwrap(),
        origin: Point::ORIGIN,
    };
    text::paint(&mut layer, pt(8.0, 8.0), secret, 20.0, fg, false);
    Image {
        width: w,
        height: h,
        rgba: layer.pm.take(),
    }
}

fn luma(p: [u8; 4]) -> u32 {
    (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8
}

fn pxi(i: &Image, x: u32, y: u32) -> [u8; 4] {
    let k = ((y * i.width + x) * 4) as usize;
    i.rgba[k..k + 4].try_into().unwrap()
}

/// `image` with a blur brush (`cell` px cells, secret `seed`) along its line
/// `band` (as line help does).
fn brushed(image: Image, band: lines::Band, cell: f32, seed: u64) -> Image {
    let (w, h) = (image.width, image.height);
    let mut layer = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(image.rgba, tiny_skia::IntSize::from_wh(w, h).unwrap())
            .unwrap(),
        origin: Point::ORIGIN,
    };
    let (middle, size) = band.stroke(1.0);
    let stroke = [pt(8.0, middle), pt(w as f32 - 8.0, middle)];
    blur::paint(&mut layer, &stroke, size, cell, seed);
    Image {
        width: w,
        height: h,
        rgba: layer.pm.take(),
    }
}

/// The whole line, x 10..230, inside the band.
fn core_of(band: lines::Band) -> Vec<(u32, u32)> {
    (band.y0.ceil() as u32..band.y1 as u32)
        .flat_map(|y| (10..230).map(move |x| (x, y)))
        .collect()
}

const PAIRS: [(&str, &str); 3] = [
    ("hunter2", "letmein"),
    ("4f9a8b7c", "0d9e8f7a"),
    ("password", "qwertyui"),
];
/// (background, text): grey, and two coloured fields.
const LOOKS: [([u8; 4], [u8; 4]); 3] = [
    ([250, 250, 250, 255], [20, 20, 30, 255]),
    ([30, 90, 160, 255], [250, 200, 40, 255]),
    ([200, 60, 60, 255], [20, 20, 120, 255]),
];

/// `image` as ClearType draws text: each pixel's red takes some of its left
/// neighbour's ink, its blue some of its right one's, so glyph edges get
/// coloured fringes (on any colours: the pixels are mixed channel by channel).
fn cleartype(image: Image) -> Image {
    let (w, h) = (image.width, image.height);
    let mut out = Image {
        rgba: image.rgba.clone(),
        ..image
    };
    for y in 0..h {
        for x in 1..w - 1 {
            let at = |x: u32, k: usize| image.rgba[((y * w + x) * 4) as usize + k] as u32;
            let i = ((y * w + x) * 4) as usize;
            out.rgba[i] = ((at(x - 1, 0) + 2 * at(x, 0)) / 3) as u8;
            out.rgba[i + 2] = ((at(x + 1, 2) + 2 * at(x, 2)) / 3) as u8;
        }
    }
    out
}

/// One view an attacker takes of a blur (its pixels, column or row averages).
type Take<'a> = dyn Fn(&Image) -> Vec<f32> + 'a;

/// Not reversible (Depix-style matching needs the text to be what changes the
/// output): two different secrets under the same stroke (same seed) differ
/// less than one secret under two seeds, for every pixel size, on grey and
/// coloured fields; and so in every view an attacker takes: the tone, the
/// colour directions R-G and G-B (no chroma leak), and the column and row
/// averages of each.
#[test]
fn blur_mosaic_text_is_below_the_noise() {
    // The views: per pixel, one projection of the colour.
    const VIEWS: [(&str, [f32; 3]); 3] = [
        ("luma", [0.299, 0.587, 0.114]),
        ("R-G", [1.0, -1.0, 0.0]),
        ("G-B", [0.0, 1.0, -1.0]),
    ];
    // Grey anti-aliasing, and ClearType-like colour fringes (Windows screens).
    let looks = LOOKS
        .iter()
        .flat_map(|&(bg, fg)| [(bg, fg, false), (bg, fg, true)]);
    for cell in [6.0, 9.0, 12.0] {
        for (bg, fg, fringed) in looks.clone() {
            let field = |s: &str| match fringed {
                true => cleartype(field_in(s, bg, fg)),
                false => field_in(s, bg, fg),
            };
            for (a, b) in PAIRS {
                let band = lines::detect(&field(a))[0];
                let core = core_of(band);
                let (y0, y1) = (band.y0.ceil() as u32, band.y1 as u32);
                for (view, k) in VIEWS {
                    let v = |i: &Image, x, y| {
                        let p = pxi(i, x, y);
                        (0..3).map(|c| k[c] * p[c] as f32).sum::<f32>()
                    };
                    let pixels = |i: &Image| core.iter().map(|&(x, y)| v(i, x, y)).collect();
                    let columns = |i: &Image| {
                        (10..230)
                            .map(|x| (y0..y1).map(|y| v(i, x, y)).sum::<f32>() / (y1 - y0) as f32)
                            .collect()
                    };
                    let rows = |i: &Image| {
                        (y0..y1)
                            .map(|y| (10..230).map(|x| v(i, x, y)).sum::<f32>() / 220.0)
                            .collect()
                    };
                    let diff = |p: &Vec<f32>, q: &Vec<f32>| {
                        p.iter().zip(q).map(|(m, n)| (m - n).abs()).sum::<f32>() / p.len() as f32
                    };
                    let takes: [(&str, &Take<'_>); 3] =
                        [("pixels", &pixels), ("columns", &columns), ("rows", &rows)];
                    for (what, take) in takes {
                        let (mut text, mut noise) = (0.0, 0.0);
                        for seed in 1u64..=8 {
                            let pa = take(&brushed(field(a), band, cell, seed));
                            let pb = take(&brushed(field(b), band, cell, seed));
                            let pc = take(&brushed(field(a), band, cell, seed + 99));
                            text += diff(&pa, &pb) / 8.0;
                            noise += diff(&pa, &pc) / 8.0;
                        }
                        let at =
                            format!("cell {cell} {a}/{b} {bg:?} fringed {fringed} {view} {what}");
                        eprintln!("{at}: text {text:.2} noise {noise:.2}");
                        // (A grey field has no colour at all: both are 0 there.)
                        assert!(text < noise || text < 0.05, "{at}: {text} vs {noise}");
                    }
                }
            }
        }
    }
}

/// ONE colour pair per stroke, never per block (review): a blue word moved
/// from one end of the line to the other (the grey one taking its place) gives
/// the same pair, and in the painted mosaic a pixel's colour is a fixed
/// function of its tone (the pair's line), so no block shows where the
/// coloured word is.
#[test]
fn blur_mosaic_colours_are_one_pair_per_stroke() {
    let line = |blue_at: f32, grey_at: f32| {
        let mut l = document::Layer {
            pm: tiny_skia::Pixmap::from_vec(
                image_of(400, 40, |_, _| [255, 255, 255, 255]).rgba,
                tiny_skia::IntSize::from_wh(400, 40).unwrap(),
            )
            .unwrap(),
            origin: Point::ORIGIN,
        };
        text::paint(
            &mut l,
            pt(blue_at, 8.0),
            "linked",
            20.0,
            [26, 13, 171, 255],
            false,
        );
        text::paint(
            &mut l,
            pt(grey_at, 8.0),
            "linked",
            20.0,
            [30, 30, 30, 255],
            false,
        );
        l
    };
    let pixels = |l: &document::Layer| {
        let d = l.pm.data().to_vec();
        (6..34u32)
            .flat_map(move |y| (0..400u32).map(move |x| (x, y)))
            .map(move |(x, y)| {
                let i = ((y * 400 + x) * 4) as usize;
                [d[i], d[i + 1], d[i + 2]]
            })
    };
    let (a, b) = (line(20.0, 260.0), line(260.0, 20.0));
    assert_ne!(a.pm.data(), b.pm.data());
    assert_eq!(blur::colours(pixels(&a)), blur::colours(pixels(&b)));
    // Painted: the same tone always has the same blue-red difference, in
    // both pictures and every block (the soft cell edges round by a level).
    let mut seen = std::collections::HashMap::new();
    for mut l in [a, b] {
        let stroke = [pt(10.0, 20.0), pt(390.0, 20.0)];
        blur::paint(&mut l, &stroke, 24.0, 6.0, 9);
        for y in 10..30u32 {
            for x in 20..380u32 {
                let i = ((y * 400 + x) * 4) as usize;
                let p = &l.pm.data()[i..i + 3];
                let tone = (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8;
                let chroma = p[2] as i32 - p[0] as i32;
                let first = *seen.entry(tone).or_insert(chroma);
                assert!(
                    (chroma - first).abs() <= 3,
                    "({x}, {y}) tone {tone}: {chroma} vs {first}"
                );
            }
        }
    }
}

/// The stroke's tint doesn't track how much ink is under it (review): a line
/// of "iiii" and one of "WWWW" (a few stems vs a lot of ink), plain and with
/// ClearType-like fringes, in grey, blue and on a coloured field, give the same
/// snapped tint and the same background.
#[test]
fn blur_tint_ignores_how_much_ink_there_is() {
    let looks = [
        ([255, 255, 255, 255], [20, 20, 30, 255]),
        ([255, 255, 255, 255], [26, 13, 171, 255]),
        ([30, 90, 160, 255], [250, 200, 40, 255]),
    ];
    for (bg, fg) in looks {
        for fringed in [false, true] {
            let pair = |s: &str| {
                let f = field_in(s, bg, fg);
                let f = if fringed { cleartype(f) } else { f };
                let rgba = f.rgba;
                blur::colours((10..30).flat_map(|y| (8..200).map(move |x| (x, y))).map(
                    |(x, y): (usize, usize)| {
                        let i = (y * 240 + x) * 4;
                        [rgba[i], rgba[i + 1], rgba[i + 2]]
                    },
                ))
            };
            let (thin, heavy) = (pair("iiiiiiii"), pair("WWWWWWWW"));
            assert_eq!(thin.1, heavy.1, "{fg:?} fringed {fringed}: background");
            assert_eq!(
                blur::snapped_tint(thin),
                blur::snapped_tint(heavy),
                "{fg:?} fringed {fringed}: {thin:?} vs {heavy:?}"
            );
        }
    }
}

/// The most saturated colours (a tint wider than 255 levels before it is
/// bounded) paint without a panic and stay in range.
#[test]
fn blur_takes_extreme_colours() {
    for colour in [
        [0, 0, 255, 255],
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [255, 0, 255, 255],
    ] {
        for seed in 0..16 {
            let band = lines::Band {
                x0: 0.0,
                x1: 240.0,
                y0: 10.0,
                y1: 30.0,
                room: (f32::INFINITY, f32::INFINITY),
            };
            let out = brushed(image_of(240, 40, |_, _| colour), band, 6.0, seed);
            assert_eq!(out.rgba.len(), 240 * 40 * 4);
        }
    }
}

#[test]
fn blur_mosaic_keeps_a_links_own_hue() {
    let (bg, fg) = ([255, 255, 255, 255], [26, 13, 171, 255]);
    let band = lines::detect(&field_in("example.com/link", bg, fg))[0];
    for seed in 1..=8 {
        let out = brushed(field_in("example.com/link", bg, fg), band, 6.0, seed);
        for &(x, y) in &core_of(band) {
            let [r, g, b, _] = pxi(&out, x, y);
            // (1 level: rounding where the cell is all but white.)
            let (r16, g16, b16) = (r as u16, g as u16, b as u16);
            assert!(b16 + 1 >= r16 && b16 + 1 >= g16, "({x}, {y}) {r} {g} {b}");
            assert!(
                r.abs_diff(g) <= 24,
                "no pink or green: ({x}, {y}) {r} {g} {b}"
            );
        }
    }
}

/// The mosaic replaces every dark glyph pixel (the text below the noise is
/// `blur_mosaic_text_is_below_the_noise`); a seed always paints the same pixels.
#[test]
fn blur_mosaic_makes_the_text_unreadable() {
    let original = field("hunter2");
    let band = lines::detect(&original)[0];
    let core = core_of(band);
    for cell in [6.0, 9.0, 12.0] {
        let out = brushed(field("hunter2"), band, cell, 1);
        for &(x, y) in &core {
            let (a, b) = (pxi(&original, x, y), pxi(&out, x, y));
            if luma(a) < 60 {
                assert!(luma(b) > luma(a) + 30, "({x}, {y}) {a:?} -> {b:?}");
            }
        }
    }
    // Deterministic for its seed: the same pixels every time (repaints).
    let again = |s| brushed(field("hunter2"), band, 6.0, s).rgba;
    assert!(again(1) == again(1) && again(1) != again(2));
}

/// No colour noise: the jitter is the same on R, G and B and the stroke has one
/// tint, so a grey source gives grey cells, and a flat colour moves only in
/// tone.
#[test]
fn blur_mosaic_has_no_tint() {
    let grey = field_in("hunter2", [250, 250, 250, 255], [20, 20, 20, 255]);
    let band = lines::detect(&grey)[0];
    let core = core_of(band);
    for seed in 1..=4 {
        let out = brushed(
            field_in("hunter2", [250; 4], [20, 20, 20, 255]),
            band,
            6.0,
            seed,
        );
        for &(x, y) in &core {
            let [r, g, b, a] = pxi(&out, x, y);
            assert!(r == g && g == b && a == 255, "({x}, {y}) {r} {g} {b}");
        }
        for colour in [[120, 140, 160, 255], [90, 120, 100, 255]] {
            let flat = brushed(image_of(240, 40, |_, _| colour), band, 6.0, seed);
            // One tint for the whole stroke: every pixel has the same channel
            // differences (its colour moves only in tone), near the source's.
            let diffs = |p: [u8; 4]| (p[0] as i32 - p[1] as i32, p[1] as i32 - p[2] as i32);
            let first = diffs(pxi(&flat, core[0].0, core[0].1));
            let (want, step) = (diffs(colour), (blur::TINT_STEP + blur::TINT_JITTER) as i32);
            assert!((first.0 - want.0).abs() <= step && (first.1 - want.1).abs() <= step);
            for &(x, y) in &core {
                let d = diffs(pxi(&flat, x, y));
                assert!(
                    (d.0 - first.0).abs() <= 1 && (d.1 - first.1).abs() <= 1,
                    "({x}, {y}) {d:?} vs {first:?} from {colour:?}"
                );
            }
        }
    }
}

/// The fine cells read nothing but the coarse blocks: moving the ink around
/// inside every block (each block mirrored left to right: other glyph shapes,
/// the same block means and colours) gives the same picture.
#[test]
fn blur_mosaic_reads_only_coarse_blocks() {
    let original = field("hunter2");
    let band = lines::detect(&original)[0];
    let (bw, bh) = blur::block(band.stroke(1.0).1);
    let (bw, bh) = (bw as u32, bh as u32);
    let mut mirrored = field("hunter2");
    for y in 0..40 - 40 % bh {
        for x0 in (0..240 - 240 % bw).step_by(bw as usize) {
            for k in 0..bw {
                let (a, b) = (
                    ((y * 240 + x0 + k) * 4) as usize,
                    ((y * 240 + x0 + bw - 1 - k) * 4) as usize,
                );
                mirrored.rgba[a..a + 4].copy_from_slice(&original.rgba[b..b + 4]);
            }
        }
    }
    assert_ne!(mirrored.rgba, original.rgba);
    for cell in [6.0, 9.0, 12.0] {
        let (p, q) = (
            brushed(field("hunter2"), band, cell, 5),
            brushed(
                Image {
                    rgba: mirrored.rgba.clone(),
                    ..mirrored
                },
                band,
                cell,
                5,
            ),
        );
        // The band's core, clear of the image's partial blocks at its right edge.
        for &(x, y) in core_of(band)
            .iter()
            .filter(|(x, _)| *x < 240 - 240 % bw - bw)
        {
            let (a, b) = (pxi(&p, x, y), pxi(&q, x, y));
            assert!(
                (0..3).all(|k| a[k].abs_diff(b[k]) <= 1),
                "cell {cell} ({x}, {y}) {a:?} {b:?}"
            );
        }
    }
}

/// A rank 0 = best of `scores` for index `i` (ties share: half a place each).
fn rank(scores: &[f32], i: usize) -> f32 {
    let better = scores.iter().filter(|&&s| s < scores[i]).count() as f32;
    let tied = scores.iter().filter(|&&s| s == scores[i]).count() as f32 - 1.0;
    better + tied / 2.0
}

/// The attack the review asks for: the attacker knows the algorithm, the font
/// and the length; they render the candidates (the true text, one other and 20
/// random distractors), blur each (averaged over seeds they pick: the expected
/// picture), and score each against the observed blur by squared error over
/// every pixel, fitting the per-stroke offset. The true text must not rank
/// first more often than chance: over many secret seeds, at every pixel size,
/// it is in the top 10 % at most about as often as a random candidate.
#[test]
fn blur_mosaic_resists_a_candidate_scoring_attack() {
    // A tiny fixed generator for the distractors (test only).
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let (truth, other) = ("hunter27", "letmein4");
    let mut candidates = vec![truth.to_string(), other.to_string()];
    for _ in 0..20 {
        candidates.push(
            (0..truth.len())
                .map(|_| CHARS[(next() % CHARS.len() as u64) as usize] as char)
                .collect(),
        );
    }
    let band = lines::detect(&field(truth))[0];
    let core = core_of(band);
    let tones = |i: &Image| -> Vec<f32> {
        core.iter()
            .map(|&(x, y)| {
                let p = pxi(i, x, y);
                0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
            })
            .collect()
    };
    for cell in [6.0, 9.0, 12.0] {
        // The attacker's expected picture of each candidate.
        let expected: Vec<Vec<f32>> = (candidates.iter())
            .map(|text| {
                let mut sum = vec![0.0; core.len()];
                for s in 0..6u64 {
                    let t = tones(&brushed(field(text), band, cell, 7000 + s));
                    sum.iter_mut().zip(t).for_each(|(a, b)| *a += b / 6.0);
                }
                sum
            })
            .collect();
        let trials = 120;
        let mut top = 0;
        for seed in 0..trials {
            let seen = tones(&brushed(field(truth), band, cell, 100 + seed));
            let scores: Vec<f32> = (expected.iter())
                .map(|e| {
                    let offset =
                        seen.iter().zip(e).map(|(s, e)| s - e).sum::<f32>() / seen.len() as f32;
                    (seen.iter().zip(e))
                        .map(|(s, e)| (s - e - offset).powi(2))
                        .sum::<f32>()
                })
                .collect();
            let place = rank(&scores, 0);
            top += usize::from(place < 0.1 * candidates.len() as f32);
        }
        eprintln!("cell {cell}: the true text in the top 10 % in {top} of {trials} trials");
        // Chance: places 0, 1 and 2 of 22 (13.6 %, 16.4 of 120); two standard
        // deviations of luck allowed (24), never a pattern (plain snapped
        // blocks: 24 of 24; with the dither only: 14 of 24).
        let p = (0.1 * candidates.len() as f32).ceil() / candidates.len() as f32;
        let n = trials as f32;
        let most = n * p + 2.0 * (n * p * (1.0 - p)).sqrt();
        assert!(
            top as f32 <= most,
            "cell {cell}: {top} of {trials} (chance {p})"
        );
    }
}

#[test]
fn blur_brush_repaint_of_a_crop_gives_the_same_pixels() {
    let full = field("hunter2");
    let band = lines::detect(&full)[0];
    let (size, cell) = (band.stroke(1.0).1, 6.0);
    let stroke = [pt(100.0, band.centre()), pt(140.0, band.centre())];
    let mut whole = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(
            full.rgba.clone(),
            tiny_skia::IntSize::from_wh(240, 40).unwrap(),
        )
        .unwrap(),
        origin: Point::ORIGIN,
    };
    blur::paint(&mut whole, &stroke, size, cell, 1);
    // A layer holding only what the stroke can touch (frame + reach).
    let r = blur::frame(&stroke, size).expand(blur::reach(size, cell));
    let (x0, y0) = (r.x.floor().max(0.0) as u32, 0u32);
    let (x1, y1) = (((r.x + r.width).ceil() as u32).min(240), 40u32);
    let mut data = Vec::new();
    for y in y0..y1 {
        let k = ((y * 240 + x0) * 4) as usize;
        data.extend_from_slice(&full.rgba[k..k + ((x1 - x0) * 4) as usize]);
    }
    let mut crop = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(
            data,
            tiny_skia::IntSize::from_wh(x1 - x0, y1 - y0).unwrap(),
        )
        .unwrap(),
        origin: Point::new(x0 as f32, y0 as f32),
    };
    blur::paint(&mut crop, &stroke, size, cell, 1);
    for y in y0..y1 {
        for x in x0..x1 {
            let a = &whole.pm.data()[((y * 240 + x) * 4) as usize..][..4];
            let b = &crop.pm.data()[(((y - y0) * (x1 - x0) + x - x0) * 4) as usize..][..4];
            assert_eq!(a, b, "({x}, {y})");
        }
    }
}

#[test]
fn redact_is_a_solid_box() {
    let redact = Item {
        shape: Shape::Redact {
            a: pt(10.0, 10.0),
            b: pt(60.0, 30.0),
        },
        colour: [0, 0, 0, 255],
        width: 4.0,
        outline: false,
        fill: false,
    };
    let out = render(&doc_with(field("hunter2"), vec![redact]));
    for y in 10..30 {
        for x in 10..60 {
            assert_eq!(px(&out, x, y), [0, 0, 0, 255]);
        }
    }
}

#[test]
fn spotlight_dims_only_outside() {
    let grey = |_, _| [200, 200, 200, 255];
    let spot = item(Shape::Spotlight {
        a: pt(30.0, 20.0),
        b: pt(80.0, 60.0),
    });
    let out = render(&doc_with(image_of(120, 90, grey), vec![spot.clone()]));
    assert_eq!(px(&out, 55, 40), [200, 200, 200, 255], "inside stays");
    assert_eq!(px(&out, 31, 40), [200, 200, 200, 255], "edge inside stays");
    for (x, y) in [(5, 5), (110, 80), (55, 10), (90, 40)] {
        let [r, g, b, a] = px(&out, x, y);
        assert!(r < 120 && r == g && g == b && a == 255, "({x}, {y}) {r}");
    }
    // Two spotlights: both areas stay bright, also where they overlap.
    let other = item(Shape::Spotlight {
        a: pt(60.0, 40.0),
        b: pt(110.0, 85.0),
    });
    let out = render(&doc_with(image_of(120, 90, grey), vec![spot, other]));
    for (x, y) in [(40, 30), (70, 50), (100, 80)] {
        assert_eq!(px(&out, x, y), [200, 200, 200, 255], "({x}, {y})");
    }
}

#[test]
fn crop_resizes_the_render() {
    let img = image_of(64, 48, |x, y| [x as u8, y as u8, 7, 255]);
    let mut doc = doc_with(img, vec![]);
    doc.exec(Edit::Crop(
        None,
        Some(Rectangle::new(pt(10.0, 6.0), Size::new(30.0, 20.0))),
    ));
    let out = render(&doc);
    assert_eq!((out.width, out.height), (30, 20));
    assert_eq!(px(&out, 0, 0), [10, 6, 7, 255]);
    assert_eq!(px(&out, 29, 19), [39, 25, 7, 255]);
    doc.undo();
    assert_eq!(render(&doc).width, 64);
}

#[test]
fn shapes_draw_their_stroke_and_leave_the_inside() {
    let white = |_, _| [255, 255, 255, 255];
    let rect = item(Shape::Rect {
        a: pt(10.0, 10.0),
        b: pt(60.0, 50.0),
    });
    let out = render(&doc_with(image_of(80, 60, white), vec![rect]));
    assert_eq!(px(&out, 35, 10), RED, "on the top edge");
    assert_eq!(px(&out, 35, 30), [255, 255, 255, 255], "inside");
    assert_eq!(px(&out, 35, 2), [255, 255, 255, 255], "outside");
}

#[test]
fn magnifier_shows_the_centre_zoomed() {
    // Left half black, right half white: zoomed 2x around x = 50, the edge stays at
    // the centre and a point 10 px right of it shows what was 5 px right (white).
    let img = image_of(
        100,
        100,
        |x, _| if x < 50 { [0, 0, 0, 255] } else { [255; 4] },
    );
    let lens = item(Shape::Magnifier {
        at: pt(50.0, 50.0),
        radius: 30.0,
    });
    let out = render(&doc_with(img, vec![lens]));
    assert_eq!(px(&out, 60, 50), [255; 4]);
    assert_eq!(px(&out, 40, 50), [0, 0, 0, 255]);
}

// ---- editor: every tool, undo / redo -------------------------------------------

fn editor(w: u32, h: u32) -> Annotator {
    let mut e = Annotator::new(checker(w, h), 1.0);
    e.assist = false; // not the user's config
    e
}

/// Press at `from`, drag through `via` to `to`, release.
fn drag(e: &mut Annotator, from: Point, via: &[Point], to: Point) {
    e.update(Message::Press(from));
    for &p in via {
        e.update(Message::Move(p));
    }
    e.update(Message::Move(to));
    e.update(Message::Release);
}

/// Draws one item with every tool; returns how many items it made.
/// (Each one clear of the last: a click on a text types into it.)
fn draw_with_every_tool(e: &mut Annotator) -> usize {
    let mut made = 0;
    for info in TOOLS {
        e.update(Message::Tool(info.tool));
        let base = pt(20.0 + 90.0 * made as f32, 30.0 + 25.0 * made as f32);
        match info.tool {
            Tool::Select | Tool::Crop => continue,
            // Click, then type (a step's note, a label's text...).
            Tool::Text | Tool::Label | Tool::Callout | Tool::Step => {
                drag(e, base, &[], base);
                e.update(Message::Type("Hi".into()));
                e.update(Message::Type("!".into()));
                e.update(Message::Backspace);
                e.update(Message::Finish);
            }
            Tool::ArrowLabel => {
                drag(e, base, &[], base + iced::Vector::new(60.0, 40.0));
                e.update(Message::Type("Hi".into()));
                e.update(Message::Finish);
            }
            _ => drag(
                e,
                base,
                &[base + iced::Vector::new(20.0, 5.0)],
                base + iced::Vector::new(60.0, 40.0),
            ),
        }
        made += 1;
        assert_eq!(e.doc.items.len(), made, "{:?}", info.tool);
    }
    made
}

#[test]
fn every_tool_makes_one_undoable_item() {
    let mut e = editor(1500, 500);
    let made = draw_with_every_tool(&mut e);
    assert_eq!(made, TOOLS.len() - 2);
    // Every tool but the variants of an existing shape (line, labelled arrow)
    // makes its own kind.
    let kinds: Vec<_> = (e.doc.items.iter().zip(TOOLS.iter().skip(2)))
        .filter(|(_, t)| !matches!(t.tool, Tool::Line | Tool::ArrowLabel))
        .map(|(i, _)| std::mem::discriminant(&i.shape))
        .collect();
    for (i, k) in kinds.iter().enumerate() {
        assert!(!kinds[i + 1..].contains(k), "two items of one kind");
    }
    let at = |tool: Tool| TOOLS.iter().position(|t| t.tool == tool).unwrap() - 2;
    assert!(matches!(
        &e.doc.items[at(Tool::Line)].shape,
        Shape::Arrow { head: false, .. }
    ));
    assert!(matches!(
        &e.doc.items[at(Tool::ArrowLabel)].shape,
        Shape::Arrow { head: true, text, .. } if text == "Hi"
    ));
    assert!(matches!(
        &e.doc.items[at(Tool::Step)].shape,
        Shape::Step { note, .. } if note == "Hi"
    ));
    let Shape::Text { text, .. } =
        &e.doc.items[TOOLS.iter().position(|t| t.tool == Tool::Text).unwrap() - 2].shape
    else {
        panic!()
    };
    assert_eq!(text, "Hi");

    let all = e.doc.items.clone();
    for n in (0..made).rev() {
        e.update(Message::Undo);
        assert_eq!(e.doc.items, all[..n]);
    }
    e.update(Message::Undo); // nothing left: no-op
    for n in 1..=made {
        e.update(Message::Redo);
        assert_eq!(e.doc.items, all[..n]);
    }
}

/// Ctrl+drag the way the canvas sends it (`Message::PressMove`).
fn ctrl_drag(e: &mut Annotator, from: Point, to: Point) {
    e.update(Message::PressMove(from));
    e.update(Message::Move(to));
    e.update(Message::Release);
}

/// A cancelled press (a lost touch) draws nothing; the same drag released does.
#[test]
fn a_cancelled_press_draws_nothing() {
    let mut e = editor(400, 300);
    e.update(Message::Tool(Tool::Rect));
    e.update(Message::Press(pt(50.0, 50.0)));
    e.update(Message::Move(pt(150.0, 120.0)));
    e.update(Message::Cancel);
    assert!(e.doc.items.is_empty() && e.pending.is_none());
    drag(&mut e, pt(50.0, 50.0), &[], pt(150.0, 120.0));
    assert_eq!(e.doc.items.len(), 1);
}

/// A drawing tool's plain drag always draws, even over an annotation; only the
/// Select tool or Ctrl+drag moves one (owner, 2026-10-31). The selection's
/// handles still resize with any tool; undo puts it all back.
#[test]
fn a_drawing_tool_draws_over_annotations_only_select_or_ctrl_moves() {
    use iced::mouse::Interaction;
    let mut e = editor(400, 300);
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(50.0, 50.0), &[], pt(150.0, 120.0));
    let drawn = e.doc.items[0].clone();
    // The move cursor only with Ctrl held.
    assert_eq!(e.cursor(pt(150.0, 85.0), false), Interaction::Crosshair);
    assert_eq!(e.cursor(pt(150.0, 85.0), true), Interaction::Move);
    assert_eq!(e.cursor(pt(300.0, 250.0), true), Interaction::Crosshair);

    // Every drawing tool, dragged from the box's edge: a new item, the box stays.
    let mut n = 1;
    for tool in [
        Tool::Rect,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Pen,
        Tool::Highlighter,
        Tool::Blur,
        Tool::Redact,
        Tool::Callout,
    ] {
        e.update(Message::Tool(tool));
        e.doc.selected = None; // (the last one's handles are right there)
        drag(
            &mut e,
            pt(150.0, 80.0),
            &[pt(170.0, 95.0)],
            pt(190.0, 110.0),
        );
        e.update(Message::Type("x".into())); // (a callout is typed)
        e.update(Message::Finish);
        n += 1;
        assert_eq!(e.doc.items.len(), n, "{tool:?}: drew");
        assert_eq!(e.doc.items[0], drawn, "{tool:?}: never moved");
    }
    for _ in 1..n {
        e.update(Message::Undo);
    }
    assert_eq!(e.doc.items, vec![drawn.clone()]);

    // A highlighter over an arrow draws (the owner's case), Ctrl+drag moves it.
    e.update(Message::Tool(Tool::Arrow));
    drag(&mut e, pt(50.0, 250.0), &[], pt(250.0, 250.0));
    let arrow = e.doc.items[1].clone();
    e.update(Message::Tool(Tool::Highlighter));
    drag(
        &mut e,
        pt(100.0, 250.0),
        &[pt(150.0, 251.0)],
        pt(200.0, 250.0),
    );
    assert_eq!(e.doc.items.len(), 3);
    assert_eq!(e.doc.items[1], arrow);
    e.update(Message::Undo);
    ctrl_drag(&mut e, pt(150.0, 250.0), pt(150.0, 270.0));
    assert_eq!(
        e.doc.items[1],
        select::moved(&arrow, iced::Vector::new(0.0, 20.0))
    );
    assert_eq!(e.doc.items.len(), 2);

    // Select moves by the edge, without Ctrl.
    e.update(Message::Tool(Tool::Select));
    assert_eq!(e.cursor(pt(150.0, 85.0), false), Interaction::Grab);
    drag(&mut e, pt(150.0, 80.0), &[pt(160.0, 85.0)], pt(170.0, 90.0));
    assert_eq!(e.doc.items.len(), 2);
    let moved = select::moved(&drawn, iced::Vector::new(20.0, 10.0));
    assert_eq!(e.doc.items[0], moved);

    // The selected box's corner handle resizes it with a drawing tool active.
    e.update(Message::Tool(Tool::Rect));
    let f = e.doc.items[0].frame();
    let corner = pt(f.x + f.width, f.y + f.height);
    drag(&mut e, corner, &[], corner + iced::Vector::new(20.0, 10.0));
    assert_eq!(e.doc.items.len(), 2);
    assert_eq!(
        e.doc.items[0].frame().size(),
        f.size() + Size::new(20.0, 10.0)
    );

    // Undo: resize, select move, Ctrl move, arrow.
    for _ in 0..4 {
        e.update(Message::Undo);
    }
    assert_eq!(e.doc.items, vec![drawn]);
}

/// A text tool's click on its own text types into it; a drag there never moves
/// it (it starts a new text); Ctrl+drag moves it.
#[test]
fn text_edits_on_click_and_moves_only_with_ctrl() {
    let mut e = editor(300, 200);
    e.update(Message::Tool(Tool::Text));
    drag(&mut e, pt(20.0, 20.0), &[], pt(20.0, 20.0));
    e.update(Message::Type("Hi".into()));
    e.update(Message::Finish);
    let Shape::Text { at, .. } = e.doc.items[0].shape else {
        panic!()
    };
    let c = e.doc.items[0].frame().center(); // clear of its corner handles
    let to = c + iced::Vector::new(40.0, 30.0);
    drag(&mut e, c, &[], to);
    assert!(matches!(e.doc.items[0].shape, Shape::Text { at: a, .. } if a == at));
    assert!(e.typing(), "a new text is typed");
    e.update(Message::Finish); // empty: dropped
    assert_eq!(e.doc.items.len(), 1);
    // A click types into it.
    drag(&mut e, c, &[], c);
    e.update(Message::Type("!".into()));
    e.update(Message::Finish);
    assert!(matches!(&e.doc.items[0].shape, Shape::Text { text, .. } if text == "Hi!"));
    // Ctrl+drag moves it.
    ctrl_drag(&mut e, c, to);
    let Shape::Text { at: moved, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert!(moved.distance(at + iced::Vector::new(40.0, 30.0)) < 0.01);
    assert_eq!(e.doc.items.len(), 1);
}

/// Pen and highlighter strokes show no selection frame or handles (drawn,
/// selected or moved); their own tool draws over them, Ctrl+drag moves one by
/// the stroke, undo puts it back, Delete removes it. Shapes keep their handles.
#[test]
fn freehand_strokes_have_no_frame_and_move_by_the_stroke_with_ctrl() {
    let canvas_px =
        |e: &Annotator| crate::shared::kit::testing::shot::save("", e.canvas(), e.size());
    let wave = |y: f32| -> Vec<Point> {
        (0..40)
            .map(|i| pt(40.0 + 5.0 * i as f32, y + 12.0 * (i as f32 / 3.0).sin()))
            .collect()
    };
    for tool in [Tool::Pen, Tool::Highlighter] {
        let mut e = editor(300, 200);
        e.update(Message::Tool(tool));
        let w = wave(60.0);
        e.update(Message::Press(w[0]));
        e.update(Message::Move(w[10]));
        assert!(e.framed().is_none(), "{tool:?}: no frame while drawn");
        for &p in &w[11..] {
            e.update(Message::Move(p));
        }
        e.update(Message::Release);
        assert_eq!(e.doc.selected, Some(0));
        assert!(e.framed().is_none(), "{tool:?}");
        let drawn = e.doc.items[0].clone();
        // The render with it selected is the render without a selection.
        let selected = canvas_px(&e);
        e.doc.selected = None;
        assert_eq!(
            canvas_px(&e),
            selected,
            "{tool:?}: no frame or handles drawn"
        );
        assert_eq!(e.cursor(w[20], true), iced::mouse::Interaction::Move);
        assert_eq!(e.cursor(w[20], false), iced::mouse::Interaction::Crosshair);

        // A drag on the stroke (2 px off) with its own tool: a new stroke.
        let on = w[20] + iced::Vector::new(0.0, 2.0);
        let away = on + iced::Vector::new(10.0, 70.0);
        drag(&mut e, on, &[on + iced::Vector::new(5.0, 20.0)], away);
        assert_eq!(e.doc.items.len(), 2, "{tool:?}");
        assert_eq!(e.doc.items[0], drawn);
        e.update(Message::Undo);
        // Ctrl+drag: moved, nothing new.
        ctrl_drag(&mut e, on, away);
        assert_eq!(e.doc.items.len(), 1, "{tool:?}");
        let (now, was) = (e.doc.items[0].frame(), drawn.frame());
        assert!((now.x - was.x - 10.0).abs() < 0.01 && (now.y - was.y - 70.0).abs() < 0.01);
        assert!(e.framed().is_none());
        e.update(Message::Undo);
        assert_eq!(e.doc.items[0], drawn);
        // Delete removes the one Ctrl picked up.
        e.update(Message::PressMove(on));
        e.update(Message::Release);
        e.update(Message::Delete);
        assert!(e.doc.items.is_empty(), "{tool:?}");
    }
    // A box keeps its handles.
    let mut e = editor(300, 200);
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(40.0, 40.0), &[], pt(140.0, 120.0));
    assert!(e.framed().is_some());
}

/// Every toolbar popover stays on screen: above the toolbar when it sits at the
/// screen's bottom, pulled in from the sides.
#[test]
fn popovers_open_above_a_toolbar_at_the_bottom_and_stay_on_screen() {
    let screen = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let on_screen = |r: Rectangle| {
        r.x >= 0.0 && r.y >= 0.0 && r.x + r.width <= 800.0 && r.y + r.height <= 600.0
    };
    // A 52 px toolbar at y 540..592: laid out under it, the popover is off screen.
    let under = Rectangle::new(pt(650.0, 600.0), Size::new(300.0, 260.0));
    let shift = toolbar::place_popover(under, 52.0, screen);
    let placed = Rectangle::new(under.position() + shift, under.size());
    assert!(on_screen(placed), "{placed:?}");
    assert_eq!(
        placed.y + placed.height,
        600.0 - 8.0 - 52.0 - 8.0,
        "just above it"
    );
    assert_eq!(placed.x, 492.0, "pulled in from the right edge, 8 px clear");
    // Room below: where it was laid out.
    let under = Rectangle::new(pt(100.0, 70.0), Size::new(300.0, 260.0));
    assert_eq!(
        toolbar::place_popover(under, 52.0, screen),
        iced::Vector::ZERO
    );
}

/// The Text tool's options (its button's strip): size, font, bold and backdrop
/// apply to the next text and to the selected one, and change what is drawn.
#[test]
fn text_options_style_the_text() {
    let mut e = editor(400, 200);
    e.update(Message::Slot(Tool::Text));
    e.update(Message::Slot(Tool::Text)); // the active tool: its options
    assert_eq!(e.menu, Some(Menu::Options(Group::Text)));
    let _ = e.toolbar(); // the options build
    e.update(Message::TextSize(38.0));
    let boxed = TextStyle {
        font: text::Font::Serif,
        bold: false,
        back: text::Back::Box,
    };
    e.update(Message::TextStyle(boxed));
    drag(&mut e, pt(20.0, 20.0), &[], pt(20.0, 20.0));
    e.update(Message::Type("Note".into()));
    e.update(Message::Finish);
    assert!(
        matches!(e.doc.items[0].shape, Shape::Text { size, style, .. } if size == 38.0 && style == boxed)
    );
    // A box paints behind the glyphs: more of the frame changes than plain.
    let frame = e.doc.items[0].frame();
    let changed = |e: &Annotator| {
        let plain = render(&doc_with(checker(400, 200), vec![]));
        let c = render(&e.doc);
        let mut n = 0;
        for y in frame.y as u32..(frame.y + frame.height) as u32 {
            for x in frame.x as u32..(frame.x + frame.width) as u32 {
                n += usize::from(px(&c, x, y) != px(&plain, x, y));
            }
        }
        n
    };
    let with_box = changed(&e);
    // The selected text takes a new style (one undoable change).
    let none = TextStyle {
        back: text::Back::None,
        ..boxed
    };
    e.update(Message::TextStyle(none));
    assert!(matches!(e.doc.items[0].shape, Shape::Text { style, .. } if style == none));
    assert!(changed(&e) * 2 < with_box, "the box is gone");
    e.update(Message::Undo);
    assert!(matches!(e.doc.items[0].shape, Shape::Text { style, .. } if style == boxed));
    // Bold is wider than regular.
    let regular = text::measure_styled("Note", 38.0, none);
    let bold = text::measure_styled("Note", 38.0, TextStyle { bold: true, ..none });
    assert!(bold.width > regular.width);
}

#[test]
fn move_resize_bend_delete_and_crop_undo() {
    let mut e = editor(400, 300);
    e.update(Message::Tool(Tool::Arrow));
    drag(&mut e, pt(50.0, 50.0), &[], pt(150.0, 50.0));
    let drawn = e.doc.items[0].clone();
    assert_eq!(e.doc.selected, Some(0));

    // Bend: drag the middle handle down 30 px.
    drag(&mut e, pt(100.0, 50.0), &[], pt(100.0, 80.0));
    let Shape::Arrow { a, b, bend, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert!(arrow::middle(a, b, bend).distance(pt(100.0, 80.0)) < 0.01);

    // Move with the select tool.
    e.update(Message::Tool(Tool::Select));
    drag(&mut e, pt(60.0, 52.0), &[pt(65.0, 55.0)], pt(70.0, 62.0));
    let Shape::Arrow { a, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!(a, pt(60.0, 60.0));

    // Its head dragged out: twice as long.
    let Shape::Arrow { a, b, .. } = e.doc.items[0].shape else {
        panic!()
    };
    drag(&mut e, b, &[], b + (b - a));
    assert!(matches!(e.doc.items[0].shape, Shape::Arrow { b: to, .. } if to == b + (b - a)));

    e.update(Message::Delete);
    assert!(e.doc.items.is_empty());

    e.update(Message::Tool(Tool::Crop));
    drag(&mut e, pt(10.0, 10.0), &[], pt(110.0, 60.0));
    assert_eq!(e.size(), Size::new(100.0, 50.0));

    for _ in 0..5 {
        e.update(Message::Undo);
    }
    assert_eq!(e.doc.items, vec![drawn.clone()]);
    assert_eq!(e.doc.crop, None);
    for _ in 0..5 {
        e.update(Message::Redo);
    }
    assert!(e.doc.items.is_empty());
    assert!(e.doc.crop.is_some());
}

#[test]
fn steps_number_themselves_and_renumber_after_delete() {
    let steps_at = |xs: &[f32]| {
        let mut e = Annotator::new(image_of(200, 60, |_, _| [255; 4]), 1.0);
        e.update(Message::Tool(Tool::Step));
        for &x in xs {
            drag(&mut e, pt(x, 30.0), &[], pt(x, 30.0));
        }
        e.update(Message::Finish); // the last one waits for its note
        e
    };
    // The last badge's pixels.
    let last = |c: &Crop| -> Vec<[u8; 4]> {
        (10..50)
            .flat_map(|y| (150..190).map(move |x| (x, y)))
            .map(|(x, y)| px(c, x, y))
            .collect()
    };
    let mut e = steps_at(&[30.0, 100.0, 170.0]);
    assert_eq!(e.doc.items.len(), 3);
    let three = render(&e.doc);

    // Deleting the middle badge: the last one now reads "2", like a fresh second step.
    e.update(Message::Tool(Tool::Select));
    drag(&mut e, pt(100.0, 30.0), &[], pt(100.0, 30.0));
    e.update(Message::Delete);
    let after = render(&e.doc);
    assert_ne!(last(&three), last(&after));
    assert_eq!(last(&after), last(&render(&steps_at(&[30.0, 170.0]).doc)));

    // Undo brings "3" back.
    e.update(Message::Undo);
    assert_eq!(last(&render(&e.doc)), last(&three));
}

// ---- live layer == committed result ------------------------------------------------

fn live_pixels(e: &Annotator) -> (Rectangle, Vec<u8>) {
    let (r, handle) = e.live.as_ref().expect("live layer");
    let image::Handle::Rgba { pixels, .. } = handle else {
        panic!()
    };
    (*r, pixels.to_vec())
}

#[test]
fn the_live_preview_matches_the_committed_result() {
    for tool in [Tool::Blur, Tool::Arrow, Tool::Highlighter, Tool::Magnifier] {
        let mut e = editor(300, 200);
        e.update(Message::Tool(Tool::Rect));
        drag(&mut e, pt(40.0, 40.0), &[], pt(200.0, 150.0)); // something underneath
        e.update(Message::Tool(tool));
        e.update(Message::Press(pt(60.0, 50.0)));
        e.update(Message::Move(pt(120.0, 90.0)));
        e.update(Message::Move(pt(170.0, 130.0)));
        let (r, live) = live_pixels(&e);
        e.update(Message::Release);
        let flat = render(&e.doc);
        let region: Vec<u8> = (r.y as u32..(r.y + r.height) as u32)
            .flat_map(|y| (r.x as u32..(r.x + r.width) as u32).map(move |x| (x, y)))
            .flat_map(|(x, y)| px(&flat, x, y))
            .collect();
        // Identical but for anti-aliasing where another item's stroke is clipped by
        // the region's edge (tiny-skia rasterises clipped paths a hair differently).
        let differ = live
            .chunks(4)
            .zip(region.chunks(4))
            .filter(|(a, b)| a != b)
            .inspect(|(a, b)| {
                let off = a.iter().zip(*b).map(|(x, y)| x.abs_diff(*y)).max();
                assert!(off < Some(32), "{tool:?}: {a:?} vs {b:?}");
            })
            .count();
        assert!(differ < 8, "{tool:?}: {differ} pixels differ in {r:?}");
    }
}

// ---- keys -------------------------------------------------------------------------

#[test]
fn keys_pick_tools_shortcuts_and_type_text() {
    let mut e = editor(100, 100);
    let ctrl = Modifiers::CTRL;
    let letter = |c: &str| Key::Character(c.into());
    let key = |e: &Annotator, k: &Key, m: Modifiers| {
        let latin = match k {
            Key::Character(s) => s.chars().next(),
            _ => None,
        };
        e.key(k, latin, m, latin.map(|_| "x"))
    };
    for t in TOOLS {
        let m = key(&e, &letter(&t.key.to_string()), Modifiers::empty());
        assert!(
            matches!(m, Some(Message::Tool(tool)) if tool == t.tool),
            "{}",
            t.key
        );
    }
    assert!(matches!(key(&e, &letter("z"), ctrl), Some(Message::Undo)));
    assert!(matches!(
        key(&e, &letter("z"), ctrl | Modifiers::SHIFT),
        Some(Message::Redo)
    ));
    assert!(matches!(key(&e, &letter("c"), ctrl), Some(Message::Copy)));
    assert!(matches!(key(&e, &letter("s"), ctrl), Some(Message::Save)));
    // Pin is Ctrl+P; plain P stays the Pen tool.
    assert!(matches!(key(&e, &letter("p"), ctrl), Some(Message::Pin)));
    assert!(matches!(
        key(&e, &letter("p"), Modifiers::empty()),
        Some(Message::Tool(Tool::Pen))
    ));
    assert!(matches!(
        key(&e, &Key::Named(Named::Escape), Modifiers::empty()),
        Some(Message::Back)
    ));
    assert!(matches!(
        key(&e, &Key::Named(Named::Delete), Modifiers::empty()),
        Some(Message::Delete)
    ));

    // Typing: letters are text, Esc ends it.
    e.update(Message::Tool(Tool::Text));
    drag(&mut e, pt(10.0, 10.0), &[], pt(10.0, 10.0));
    assert!(matches!(
        key(&e, &letter("r"), Modifiers::empty()),
        Some(Message::Type(_))
    ));
    assert!(matches!(
        key(&e, &Key::Named(Named::Escape), Modifiers::empty()),
        Some(Message::Finish)
    ));

    // The picker's hex field has the keys; Esc closes the picker.
    e.update(Message::Finish);
    e.update(Message::Picker);
    assert!(key(&e, &letter("r"), Modifiers::empty()).is_none());
    assert!(matches!(
        key(&e, &Key::Named(Named::Escape), Modifiers::empty()),
        Some(Message::Picker)
    ));
    e.update(Message::Hex("#00ff00".into()));
    assert_eq!(e.colour, [0, 255, 0, 255]);
}

#[test]
fn every_icon_is_in_the_subset_font() {
    let script = include_str!("../../../assets/lucide/subset.sh");
    let icons = TOOLS
        .iter()
        .map(|t| t.icon)
        .chain(toolbar::GLYPHS.iter().copied())
        .chain(crate::ui::screens::tools::GLYPHS.iter().copied())
        .chain(['\u{e13e}', crate::shared::kit::icon::Icon::Grip.glyph()]);
    for icon in icons {
        let code = format!("U+{:04X}", icon as u32);
        assert!(script.contains(&code), "add {code} to subset.sh");
    }
}

// ---- steps' legend -------------------------------------------------------------------

/// Places a step at `p` and types its note.
fn step_with(e: &mut Annotator, p: Point, note: &str) {
    e.update(Message::Tool(Tool::Step));
    drag(e, p, &[], p);
    e.update(Message::Type(note.into()));
    e.update(Message::Finish);
}

const SLATE: [u8; 4] = [40, 44, 60, 255];

#[test]
fn legend_lists_the_notes_in_a_free_corner_or_a_band() {
    let mut e = Annotator::new(image_of(400, 300, |_, _| SLATE), 1.0);
    step_with(&mut e, pt(40.0, 40.0), "Open settings");
    step_with(&mut e, pt(100.0, 60.0), ""); // numbered, not listed
    step_with(&mut e, pt(160.0, 80.0), "Click Save");
    let l = e.doc.legend_layout().expect("a legend");
    let rows: Vec<_> = l.rows.iter().map(|r| (r.number, r.note.as_str())).collect();
    assert_eq!(rows, [(1, "Open settings"), (3, "Click Save")]);
    // Bottom-right is free: the box sits there, inside the image.
    assert_eq!(l.band, 0.0);
    assert!(l.at.x > 200.0 && l.at.y > 150.0, "{:?}", l.at);
    assert!(l.at.x + l.at.width < 400.0 && l.at.y + l.at.height < 300.0);

    // Flattened: the paper shows in the box, nowhere else changes size.
    let flat = render(&e.doc);
    assert_eq!((flat.width, flat.height), (400, 300));
    let inside = (
        (l.at.x + l.at.width - 4.0) as u32,
        (l.at.y + l.at.height / 2.0) as u32,
    );
    assert!(luma(px(&flat, inside.0, inside.1)) > 200, "paper");
    // Off: gone.
    e.update(Message::Legend);
    assert!(e.doc.legend_layout().is_none());
    assert_eq!(px(&render(&e.doc), inside.0, inside.1), SLATE);
    e.update(Message::Legend);

    // A box over every corner: the legend moves to a band under the image.
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(6.0, 6.0), &[], pt(394.0, 294.0));
    let l = e.doc.legend_layout().unwrap();
    assert!(l.band > 20.0, "{}", l.band);
    let flat = render(&e.doc);
    assert_eq!((flat.width, flat.height), (400, 300 + l.band as u32));
    assert_eq!(e.size(), Size::new(400.0, 300.0 + l.band));
    assert_eq!(
        px(&flat, 395, 300 + l.band as u32 - 3),
        [255, 255, 255, 255]
    );
    // The band's rows: a badge in the first step's colour, then dark text.
    let badge = (0..flat.width)
        .flat_map(|x| (300..flat.height).map(move |y| (x, y)))
        .filter(|&(x, y)| px(&flat, x, y) == RED)
        .count();
    assert!(badge > 50, "{badge}");
}

#[test]
fn copy_is_the_image_only_with_or_without_shift() {
    let mut e = editor(300, 200);
    step_with(&mut e, pt(30.0, 30.0), "Open settings");
    for mods in [Modifiers::CTRL, Modifiers::CTRL | Modifiers::SHIFT] {
        let key = e.key(&Key::Character("c".into()), Some('c'), mods, None);
        assert!(matches!(key, Some(Message::Copy)), "{mods:?}");
    }
    let Some(Output::Copy(flat)) = e.update(Message::Copy) else {
        panic!("Copy gives the picture and nothing else")
    };
    assert!(flat.width == 300 && flat.height >= 200, "with the legend");
}

#[test]
fn the_scroll_button_asks_only_when_scrolling_can_run() {
    let mut e = editor(300, 200);
    assert!(e.update(Message::Scroll).is_none(), "dimmed: nothing");
    e.scroll = true;
    assert!(matches!(e.update(Message::Scroll), Some(Output::Scroll)));
}

// ---- new tools ------------------------------------------------------------------------

#[test]
fn lines_snap_to_45_degrees_with_shift() {
    let snapped = arrow::snap_angle(pt(0.0, 0.0), pt(100.0, 7.0));
    assert!(
        snapped.y.abs() < 1e-3 && (snapped.x - 100.24).abs() < 0.01,
        "{snapped:?}"
    );
    let snapped = arrow::snap_angle(pt(10.0, 10.0), pt(60.0, 57.0));
    assert!(((snapped.x - 10.0) - (snapped.y - 10.0)).abs() < 1e-3);
    let snapped = arrow::snap_angle(pt(0.0, 0.0), pt(3.0, -80.0));
    assert!(snapped.x.abs() < 1e-3 && snapped.y < -79.0);

    let mut e = editor(200, 100);
    e.update(Message::Tool(Tool::Line));
    e.update(Message::Shift(true));
    drag(&mut e, pt(10.0, 10.0), &[pt(60.0, 14.0)], pt(110.0, 17.0));
    let Shape::Arrow { a, b, head, .. } = &e.doc.items[0].shape else {
        panic!()
    };
    assert!(!head && (b.y - a.y).abs() < 1e-3, "{a:?} {b:?}");
    // Without Shift it goes where the pointer is.
    e.update(Message::Shift(false));
    drag(&mut e, pt(10.0, 60.0), &[], pt(110.0, 67.0));
    let Shape::Arrow { b, .. } = &e.doc.items[1].shape else {
        panic!()
    };
    assert_eq!(*b, pt(110.0, 67.0));
}

#[test]
fn label_text_contrasts_with_its_fill() {
    use crate::shared::theme::ANNOTATION;
    for (i, dark_text) in [(2, true), (9, true), (5, false), (11, false), (0, false)] {
        let colour = ANNOTATION[i].into_rgba8();
        let at = pt(10.0, 10.0);
        let label = Item {
            shape: Shape::Label {
                at,
                text: "Hello".into(),
                size: 24.0,
            },
            colour,
            width: 4.0,
            outline: false,
            fill: false,
        };
        let out = render(&doc_with(image_of(160, 60, |_, _| SLATE), vec![label]));
        let origin = label::text_origin(at, 24.0);
        let glyphs: Vec<u32> = (origin.y as u32..origin.y as u32 + 28)
            .flat_map(|y| (origin.x as u32..origin.x as u32 + 60).map(move |x| (x, y)))
            .map(|(x, y)| px(&out, x, y))
            .filter(|p| *p != colour)
            .map(luma)
            .collect();
        assert!(glyphs.len() > 30, "{i}: text drawn");
        if dark_text {
            assert!(
                glyphs.iter().any(|&l| l < 60),
                "{i}: dark text on a light fill"
            );
        } else {
            assert!(
                glyphs.iter().any(|&l| l > 220),
                "{i}: white text on a dark fill"
            );
        }
        assert_eq!(document::on(colour)[0] < 128, dark_text);
    }
}

#[test]
fn pointer_and_label_are_drawn_and_editable() {
    let mut e = editor(300, 200);
    e.update(Message::Tool(Tool::Pointer));
    drag(&mut e, pt(150.0, 120.0), &[], pt(150.0, 120.0));
    assert!(matches!(e.doc.items[0].shape, Shape::Pointer { .. }));
    // Its arrow comes from up-left.
    let f = e.doc.items[0].frame();
    assert!(f.x < 120.0 && f.y < 90.0, "{f:?}");
    e.update(Message::Tool(Tool::Label));
    drag(&mut e, pt(20.0, 20.0), &[], pt(20.0, 20.0));
    e.update(Message::Type("Tag".into()));
    e.update(Message::Finish);
    // Clicking the label with its tool edits it.
    drag(&mut e, pt(35.0, 30.0), &[], pt(35.0, 30.0));
    e.update(Message::Type("!".into()));
    e.update(Message::Finish);
    assert!(matches!(&e.doc.items[1].shape, Shape::Label { text, .. } if text == "Tag!"));
    assert_eq!(e.doc.items.len(), 2);
}

/// The grouped toolbar (owner, 2026-11-02): a group button picks its last
/// tool; a click on the active one opens its panel, where another of the
/// group's tools switches and keeps it open; Esc closes it first.
#[test]
fn group_buttons_pick_their_last_tool_and_open_one_panel() {
    let mut e = editor(100, 100);
    assert_eq!(e.tool, Tool::Arrow);
    assert_eq!(
        e.last,
        [Tool::Select, Tool::Arrow, Tool::Pen, Tool::Text, Tool::Blur]
    );
    // The active group's button: its panel; again: closed.
    e.update(Message::Slot(Tool::Arrow));
    assert_eq!(e.menu, Some(Menu::Options(Group::Shapes)));
    // a tool of the panel's row: switched, the panel stays (its options)
    e.update(Message::Tool(Tool::Rect));
    assert_eq!(
        (e.tool, e.menu),
        (Tool::Rect, Some(Menu::Options(Group::Shapes)))
    );
    e.update(Message::Slot(Tool::Rect));
    assert_eq!(e.menu, None);
    // Hide's button shows Blur; its first click picks it, no panel.
    e.update(Message::Slot(e.last[Group::Hide as usize]));
    assert_eq!((e.tool, e.menu), (Tool::Blur, None));
    // A key letter: that tool, its group's last from now on.
    e.update(Message::Tool(Tool::Ellipse));
    assert_eq!(e.last[Group::Shapes as usize], Tool::Ellipse);
    e.update(Message::Slot(e.last[Group::Text as usize]));
    assert_eq!(e.tool, Tool::Text);
    // Shapes shows Ellipse now: a click goes back to it.
    e.update(Message::Slot(e.last[Group::Shapes as usize]));
    assert_eq!(e.tool, Tool::Ellipse);
    // Text's option Background is Label: the Text button stays its group.
    e.update(Message::Tool(Tool::Label));
    e.update(Message::Slot(e.last[Group::Text as usize]));
    assert_eq!(
        (e.tool, e.menu),
        (Tool::Label, Some(Menu::Options(Group::Text)))
    );
    let _ = e.toolbar(); // the panel builds
    // Esc closes a panel before it leaves the editor; Select has none.
    let esc = |e: &Annotator| e.key(&Key::Named(Named::Escape), None, Modifiers::empty(), None);
    assert!(matches!(
        esc(&e),
        Some(Message::Menu(Menu::Options(Group::Text)))
    ));
    e.update(Message::Menu(Menu::Options(Group::Text)));
    assert!(matches!(esc(&e), Some(Message::Back)));
    e.update(Message::Slot(Tool::Select));
    e.update(Message::Slot(Tool::Select));
    assert_eq!(e.menu, None, "Select has no panel");
    // Every tool is in a group's panel or an option of one there (Crop:
    // the area's handles).
    for t in TOOLS.iter().filter(|t| t.tool != Tool::Crop) {
        assert!(t.group.tools().any(|s| s.tool == t.slot), "{:?}", t.tool);
    }
    // Remembered across sessions: the next editor's buttons show them.
    let saved = e.remembered();
    assert!(Group::valid(saved.group_tools.as_deref().unwrap()));
    let next = Annotator::restored(checker(10, 10), 1.0, saved);
    assert_eq!(next.last, e.last);
}

/// Fill applies to rectangles and ellipses only: lines and arrows show it
/// dimmed (the panel doesn't jump); Outline everywhere in Shapes.
#[test]
fn fill_is_greyed_for_lines_and_arrows() {
    use crate::shared::kit::testing::shot;
    for (tool, fills) in [
        (Tool::Rect, true),
        (Tool::Ellipse, true),
        (Tool::Line, false),
        (Tool::Arrow, false),
    ] {
        assert_eq!(fills_inside(tool), fills, "{tool:?}");
        let mut e = editor(100, 100);
        e.update(Message::Tool(tool));
        e.update(Message::Slot(tool));
        let texts = shot::texts(e.toolbar());
        assert!(texts.iter().any(|t| t == "Fill"), "{texts:?}");
        assert_eq!(
            texts.iter().any(|t| t == "rectangle, ellipse"),
            !fills,
            "{texts:?}"
        );
    }
}

/// Undo undoes; a second click right after opens its panel (Undo, Redo,
/// where the history is) instead of undoing again; any other change ends it.
#[test]
fn undos_second_click_opens_its_panel() {
    let mut e = editor(200, 100);
    for x in [20.0, 60.0, 100.0] {
        drag(&mut e, pt(x, 20.0), &[], pt(x + 30.0, 60.0));
    }
    assert_eq!(e.doc.steps(), (3, 3));
    e.update(Message::UndoClick);
    assert_eq!((e.doc.steps(), e.menu), ((2, 3), None));
    e.update(Message::UndoClick);
    assert_eq!((e.doc.steps(), e.menu), ((2, 3), Some(Menu::Undo)));
    let texts = crate::shared::kit::testing::shot::texts(e.toolbar());
    assert!(texts.iter().any(|t| t == "2 of 3 steps"), "{texts:?}");
    e.update(Message::Redo);
    assert_eq!(e.doc.steps(), (3, 3));
    e.update(Message::Menu(Menu::Undo));
    // a new drawing: Undo's click undoes again
    e.update(Message::Tool(Tool::Rect));
    e.update(Message::UndoClick);
    assert_eq!((e.doc.steps(), e.menu), ((2, 3), None));
}

/// No close button (owner, 2026-11-02): Esc closes; Copy and Save are
/// icon-only (their tooltips say them and their keys).
#[test]
fn the_toolbar_has_no_close_button() {
    use crate::shared::kit::tooltip::tests_support::shown;
    let mut e = editor(100, 100);
    e.movable = true;
    let _ = shown();
    let _ = e.toolbar();
    let tips = shown();
    assert!(!tips.iter().any(|t| t.starts_with("Close")), "{tips:?}");
    assert!(tips.contains(&"Copy · Ctrl+C".to_string()), "{tips:?}");
    assert!(tips.contains(&"Save · Ctrl+S".to_string()), "{tips:?}");
    assert!(tips.contains(&"Move toolbar".to_string()), "{tips:?}");
    assert!(tips.contains(&"Shapes · Arrow (A)".to_string()), "{tips:?}");
    assert!(
        !toolbar::stops(&e)
            .iter()
            .any(|m| matches!(m, Message::Back))
    );
}

/// The laid-out size of `element` (logical px).
fn laid_out(element: Element<'_, Message>) -> Size {
    use iced::advanced::layout;
    use iced::advanced::widget::Tree;
    let mut element = element;
    let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(4000.0, 2000.0)),
    );
    node.size()
}

/// One bar, the same size in every state (about half the 0.1.3 dev bar's
/// 1129 px: the design's grouped bar is 553).
#[test]
fn toolbar_size_matches_its_layout() {
    let mut e = editor(100, 100);
    e.movable = true;
    let one = toolbar::size();
    assert_eq!(laid_out(e.toolbar()), one);
    e.update(Message::UndoClick);
    e.update(Message::Tool(Tool::Blur));
    assert_eq!(laid_out(e.toolbar_at(Rectangle::default()).0), one);
    assert!(one.width < 600.0 && one.width > 500.0, "{one:?}");
}

// ---- presets, copy / paste style, eyedropper -------------------------------------------

#[test]
fn presets_style_the_active_tool() {
    use crate::shared::theme::ANNOTATION;
    let colour = |i: usize| ANNOTATION[i].into_rgba8();
    let mut e = editor(200, 150);
    let key = |e: &Annotator, c: char| {
        e.key(
            &Key::Character(c.to_string().into()),
            Some(c),
            Modifiers::empty(),
            None,
        )
    };
    assert!(matches!(key(&e, '3'), Some(Message::Preset(2))));
    assert!(key(&e, '9').is_none(), "no ninth preset");

    // Blue callout from the arrow: the callout tool, blue.
    e.update(Message::Preset(2));
    assert_eq!((e.tool, e.colour), (Tool::Callout, colour(5)));
    // Red box + arrow keeps the arrow (it suits it), red.
    e.update(Message::Tool(Tool::Arrow));
    e.update(Message::Width(Width::Thick));
    e.update(Message::Preset(0));
    assert_eq!(
        (e.tool, e.colour, e.width),
        (Tool::Arrow, colour(0), Width::Medium)
    );

    // Redact on a selected box: it becomes a solid black box.
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(20.0, 20.0), &[], pt(80.0, 60.0));
    e.update(Message::Preset(3));
    assert!(matches!(e.doc.items[0].shape, Shape::Redact { .. }));
    assert_eq!(e.doc.items[0].colour, colour(11));
    assert_eq!(e.tool, Tool::Redact);
    assert_eq!(px(&render(&e.doc), 50, 40), colour(11));
    // One undo brings the box back.
    e.update(Message::Undo);
    assert!(matches!(e.doc.items[0].shape, Shape::Rect { .. }));

    // Highlight yellow: the highlighter; the selected item is left alone.
    e.update(Message::Tool(Tool::Select));
    drag(&mut e, pt(20.0, 40.0), &[], pt(20.0, 40.0));
    let before = e.doc.items[0].clone();
    e.update(Message::Preset(1));
    assert_eq!((e.tool, e.colour), (Tool::Highlighter, colour(2)));
    assert_eq!(e.doc.items[0], before);
}

#[test]
fn copy_style_and_paste_it_on_another_item() {
    let mut e = editor(300, 200);
    e.update(Message::Colour([10, 132, 255, 255]));
    e.update(Message::Width(Width::Thick));
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(10.0, 10.0), &[], pt(60.0, 60.0));
    e.update(Message::CopyStyle);
    e.update(Message::Colour(RED));
    e.update(Message::Width(Width::Thin));
    e.update(Message::Tool(Tool::Text));
    drag(&mut e, pt(150.0, 100.0), &[], pt(150.0, 100.0));
    e.update(Message::Type("note".into()));
    e.update(Message::Finish);
    let thin = e.doc.items[1].clone();
    assert_eq!(style::level(&thin, 1.0), Width::Thin);
    e.update(Message::PasteStyle);
    let pasted = &e.doc.items[1];
    assert_eq!(pasted.colour, [10, 132, 255, 255]);
    assert_eq!(style::level(pasted, 1.0), Width::Thick);
    assert!(matches!(&pasted.shape, Shape::Text { size, .. } if *size == Width::Thick.font()));
}

#[test]
fn eyedropper_picks_a_colour_from_the_image() {
    let image = image_of(50, 40, |x, y| [(x * 5) as u8, (y * 6) as u8, 77, 255]);
    let mut e = Annotator::new(image, 1.0);
    let d = e.key(
        &Key::Character("d".into()),
        Some('d'),
        Modifiers::empty(),
        None,
    );
    assert!(matches!(d, Some(Message::Eyedropper)));
    e.update(Message::Picker);
    e.update(Message::Eyedropper);
    assert!(
        e.picking && e.menu.is_none(),
        "the popover closes to show the image"
    );
    let esc = e.key(&Key::Named(Named::Escape), None, Modifiers::empty(), None);
    assert!(matches!(esc, Some(Message::Eyedropper)), "Esc cancels");
    drag(&mut e, pt(10.5, 20.5), &[], pt(10.5, 20.5));
    assert_eq!(e.colour, [50, 120, 77, 255]);
    assert_eq!(e.hex, "#32784D");
    assert!(!e.picking && e.doc.items.is_empty(), "a pick draws nothing");
    // Outside the image: nothing picked.
    e.update(Message::Eyedropper);
    drag(&mut e, pt(80.0, 20.0), &[], pt(80.0, 20.0));
    assert_eq!(e.colour, [50, 120, 77, 255]);
    // The pick styles the selected item, like any colour.
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(5.0, 5.0), &[], pt(30.0, 30.0));
    e.update(Message::Eyedropper);
    drag(&mut e, pt(2.5, 1.5), &[], pt(2.5, 1.5));
    assert_eq!(e.doc.items[0].colour, [10, 6, 77, 255]);
}

// ---- drag-out ------------------------------------------------------------------------

#[test]
fn drag_out_writes_a_temp_png_that_goes_with_the_editor() {
    let crop = Crop {
        width: 2,
        height: 2,
        rgba: vec![
            1, 2, 3, 255, 4, 5, 6, 255, //
            7, 8, 9, 255, 10, 11, 12, 255,
        ],
    };
    let file = dragout::DragFile::write(&crop).unwrap();
    let path = file.path().to_owned();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    assert!(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("opencircle-drag-")
    );
    drop(file);
    assert!(!path.exists(), "deleted when dropped");

    // The bitmap: a 40-byte header, then bottom-up BGRA rows.
    let dib = dragout::dib(&crop);
    assert_eq!(dib.len(), 40 + 16);
    assert_eq!(&dib[..12], &[40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(&dib[14..16], &[32, 0]);
    assert_eq!(&dib[40..48], &[9, 8, 7, 255, 12, 11, 10, 255]);

    // The editor keeps one file (the next drag replaces it) until it closes.
    let mut e = editor(30, 20);
    e.update(Message::DragOut); // off Windows the OS drag reports "not yet"
    let first = e.dragged.as_ref().unwrap().path().to_owned();
    assert!(first.exists());
    e.update(Message::DragOut);
    let second = e.dragged.as_ref().unwrap().path().to_owned();
    assert!(!first.exists() && second.exists());
    drop(e);
    assert!(!second.exists(), "deleted with the editor");
}

// ---- blur size and line help -------------------------------------------------------------------

/// Text `lines` (top-left, text) at `size` on `bg`, in `fg`.
fn page(w: u32, h: u32, bg: [u8; 4], fg: [u8; 4], lines: &[(f32, f32, &str)]) -> Image {
    let mut layer = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(
            image_of(w, h, |_, _| bg).rgba,
            tiny_skia::IntSize::from_wh(w, h).unwrap(),
        )
        .unwrap(),
        origin: Point::ORIGIN,
    };
    for &(x, y, line) in lines {
        text::paint(&mut layer, pt(x, y), line, 20.0, fg, false);
    }
    Image {
        width: w,
        height: h,
        rgba: layer.pm.take(),
    }
}

const PAPER: [u8; 4] = [250, 250, 250, 255];
const INK: [u8; 4] = [20, 20, 30, 255];
const PARAGRAPH: [(f32, f32, &str); 3] = [
    (10.0, 10.0, "The first line of the note"),
    (10.0, 50.0, "a second one, just as long"),
    (10.0, 90.0, "and a short third"),
];

#[test]
fn text_lines_are_found_light_dark_and_in_columns() {
    for (bg, fg) in [(PAPER, INK), ([28, 28, 36, 255], [235, 235, 240, 255])] {
        let bands = lines::detect(&page(400, 160, bg, fg, &PARAGRAPH));
        assert_eq!(bands.len(), 3, "{bands:?}");
        for (b, (x, top, _)) in bands.iter().zip(PARAGRAPH) {
            // Each band covers its line: ascenders to descenders, from its start.
            assert!(b.y0 >= top - 2.0 && b.y0 <= top + 8.0, "{b:?} {top}");
            assert!(b.y1 >= top + 18.0 && b.y1 <= top + 28.0, "{b:?} {top}");
            assert!((b.x0 - x).abs() < 4.0 && b.x1 > 150.0, "{b:?}");
        }
    }
    // Two columns whose lines don't line up: bands per column.
    let page2 = page(
        620,
        120,
        PAPER,
        INK,
        &[
            (10.0, 10.0, "left column text"),
            (10.0, 50.0, "more on the left"),
            (360.0, 28.0, "right column text"),
            (360.0, 70.0, "more on the right"),
        ],
    );
    let bands = lines::detect(&page2);
    assert_eq!(bands.len(), 4, "{bands:?}");
    let left: Vec<_> = bands.iter().filter(|b| b.x1 < 300.0).collect();
    let right: Vec<_> = bands.iter().filter(|b| b.x0 > 340.0).collect();
    assert_eq!((left.len(), right.len()), (2, 2), "{bands:?}");
    assert!((right[0].y0 - 28.0).abs() < 8.0 && (left[1].y0 - 50.0).abs() < 8.0);
    // A panel border down the page (an edge on every row) is not a line.
    let mut framed = page(400, 160, PAPER, INK, &PARAGRAPH);
    for y in 0..160 {
        let i = (y * 400 + 3) * 4;
        framed.rgba[i..i + 4].copy_from_slice(&INK);
    }
    assert_eq!(lines::detect(&framed).len(), 3);
    // A plain image has none.
    assert!(lines::detect(&image_of(200, 100, |_, _| SLATE)).is_empty());
}

/// `line` at `size` px, top-left `at`, on `image` (in place).
fn write_line(image: &mut Image, at: Point, line: &str, size: f32) {
    let (w, h) = (image.width, image.height);
    let mut layer = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(
            std::mem::take(&mut image.rgba),
            tiny_skia::IntSize::from_wh(w, h).unwrap(),
        )
        .unwrap(),
        origin: Point::ORIGIN,
    };
    text::paint(&mut layer, at, line, size, INK, false);
    image.rgba = layer.pm.take();
}

/// Auto finds ONE text line (owner, 2026-10-31: it covered more than one) and
/// covers ALL of it: three lines at 13, 16 and 20 px, loose to touching (1.6,
/// 1.3, 1.2, 1.1 x the size); a stroke started on the middle one (2 px above,
/// on, 2 px below its middle) pixelates every pixel of that line's ink
/// (accents, ascenders, descenders: É, g, y, j) and at most 10 % of the lines
/// above and below (their tails in the gap tight lines share; the margin stops
/// at their cores).
#[test]
fn auto_blur_covers_all_of_the_line_it_starts_on_and_no_other() {
    const TEXT: [&str; 3] = [
        "Typography quickly jumps over lazy dogs",
        "Église: gently, the fjord's quay by July",
        "Pack my box with five dozen liquor jugs",
    ];
    for size in [13.0f32, 16.0, 20.0] {
        for pitch in [1.6f32, 1.3, 1.2, 1.1] {
            let (w, h) = (420u32, (size * pitch * 3.0 + 20.0) as u32);
            let tops: Vec<f32> = (0..3)
                .map(|i| 10.0 + (size * pitch * i as f32).round())
                .collect();
            let mut page = image_of(w, h, |_, _| PAPER);
            let mut solo = Vec::new();
            for (i, line) in TEXT.iter().enumerate() {
                write_line(&mut page, pt(10.0, tops[i]), line, size);
                let mut alone = image_of(w, h, |_, _| PAPER);
                write_line(&mut alone, pt(10.0, tops[i]), line, size);
                solo.push(alone);
            }
            let found = lines::detect(&page);
            assert_eq!(found.len(), 3, "{size} px x{pitch}: {found:?}");
            let ink = |i: usize, x: u32, y: u32| pxi(&solo[i], x, y) != PAPER;
            let rows: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| ink(1, x, y))).collect();
            let (top, bottom) = (rows[0] as f32, *rows.last().unwrap() as f32);
            let cols: Vec<u32> = (0..w).filter(|&x| (0..h).any(|y| ink(1, x, y))).collect();
            let (left, right) = (cols[0] as f32, *cols.last().unwrap() as f32);
            let line = found[1];
            let c = line.centre();
            for start in [c - 2.0, c, c + 2.0] {
                let mut e = Annotator::new(
                    Image {
                        rgba: page.rgba.clone(),
                        ..page
                    },
                    1.0,
                );
                e.assist = true;
                e.update(Message::Tool(Tool::Blur));
                drag(
                    &mut e,
                    pt(left, start),
                    &[pt(200.0, start + 1.0)],
                    pt(right, start),
                );
                let Shape::Blur { size: brush, .. } = e.doc.items[0].shape else {
                    panic!()
                };
                // Full strength over the band, so compare with the band's own paint.
                let band = |y: u32| {
                    let Shape::Blur { points, .. } = &e.doc.items[0].shape else {
                        panic!()
                    };
                    (y as f32 + 0.5 - points[0].y).abs() <= brush / 2.0 - 0.5
                };
                let out = render(&e.doc);
                let at = |x, y| px(&out, x, y);
                let (mut others, mut touched) = (0, 0);
                for y in 0..h {
                    for x in 0..w {
                        let here = format!("{size} px x{pitch}, from {start}: ({x}, {y})");
                        if ink(1, x, y) {
                            assert!(
                                band(y),
                                "ink outside the band {here}: brush {brush} rows {top}..{bottom} bands {found:?}"
                            );
                            if luma(pxi(&page, x, y)) < 100 {
                                assert_ne!(at(x, y), pxi(&page, x, y), "ink left {here}");
                            }
                        } else if ink(0, x, y) || ink(2, x, y) {
                            others += 1;
                            touched += usize::from(at(x, y) != pxi(&page, x, y));
                        }
                    }
                }
                eprintln!(
                    "{size} px x{pitch} from {start}: {touched} of {others} px of the other lines"
                );
                assert!(
                    touched * 100 <= others * 10,
                    "{size} px x{pitch} from {start}: {touched} of {others} px of the other lines"
                );
            }
        }
    }
}

/// Lines set so tight that one's descenders nearly touch the next one's
/// ascenders are still two lines (each one's core and the sparse rows between the
/// dense x-height cores), and an Auto stroke on the second leaves the first
/// one's core.
#[test]
fn touching_lines_are_split_at_their_sparse_rows() {
    // Glyph-like bars: dense in the core rows, one in ten above / below.
    let bars = |x: u32, y: u32| {
        let core = (10..20).contains(&y) || (28..38).contains(&y);
        let tail = (20..24).contains(&y) || (25..28).contains(&y);
        let on = (x.is_multiple_of(4) && core) || (x.is_multiple_of(40) && tail);
        if (10..390).contains(&x) && on {
            INK
        } else {
            PAPER
        }
    };
    let image = image_of(400, 50, bars);
    let found = lines::detect(&image);
    assert_eq!(found.len(), 2, "{found:?}");
    // Both get the sparse rows between the cores (each one's tails may be there).
    assert!(found[0].y1 >= 28.0 && found[1].y0 <= 20.0, "{found:?}");
    assert!(found[0].y0 >= 10.0 && found[1].y1 <= 38.0, "{found:?}");
    let mut e = Annotator::new(image_of(400, 50, bars), 1.0);
    e.update(Message::Tool(Tool::Blur));
    drag(&mut e, pt(12.0, 30.0), &[], pt(380.0, 30.0));
    let out = render(&e.doc);
    // The first line's core stays, but for the least margin (2 px) at its foot.
    for y in 0..18 {
        for x in 0..400 {
            assert_eq!(px(&out, x, y), bars(x, y), "line 1 touched at ({x}, {y})");
        }
    }
    assert_ne!(px(&out, 100, 32), bars(100, 32));
}

#[test]
fn line_help_holds_the_line_until_a_clear_move() {
    let mut held = 20.0;
    // Wobbling up to one and a half brushes off: still on its line.
    for y in [26.0, 14.0, 49.0, -9.0] {
        assert_eq!(blur::follow(&mut held, pt(50.0, y), 20.0), pt(50.0, 20.0));
    }
    // A clear move down: on to the pointer's height, held there.
    assert_eq!(
        blur::follow(&mut held, pt(60.0, 52.0), 20.0),
        pt(60.0, 52.0)
    );
    assert_eq!(
        blur::follow(&mut held, pt(70.0, 60.0), 20.0),
        pt(70.0, 52.0)
    );
    // And a clear move up.
    assert_eq!(
        blur::follow(&mut held, pt(80.0, 10.0), 20.0),
        pt(80.0, 10.0)
    );
}

/// Sliding the pointer back while the button is down un-blurs what it left (#154).
#[test]
fn blur_follows_the_pointer_back() {
    let mut e = Annotator::new(image_of(400, 50, |_, _| [255; 4]), 1.0);
    e.update(Message::Tool(Tool::Blur));
    e.update(Message::Press(pt(100.0, 25.0)));
    for x in [150.0, 200.0, 300.0, 250.0, 200.0] {
        e.update(Message::Move(pt(x, 25.0)));
    }
    let right = |e: &Annotator| match &e.pending.as_ref().unwrap().item.shape {
        Shape::Blur { points, .. } => points.iter().map(|p| p.x).fold(f32::MIN, f32::max),
        _ => panic!(),
    };
    assert_eq!(right(&e), 200.0);
    // And forward again.
    e.update(Message::Move(pt(260.0, 25.0)));
    assert_eq!(right(&e), 260.0);
    e.update(Message::Release);
}

/// The blur brush: Auto = as tall as the text line it starts on (the
/// default off any text); a size picked in its options overrides that, Auto
/// gives it back; line help keeps a wobbly stroke on its line, Shift frees it.
/// No suggestion is drawn: a blur has no handles, drawn or selected.
#[test]
fn the_blur_brush_is_sized_to_its_line_auto_or_picked() {
    let image = page(400, 160, PAPER, INK, &PARAGRAPH);
    let bands = lines::detect(&image);
    let mut e = Annotator::new(image, 1.0);
    e.assist = true;
    e.update(Message::Tool(Tool::Blur));
    assert_eq!(e.blur_size, None, "Auto by default");
    let b = bands[1];
    let c = b.centre();
    let size_of = |e: &Annotator, i: usize| match &e.doc.items[i].shape {
        Shape::Blur { size, points, .. } => (*size, points.clone()),
        _ => panic!(),
    };
    // Auto on line 2 (20 px text): its height; held on it through a wobble.
    e.update(Message::Press(pt(15.0, c + 3.0)));
    e.update(Message::Move(pt(80.0, c - 5.0)));
    assert!(e.handle_marks().is_empty(), "nothing drawn while brushing");
    e.update(Message::Move(pt(150.0, c + 6.0)));
    e.update(Message::Move(pt(220.0, c)));
    e.update(Message::Release);
    let (size, points) = size_of(&e, 0);
    assert_eq!(size, b.stroke(1.0).1);
    assert!((14.0..40.0).contains(&size), "one line: {size}");
    assert!(
        points.iter().all(|p| p.y == c),
        "on the line's middle: {points:?}"
    );
    assert_eq!(e.doc.selected, Some(0));
    assert!(
        e.handle_marks().is_empty(),
        "no handles or box when selected"
    );
    // Off any text: the default (one line of UI text).
    drag(&mut e, pt(15.0, 145.0), &[], pt(220.0, 145.0));
    assert_eq!(size_of(&e, 1).0, Width::Medium.brush());
    assert_eq!(Width::Medium.brush(), 20.0);
    // Shift: free.
    e.update(Message::Shift(true));
    drag(
        &mut e,
        pt(15.0, c + 3.0),
        &[pt(80.0, c - 5.0)],
        pt(220.0, c),
    );
    assert!(size_of(&e, 2).1.iter().any(|p| p.y != c + 3.0));
    e.update(Message::Shift(false));

    // Its options: a click on the active Blur button; a size there overrides Auto.
    e.update(Message::Slot(Tool::Blur));
    assert_eq!(e.menu, Some(Menu::Options(Group::Hide)));
    let _ = e.toolbar(); // they build
    e.update(Message::BlurSize(Some(Width::Thick)));
    drag(&mut e, pt(15.0, c), &[], pt(220.0, c));
    assert_eq!(size_of(&e, 3).0, Width::Thick.brush());
    // Auto: the line's size again.
    e.update(Message::BlurSize(None));
    drag(&mut e, pt(15.0, c), &[], pt(220.0, c));
    assert_eq!(size_of(&e, 4).0, b.stroke(1.0).1);
    // Pixel size: fine 6 px cells by default (display px), coarser picked.
    let cell_of = |e: &Annotator, i: usize| match &e.doc.items[i].shape {
        Shape::Blur { cell, .. } => *cell,
        _ => panic!(),
    };
    assert_eq!(cell_of(&e, 4), 6.0);
    e.update(Message::BlurCell(Width::Thick));
    assert_eq!(cell_of(&e, 4), 12.0, "the selected one too");
    e.update(Message::BlurCell(Width::Thin));

    // Line help off: the stroke follows the pointer.
    e.update(Message::Assist);
    assert!(!e.assist);
    drag(
        &mut e,
        pt(15.0, c + 3.0),
        &[pt(80.0, c - 5.0)],
        pt(220.0, c),
    );
    assert!(size_of(&e, 5).1.iter().any(|p| p.y != c + 3.0));
}

/// A selected annotation shows only its own small handles, never a frame:
/// every pixel the selection adds to the canvas is at one of its handles
/// (an arrow: its two ends and its bend; a callout: its bubble's corners
/// and its tail's tip; a box: its corners).
#[test]
fn a_selection_has_no_frame_only_its_handles() {
    let canvas_px =
        |e: &Annotator| crate::shared::kit::testing::shot::save("", e.canvas(), e.size());
    for tool in [
        Tool::Rect,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Line,
        Tool::Callout,
        Tool::Text,
        Tool::Step,
        Tool::Spotlight,
        Tool::Magnifier,
        Tool::Blur,
    ] {
        let mut e = editor(300, 200);
        e.update(Message::Tool(tool));
        match tool {
            Tool::Callout | Tool::Text | Tool::Step => {
                drag(&mut e, pt(60.0, 120.0), &[], pt(60.0, 120.0));
                e.update(Message::Type("Hi".into()));
                e.update(Message::Finish);
            }
            _ => drag(&mut e, pt(60.0, 50.0), &[pt(100.0, 70.0)], pt(200.0, 140.0)),
        }
        e.doc.selected = Some(0);
        let marks = e.handle_marks();
        let selected = canvas_px(&e);
        e.doc.selected = None;
        let plain = canvas_px(&e);
        let w = e.size().width as usize;
        for (i, (a, b)) in selected.chunks(4).zip(plain.chunks(4)).enumerate() {
            if a != b {
                let p = pt((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                assert!(
                    marks.iter().any(|m| m.distance(p) <= 7.0),
                    "{tool:?}: {p:?} drawn away from its handles {marks:?}"
                );
            }
        }
        let want = match tool {
            Tool::Arrow | Tool::Line => 3,
            Tool::Callout => 5,
            Tool::Blur => 0,
            _ => 4,
        };
        assert_eq!(marks.len(), want, "{tool:?}");
    }
}

/// An arrow's ends and a callout's tip are handles: dragged with any tool,
/// one undoable change each.
#[test]
fn arrow_ends_and_callout_tip_drag() {
    let mut e = editor(400, 300);
    drag(&mut e, pt(50.0, 50.0), &[], pt(150.0, 50.0)); // the starting Arrow
    drag(&mut e, pt(150.0, 50.0), &[], pt(200.0, 100.0));
    let Shape::Arrow { a, b, bend, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!((a, b), (pt(50.0, 50.0), pt(200.0, 100.0)));
    assert!(
        arrow::middle(a, b, bend).distance(pt(125.0, 75.0)) < 0.01,
        "still straight"
    );
    assert_eq!(e.doc.items.len(), 1);
    e.update(Message::Tool(Tool::Callout));
    drag(&mut e, pt(100.0, 250.0), &[], pt(100.0, 250.0));
    e.update(Message::Type("Note".into()));
    e.update(Message::Finish);
    drag(&mut e, pt(100.0, 250.0), &[], pt(40.0, 280.0));
    let Shape::Callout { tip, .. } = e.doc.items[1].shape else {
        panic!()
    };
    assert_eq!(tip, pt(40.0, 280.0));
    e.update(Message::Undo);
    e.update(Message::Undo);
    let Shape::Arrow { b, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!(b, pt(200.0, 100.0));
}

/// The first click on a tool button picks it; a click on the active one
/// opens (then closes) its options. Shapes, arrows and callouts start with no
/// outline (a solid arrow, a solid bubble) and no fill; the options turn them
/// on for the next item and the selected one.
#[test]
fn options_open_on_the_second_click_and_outlines_are_off_by_default() {
    let mut e = editor(300, 200);
    e.update(Message::Tool(Tool::Select)); // Rect's group not active yet
    e.update(Message::Slot(Tool::Rect));
    assert_eq!(
        (e.tool, e.menu),
        (Tool::Rect, None),
        "first click: the tool"
    );
    e.update(Message::Slot(Tool::Rect));
    assert_eq!(e.menu, Some(Menu::Options(Group::Shapes)));
    let _ = e.toolbar();
    e.update(Message::Slot(Tool::Rect));
    assert_eq!(e.menu, None);
    for tool in [Tool::Rect, Tool::Arrow, Tool::Callout] {
        e.update(Message::Tool(tool));
        e.update(Message::Press(pt(30.0, 150.0)));
        e.update(Message::Move(pt(90.0, 190.0)));
        e.update(Message::Release);
        e.update(Message::Finish);
    }
    assert!(e.doc.items.iter().all(|i| !i.outline && !i.fill));
    // A plain red arrow on white: no white or dark ring beside its shaft.
    let mut a = editor(200, 100);
    a.doc = doc_with(image_of(200, 100, |_, _| SLATE), vec![]);
    drag(&mut a, pt(20.0, 50.0), &[], pt(180.0, 50.0));
    let flat = render(&a.doc);
    let w = a.doc.items[0].width;
    let beside = (50.0 + w / 2.0 + 0.6) as u32; // just off the shaft, inside a ring
    assert_eq!(px(&flat, 80, beside), SLATE, "no halo");
    assert_eq!(px(&flat, 80, 50), RED);
    // Outline on: the selected arrow gets its ring.
    a.update(Message::Outline(true));
    assert!(a.doc.items[0].outline);
    assert_ne!(px(&render(&a.doc), 80, beside), SLATE);
    // Fill: a rectangle's inside is tinted.
    e.update(Message::Tool(Tool::Rect));
    e.update(Message::Fill(true));
    drag(&mut e, pt(150.0, 20.0), &[], pt(250.0, 90.0));
    let inside = px(&render(&e.doc), 200, 55);
    let bare = px(&render(&doc_with(checker(300, 200), vec![])), 200, 55);
    assert!(e.doc.items.last().unwrap().fill);
    assert_ne!(inside, bare, "tinted");
}

/// Ctrl+press moves any annotation whatever the tool, even where a plain
/// press would draw (inside a box); a plain drag there still draws.
#[test]
fn ctrl_drag_moves_with_any_tool() {
    let mut e = editor(400, 300);
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(50.0, 50.0), &[], pt(150.0, 150.0));
    let drawn = e.doc.items[0].clone();
    for tool in [Tool::Rect, Tool::Pen, Tool::Blur, Tool::Text, Tool::Arrow] {
        e.update(Message::Tool(tool));
        e.update(Message::PressMove(pt(100.0, 100.0)));
        e.update(Message::Move(pt(110.0, 105.0)));
        e.update(Message::Release);
        assert_eq!(e.doc.items.len(), 1, "{tool:?}: moved, nothing drawn");
    }
    assert_eq!(
        e.doc.items[0],
        select::moved(&drawn, iced::Vector::new(50.0, 25.0))
    );
    // Without Ctrl, inside the box: a new one.
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(110.0, 110.0), &[], pt(140.0, 140.0));
    assert_eq!(e.doc.items.len(), 2);
    // Ctrl over nothing: draws as usual.
    e.update(Message::PressMove(pt(250.0, 200.0)));
    e.update(Message::Move(pt(300.0, 250.0)));
    e.update(Message::Release);
    assert_eq!(e.doc.items.len(), 3);
}

/// The editor remembers its tool, colour, sizes and options (owner,
/// 2026-10-31): saved through the config file, the next editor opens on them;
/// pointer messages never count as a choice; Crop is never the opening tool.
#[test]
fn the_annotator_opens_on_the_last_tool_colour_sizes_and_options() {
    let mut e = editor(200, 100);
    let fresh = e.remembered();
    e.update(Message::Press(pt(10.0, 10.0)));
    e.update(Message::Release);
    assert_eq!(
        e.saved,
        crate::config::EditorConfig::default(),
        "no choice yet"
    );
    let purple = style::ANNOTATION[7].into_rgba8();
    e.update(Message::Tool(Tool::Arrow));
    e.update(Message::Colour(purple));
    e.update(Message::Width(Width::Thick));
    e.update(Message::Outline(true));
    e.update(Message::Fill(true));
    e.update(Message::TextSize(52.0));
    e.update(Message::BlurSize(Some(Width::Thin)));
    e.update(Message::BlurCell(Width::Thick));
    e.update(Message::Tool(Tool::Highlighter));
    e.update(Message::Tool(Tool::Crop)); // a step: the tool before it stays
    assert_ne!(e.saved, fresh);
    assert_eq!(e.saved, e.remembered(), "every choice is remembered");

    let dir = std::env::temp_dir().join(format!("oc-editor-remember-{}", std::process::id()));
    let path = dir.join("config.toml");
    let c = crate::config::Config {
        editor: e.remembered(),
        ..Default::default()
    };
    c.save_to(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("tool = \"highlighter\"") && text.contains("colour = \"#AF52DE\""),
        "{text}"
    );
    let (back, w) = crate::config::Config::load_from(&path);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(w.is_empty(), "{w:?}");

    let r = Annotator::restored(checker(200, 100), 1.0, back.editor);
    assert_eq!(r.tool, Tool::Highlighter);
    assert_eq!((r.colour, r.hex.as_str()), (purple, "#AF52DE"));
    assert_eq!(
        (r.width, r.text_size, r.blur_cell),
        (Width::Thick, 52.0, Width::Thick)
    );
    assert_eq!(
        (r.outline, r.fill, r.blur_size),
        (true, true, Some(Width::Thin))
    );
    assert!(!r.assist, "line help too");
    // A new arrow gets them.
    let mut r = r;
    r.update(Message::Tool(Tool::Arrow));
    drag(&mut r, pt(20.0, 20.0), &[], pt(120.0, 60.0));
    let item = &r.doc.items[0];
    assert_eq!((item.colour, item.outline), (purple, true));
    assert_eq!(item.width, Width::Thick.stroke());

    // Nothing remembered (an old file): the defaults.
    let d = Annotator::restored(checker(20, 20), 1.0, Default::default());
    assert_eq!(
        (d.tool, d.width, d.text_size),
        (Tool::Arrow, Width::Medium, 26.0)
    );
    assert_eq!(d.colour, style::ANNOTATION[0].into_rgba8());
    assert_eq!(
        (d.outline, d.fill, d.blur_size, d.assist),
        (false, false, None, true)
    );
    // Auto blur is remembered as no key.
    r.update(Message::BlurSize(None));
    assert_eq!(r.saved.blur_size, None);
}

// ---- snapshots and timing (run by hand) -------------------------------------------

/// A fake app window to annotate: a card with a title, text lines, a password
/// field and a button, on a gradient.
fn demo_image() -> Image {
    let (w, h) = (820u32, 480u32);
    let inside = |x: u32, y: u32, r: [u32; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
    let lines = [
        [90, 130, 440, 140],
        [90, 160, 400, 170],
        [90, 190, 460, 200],
        [90, 220, 300, 230],
    ];
    let base = image_of(w, h, |x, y| {
        let t = y as f32 / h as f32;
        if lines.iter().any(|&l| inside(x, y, l)) {
            [190, 194, 204, 255]
        } else if inside(x, y, [90, 290, 400, 336]) {
            [236, 238, 244, 255]
        } else if inside(x, y, [560, 360, 760, 410]) {
            [10, 132, 255, 255]
        } else if inside(x, y, [60, 50, 500, 96]) {
            [244, 245, 248, 255]
        } else if inside(x, y, [60, 50, 500, 430]) || inside(x, y, [540, 50, 780, 430]) {
            [255, 255, 255, 255]
        } else {
            [
                (30.0 + 28.0 * t) as u8,
                (34.0 + 10.0 * t) as u8,
                (52.0 + 36.0 * t) as u8,
                255,
            ]
        }
    });
    let dark = [28, 28, 36, 255];
    let grey = [90, 90, 110, 255];
    let labels = [
        (80.0, 60.0, "Account settings", 22.0, dark),
        (100.0, 300.0, "Password: hunter2-secret", 20.0, dark),
        (90.0, 258.0, "Sign-in", 15.0, grey),
        (636.0, 372.0, "Save", 20.0, [255; 4]),
        (560.0, 70.0, "Plan: Pro, 3 seats", 15.0, dark),
        (560.0, 100.0, "Renews 2026-11-01", 13.0, grey),
    ];
    // Labels are part of the picture: plain text, no halo.
    let mut layer = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(base.rgba, tiny_skia::IntSize::from_wh(w, h).unwrap())
            .unwrap(),
        origin: Point::ORIGIN,
    };
    for (x, y, label, size, colour) in labels {
        text::paint(&mut layer, pt(x, y), label, size, colour, false);
    }
    Image {
        width: w,
        height: h,
        rgba: layer.pm.take(),
    }
}

/// The demo document, made with the editor's own tools (every one of them).
fn demo_editor() -> Annotator {
    let mut e = Annotator::new(demo_image(), 1.0);
    let colour = |e: &mut Annotator, i: usize| {
        e.update(Message::Colour(
            crate::shared::theme::ANNOTATION[i].into_rgba8(),
        ));
    };
    // Draws with `tool` from `from` through `via` to `to`.
    let with = |e: &mut Annotator, tool: Tool, from: Point, via: &[Point], to: Point| {
        e.update(Message::Tool(tool));
        drag(e, from, via, to);
    };

    with(
        &mut e,
        Tool::Spotlight,
        pt(52.0, 42.0),
        &[],
        pt(508.0, 438.0),
    );
    with(&mut e, Tool::Rect, pt(72.0, 54.0), &[], pt(300.0, 92.0));
    // The password line, brushed with line assist (the wobble is ignored).
    e.assist = true;
    with(
        &mut e,
        Tool::Blur,
        pt(186.0, 310.0),
        &[pt(260.0, 316.0), pt(330.0, 306.0)],
        pt(398.0, 312.0),
    );
    e.doc.selected = None;
    colour(&mut e, 2);
    let line: Vec<_> = (0..40).map(|i| pt(90.0 + 8.0 * i as f32, 165.0)).collect();
    with(
        &mut e,
        Tool::Highlighter,
        pt(88.0, 165.0),
        &line,
        pt(402.0, 165.0),
    );
    e.doc.selected = None;
    colour(&mut e, 3);
    let wave: Vec<_> = (0..60)
        .map(|i| pt(90.0 + 3.5 * i as f32, 240.0 + 18.0 * (i as f32 / 4.0).sin()))
        .collect();
    with(&mut e, Tool::Pen, pt(90.0, 240.0), &wave, pt(300.0, 240.0));
    e.doc.selected = None;
    // A rough circle drawn with the pen snaps to an ellipse.
    colour(&mut e, 0);
    let circle = rough_ellipse(660.0, 385.0, 120.0, 42.0);
    with(&mut e, Tool::Pen, circle[0], &circle[1..], circle[60]);
    assert!(matches!(
        e.doc.items.last().unwrap().shape,
        Shape::Ellipse { .. }
    ));
    e.doc.selected = None;
    colour(&mut e, 7);
    with(&mut e, Tool::Ellipse, pt(548.0, 58.0), &[], pt(712.0, 92.0));
    e.doc.selected = None;
    colour(&mut e, 5);
    with(
        &mut e,
        Tool::Callout,
        pt(640.0, 120.0),
        &[],
        pt(640.0, 215.0),
    );
    e.update(Message::Type("Upgrade first".into()));
    e.update(Message::Finish);
    e.doc.selected = None;
    colour(&mut e, 2);
    with(&mut e, Tool::Text, pt(560.0, 436.0), &[], pt(560.0, 436.0));
    e.update(Message::Type("Then save".into()));
    e.update(Message::Finish);
    e.doc.selected = None;
    colour(&mut e, 0);
    for p in [pt(40.0, 72.0), pt(430.0, 312.0), pt(530.0, 386.0)] {
        with(&mut e, Tool::Step, p, &[], p);
        e.doc.selected = None;
    }
    colour(&mut e, 10);
    with(
        &mut e,
        Tool::Magnifier,
        pt(612.0, 104.0),
        &[],
        pt(664.0, 104.0),
    );
    e.doc.selected = None;
    colour(&mut e, 0);
    with(&mut e, Tool::Arrow, pt(520.0, 250.0), &[], pt(580.0, 372.0));
    // Curve it from its middle handle (it is selected after drawing).
    let Shape::Arrow { a, b, bend, .. } = e.doc.items.last().unwrap().shape else {
        panic!()
    };
    let middle = arrow::middle(a, b, bend);
    drag(&mut e, middle, &[], middle + iced::Vector::new(-40.0, 10.0));
    with(&mut e, Tool::Crop, pt(20.0, 20.0), &[], pt(800.0, 466.0));
    e.update(Message::Tool(Tool::Select));
    e.doc.selected = Some(e.doc.items.len() - 1);
    e
}

/// Draws `element` (laid out in `size`, logical px) with the headless tiny-skia
/// renderer at 2x and writes it as a PNG.
fn shoot(
    mut element: Element<'_, Message>,
    size: Size,
    background: iced::Color,
    path: &std::path::Path,
) {
    use iced::advanced::renderer::{Headless, Style};
    use iced::advanced::widget::Tree;
    use iced::advanced::{Layout, layout};
    let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    element.as_widget().draw(
        &tree,
        &mut renderer,
        &iced::Theme::Dark,
        &Style::default(),
        Layout::new(&node),
        iced::mouse::Cursor::Unavailable,
        &Rectangle::with_size(size),
    );
    // Floating layers (a popover moved to stay on screen) draw over it.
    if let Some(mut overlay) = element.as_widget_mut().overlay(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &Rectangle::with_size(size),
        iced::Vector::ZERO,
    ) {
        let floating = overlay.as_overlay_mut().layout(&renderer, size);
        overlay.as_overlay().draw(
            &mut renderer,
            &iced::Theme::Dark,
            &Style::default(),
            Layout::new(&floating),
            iced::mouse::Cursor::Unavailable,
        );
    }
    let scale = 2.0;
    let (w, h) = ((size.width * scale) as u32, (size.height * scale) as u32);
    let rgba = renderer.screenshot(Size::new(w, h), scale, background);
    crate::actions::write_png(path, w, h, &rgba).unwrap();
}

/// `OPENCIRCLE_EDITOR_PNG=<dir> cargo test annotate::tests::snapshots_move -- --ignored`
/// writes the editor before and after a box and an arrow are dragged with the
/// Rect tool active (no Select).
#[test]
#[ignore = "writes PNGs to look at"]
fn snapshots_move() {
    let dir = png_dir();
    let mut e = Annotator::new(demo_image(), 1.0);
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(72.0, 54.0), &[], pt(300.0, 92.0));
    e.update(Message::Tool(Tool::Arrow));
    drag(&mut e, pt(520.0, 250.0), &[], pt(600.0, 372.0));
    e.update(Message::Tool(Tool::Rect));
    e.doc.selected = None;
    let size = e.size() + Size::new(460.0, 90.0);
    let backdrop = iced::Color::from_rgb8(0x2a, 0x2d, 0x3e);
    shoot(e.view(), size, backdrop, &dir.join("move-0-before.png"));
    // The box by its top edge, the arrow by its line: both move, nothing new.
    drag(
        &mut e,
        pt(180.0, 54.0),
        &[pt(200.0, 90.0)],
        pt(220.0, 134.0),
    );
    drag(&mut e, pt(560.0, 311.0), &[], pt(470.0, 331.0));
    assert_eq!(e.doc.items.len(), 2);
    shoot(e.view(), size, backdrop, &dir.join("move-1-after.png"));
}

/// `OPENCIRCLE_EDITOR_PNG=<dir> cargo test annotate::tests::snapshots_popovers -- --ignored`
/// writes the colour and the text popovers of a toolbar at the bottom of a small
/// screen (they open above it).
#[test]
#[ignore = "writes PNGs to look at"]
fn snapshots_popovers_at_the_bottom() {
    use iced::widget::{pin, stack};
    let dir = png_dir();
    let screen = Size::new(1000.0, 520.0);
    let mut e = Annotator::new(demo_image(), 0.5);
    let bar = toolbar::size();
    let at = Point::new(
        (screen.width - bar.width) / 2.0,
        screen.height - bar.height - 12.0,
    );
    let backdrop = iced::Color::from_rgb8(0x2a, 0x2d, 0x3e);
    for (name, open) in [
        ("popover-colour-bottom.png", Message::Picker),
        ("popover-text-bottom.png", Message::Slot(Tool::Text)),
    ] {
        e.update(open);
        let (toolbar, popover) = e.toolbar_at(Rectangle::new(at, bar));
        let view = stack![
            pin(e.canvas()).position(Point::new(12.0, 12.0)),
            pin(toolbar).position(at),
        ]
        .extend(popover)
        .width(screen.width)
        .height(screen.height);
        shoot(view.into(), screen, backdrop, &dir.join(name));
    }
}

/// `OPENCIRCLE_EDITOR_PNG=<dir> cargo test annotate::tests::snapshots -- --ignored`
/// writes the toolbar (Dark, Light), the editor with every tool, and the flattened
/// result. Not run by default: it writes files and uses the system's sans-serif.
#[test]
#[ignore = "writes PNGs to look at"]
fn snapshots() {
    use crate::shared::theme;
    let dir = std::path::PathBuf::from(
        std::env::var("OPENCIRCLE_EDITOR_PNG").expect("OPENCIRCLE_EDITOR_PNG=<dir>"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    iced::advanced::graphics::text::font_system()
        .write()
        .unwrap()
        .load_font(std::borrow::Cow::Borrowed(
            crate::shared::kit::icon::FONT_BYTES,
        ));

    let mut e = demo_editor();
    e.update(Message::Picker);
    for (name, light, backdrop) in [
        (
            "toolbar-dark.png",
            false,
            iced::Color::from_rgb8(0x2a, 0x2d, 0x3e),
        ),
        (
            "toolbar-light.png",
            true,
            iced::Color::from_rgb8(0xdc, 0xe0, 0xe8),
        ),
    ] {
        theme::set_light(light);
        shoot(
            e.toolbar(),
            Size::new(1220.0, 330.0),
            backdrop,
            &dir.join(name),
        );
    }
    theme::set_light(false);
    e.update(Message::Picker);
    let size = e.size() + Size::new(460.0, 90.0);
    let backdrop = iced::Color::from_rgb8(0x2a, 0x2d, 0x3e);
    shoot(e.view(), size, backdrop, &dir.join("editor-dark.png"));
    let flat = render(&e.doc);
    let path = dir.join("flattened.png");
    crate::actions::write_png(&path, flat.width, flat.height, &flat.rgba).unwrap();
}

/// The toolbar is icon-only: each button's tooltip is its only name, and none of
/// them repeats words already on the button.
#[test]
fn toolbar_tooltips_are_for_icon_only_buttons() {
    use crate::shared::kit::tooltip::tests_support::{assert_tips_add_something, shown};
    let e = Annotator::new(checker(200, 100), 1.0);
    let _ = e.toolbar();
    let tips = shown();
    assert!(tips.len() >= 11, "{tips:?}");
    assert!(tips.iter().any(|t| t == "Undo · Ctrl+Z"), "{tips:?}");
    assert_tips_add_something("editor toolbar");
}

/// The PNG folder and the icon font loaded (snapshot tests).
fn png_dir() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(
        std::env::var("OPENCIRCLE_EDITOR_PNG").expect("OPENCIRCLE_EDITOR_PNG=<dir>"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    iced::advanced::graphics::text::font_system()
        .write()
        .unwrap()
        .load_font(std::borrow::Cow::Borrowed(
            crate::shared::kit::icon::FONT_BYTES,
        ));
    dir
}

/// Timings on a 3440x1440 crop:
/// `cargo test --release annotate::tests::timing -- --ignored --nocapture`
#[test]
#[ignore = "timing; meaningful in release"]
fn timing() {
    use std::time::Instant;
    let mut e = Annotator::new(checker(3440, 1440), 1.0);
    for k in 0..3 {
        let o = iced::Vector::new(1100.0 * k as f32, 0.0);
        for (t, a, b) in [
            (Tool::Rect, pt(100.0, 100.0), pt(700.0, 500.0)),
            (Tool::Blur, pt(200.0, 600.0), pt(800.0, 900.0)),
            (Tool::Spotlight, pt(50.0, 50.0), pt(850.0, 1300.0)),
            (Tool::Arrow, pt(300.0, 1000.0), pt(700.0, 1300.0)),
            (Tool::Magnifier, pt(500.0, 300.0), pt(600.0, 300.0)),
            (Tool::Step, pt(80.0, 80.0), pt(80.0, 80.0)),
        ] {
            e.update(Message::Tool(t));
            drag(&mut e, a + o, &[], b + o);
            e.doc.selected = None;
        }
    }
    let n = e.doc.items.len();
    let t = Instant::now();
    e.refresh();
    let base = t.elapsed();

    // A pen stroke across the crop: one live repaint per pointer move.
    e.update(Message::Tool(Tool::Pen));
    e.update(Message::Press(pt(100.0, 700.0)));
    let t = Instant::now();
    let moves = 100;
    for i in 0..moves {
        let wiggle = 3.0 * (i % 7) as f32;
        e.update(Message::Move(pt(100.0 + 20.0 * i as f32, 700.0 + wiggle)));
    }
    let pen = t.elapsed() / moves;
    let t = Instant::now();
    e.update(Message::Release);
    let commit = t.elapsed();

    // Moving a small item: one live repaint of its box per move.
    e.update(Message::Tool(Tool::Select));
    e.update(Message::Press(pt(1180.0, 80.0)));
    let t = Instant::now();
    for i in 0..moves {
        e.update(Message::Move(pt(
            1180.0 + 5.0 * i as f32,
            80.0 + 2.0 * i as f32,
        )));
    }
    let step_move = t.elapsed() / moves;
    e.update(Message::Release);

    // A new spotlight: the whole view repaints per move.
    e.update(Message::Tool(Tool::Spotlight));
    e.update(Message::Press(pt(2000.0, 200.0)));
    let t = Instant::now();
    for i in 0..10 {
        e.update(Message::Move(pt(2400.0 + 10.0 * i as f32, 800.0)));
    }
    let spot = t.elapsed() / 10;
    e.update(Message::Release);

    let t = Instant::now();
    let flat = render(&e.doc);
    let flatten = t.elapsed();
    assert_eq!((flat.width, flat.height), (3440, 1440));
    eprintln!(
        "3440x1440, {n} items: base repaint {base:?}; pen (2000 px) per move {pen:?}, \
         commit {commit:?}; step move per move {step_move:?}; spotlight drag per move \
         {spot:?}; render (Copy/Save) {flatten:?}"
    );
}

#[test]
fn pin_hands_over_the_flattened_image() {
    let mut e = editor(40, 30);
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(5.0, 5.0), &[], pt(25.0, 20.0));
    let Some(Output::Pin(flat)) = e.update(Message::Pin) else {
        panic!("Pin gives Output::Pin")
    };
    assert_eq!((flat.width, flat.height), (40, 30));
    assert_eq!(flat.rgba.len(), 40 * 30 * 4);
    // The annotation is in it: the same image as Copy gets.
    let Some(Output::Copy(copy)) = e.update(Message::Copy) else {
        panic!("Copy gives Output::Copy")
    };
    assert!(flat.rgba == copy.rgba);
}

/// Snip's live area (N-73): a 200x100 frame whose pixel (x, y) = [x, y, 0];
/// the area starts at (40, 20) 60x30, shown at 1:1, Select tool.
fn live_area() -> Annotator {
    let frame = image_of(200, 100, |x, y| [x as u8, y as u8, 0, 255]);
    let mut e = Annotator::new(frame, 1.0);
    e.doc.crop = Some(Rectangle::new(pt(40.0, 20.0), Size::new(60.0, 30.0)));
    e.movable = true;
    e.tool = Tool::Select;
    e.refresh();
    e
}

#[test]
fn snips_live_area_moves_with_its_annotations_and_resizes_without_scaling_them() {
    let mut e = live_area();
    // A box drawn in the area (clear of its edges' grips).
    e.update(Message::Tool(Tool::Rect));
    e.update(Message::Press(pt(55.0, 30.0)));
    e.update(Message::Move(pt(70.0, 40.0)));
    e.update(Message::Release);
    assert_eq!(e.doc.items.len(), 1);
    let drawn = e.doc.items[0].clone();
    // Back to Select: a press on empty space inside drags the area.
    e.update(Message::Tool(Tool::Select));
    e.update(Message::Press(pt(90.0, 38.0)));
    e.update(Message::Move(pt(100.0, 48.0)));
    // Live while dragged.
    let moved = Rectangle::new(pt(50.0, 30.0), Size::new(60.0, 30.0));
    assert_eq!(e.doc.view(), moved);
    e.update(Message::Release);
    assert_eq!(e.doc.view(), moved);
    let mut expect = drawn.clone();
    expect.shape.transform(|p| p + Vector::new(10.0, 10.0), 1.0);
    assert_eq!(e.doc.items[0], expect, "the annotation moved with the area");
    // The capture shows the frame's pixels at the new place.
    let out = render(&e.doc);
    assert_eq!((out.width, out.height), (60, 30));
    assert_eq!(px(&out, 0, 0)[..2], [50, 30]);

    // Resize from the bottom-right handle: the crop grows, the box stays put
    // and keeps its size (the drawing is never scaled).
    e.update(Message::Press(pt(110.0, 60.0)));
    e.update(Message::Move(pt(140.0, 80.0)));
    e.update(Message::Release);
    let grown = Rectangle::new(pt(50.0, 30.0), Size::new(90.0, 50.0));
    assert_eq!(e.doc.view(), grown);
    assert_eq!(e.doc.items[0], expect);
    let out = render(&e.doc);
    assert_eq!((out.width, out.height), (90, 50));
    assert_eq!(
        px(&out, 89, 49)[..2],
        [139, 79],
        "extended from the frozen frame"
    );

    // Each is one undo step, back to where it was picked.
    assert!(e.doc.undo());
    assert_eq!(e.doc.view(), moved);
    assert!(e.doc.undo());
    assert_eq!(
        e.doc.view(),
        Rectangle::new(pt(40.0, 20.0), Size::new(60.0, 30.0))
    );
    assert_eq!(e.doc.items[0], drawn);
    assert!(e.doc.redo());
    assert_eq!(e.doc.items[0], expect);
}

#[test]
fn snips_live_area_handles_work_with_any_tool_and_a_drawing_tool_draws_inside() {
    let mut e = live_area();
    e.update(Message::Tool(Tool::Pen));
    // The top-left handle with the pen: a resize, nothing drawn.
    e.update(Message::Press(pt(41.0, 21.0)));
    e.update(Message::Move(pt(20.0, 10.0)));
    e.update(Message::Release);
    assert!(e.doc.items.is_empty());
    let r = Rectangle::new(pt(19.0, 9.0), Size::new(81.0, 41.0));
    assert_eq!(e.doc.view(), r);
    // Inside, the pen draws (no move).
    e.update(Message::Press(pt(50.0, 30.0)));
    e.update(Message::Move(pt(60.0, 35.0)));
    e.update(Message::Release);
    assert_eq!(e.doc.items.len(), 1);
    assert_eq!(e.doc.view(), r);
    // Never past the frame, never under the minimum.
    e.update(Message::Press(pt(19.0, 9.0)));
    e.update(Message::Move(pt(-500.0, -500.0)));
    e.update(Message::Release);
    assert_eq!(e.doc.view().position(), Point::ORIGIN);
    e.update(Message::Press(pt(100.0, 30.0)));
    e.update(Message::Move(pt(-500.0, 30.0)));
    e.update(Message::Release);
    assert_eq!(e.doc.view().width, area::MIN_SIDE);
}

#[test]
fn snips_live_area_follows_the_arrow_keys() {
    let mut e = live_area();
    let key = |e: &Annotator, named, m: Modifiers| e.key(&Key::Named(named), None, m, None);
    let none = Modifiers::empty();
    assert!(matches!(
        key(&e, Named::ArrowRight, none),
        Some(Message::Nudge((1, 0), (false, false, false)))
    ));
    for (named, m) in [
        (Named::ArrowRight, none),
        (Named::ArrowDown, Modifiers::SHIFT),
        (Named::ArrowRight, Modifiers::CTRL),
        (Named::ArrowLeft, Modifiers::CTRL | Modifiers::ALT),
    ] {
        let msg = key(&e, named, m).expect("an arrow nudges the area");
        e.update(msg);
    }
    // +1 x, +10 y, 1 wider on the right, 1 wider on the left.
    assert_eq!(
        e.doc.view(),
        Rectangle::new(pt(40.0, 30.0), Size::new(62.0, 30.0))
    );
    // Outside Snip, arrows do nothing to the crop.
    let plain = Annotator::new(checker(20, 20), 1.0);
    assert!(key(&plain, Named::ArrowRight, none).is_none());
}

/// Snip's Enter copies; with the Steps strip open it adds the step (full:
/// copies); Ctrl+Enter = Add to steps. AddStep gives the flattened image.
#[test]
fn snips_enter_adds_the_step_while_the_steps_strip_is_open() {
    let mut e = live_area();
    let enter = |e: &Annotator, m| e.key(&Key::Named(Named::Enter), None, m, None);
    let none = Modifiers::empty();
    assert!(matches!(enter(&e, none), Some(Message::Copy)));
    assert!(matches!(enter(&e, Modifiers::CTRL), Some(Message::AddStep)));
    e.steps = Some(3);
    assert!(matches!(enter(&e, none), Some(Message::AddStep)));
    e.steps = Some(STEPS_MAX);
    assert!(
        matches!(enter(&e, none), Some(Message::Copy)),
        "full: copies"
    );
    assert!(e.update(Message::AddStep).is_none(), "full: nothing added");
    e.updating = Some(0);
    let Some(Output::AddStep(flat)) = e.update(Message::AddStep) else {
        panic!("a step edited is updated even when full")
    };
    assert_eq!((flat.width, flat.height), (60, 30), "the area, flattened");
}

/// Ctrl+Z undoes whatever was last done in every state, also while a text,
/// callout, label or arrow label is being typed (the key used to be dropped
/// there: owner, 2026-11-03).
#[test]
fn ctrl_z_undoes_the_last_thing_also_while_typing() {
    let ctrl_z = |e: &Annotator| {
        e.key(
            &Key::Character("z".into()),
            Some('z'),
            Modifiers::CTRL,
            None,
        )
    };
    for tool in [Tool::Text, Tool::Label, Tool::Callout, Tool::ArrowLabel] {
        let mut e = editor(200, 200);
        drag(&mut e, Point::new(10.0, 10.0), &[], Point::new(60.0, 60.0));
        let before = e.doc.items.len();
        e.update(Message::Tool(tool));
        drag(
            &mut e,
            Point::new(80.0, 80.0),
            &[],
            Point::new(150.0, 140.0),
        );
        e.update(Message::Type("hi".into()));
        assert!(e.typing(), "{tool:?}: typing");
        let undo = ctrl_z(&e);
        assert!(matches!(undo, Some(Message::Undo)), "{tool:?}: key");
        e.update(undo.unwrap());
        assert!(!e.typing(), "{tool:?}: typing ended");
        assert_eq!(
            e.doc.items.len(),
            before,
            "{tool:?}: what was typed is gone"
        );
    }
    // an empty box just closes; the next Ctrl+Z goes further back
    let mut e = editor(200, 200);
    drag(&mut e, Point::new(10.0, 10.0), &[], Point::new(60.0, 60.0));
    e.update(Message::Tool(Tool::Text));
    e.update(Message::Press(Point::new(100.0, 100.0)));
    e.update(Message::Release);
    assert!(e.typing());
    e.update(ctrl_z(&e).unwrap());
    assert!(!e.typing());
    assert_eq!(e.doc.items.len(), 1, "the rectangle stays");
    e.update(ctrl_z(&e).unwrap());
    assert_eq!(e.doc.items.len(), 0);
}

/// The Text panel's Font, Bold and Back are remembered; the first run is plain
/// text (no outline); a config without the keys keeps working.
#[test]
fn text_font_bold_and_background_are_remembered_and_default_to_plain() {
    use crate::config::EditorConfig;
    let fresh = Annotator::restored(checker(50, 50), 1.0, EditorConfig::default());
    assert_eq!(fresh.text_style.back, text::Back::None);
    let mut e = Annotator::restored(checker(50, 50), 1.0, EditorConfig::default());
    let style = TextStyle {
        font: text::Font::Mono,
        bold: false,
        back: text::Back::Box,
    };
    e.update(Message::TextStyle(style));
    let saved = e.remembered();
    assert_eq!(saved.text_back, Some(text::Back::Box));
    let again = Annotator::restored(checker(50, 50), 1.0, saved);
    assert_eq!(again.text_style, style);
    // the file: written snake_case, read back
    let toml = toml::to_string(&again.remembered()).unwrap();
    assert!(toml.contains("text_font = \"mono\""), "{toml}");
    let (c, _) = crate::config::Config::parse(&format!("[editor]\n{toml}"));
    assert_eq!(c.editor.text_back, Some(text::Back::Box));
    assert_eq!(c.editor.text_bold, Some(false));
}

/// Ctrl+V (owner, 2026-11-03): with no text box, a new text with the
/// clipboard's text at the pointer (the area's middle when outside), being
/// typed into; while typing, into the text. No text on the clipboard: nothing.
#[test]
fn ctrl_v_pastes_the_clipboard_text() {
    let ctrl_v = |e: &Annotator| {
        e.key(
            &Key::Character("v".into()),
            Some('v'),
            Modifiers::CTRL,
            None,
        )
    };
    let text_of = |e: &Annotator| match e.pending.as_ref().map(|p| &p.item.shape) {
        Some(Shape::Text { at, text, .. }) => (*at, text.clone()),
        other => panic!("not a text: {other:?}"),
    };
    let mut e = editor(200, 200);
    e.update(Message::Tool(Tool::Arrow));
    assert_eq!(ctrl_v(&e), Some(Message::Paste(None)));
    crate::actions::copy::copied::set_paste("hello\r\nworld\t!\n\n");
    e.update(Message::Paste(Some(Point::new(40.0, 50.0))));
    assert!(e.typing());
    assert_eq!(
        text_of(&e),
        (Point::new(40.0, 50.0), "hello\nworld!".into())
    );
    // typing: into the same text
    assert_eq!(ctrl_v(&e), Some(Message::Paste(None)));
    crate::actions::copy::copied::set_paste(" again");
    e.update(Message::Paste(None));
    assert_eq!(text_of(&e).1, "hello\nworld! again");
    e.update(Message::Finish);
    assert_eq!(e.doc.items.len(), 1, "committed like typed text");
    // outside the area: its middle; the tool stays the user's
    crate::actions::copy::copied::set_paste("x");
    e.update(Message::Paste(None));
    assert_eq!(text_of(&e).0, e.doc.view().center());
    assert_eq!(e.tool, Tool::Arrow);
    e.update(Message::Finish);
    // nothing (or a picture) on the clipboard: nothing happens
    e.update(Message::Paste(None));
    assert!(!e.typing() && e.doc.items.len() == 2);
}

/// The live area's edges snap gently to a window's edges (#106): within the
/// reach a handle lands on the edge, a move brings its nearest edge to it;
/// Shift or distance leaves the pointer alone.
#[test]
fn the_live_area_snaps_to_window_edges() {
    let area_after = |tweak: &dyn Fn(&mut Annotator), press: Point, to: Point| {
        let mut e = live_area(); // area (40, 20) 60x30
        e.edges = vec![Rectangle::new(pt(130.0, 10.0), Size::new(40.0, 80.0))];
        tweak(&mut e);
        drag(&mut e, press, &[], to);
        e.doc.crop.unwrap()
    };
    let none = |_: &mut Annotator| {};
    // The right handle at 125: 5 px from the window's left edge (130).
    let r = area_after(&none, pt(100.0, 35.0), pt(125.0, 35.0));
    assert_eq!(r.x + r.width, 130.0);
    // Shift: off.
    let r = area_after(&|e| e.shift = true, pt(100.0, 35.0), pt(125.0, 35.0));
    assert_eq!(r.x + r.width, 125.0);
    // Farther than the reach: off.
    let r = area_after(&none, pt(100.0, 35.0), pt(120.0, 35.0));
    assert_eq!(r.x + r.width, 120.0);
    // Moving the area by 27: its right edge (127) lands on 130, size kept.
    let r = area_after(&none, pt(70.0, 35.0), pt(97.0, 35.0));
    assert_eq!((r.x, r.width), (70.0, 60.0));
    // A keyboard nudge is never snapped.
    let mut e = live_area();
    e.edges = vec![Rectangle::new(pt(102.0, 10.0), Size::new(40.0, 80.0))];
    let before = e.doc.crop.unwrap();
    e.update(Message::Nudge((1, 0), (false, false, false)));
    assert_eq!(e.doc.crop.unwrap().x, before.x + 1.0);
}

/// A window beside a tall area (its rows nowhere near the area's top, middle or
/// bottom) still catches a handle dragged level with it.
#[test]
fn the_live_area_snaps_to_a_window_the_pointer_is_level_with() {
    let mut e = live_area(); // area (40, 20) 60x30 in a 200x100 frame
    e.edges = vec![Rectangle::new(pt(130.0, 70.0), Size::new(40.0, 20.0))];
    // right handle (press at its edge, x=100) dragged to 125 at y=80
    drag(&mut e, pt(100.0, 35.0), &[pt(110.0, 80.0)], pt(125.0, 80.0));
    let r = e.doc.crop.unwrap();
    assert_eq!(r.x + r.width, 130.0);
}

// ---- More: which picture each row gets ------------------------------------------

/// Ask AI gets the annotated picture (what Copy would copy); Copy text and
/// Translate get the capture under the drawing, with Blur / Redact painted out
/// (so a hidden secret has nothing left to read), in the cropped view's own px.
#[test]
fn more_rows_get_the_picture_decided_for_them() {
    use crate::actions::snip_more;
    let redact = item(Shape::Redact {
        a: pt(30.0, 20.0),
        b: pt(60.0, 40.0),
    });
    let arrow = item(Shape::Rect {
        a: pt(5.0, 5.0),
        b: pt(25.0, 15.0),
    });
    let mut e = Annotator::new(checker(100, 80), 1.0);
    for i in [redact, arrow] {
        let n = e.doc.items.len();
        e.doc.exec(Edit::Insert(n, i));
    }
    // Ask AI: the flattened picture, drawing and redaction on it
    let Some(Output::AskAi(asked)) = e.update(Message::AskAi) else {
        panic!("Ask AI answers with the annotated picture");
    };
    let flat = render(&e.doc);
    assert_eq!(asked.rgba, flat.rgba);
    // Copy text / Translate: the clean capture
    let plain = render(&doc_with(checker(100, 80), vec![]));
    let clean = super::clean(&e.doc);
    assert_eq!(clean.rgba, plain.rgba, "nothing drawn on it");
    assert_ne!(clean.rgba, flat.rgba);
    assert!(matches!(
        e.update(Message::CopyText),
        Some(Output::CopyText)
    ));
    assert!(matches!(
        e.update(Message::Translate),
        Some(Output::Translate)
    ));
    // the redacted part is hidden (the rectangle's own box, in image px)...
    let hidden = super::hidden(&e.doc);
    assert!(hidden.boxes.len() == 1 && hidden.covers(45.0, 30.0) && !hidden.covers(10.0, 10.0));
    // ...and painted out of what is read
    let mut read = clean.clone();
    snip_more::mask(&mut read, &super::hidden_in_clean(&e.doc));
    assert_eq!(px(&read, 45, 30), [255, 255, 255, 255]);
    assert_eq!(
        px(&read, 10, 10),
        px(&plain, 10, 10),
        "the rest is as captured"
    );
    // a cropped view: the hidden part moves with the crop's origin
    e.doc.crop = Some(Rectangle::new(pt(20.0, 10.0), Size::new(60.0, 50.0)));
    let (clean, hidden) = (super::clean(&e.doc), super::hidden_in_clean(&e.doc));
    assert_eq!((clean.width, clean.height), (60, 50));
    assert!(
        hidden.covers(25.0, 20.0),
        "(45, 30) in the image is (25, 20) here"
    );
    assert!(!hidden.covers(5.0, 5.0));
}

/// What a blur hides is its footprint, not the pixels its mosaic reads around
/// it: the lines beside it still copy (found on the VM: the reach, 190 px,
/// took the whole page's text).
#[test]
fn a_blur_hides_its_footprint_not_its_reach() {
    let blur = item(Shape::Blur {
        points: vec![pt(200.0, 150.0), pt(260.0, 150.0)],
        size: 20.0,
        cell: 6.0,
        seed: 3,
    });
    let doc = doc_with(checker(500, 300), vec![blur]);
    let hidden = super::hidden(&doc);
    assert!(hidden.covers(230.0, 150.0), "the stroke itself");
    assert!(
        !hidden.covers(230.0, 110.0),
        "a line 40 px above is not hidden"
    );
    assert!(!hidden.covers(100.0, 150.0), "nor text beside it");
}
