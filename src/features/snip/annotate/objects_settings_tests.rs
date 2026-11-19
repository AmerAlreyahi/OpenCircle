//! The `[studio]` settings on the editor side: pasted pictures merge at once,
//! the canvas grows past the edge (undoable), the drag-out file type.
use super::objects_tests::{CLIP, studio};
use super::*;

fn green(w: u32, h: u32) -> Crop {
    Crop {
        width: w,
        height: h,
        rgba: [0, 200, 0, 255].repeat((w * h) as usize),
    }
}

#[test]
fn a_pasted_picture_merges_at_once_when_it_is_not_movable() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    e.paste_movable = false;
    let depth = e.doc.depth();
    assert!(e.paste_clipboard(Some(green(30, 20))));
    assert_eq!(e.doc.items.len(), 3, "no object came of it");
    assert_eq!(e.doc.depth(), depth + 1, "one undo step");
    let at = (150 * 400 + 200) * 4; // the picture's middle
    assert_eq!(e.doc.image.rgba[at + 1], 200);
    e.update(Message::Undo);
    assert_eq!(e.doc.image.rgba[at + 1], 255);
}

#[test]
fn the_canvas_grows_past_the_edge_and_undoes_in_one_step() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    // off: nothing grows
    e.update(Message::SelectAt(Point::new(30.0, 30.0), false));
    e.update(Message::Shove((1, 0), true));
    assert_eq!(e.doc.image.width, 400);
    // on: a nudge that leaves the right edge grows it
    e.grow_canvas = true;
    e.update(Message::SelectAt(Point::new(230.0, 130.0), false));
    let depth = e.doc.depth();
    for _ in 0..20 {
        e.update(Message::Shove((1, 0), true));
    }
    assert!(e.doc.image.width > 400, "{}", e.doc.image.width);
    assert!(e.doc.depth() > depth);
    let wide = e.doc.image.width;
    // a drag past the left edge grows to the left; the drawings move with it
    let (x0, w0) = (e.doc.items[0].frame().x, e.doc.image.width);
    e.update(Message::SelectAt(Point::new(30.0, 30.0), false));
    e.update(Message::Press(Point::new(30.0, 30.0)));
    e.update(Message::Move(Point::new(-20.0, 30.0)));
    e.update(Message::Release);
    assert!(e.doc.image.width > w0, "grew to the left");
    assert!(e.doc.items[0].frame().x >= 0.0, "inside the new canvas");
    assert!(
        e.doc.items[1].frame().x > 100.0,
        "the others moved with the picture"
    );
    // one undo takes the drag AND the growth back
    e.update(Message::Undo);
    assert_eq!(e.doc.image.width, wide);
    assert_eq!(e.doc.items[0].frame().position(), Point::new(x0, 10.0));
}

#[test]
fn drag_out_writes_a_jpeg_when_the_setting_says_so() {
    let crop = green(8, 6);
    let png = dragout::DragFile::write_as(&crop, false).unwrap();
    let jpg = dragout::DragFile::write_as(&crop, true).unwrap();
    assert_eq!(png.path().extension().unwrap(), "png");
    assert_eq!(jpg.path().extension().unwrap(), "jpg");
    let bytes = std::fs::read(jpg.path()).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "a JPEG");
}
