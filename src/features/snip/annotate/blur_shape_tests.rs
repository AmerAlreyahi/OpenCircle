//! W102 step 2: Hide's shapes (rectangle, ellipse, free brush, "Hide this
//! shape"), outward feather, an ellipse stays an ellipse.
use super::blur_tests::{pt, rendered};
use super::tests::{image_of, item, px};
use super::*;

fn points_of(e: &Annotator) -> Vec<Point> {
    let item = match (&e.pending, e.doc.items.first()) {
        (Some(p), _) => &p.item,
        (None, Some(i)) => i,
        _ => panic!("no blur"),
    };
    match &item.shape {
        Shape::Blur { points, .. } => points.clone(),
        other => panic!("{other:?}"),
    }
}

/// W102-2.a: a ring traced with the brush keeps its points, with line help on
/// (it only holds a stroke on one line) and off; today 1 of 240.
#[test]
fn w102_2a_a_ring_trace_keeps_its_points() {
    for assist in [true, false] {
        let mut e = Annotator::new(image_of(400, 400, |_, _| [250, 250, 250, 255]), 1.0);
        e.assist = assist;
        e.update(Message::Tool(Tool::Blur));
        let ring = |i: u32| {
            let a = i as f32 / 240.0 * std::f32::consts::TAU;
            pt(200.0 + 120.0 * a.cos(), 200.0 + 120.0 * a.sin())
        };
        e.update(Message::Press(ring(0)));
        for i in 1..=240 {
            e.update(Message::Move(ring(i)));
        }
        let kept = points_of(&e).len();
        e.update(Message::Release);
        assert!(kept >= 150, "line help {assist}: {kept} of 240 points kept");
    }
}

/// W102-2.b: a line held by line help still un-blurs when the pointer slides
/// back (#154); with line help off the brush never loses a point.
#[test]
fn w102_2b_drag_back_shrinks_only_a_held_line() {
    let run = |assist: bool| {
        let mut e = Annotator::new(image_of(400, 200, |_, _| [250, 250, 250, 255]), 1.0);
        e.assist = assist;
        e.update(Message::Tool(Tool::Blur));
        e.update(Message::Press(pt(20.0, 100.0)));
        e.update(Message::Move(pt(200.0, 100.0)));
        e.update(Message::Move(pt(100.0, 100.0)));
        points_of(&e).iter().map(|p| p.x).fold(f32::MIN, f32::max)
    };
    assert_eq!(run(true), 100.0, "held line: shrinks back");
    assert_eq!(run(false), 200.0, "free brush: nothing lost");
}

fn drawn(kind: blur::Kind, a: Point, b: Point, outline: f32, image: Image) -> Crop {
    rendered(
        image,
        4,
        vec![item(Shape::Blur {
            points: vec![a, b],
            size: 16.0,
            cell: 6.0,
            style: blur::Style::Frost,
            kind,
            outline,
            ramp: Vec::new(),
        })],
    )
}

fn busy(w: u32, h: u32) -> Image {
    image_of(w, h, |x, y| {
        if (x / 3 + y / 3) % 3 != 0 {
            [250, 250, 250, 255]
        } else {
            [10, 20, 30, 255]
        }
    })
}

fn pxi(i: &Image, x: u32, y: u32) -> [u8; 4] {
    let k = ((y * i.width + x) * 4) as usize;
    i.rgba[k..k + 4].try_into().unwrap()
}

/// W102-2.c: an ellipse covers an ellipse (its box corners stay), a rectangle
/// its box; the inside is whole (nothing of the source shows) and the edge
/// fades OUTWARD only: the source is untouched past the feather.
#[test]
fn w102_2c_ellipse_and_rectangle_cover_their_shape_and_feather_outward() {
    let (a, b) = (pt(60.0, 40.0), pt(180.0, 120.0));
    let plain = busy(240, 160);
    let ellipse = drawn(blur::Kind::Ellipse, a, b, 0.0, busy(240, 160));
    let rect = drawn(blur::Kind::Rect, a, b, 0.0, busy(240, 160));
    let same = |c: &Crop, x, y| px(c, x, y) == pxi(&plain, x, y);
    assert!(!same(&ellipse, 120, 80), "centre hidden");
    assert!(!same(&rect, 62, 42), "a rectangle hides its corner");
    assert!(same(&ellipse, 62, 42), "an ellipse keeps its box corner");
    // Past the feather the source is untouched, in both.
    let far = blur::feather(6.0) as u32 + 3;
    for c in [&ellipse, &rect] {
        assert!(same(c, 60 - far, 80) && same(c, 180 + far, 80));
        assert!(same(c, 120, 40 - far) && same(c, 120, 120 + far));
    }
    // Inside whole: another source pattern (same colours) gives the same
    // pixels well inside.
    let other = drawn(
        blur::Kind::Ellipse,
        a,
        b,
        0.0,
        image_of(240, 160, |x, y| {
            if (x * 7 + y * 13) % 5 < 2 {
                [10, 20, 30, 255]
            } else {
                [250, 250, 250, 255]
            }
        }),
    );
    for (x, y) in [(120, 80), (100, 60), (150, 100), (90, 80)] {
        assert_eq!(px(&ellipse, x, y), px(&other, x, y), "({x}, {y})");
    }
}

/// W102-2.d: Redact keeps an ellipse an ellipse.
#[test]
fn w102_2d_redact_keeps_an_ellipse_an_ellipse() {
    let mut e = Annotator::new(image_of(240, 160, |_, _| [250, 250, 250, 255]), 1.0);
    let ellipse = item(Shape::Ellipse {
        a: pt(60.0, 40.0),
        b: pt(180.0, 120.0),
    });
    e.doc.exec(Edit::Insert(0, ellipse));
    e.doc.select_one(Some(0));
    let redact = PRESETS.iter().position(|p| p.tool == Tool::Redact).unwrap();
    e.update(Message::Preset(redact));
    assert!(matches!(
        e.doc.items[0].shape,
        Shape::Redact { oval: true, .. }
    ));
    let out = render(&e.doc);
    assert_ne!(px(&out, 120, 80), [250, 250, 250, 255], "centre redacted");
    assert_eq!(
        px(&out, 62, 42),
        [250, 250, 250, 255],
        "the box's corner is outside the ellipse"
    );
}

/// W102-2.e: "Hide this shape" puts a Hide over the selected ellipse (inside),
/// or along its outline only; one undo step.
#[test]
fn w102_2e_hide_this_shape_fills_or_follows_the_outline() {
    let image = busy(240, 160);
    let setup = || {
        let mut e = Annotator::new(busy(240, 160), 1.0);
        let ellipse = Item {
            width: 6.0,
            ..item(Shape::Ellipse {
                a: pt(60.0, 40.0),
                b: pt(180.0, 120.0),
            })
        };
        e.doc.exec(Edit::Insert(0, ellipse));
        e.doc.select_one(Some(0));
        e
    };
    let mut ring = setup();
    ring.update(Message::HideShape(true));
    let out = render(&ring.doc);
    let at = |x, y| px(&out, x, y);
    assert!(
        (176..184).any(|x| at(x, 80) != pxi(&image, x, 80)),
        "the edge is hidden"
    );
    assert_eq!(at(120, 80), pxi(&image, 120, 80), "the inside is not");
    let mut fill = setup();
    fill.update(Message::HideShape(false));
    assert_ne!(px(&render(&fill.doc), 120, 80), pxi(&image, 120, 80));
    fill.update(Message::Undo);
    assert_eq!(fill.doc.items.len(), 1, "one undo step");
}
