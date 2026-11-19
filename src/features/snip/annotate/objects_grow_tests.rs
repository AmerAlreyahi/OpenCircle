//! "Grow the canvas past the edge": the budget, the refusals, the one-image
//! history, and the drag-out JPEG's quality.
use super::objects_tests::{rect, studio};
use super::*;

fn growing() -> Annotator {
    let mut e = studio(); // 400x300, three boxes
    e.grow_canvas = true;
    e
}

fn bounds_at(e: &Annotator, x: f32, y: f32) -> Rectangle {
    let b = e.selection_bounds().unwrap();
    Rectangle::new(Point::new(x, y), b.size())
}

#[test]
fn a_move_far_outside_is_refused_and_never_panics_or_cuts_the_picture() {
    let mut e = growing();
    e.doc.image.rgba[0..4].copy_from_slice(&[1, 2, 3, 255]);
    e.update(Message::SelectIndex(0, false));
    let (before, depth) = (e.doc.items.clone(), e.doc.depth());
    for (x, y) in [
        (-20000.0, 10.0),
        (10.0, -20000.0),
        (30000.0, 10.0),
        (f32::NAN, 10.0),
        (-1.0e9, -1.0e9),
    ] {
        let to = bounds_at(&e, x, y);
        e.update(Message::Bounds(to));
        assert_eq!(
            (e.doc.image.width, e.doc.image.height),
            (400, 300),
            "{x} {y}"
        );
        assert_eq!(e.doc.items, before, "nothing moved: {x} {y}");
        assert_eq!(e.doc.depth(), depth, "no history step");
        assert_eq!(
            e.take_grow_refusal(),
            Some(GrowRefusal::TooLarge),
            "{x} {y}"
        );
    }
    assert_eq!(
        e.doc.image.rgba[0..4],
        [1, 2, 3, 255],
        "the picture is whole"
    );
    assert_eq!(e.take_grow_refusal(), None, "said once");
}

#[test]
fn the_pixel_budget_refuses_a_growth_that_is_too_big() {
    // 1060 x 760 is about 6.7x the area: over 4x
    let mut e = growing();
    e.update(Message::SelectIndex(2, false));
    let to = bounds_at(&e, 1000.0, 700.0);
    e.update(Message::Bounds(to));
    assert_eq!((e.doc.image.width, e.doc.image.height), (400, 300));
    assert_eq!(e.take_grow_refusal(), Some(GrowRefusal::TooManyPixels));
    // a size that is allowed grows
    let to = bounds_at(&e, 420.0, 100.0);
    e.update(Message::Bounds(to));
    assert!(e.doc.image.width > 400);
    assert_eq!(e.take_grow_refusal(), None);
    // the big absolute budget: a picture already near it cannot double
    let mut big = Annotator::new(
        Image {
            width: 8192,
            height: 8192,
            rgba: vec![255; 8192 * 8192 * 4],
        },
        1.0,
    );
    big.studio = true;
    big.grow_canvas = true;
    big.doc.exec(Edit::Insert(0, rect(10.0, 10.0, 60.0, 60.0)));
    big.doc.select(&[0]);
    let to = Rectangle::new(Point::new(8100.0, 8100.0), Size::new(1000.0, 1000.0));
    big.update(Message::Bounds(to)); // 9100 x 9100 = 82 MP
    assert_eq!(big.doc.image.width, 8192);
    assert_eq!(big.take_grow_refusal(), Some(GrowRefusal::TooManyPixels));
}

#[test]
fn growth_keeps_every_pixel_and_undo_redo_rebuild_the_same_canvas() {
    let mut e = growing();
    e.doc.image.rgba[0..4].copy_from_slice(&[9, 8, 7, 255]); // pixel (0, 0)
    let first = e.doc.image.clone();
    e.update(Message::SelectIndex(0, false));
    let to = bounds_at(&e, -50.0, -40.0);
    e.update(Message::Bounds(to));
    let (w, h) = (e.doc.image.width as usize, e.doc.image.height as usize);
    assert_eq!((w, h), (400 + 50, 300 + 40));
    let at = (40 * w + 50) * 4;
    assert_eq!(
        e.doc.image.rgba[at..at + 4],
        [9, 8, 7, 255],
        "moved by the offset"
    );
    assert_eq!(
        e.doc.image.rgba[0..4],
        [0, 0, 0, 0],
        "new pixels see through"
    );
    let grown = e.doc.image.clone();
    e.update(Message::Undo);
    assert_eq!(e.doc.image.rgba, first.rgba, "undo: the same picture");
    e.update(Message::Redo);
    assert_eq!(
        (e.doc.image.width, e.doc.image.height, &e.doc.image.rgba),
        (grown.width, grown.height, &grown.rgba),
        "redo: rebuilt, identical"
    );
}

#[test]
fn the_history_keeps_one_picture_per_growth_not_two() {
    let e = {
        let mut e = growing();
        e.update(Message::SelectIndex(2, false));
        let to = bounds_at(&e, 430.0, 100.0);
        e.update(Message::Bounds(to));
        e
    };
    // the Canvas edit holds the OLD picture and a size, nothing of the grown one
    let (_, edit) = e.doc.last_edit().expect("an edit");
    let Edit::Seq(all) = edit else {
        panic!("{edit:?}")
    };
    let Some(Edit::Canvas { old, size, .. }) = all.last() else {
        panic!("{all:?}")
    };
    assert_eq!((old.width, old.height), (400, 300));
    assert!(size.0 > 400);
}

#[test]
fn the_drag_out_jpeg_uses_the_studio_quality() {
    // noise, so quality shows in the size
    let rgba: Vec<u8> = (0..64u32 * 64)
        .flat_map(|i| {
            let v = (i.wrapping_mul(2654435761) >> 24) as u8;
            [v, v.wrapping_mul(3), v.wrapping_add(90), 255]
        })
        .collect();
    let crop = Crop {
        width: 64,
        height: 64,
        rgba,
    };
    let size = |q| {
        let f = dragout::DragFile::write_jpeg(&crop, q).unwrap();
        std::fs::metadata(f.path()).unwrap().len()
    };
    assert!(size(20) < size(95), "the quality is passed on");
    // write_as reads [studio] quality (85 by default), not a fixed 90
    let f = dragout::DragFile::write_as(&crop, true).unwrap();
    assert_eq!(std::fs::metadata(f.path()).unwrap().len(), size(85));
    assert_ne!(size(85), size(90));
}
