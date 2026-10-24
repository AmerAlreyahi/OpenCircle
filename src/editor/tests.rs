use super::*;
use iced::keyboard::{Key, Modifiers, key::Named};

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
fn blur_changes_only_its_stroke() {
    let size = 16.0;
    let stroke = vec![pt(20.0, 40.0), pt(50.0, 40.0), pt(80.0, 40.0)];
    let blur = item(Shape::Blur {
        points: stroke.clone(),
        size,
        seed: 7,
    });
    let plain = render(&doc_with(checker(100, 80), vec![]));
    let blurred = render(&doc_with(checker(100, 80), vec![blur]));
    // Everything beyond the soft edge stays; the core is all new.
    let edge = size / 2.0 + 2.0 * blur::feather(size) + 2.0;
    let mut core = 0;
    for y in 0..80u32 {
        for x in 0..100u32 {
            let p = pt(x as f32 + 0.5, y as f32 + 0.5);
            let d = stroke
                .iter()
                .map(|q| q.distance(p))
                .fold(f32::MAX, f32::min)
                .min(if (20.0..80.0).contains(&p.x) {
                    (p.y - 40.0).abs()
                } else {
                    f32::MAX
                });
            if d > edge {
                assert_eq!(px(&plain, x, y), px(&blurred, x, y), "({x}, {y})");
            } else if d < size / 2.0 - 1.0 {
                core += 1;
                // The checker is gone: the core is a noisy grey, not black or white.
                let [r, ..] = px(&blurred, x, y);
                assert!((60..200).contains(&r), "({x}, {y}) {r}");
            }
        }
    }
    assert!(core > 60 * 12, "{core}");
}

/// A password field: `secret` in dark text on an off-white field.
fn field(secret: &str) -> Image {
    let (w, h) = (240, 40);
    let mut layer = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(
            image_of(w, h, |_, _| [250, 250, 250, 255]).rgba,
            tiny_skia::IntSize::from_wh(w, h).unwrap(),
        )
        .unwrap(),
        origin: Point::ORIGIN,
    };
    text::paint(
        &mut layer,
        pt(8.0, 8.0),
        secret,
        20.0,
        [20, 20, 30, 255],
        false,
    );
    Image {
        width: w,
        height: h,
        rgba: layer.pm.take(),
    }
}

fn luma(p: [u8; 4]) -> u32 {
    (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8
}

#[test]
fn blur_brush_destroys_the_text_it_hides() {
    // The line under the brush, found by line assist; the brush fits it.
    let bands = lines::detect(&field("hunter2"));
    assert_eq!(bands.len(), 1, "{bands:?}");
    let band = bands[0];
    let size = band.brush();
    let stroke = vec![pt(8.0, band.centre()), pt(232.0, band.centre())];
    let blurred = |secret: &str, seed| {
        let blur = item(Shape::Blur {
            points: stroke.clone(),
            size,
            seed,
        });
        render(&doc_with(field(secret), vec![blur]))
    };
    // The core: the whole line, where the brush is at full strength.
    let (y0, y1) = (band.y0 as u32, band.y1.ceil() as u32);
    let core: Vec<(u32, u32)> = (y0..y1)
        .flat_map(|y| (10..230).map(move |x| (x, y)))
        .collect();

    // Strong enough to erase glyph shapes: sigma >= 0.6 x the line height.
    let r = blur::radius(size) as f32;
    assert!((r * (r + 1.0)).sqrt() >= 0.6 * band.height());

    // No ink survives: every text pixel is replaced, nothing dark is left.
    let original = field("hunter2");
    let raw = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y * 240 + x) * 4) as usize;
        original.rgba[i..i + 4].try_into().unwrap()
    };
    let out = blurred("hunter2", 1);
    let mut ink = 0;
    for &(x, y) in &core {
        let (before, after) = (raw(x, y), px(&out, x, y));
        assert!(luma(after) > 140, "({x}, {y}) still dark: {after:?}");
        if luma(before) < 200 {
            ink += 1;
            assert_ne!(after, before, "({x}, {y}) survived");
        }
    }
    assert!(ink > 200, "the field has text: {ink}");

    // Two different passwords differ less than the same password under two seeds:
    // what is left of the text is drowned by the noise.
    let diff = |p: &Crop, q: &Crop| -> f32 {
        let total: u32 = core
            .iter()
            .map(|&(x, y)| {
                let (a, b) = (px(p, x, y), px(q, x, y));
                (0..3).map(|c| a[c].abs_diff(b[c]) as u32).sum::<u32>()
            })
            .sum();
        total as f32 / (core.len() * 3) as f32
    };
    let noise = diff(&out, &blurred("hunter2", 2));
    let text = diff(&out, &blurred("letmein", 1));
    assert!(noise > 4.0, "noise {noise}");
    assert!(text < noise, "text signal {text} vs noise {noise}");
}

/// `secret` in the field, brushed over its line (as the editor does with line
/// assist); `grain` = (cell, fine) amplitudes, `None` = the real ones.
fn brushed(secret: &str, seed: u64, grain: Option<(f32, f32)>) -> (Image, lines::Band) {
    let band = lines::detect(&field("hunter2"))[0];
    let mut layer = document::Layer {
        pm: tiny_skia::Pixmap::from_vec(
            field(secret).rgba,
            tiny_skia::IntSize::from_wh(240, 40).unwrap(),
        )
        .unwrap(),
        origin: Point::ORIGIN,
    };
    let stroke = [pt(8.0, band.centre()), pt(232.0, band.centre())];
    match grain {
        Some((cell, fine)) => blur::paint_with(&mut layer, &stroke, band.brush(), seed, cell, fine),
        None => blur::paint(&mut layer, &stroke, band.brush(), seed),
    }
    let image = Image {
        width: 240,
        height: 40,
        rgba: layer.pm.take(),
    };
    (image, band)
}

/// Mean brightness of each column and each row of the band (x 10..230).
fn profiles(image: &Image, band: lines::Band) -> (Vec<f32>, Vec<f32>) {
    let (y0, y1) = (band.y0 as u32, band.y1.ceil() as u32);
    let at = |x: u32, y: u32| {
        let i = ((y * 240 + x) * 4) as usize;
        luma(image.rgba[i..i + 4].try_into().unwrap()) as f32
    };
    let columns = (10..230)
        .map(|x| (y0..y1).map(|y| at(x, y)).sum::<f32>() / (y1 - y0) as f32)
        .collect();
    let rows = (y0..y1)
        .map(|y| (10..230).map(|x| at(x, y)).sum::<f32>() / 220.0)
        .collect();
    (columns, rows)
}

fn mean_gap(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(p, q)| (p - q).abs()).sum::<f32>() / a.len() as f32
}

const SECRETS: [(&str, &str); 3] = [
    ("hunter2", "letmein"),
    ("4f9a8b7c", "0d9e8f7a"),
    ("password", "qwertyui"),
];

/// What is left of the text after the Gaussian alone, as brightness per pixel
/// and per column average: the grain must be larger.
#[test]
fn blur_brush_residual_is_below_the_grain() {
    let mut worst: f32 = 0.0;
    for (a, b) in SECRETS {
        let (pa, band) = brushed(a, 1, Some((0.0, 0.0)));
        let (pb, _) = brushed(b, 1, Some((0.0, 0.0)));
        let (ca, _) = profiles(&pa, band);
        let (cb, _) = profiles(&pb, band);
        let column = ca
            .iter()
            .zip(&cb)
            .map(|(p, q)| (p - q).abs())
            .fold(0.0, f32::max);
        worst = worst.max(column);
    }
    eprintln!("residual text signal: max column-average gap {worst:.1} levels");
    assert!(
        blur::GRAIN >= 2.0 * worst,
        "grain ±{} vs residual {worst}",
        blur::GRAIN
    );
}

/// An attacker averages columns (or rows) to cancel per-pixel noise. With the
/// cell grain the averaged profiles of two different secrets differ less than
/// those of one secret under two seeds: the text is below the noise.
#[test]
fn blur_brush_column_and_row_averages_hide_the_text() {
    for (a, b) in SECRETS {
        let (a1, band) = brushed(a, 1, None);
        let (b1, _) = brushed(b, 1, None);
        let (a2, _) = brushed(a, 2, None);
        let (ca1, ra1) = profiles(&a1, band);
        let (cb1, rb1) = profiles(&b1, band);
        let (ca2, ra2) = profiles(&a2, band);
        let (text_c, noise_c) = (mean_gap(&ca1, &cb1), mean_gap(&ca1, &ca2));
        let (text_r, noise_r) = (mean_gap(&ra1, &rb1), mean_gap(&ra1, &ra2));
        eprintln!(
            "{a}/{b}: columns text {text_c:.1} noise {noise_c:.1}; rows text {text_r:.1} noise {noise_r:.1}"
        );
        assert!(
            2.0 * text_c < noise_c,
            "{a}/{b} columns: {text_c} vs {noise_c}"
        );
        assert!(
            2.0 * text_r < noise_r,
            "{a}/{b} rows: {text_r} vs {noise_r}"
        );
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

fn editor(w: u32, h: u32) -> Editor {
    let mut e = Editor::new(checker(w, h), 1.0);
    e.assist = false; // not the user's config
    e
}

/// Press at `from`, drag through `via` to `to`, release.
fn drag(e: &mut Editor, from: Point, via: &[Point], to: Point) {
    e.update(Message::Press(from));
    for &p in via {
        e.update(Message::Move(p));
    }
    e.update(Message::Move(to));
    e.update(Message::Release);
}

/// Draws one item with every tool; returns how many items it made.
fn draw_with_every_tool(e: &mut Editor) -> usize {
    let mut made = 0;
    for info in TOOLS {
        e.update(Message::Tool(info.tool));
        let base = pt(20.0 + 70.0 * made as f32, 30.0 + 25.0 * made as f32);
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
    let mut e = editor(1300, 500);
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

    // Resize from the bottom-right corner: twice as wide.
    let f = e.doc.items[0].frame();
    let corner = pt(f.x + f.width, f.y + f.height);
    drag(&mut e, corner, &[], pt(f.x + 2.0 * f.width, f.y + f.height));
    assert!((e.doc.items[0].frame().width - 2.0 * f.width).abs() < 0.01);

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
        let mut e = Editor::new(image_of(200, 60, |_, _| [255; 4]), 1.0);
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

fn live_pixels(e: &Editor) -> (Rectangle, Vec<u8>) {
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
    let key = |e: &Editor, k: &Key, m: Modifiers| {
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
    let script = include_str!("../../assets/lucide/subset.sh");
    let icons = TOOLS
        .iter()
        .map(|t| t.icon)
        .chain(toolbar::GLYPHS.iter().copied())
        .chain(['\u{e13e}', crate::ui::widgets::DRAG]);
    for icon in icons {
        let code = format!("U+{:04X}", icon as u32);
        assert!(script.contains(&code), "add {code} to subset.sh");
    }
}

// ---- steps' legend -------------------------------------------------------------------

/// Places a step at `p` and types its note.
fn step_with(e: &mut Editor, p: Point, note: &str) {
    e.update(Message::Tool(Tool::Step));
    drag(e, p, &[], p);
    e.update(Message::Type(note.into()));
    e.update(Message::Finish);
}

const SLATE: [u8; 4] = [40, 44, 60, 255];

#[test]
fn legend_lists_the_notes_in_a_free_corner_or_a_band() {
    let mut e = Editor::new(image_of(400, 300, |_, _| SLATE), 1.0);
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
fn copy_all_gives_the_image_and_its_words() {
    let mut e = editor(300, 200);
    step_with(&mut e, pt(30.0, 30.0), "Open settings");
    step_with(&mut e, pt(90.0, 30.0), "Click Save");
    e.update(Message::Tool(Tool::Label));
    drag(&mut e, pt(30.0, 120.0), &[], pt(30.0, 120.0));
    e.update(Message::Type("Done".into()));
    e.update(Message::Tool(Tool::ArrowLabel));
    drag(&mut e, pt(200.0, 150.0), &[], pt(260.0, 100.0));
    e.update(Message::Type("here".into()));
    e.update(Message::Finish);
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    let key = e.key(&Key::Character("c".into()), Some('c'), ctrl_shift, None);
    assert!(matches!(key, Some(Message::CopyAll)));
    let Some(Output::CopyWithText(flat, text)) = e.update(Message::CopyAll) else {
        panic!("CopyAll gives the image and the text")
    };
    assert!(flat.width == 300 && flat.height >= 200, "with the legend");
    assert_eq!(text, "1. Open settings\n2. Click Save\nDone\nhere");
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
    use crate::ui::theme::ANNOTATION;
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

#[test]
fn tool_buttons_open_their_variants() {
    let mut e = editor(100, 100);
    assert_eq!(e.tool, Tool::Arrow);
    e.update(Message::Slot(Tool::Arrow));
    assert_eq!(e.menu, Some(Menu::Variants(Tool::Arrow)));
    e.update(Message::Slot(Tool::Arrow));
    assert_eq!(e.menu, None);
    e.update(Message::Slot(Tool::Text));
    assert_eq!(
        (e.tool, e.menu),
        (Tool::Text, Some(Menu::Variants(Tool::Text)))
    );
    e.update(Message::Tool(Tool::Label));
    assert_eq!((e.tool, e.menu), (Tool::Label, None));
    // A button without variants just picks its tool.
    e.update(Message::Slot(Tool::Rect));
    assert_eq!((e.tool, e.menu), (Tool::Rect, None));
    // Esc closes a popover before it leaves the editor.
    e.update(Message::Menu(Menu::Board));
    let esc = |e: &Editor| e.key(&Key::Named(Named::Escape), None, Modifiers::empty(), None);
    assert!(matches!(esc(&e), Some(Message::Menu(Menu::Board))));
    e.update(Message::Menu(Menu::Board));
    assert!(matches!(esc(&e), Some(Message::Back)));
    // Every tool is reachable from a toolbar button.
    for t in TOOLS {
        assert!(TOOLS.iter().any(|s| s.tool == t.slot && s.slot == s.tool));
    }
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

#[test]
fn toolbar_size_matches_its_layout() {
    let e = editor(100, 100);
    assert_eq!(laid_out(e.toolbar()), toolbar::size());
    // It still fits a 1366 px laptop screen.
    assert!(
        toolbar::size().width < 1366.0 - 48.0,
        "{:?}",
        toolbar::size()
    );
}

// ---- presets, copy / paste style, eyedropper -------------------------------------------

#[test]
fn presets_style_the_active_tool() {
    use crate::ui::theme::ANNOTATION;
    let colour = |i: usize| ANNOTATION[i].into_rgba8();
    let mut e = editor(200, 150);
    let key = |e: &Editor, c: char| {
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
    let mut e = Editor::new(image, 1.0);
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

// ---- combine board ----------------------------------------------------------------------

#[test]
fn board_layouts_place_captures() {
    use board::{GAP as G, Layout, place};
    let sizes = [(100, 50), (60, 80), (120, 40)];
    let at =
        |tiles: &[Rectangle]| -> Vec<(f32, f32)> { tiles.iter().map(|t| (t.x, t.y)).collect() };
    let g = G as f32;

    let (tiles, size) = place(&sizes[..1], Layout::Grid);
    assert_eq!(
        (at(&tiles), size),
        (vec![(0.0, 0.0)], Size::new(100.0, 50.0))
    );

    let (tiles, size) = place(&sizes, Layout::Stack);
    assert_eq!(
        at(&tiles),
        [
            (g + 10.0, g),
            (g + 30.0, 2.0 * g + 50.0),
            (g, 3.0 * g + 130.0)
        ]
    );
    assert_eq!(size, Size::new(120.0 + 2.0 * g, 170.0 + 4.0 * g));

    let (tiles, size) = place(&sizes, Layout::Side);
    assert_eq!(
        at(&tiles),
        [
            (g, g + 15.0),
            (2.0 * g + 100.0, g),
            (3.0 * g + 160.0, g + 20.0)
        ]
    );
    assert_eq!(size, Size::new(280.0 + 4.0 * g, 80.0 + 2.0 * g));

    // Grid: 2 columns of 120x80 cells, each capture centred in its cell.
    let (tiles, size) = place(&sizes, Layout::Grid);
    assert_eq!(
        at(&tiles),
        [
            (g + 10.0, g + 15.0),
            (2.0 * g + 120.0 + 30.0, g),
            (g, 2.0 * g + 80.0 + 20.0)
        ]
    );
    assert_eq!(size, Size::new(240.0 + 3.0 * g, 160.0 + 3.0 * g));
    for (t, s) in tiles.iter().zip(sizes) {
        assert_eq!(t.size(), Size::new(s.0 as f32, s.1 as f32));
    }
}

#[test]
fn the_board_keeps_annotations_on_their_capture() {
    use board::{BACKGROUND, GAP as G, Layout};
    let g = G as f32;
    let blue = [10, 132, 255, 255];
    let mut e = Editor::new(image_of(100, 50, |_, _| SLATE), 1.0);
    e.update(Message::Tool(Tool::Rect));
    drag(&mut e, pt(40.0, 15.0), &[], pt(80.0, 40.0));
    e.update(Message::Tool(Tool::Crop));
    drag(&mut e, pt(0.0, 0.0), &[], pt(90.0, 45.0));
    assert!(matches!(
        e.update(Message::AddCapture),
        Some(Output::AddCapture)
    ));

    e.append(image_of(60, 80, |_, _| blue));
    assert_eq!(e.doc.crop, None, "the board shows whole");
    assert_eq!(
        (e.doc.image.width, e.doc.image.height),
        (100 + 2 * G, 130 + 3 * G)
    );
    let flat = render(&e.doc);
    assert_eq!(px(&flat, 2, 2), BACKGROUND);
    assert_eq!(
        px(&flat, (g + 20.0 + 50.0) as u32, (2.0 * g + 90.0) as u32),
        blue
    );
    // The number label on capture 1 (dark), the rectangle moved with it.
    assert!(luma(px(&flat, (g + 9.0) as u32, (g + 12.0) as u32)) < 60);
    let Shape::Rect { a, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!(a, pt(40.0 + g, 15.0 + g));

    // An arrow on capture 2, then side by side: each moves with its capture.
    e.update(Message::Tool(Tool::Arrow));
    drag(&mut e, pt(60.0, 120.0), &[], pt(80.0, 150.0));
    e.update(Message::Layout(Layout::Side));
    let tiles = e.doc.tiles().to_vec();
    assert_eq!(
        tiles.iter().map(|t| (t.x, t.y)).collect::<Vec<_>>(),
        [(g, g + 15.0), (2.0 * g + 100.0, g)]
    );
    let Shape::Rect { a, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!(a, pt(40.0 + g, 30.0 + g));
    let Shape::Arrow { a, .. } = e.doc.items[1].shape else {
        panic!()
    };
    // From (60, 120) on capture 2 at (g + 20, 2g + 50) to its new place.
    assert_eq!(
        a,
        pt(
            60.0 - (g + 20.0) + tiles[1].x,
            120.0 - (2.0 * g + 50.0) + tiles[1].y
        )
    );

    // Numbers off: the label is gone (the capture shows there).
    e.update(Message::Numbers);
    assert_eq!(
        px(&render(&e.doc), (g + 9.0) as u32, (g + 27.0) as u32),
        SLATE
    );

    // Undo walks it all back: numbers, layout, arrow, the added capture (and the crop).
    for _ in 0..4 {
        e.update(Message::Undo);
    }
    assert_eq!((e.doc.image.width, e.doc.image.height), (100, 50));
    assert!(e.doc.crop.is_some());
    assert_eq!(e.doc.items.len(), 1);
    let Shape::Rect { a, .. } = e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!(a, pt(40.0, 15.0));
    for _ in 0..4 {
        e.update(Message::Redo);
    }
    assert_eq!(e.doc.board.layout, Layout::Side);
    assert!(!e.doc.board.numbers);
    assert_eq!(e.doc.items.len(), 2);

    // A third capture in a grid.
    e.append(image_of(40, 40, |_, _| RED));
    e.update(Message::Layout(Layout::Grid));
    assert_eq!(e.doc.board.count, 3);
    assert_eq!(e.doc.tiles().len(), 3);
    assert_eq!(e.size(), Size::new(200.0 + 3.0 * g, 160.0 + 3.0 * g));
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

// ---- line assist -------------------------------------------------------------------

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

#[test]
fn the_lock_holds_a_line_and_switches_after_a_moment() {
    use lines::{Band, Follow, Lock, SWITCH_AFTER};
    use std::time::Duration;
    let band = |y0: f32| Band {
        x0: 0.0,
        x1: 300.0,
        y0,
        y1: y0 + 20.0,
    };
    let bands = [band(10.0), band(50.0)];
    let t0 = std::time::Instant::now();
    let ms = |n: u64| t0 + Duration::from_millis(n);
    let stay = |f: Follow| match f {
        Follow::Stay(p) => p,
        Follow::Switch(b) => panic!("switched to {b:?}"),
    };
    let mut lock = Lock::new(bands[0]);
    assert_eq!(
        stay(lock.follow(&bands, pt(50.0, 25.0), ms(0))),
        pt(50.0, 20.0)
    );
    // Wobbling near the line: still on it.
    assert_eq!(
        stay(lock.follow(&bands, pt(60.0, 36.0), ms(10))),
        pt(60.0, 20.0)
    );
    // Over the next line, but only briefly: stays.
    stay(lock.follow(&bands, pt(70.0, 60.0), ms(20)));
    stay(lock.follow(&bands, pt(80.0, 60.0), ms(100)));
    stay(lock.follow(&bands, pt(85.0, 22.0), ms(120))); // back: the clock resets
    stay(lock.follow(&bands, pt(90.0, 60.0), ms(130)));
    stay(lock.follow(&bands, pt(95.0, 60.0), ms(200)));
    // Held there long enough: on to that line.
    let at = 130 + SWITCH_AFTER.as_millis() as u64;
    assert!(
        matches!(lock.follow(&bands, pt(99.0, 60.0), ms(at)), Follow::Switch(b) if b == bands[1])
    );
    assert_eq!(
        stay(lock.follow(&bands, pt(100.0, 58.0), ms(at + 10))),
        pt(100.0, 60.0)
    );
    // Far away with no line there: held where it is.
    assert_eq!(
        stay(lock.follow(&bands, pt(100.0, 150.0), ms(at + 500))),
        pt(100.0, 60.0)
    );
}

#[test]
fn the_blur_brush_follows_text_lines() {
    let image = page(400, 160, PAPER, INK, &PARAGRAPH);
    let bands = lines::detect(&image);
    let mut e = Editor::new(image, 1.0);
    e.assist = true;
    e.update(Message::Tool(Tool::Blur));
    let b = bands[1];
    let c = b.centre();
    // A wobbly stroke along line 2: held on it, as tall as it.
    drag(
        &mut e,
        pt(15.0, c + 3.0),
        &[pt(80.0, c - 5.0), pt(150.0, c + 6.0)],
        pt(220.0, c),
    );
    let Shape::Blur { points, size, .. } = &e.doc.items[0].shape else {
        panic!()
    };
    assert!(points.iter().all(|p| p.y == c), "{points:?}");
    assert_eq!(*size, b.brush());
    // Shift: free.
    e.update(Message::Shift(true));
    drag(
        &mut e,
        pt(15.0, c + 3.0),
        &[pt(80.0, c - 5.0)],
        pt(220.0, c),
    );
    let Shape::Blur { points, .. } = &e.doc.items[1].shape else {
        panic!()
    };
    assert!(points.iter().any(|p| p.y != c));
    e.update(Message::Shift(false));

    // Dragged onto line 3 and held there: the stroke goes on along line 3.
    e.update(Message::Press(pt(15.0, bands[0].centre())));
    e.update(Message::Move(pt(60.0, bands[0].centre())));
    e.update(Message::Move(pt(70.0, bands[2].centre())));
    std::thread::sleep(lines::SWITCH_AFTER + std::time::Duration::from_millis(20));
    e.update(Message::Move(pt(80.0, bands[2].centre() + 2.0)));
    e.update(Message::Move(pt(150.0, bands[2].centre() - 2.0)));
    e.update(Message::Release);
    let ys: Vec<f32> = e.doc.items[2..]
        .iter()
        .map(|i| match &i.shape {
            Shape::Blur { points, .. } => points[0].y,
            _ => panic!(),
        })
        .collect();
    assert_eq!(ys, [bands[0].centre(), bands[2].centre()]);

    // Assist off: the stroke is free but still fits the line it starts on;
    // off any text it takes the weight's size.
    e.update(Message::Assist);
    assert!(!e.assist);
    drag(
        &mut e,
        pt(15.0, c + 3.0),
        &[pt(80.0, c - 5.0)],
        pt(220.0, c),
    );
    let Shape::Blur { points, size, .. } = &e.doc.items[4].shape else {
        panic!()
    };
    assert!(points.iter().any(|p| p.y != c) && *size == b.brush());
    drag(&mut e, pt(15.0, 140.0), &[], pt(220.0, 140.0));
    let Shape::Blur { size, .. } = &e.doc.items[5].shape else {
        panic!()
    };
    assert_eq!(*size, Width::Medium.brush());
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
fn demo_editor() -> Editor {
    let mut e = Editor::new(demo_image(), 1.0);
    let colour = |e: &mut Editor, i: usize| {
        e.update(Message::Colour(
            crate::ui::theme::ANNOTATION[i].into_rgba8(),
        ));
    };
    // Draws with `tool` from `from` through `via` to `to`.
    let with = |e: &mut Editor, tool: Tool, from: Point, via: &[Point], to: Point| {
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
    let scale = 2.0;
    let (w, h) = ((size.width * scale) as u32, (size.height * scale) as u32);
    let rgba = renderer.screenshot(Size::new(w, h), scale, background);
    crate::actions::write_png(path, w, h, &rgba).unwrap();
}

/// `OPENCIRCLE_EDITOR_PNG=<dir> cargo test editor::tests::snapshots -- --ignored`
/// writes the toolbar (Dark, Light), the editor with every tool, and the flattened
/// result. Not run by default: it writes files and uses the system's sans-serif.
#[test]
#[ignore = "writes PNGs to look at"]
fn snapshots() {
    use crate::ui::{theme, widgets};
    let dir = std::path::PathBuf::from(
        std::env::var("OPENCIRCLE_EDITOR_PNG").expect("OPENCIRCLE_EDITOR_PNG=<dir>"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    iced::advanced::graphics::text::font_system()
        .write()
        .unwrap()
        .load_font(std::borrow::Cow::Borrowed(widgets::ICON_FONT_BYTES));

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
            crate::ui::widgets::ICON_FONT_BYTES,
        ));
    dir
}

fn write(dir: &std::path::Path, name: &str, c: &Crop) {
    crate::actions::write_png(&dir.join(name), c.width, c.height, &c.rgba).unwrap();
}

const NIGHT: [u8; 4] = [30, 30, 30, 255];
const MOON: [u8; 4] = [230, 230, 232, 255];

/// A dark chat answer: five lines of text.
fn chat_block() -> Image {
    page(
        620,
        230,
        NIGHT,
        MOON,
        &[
            (
                20.0,
                16.0,
                "The card now floats with a thin border and a grip,",
            ),
            (
                20.0,
                56.0,
                "the action bar wraps neatly into the free space.",
            ),
            (20.0, 96.0, "API key: sk-live-4f9a8b7c6d5e4f3a2b1c0d9e8f7a"),
            (20.0, 136.0, "Account: amer@example.com, plan Pro, 3 seats"),
            (
                20.0,
                176.0,
                "Both commits are squashed; the build is green.",
            ),
        ],
    )
}

/// `OPENCIRCLE_EDITOR_PNG=<dir> cargo test editor::tests::snapshots_board -- --ignored`
/// writes a 3-capture board (steps + legend, presets, the new tools), the editor
/// around it, the toolbar with each popover, and a text block blurred line by line.
#[test]
#[ignore = "writes PNGs to look at"]
fn snapshots_board() {
    use crate::ui::theme;
    let dir = png_dir();

    // Blur brush with line assist: lines 3 and 4 (a key, an e-mail), wobbly strokes.
    let mut e = Editor::new(chat_block(), 1.0);
    e.assist = true;
    write(&dir, "blur-before.png", &render(&e.doc));
    let bands = lines::detect(&e.doc.image);
    e.update(Message::Tool(Tool::Blur));
    for b in &bands[2..4] {
        let c = b.centre();
        drag(
            &mut e,
            pt(18.0, c + 4.0),
            &[pt(150.0, c - 6.0), pt(300.0, c + 7.0), pt(450.0, c - 3.0)],
            pt(b.x1 + 4.0, c + 2.0),
        );
    }
    write(&dir, "blur-after.png", &render(&e.doc));

    // The board: an app window, a form, the chat block.
    let mut e = Editor::new(demo_image(), 1.0);
    e.assist = true;
    e.append(field("hunter2"));
    e.append(chat_block());
    e.update(Message::Layout(board::Layout::Grid));
    let t = e.doc.tiles().to_vec();
    let at = |i: usize, x: f32, y: f32| pt(t[i].x + x, t[i].y + y);
    // 1: red box + arrow on the Save button, steps with notes.
    e.update(Message::Tool(Tool::Rect));
    e.update(Message::Preset(0));
    drag(&mut e, at(0, 552.0, 352.0), &[], at(0, 768.0, 418.0));
    e.update(Message::Tool(Tool::ArrowLabel));
    drag(&mut e, at(0, 470.0, 455.0), &[], at(0, 555.0, 400.0));
    e.update(Message::Type("then this".into()));
    e.update(Message::Finish);
    step_with(&mut e, at(0, 40.0, 72.0), "Open Account settings");
    step_with(&mut e, at(1, 222.0, 20.0), "Type the new password");
    step_with(&mut e, at(0, 532.0, 386.0), "Save; it renews on the 1st");
    // 2: highlight yellow over a text line.
    e.update(Message::Preset(1));
    let line: Vec<_> = (0..20)
        .map(|i| at(0, 90.0 + 15.0 * i as f32, 165.0))
        .collect();
    drag(&mut e, line[0], &line[1..], at(0, 400.0, 165.0));
    // 3: blue callout.
    e.update(Message::Preset(2));
    drag(&mut e, at(0, 700.0, 85.0), &[], at(0, 660.0, 200.0));
    e.update(Message::Type("Pro plan".into()));
    e.update(Message::Finish);
    // 4: redact the password; a label and a pointer; the key blurred.
    e.update(Message::Preset(3));
    drag(&mut e, at(1, 92.0, 10.0), &[], at(1, 200.0, 32.0));
    e.doc.selected = None;
    e.update(Message::Colour(theme::ANNOTATION[3].into_rgba8()));
    e.update(Message::Tool(Tool::Label));
    drag(&mut e, at(2, 360.0, 200.0), &[], at(2, 360.0, 200.0));
    e.update(Message::Type("build is green".into()));
    e.update(Message::Finish);
    e.doc.selected = None;
    e.update(Message::Colour(theme::ANNOTATION[0].into_rgba8()));
    e.update(Message::Tool(Tool::Pointer));
    drag(&mut e, at(0, 160.0, 312.0), &[], at(0, 160.0, 312.0));
    let b = lines::detect(&e.doc.image)
        .into_iter()
        .find(|b| b.y0 > t[2].y + 90.0 && b.x0 > t[2].x)
        .unwrap();
    e.update(Message::Tool(Tool::Blur));
    drag(
        &mut e,
        pt(b.x0, b.centre() + 3.0),
        &[pt(b.x0 + 200.0, b.centre() - 4.0)],
        pt(b.x1, b.centre()),
    );
    e.doc.selected = None;
    write(&dir, "board.png", &render(&e.doc));

    let backdrop = iced::Color::from_rgb8(0x2a, 0x2d, 0x3e);
    e.set_zoom(0.6);
    e.update(Message::Tool(Tool::Select));
    let size = e.size() + Size::new(560.0, 90.0);
    shoot(e.view(), size, backdrop, &dir.join("board-editor.png"));

    // The toolbar with each popover.
    let bar = Size::new(1300.0, 470.0);
    for (name, open, light) in [
        ("toolbar-colour-dark.png", Message::Picker, false),
        ("toolbar-colour-light.png", Message::Picker, true),
        ("toolbar-board.png", Message::Menu(Menu::Board), false),
        ("toolbar-arrows.png", Message::Slot(Tool::Arrow), false),
        ("toolbar-blur.png", Message::Slot(Tool::Blur), false),
    ] {
        let mut e = Editor::new(demo_image(), 1.0);
        e.append(field("x"));
        if matches!(open, Message::Slot(Tool::Arrow)) {
            e.update(Message::Tool(Tool::Line));
        }
        e.update(open);
        theme::set_light(light);
        let backdrop = if light {
            iced::Color::from_rgb8(0xdc, 0xe0, 0xe8)
        } else {
            backdrop
        };
        shoot(e.toolbar(), bar, backdrop, &dir.join(name));
    }
    theme::set_light(false);
}

/// Timings on a 3440x1440 crop:
/// `cargo test --release editor::tests::timing -- --ignored --nocapture`
#[test]
#[ignore = "timing; meaningful in release"]
fn timing() {
    use std::time::Instant;
    let mut e = Editor::new(checker(3440, 1440), 1.0);
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
