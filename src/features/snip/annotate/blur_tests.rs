//! W102 step 1: Hide's consistent fill (background + a fixed pull toward the
//! ink, one texture per picture, strength in image px).
use super::tests::{image_of, item, px};
use super::*;

fn pt(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn hide(points: Vec<Point>, size: f32, style: blur::Style) -> Item {
    item(Shape::Blur {
        points,
        size,
        cell: 6.0,
        style,
    })
}

/// A card: `bg`, with three lines of dark text.
fn card(bg: [u8; 4]) -> Image {
    image_of(300, 150, |x, y| {
        let line = [30..40, 70..80, 110..120].iter().any(|r| r.contains(&y));
        if line && x % 5 < 3 && (10..280).contains(&x) {
            [20, 20, 30, 255]
        } else {
            bg
        }
    })
}

fn rendered(image: Image, seed: u64, items: Vec<Item>) -> Crop {
    let mut doc = Document::new(image);
    doc.seed = seed;
    for i in items {
        doc.exec(Edit::Insert(doc.items.len(), i));
    }
    render(&doc)
}

fn mean(c: &Crop, x0: u32, x1: u32, y0: u32, y1: u32) -> [f32; 3] {
    let mut sum = [0.0; 3];
    for y in y0..y1 {
        for x in x0..x1 {
            let p = px(c, x, y);
            (0..3).for_each(|k| sum[k] += p[k] as f32);
        }
    }
    let n = ((x1 - x0) * (y1 - y0)) as f32;
    sum.map(|v| v / n)
}

/// W102-1.a: two Hides on different lines of one background match within four
/// levels on average (today: median 7, p90 19; the picture's texture itself is
/// +-10 and averages out only over its scale), in both styles, whatever the seed.
#[test]
fn w102_1a_two_strokes_on_one_background_match() {
    for style in [blur::Style::Frost, blur::Style::Pixelate] {
        for seed in [1, 2, 3, 4] {
            let line = |y: f32| hide(vec![pt(20.0, y), pt(280.0, y)], 16.0, style);
            let out = rendered(
                card([250, 250, 250, 255]),
                seed,
                vec![line(35.0), line(75.0)],
            );
            let (a, b) = (mean(&out, 20, 280, 30, 40), mean(&out, 20, 280, 70, 80));
            for k in 0..3 {
                assert!(
                    (a[k] - b[k]).abs() <= 4.0,
                    "{style:?} seed {seed}: {a:?} {b:?}"
                );
            }
        }
    }
}

/// W102-1.b: the fill is a background pulled 12 % toward the ink, and keeps a
/// pastel's own hue (today it snapped to grey).
#[test]
fn w102_1b_background_pulled_toward_ink_keeps_hue() {
    let lavender = [200, 190, 240, 255];
    let out = rendered(
        card(lavender),
        5,
        vec![hide(
            vec![pt(20.0, 35.0), pt(280.0, 35.0)],
            16.0,
            blur::Style::Frost,
        )],
    );
    let m = mean(&out, 20, 280, 30, 40);
    // 0.88 * bg + 0.12 * ink (20, 20, 30): about (178, 170, 215).
    for (k, want) in [178.0, 170.0, 215.0].into_iter().enumerate() {
        assert!((m[k] - want).abs() <= 4.0, "{m:?}");
    }
    assert!(m[2] - m[0] > 25.0, "still lavender: {m:?}");
}

/// W102-1.c: the texture belongs to the picture: a stroke drawn as two
/// overlapping ones gives the pixels of one stroke (no seam where they meet).
#[test]
fn w102_1c_overlapping_strokes_merge() {
    for style in [blur::Style::Frost, blur::Style::Pixelate] {
        let one = hide(vec![pt(20.0, 75.0), pt(280.0, 75.0)], 16.0, style);
        let a = hide(vec![pt(20.0, 75.0), pt(160.0, 75.0)], 16.0, style);
        let b = hide(vec![pt(120.0, 75.0), pt(280.0, 75.0)], 16.0, style);
        let whole = rendered(card([250, 250, 250, 255]), 9, vec![one]);
        let joined = rendered(card([250, 250, 250, 255]), 9, vec![a, b]);
        let mut worst = 0;
        for y in 68..82 {
            for x in 22..278 {
                let (p, q) = (px(&whole, x, y), px(&joined, x, y));
                worst = worst.max((0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap());
            }
        }
        assert!(
            worst <= 2,
            "{style:?}: two strokes vs one differ by {worst}"
        );
    }
}

/// W102-1.d: Hide never reads the content: the same line with other glyph
/// positions gives identical pixels (the lab: anything that reads it is
/// recovered 100 %).
#[test]
fn w102_1d_hide_does_not_read_the_text() {
    let shifted = |dx: u32| {
        let mut img = card([250, 250, 250, 255]);
        for y in 30..40 {
            for x in 0..300u32 {
                let i = ((y * 300 + x) * 4) as usize;
                let on = (x + dx) % 5 < 3 && (10..280).contains(&x);
                let v = if on {
                    [20, 20, 30, 255]
                } else {
                    [250, 250, 250, 255]
                };
                img.rgba[i..i + 4].copy_from_slice(&v);
            }
        }
        img
    };
    for style in [blur::Style::Frost, blur::Style::Pixelate] {
        let at = |dx| {
            let out = rendered(
                shifted(dx),
                3,
                vec![hide(vec![pt(20.0, 35.0), pt(280.0, 35.0)], 16.0, style)],
            );
            // Inside the footprint (x 12..288, y 27..43).
            (30..40)
                .flat_map(|y| (14..286).map(move |x| (x, y)))
                .map(|(x, y)| px(&out, x, y))
                .collect::<Vec<_>>()
        };
        assert_eq!(at(0), at(2), "{style:?}");
    }
}

/// W102-1.e: the strength is in image px times the capture scale, never the
/// zoom: the same stroke at zoom 0.5, 1 and 2 has the same texture scale.
#[test]
fn w102_1e_strength_does_not_follow_the_zoom() {
    let cell_at = |zoom: f32| {
        let mut e = Annotator::new(image_of(400, 200, |_, _| [250, 250, 250, 255]), 1.0);
        e.zoom = zoom;
        e.tool = Tool::Blur;
        e.update(Message::Press(pt(50.0, 100.0)));
        e.update(Message::Move(pt(150.0, 100.0)));
        e.update(Message::Release);
        let Shape::Blur { cell, .. } = e.doc.items[0].shape else {
            panic!("a blur")
        };
        cell
    };
    assert_eq!(cell_at(0.5), cell_at(1.0));
    assert_eq!(cell_at(1.0), cell_at(2.0));
}

/// W102-1.f: a saved Hide from before the style existed (and with the old
/// per-stroke seed) still loads, as the default look.
#[test]
fn w102_1f_old_saved_blur_loads_as_frost() {
    let old = r#"{"Blur":{"points":[[1.0,2.0],[30.0,2.0]],"size":20.0,"cell":6.0,"seed":12345}}"#;
    let shape: Shape = serde_json::from_str(old).expect("old blur loads");
    assert!(
        matches!(
            shape,
            Shape::Blur {
                style: blur::Style::Frost,
                ..
            }
        ),
        "{shape:?}"
    );
}
