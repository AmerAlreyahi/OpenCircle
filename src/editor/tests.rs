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
fn blur_changes_only_its_region() {
    let blur = item(Shape::Blur {
        a: pt(20.0, 16.0),
        b: pt(68.0, 52.0),
    });
    let plain = render(&doc_with(checker(100, 80), vec![]));
    let blurred = render(&doc_with(checker(100, 80), vec![blur]));
    let mut changed = 0;
    for y in 0..80 {
        for x in 0..100 {
            let inside = (20..68).contains(&x) && (16..52).contains(&y);
            if !inside {
                assert_eq!(px(&plain, x, y), px(&blurred, x, y), "({x}, {y})");
            } else if px(&plain, x, y) != px(&blurred, x, y) {
                changed += 1;
            }
        }
    }
    assert!(changed > 48 * 36 / 2, "{changed}");
    // One cell is one colour.
    let cell = px(&blurred, 20, 16);
    for (x, y) in [(31, 16), (20, 27), (31, 27)] {
        assert_eq!(px(&blurred, x, y), cell);
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
    Editor::new(checker(w, h), 1.0)
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
            Tool::Text | Tool::Callout => {
                drag(e, base, &[], base);
                e.update(Message::Type("Hi".into()));
                e.update(Message::Type("!".into()));
                e.update(Message::Backspace);
                e.update(Message::Finish);
            }
            Tool::Step => drag(e, base, &[], base),
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
    let mut e = editor(900, 400);
    let made = draw_with_every_tool(&mut e);
    assert_eq!(made, TOOLS.len() - 2);
    let kinds: Vec<_> = e
        .doc
        .items
        .iter()
        .map(|i| std::mem::discriminant(&i.shape))
        .collect();
    for (i, k) in kinds.iter().enumerate() {
        assert!(!kinds[i + 1..].contains(k), "two items of one kind");
    }
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
        .chain(['\u{e04c}', '\u{e2a0}', '\u{e29f}']);
    for icon in icons {
        let code = format!("U+{:04X}", icon as u32);
        assert!(script.contains(&code), "add {code} to subset.sh");
    }
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
    with(&mut e, Tool::Blur, pt(186.0, 292.0), &[], pt(398.0, 334.0));
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
