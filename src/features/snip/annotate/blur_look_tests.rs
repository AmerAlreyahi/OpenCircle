//! W102 step 4: the look styles (Soft, Frosted, Progressive): not for secrets,
//! never the default, one options list shared by Snip and the Studio.
use super::blur_tests::{pt, rendered};
use super::tests::{image_of, item, px};
use super::*;

/// Text-like stripes on white: lots of detail to blur.
fn stripes(w: u32, h: u32, phase: u32) -> Image {
    image_of(w, h, |x, y| {
        if (x + phase + y / 4) % 6 < 3 {
            [20, 20, 30, 255]
        } else {
            [250, 250, 250, 255]
        }
    })
}

fn look(style: blur::Style, ramp: Vec<Point>) -> Item {
    item(Shape::Blur {
        points: vec![pt(20.0, 20.0), pt(220.0, 140.0)],
        size: 16.0,
        cell: 12.0,
        style,
        kind: blur::Kind::Rect,
        outline: 0.0,
        ramp,
    })
}

fn variation(c: &Crop, y0: u32, y1: u32) -> f32 {
    let mut sum = 0.0;
    for y in y0..y1 {
        for x in 30..210 {
            sum += (px(c, x, y)[0] as f32 - px(c, x + 1, y)[0] as f32).abs();
        }
    }
    sum / ((y1 - y0) * 180) as f32
}

/// W102-4.a: Soft blurs what is there (the opposite of Hide: it reads the
/// picture, so two pictures differ), and its note is "for looks".
#[test]
fn w102_4a_soft_reads_the_picture_and_is_a_look() {
    assert!(blur::Style::Soft.is_look() && !blur::Style::Frost.is_look());
    let (a, b) = (
        rendered(
            stripes(240, 160, 0),
            1,
            vec![look(blur::Style::Soft, vec![])],
        ),
        rendered(
            stripes(240, 160, 2),
            1,
            vec![look(blur::Style::Soft, vec![])],
        ),
    );
    assert_ne!(a.rgba, b.rgba, "a look reads the picture");
    let plain = rendered(stripes(240, 160, 0), 1, vec![]);
    assert!(
        variation(&a, 40, 120) < variation(&plain, 40, 120) / 3.0,
        "blurred: {} vs {}",
        variation(&a, 40, 120),
        variation(&plain, 40, 120)
    );
}

/// W102-4.b: Frosted is Soft plus a light tint: lighter on dark detail.
#[test]
fn w102_4b_frosted_adds_a_light_tint() {
    let mean = |c: &Crop| {
        let mut s = 0.0;
        for y in 40..120 {
            for x in 40..200 {
                s += px(c, x, y)[0] as f32;
            }
        }
        s / (80.0 * 160.0)
    };
    let soft = rendered(
        stripes(240, 160, 0),
        1,
        vec![look(blur::Style::Soft, vec![])],
    );
    let frosted = rendered(
        stripes(240, 160, 0),
        1,
        vec![look(blur::Style::Frosted, vec![])],
    );
    assert!(
        mean(&frosted) > mean(&soft) + 20.0,
        "{} vs {}",
        mean(&frosted),
        mean(&soft)
    );
}

/// W102-4.c: Progressive fades from its strongest handle to clear at the other:
/// the picture is whole at the end and blurred at the start, between them
/// in steps.
#[test]
fn w102_4c_progressive_fades_from_the_start_handle_to_clear() {
    let ramp = vec![pt(120.0, 140.0), pt(120.0, 20.0)];
    let out = rendered(
        stripes(240, 160, 0),
        1,
        vec![look(blur::Style::Progressive, ramp)],
    );
    let plain = rendered(stripes(240, 160, 0), 1, vec![]);
    let (top, mid, bottom) = (
        variation(&out, 22, 34),
        variation(&out, 74, 86),
        variation(&out, 126, 138),
    );
    let whole = variation(&plain, 22, 34);
    assert!(
        top > whole * 0.8,
        "clear at the end handle: {top} of {whole}"
    );
    assert!(bottom < whole / 3.0, "blurred at the start: {bottom}");
    assert!(bottom < mid && mid < top, "{bottom} < {mid} < {top}");
}

/// W102-4.d: a look is still skipped by Copy text / Ask AI (hidden boxes), and
/// the editor opens on Hide, never on a look.
#[test]
fn w102_4d_looks_are_hidden_from_text_reading_and_never_the_default() {
    let doc = {
        let mut d = Document::new(stripes(240, 160, 0));
        d.exec(Edit::Insert(0, look(blur::Style::Soft, vec![])));
        d
    };
    assert!(super::hidden(&doc).covers(100.0, 80.0));
    let e = Annotator::new(stripes(240, 160, 0), 1.0);
    assert_eq!(e.blur_style, blur::Style::Frost);
    let styles = blur_styles();
    assert_eq!(styles.iter().filter(|(s, _)| s.is_look()).count(), 3);
    assert!(
        !styles[0].0.is_look(),
        "the first, the default, is not a look"
    );
}

/// W102-4.e: picking a style restyles the selected Hide (the Studio), and a
/// progressive one has two handles that move its ramp.
#[test]
fn w102_4e_style_applies_to_the_selected_hide_and_progressive_has_handles() {
    let mut e = Annotator::new(stripes(240, 160, 0), 1.0);
    e.studio = true;
    e.doc.exec(Edit::Insert(
        0,
        look(
            blur::Style::Progressive,
            vec![pt(120.0, 140.0), pt(120.0, 20.0)],
        ),
    ));
    e.doc.select_one(Some(0));
    e.update(Message::BlurStyle(blur::Style::Soft));
    let Shape::Blur { style, .. } = &e.doc.items[0].shape else {
        panic!()
    };
    assert_eq!(*style, blur::Style::Soft);
    e.update(Message::BlurStyle(blur::Style::Progressive));
    let handles = select::handles(&e.doc.items[0]);
    assert_eq!(handles.len(), 2, "start and end");
    let moved = select::dragged(&e.doc.items[0], handles[1].0, pt(40.0, 60.0));
    let Shape::Blur { ramp, .. } = &moved.shape else {
        panic!()
    };
    assert_eq!(ramp[1], pt(40.0, 60.0));
    assert_eq!(ramp[0], pt(120.0, 140.0));
}
