//! Studio object editing (STUDIO_V3_SPEC S1, S5, S7). Criterion ids: S1.1
//! selection, S1.2 object clipboard, S1.3 duplicate / delete / nudge / alt-drag /
//! order, S1.4 options follow the selection, S7 keys. Each edit = one undo step.
use std::sync::Mutex;

use iced::keyboard::{Key, Modifiers, key::Named};

use super::tests::{RED, image_of, item, px};
use super::*;

/// The object clipboard is one per process: tests that use it take turns.
pub(super) static CLIP: Mutex<()> = Mutex::new(());

pub(super) fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Item {
    item(Shape::Rect {
        a: Point::new(x0, y0),
        b: Point::new(x1, y1),
    })
}

/// A Studio editor on a white 400x300 picture with three boxes.
pub(super) fn studio() -> Annotator {
    let mut e = Annotator::new(image_of(400, 300, |_, _| [255, 255, 255, 255]), 1.0);
    e.studio = true;
    e.tool = Tool::Select;
    for r in [
        rect(10.0, 10.0, 60.0, 60.0),
        rect(100.0, 10.0, 150.0, 60.0),
        rect(200.0, 100.0, 260.0, 160.0),
    ] {
        e.doc.exec(Edit::Insert(e.doc.items.len(), r));
    }
    e.doc.deselect();
    e.refresh();
    e
}

pub(super) fn click(e: &mut Annotator, x: f32, y: f32) {
    e.update(Message::Press(Point::new(x, y)));
    e.update(Message::Release);
}

fn key(e: &Annotator, k: Key, m: Modifiers) -> Option<Message> {
    e.key(&k, None, m, None)
}

fn ch(e: &Annotator, c: char, m: Modifiers) -> Option<Message> {
    e.key(&Key::Character(c.to_string().into()), Some(c), m, None)
}

#[test]
fn s1_1_click_shift_click_select_all_and_escape() {
    let mut e = studio();
    click(&mut e, 30.0, 30.0);
    assert_eq!(e.selection(), vec![0]);
    e.update(Message::Shift(true));
    click(&mut e, 120.0, 30.0);
    assert_eq!(e.selection(), vec![0, 1], "Shift+click adds");
    click(&mut e, 120.0, 30.0);
    assert_eq!(e.selection(), vec![0], "Shift+click again takes it out");
    e.update(Message::Shift(false));
    click(&mut e, 120.0, 30.0);
    assert_eq!(
        e.selection(),
        vec![1],
        "a plain click selects only that one"
    );
    assert_eq!(ch(&e, 'a', Modifiers::CTRL), Some(Message::SelectAll));
    e.update(Message::SelectAll);
    assert_eq!(e.selection(), vec![0, 1, 2]);
    // Esc ladder: selection first, then the tool back to Select
    let esc = || Key::Named(Named::Escape);
    assert_eq!(key(&e, esc(), Modifiers::empty()), Some(Message::Deselect));
    e.update(Message::Deselect);
    assert!(e.selection().is_empty());
    e.update(Message::Tool(Tool::Rect));
    assert_eq!(
        key(&e, esc(), Modifiers::empty()),
        Some(Message::Tool(Tool::Select))
    );
    click(&mut e, 300.0, 250.0);
    assert!(e.selection().is_empty(), "a click on nothing deselects");
}

#[test]
fn s1_1_rubber_band_selects_what_it_touches() {
    let mut e = studio();
    e.update(Message::Press(Point::new(175.0, 75.0)));
    e.update(Message::Move(Point::new(5.0, 5.0)));
    assert!(e.band_rect().is_some());
    e.update(Message::Release);
    assert_eq!(e.selection(), vec![0, 1]);
    assert!(e.band_rect().is_none());
    assert_eq!(e.doc.depth(), 3, "selecting is not an edit");
}

#[test]
fn s1_1_dragging_one_of_several_moves_them_all_in_one_step() {
    let mut e = studio();
    e.update(Message::SelectAll);
    let depth = e.doc.depth();
    e.update(Message::Press(Point::new(30.0, 30.0)));
    e.update(Message::Move(Point::new(50.0, 40.0)));
    e.update(Message::Release);
    let moved = |i: usize| e.doc.items[i].frame().position();
    assert_eq!(moved(0), Point::new(30.0, 20.0));
    assert_eq!(moved(2), Point::new(220.0, 110.0));
    assert_eq!(e.doc.depth(), depth + 1);
    e.update(Message::Undo);
    assert_eq!(e.doc.items[2].frame().position(), Point::new(200.0, 100.0));
}

#[test]
fn s1_2_copy_cut_paste_objects() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    e.update(Message::SelectAll);
    e.update(Message::Deselect);
    // nothing selected: the whole picture (the setting's default)
    let Some(Output::Copy(whole)) = e.update(Message::CopySelection) else {
        panic!("the picture")
    };
    assert_eq!((whole.width, whole.height), (400, 300));
    e.copy_picture_when_empty = false;
    assert!(e.update(Message::CopySelection).is_none(), "setting off");
    e.copy_picture_when_empty = true;
    assert_eq!(ch(&e, 'c', Modifiers::CTRL), Some(Message::CopySelection));
    assert_eq!(
        ch(&e, 'c', Modifiers::CTRL | Modifiers::SHIFT),
        Some(Message::Copy),
        "Ctrl+Shift+C is always the whole picture"
    );
    // two objects: a PNG of just them
    click(&mut e, 30.0, 30.0);
    e.update(Message::Shift(true));
    click(&mut e, 120.0, 30.0);
    e.update(Message::Shift(false));
    let Some(Output::Copy(png)) = e.update(Message::CopySelection) else {
        panic!("the objects")
    };
    assert!(
        png.width < 200 && png.height < 80,
        "{}x{}",
        png.width,
        png.height
    );
    assert_eq!(e.doc.items.len(), 3, "copy changes nothing");
    let depth = e.doc.depth();
    // paste: ours -> the same two, offset, selected, one step
    assert!(e.paste_clipboard(Some(png.clone())));
    assert_eq!(e.doc.items.len(), 5);
    assert_eq!(e.selection(), vec![3, 4]);
    assert_eq!(e.doc.items[3].frame().position(), Point::new(20.0, 20.0));
    assert_eq!(e.doc.depth(), depth + 1);
    e.update(Message::Undo);
    assert_eq!(e.doc.items.len(), 3);
    // cut: copies, removes, one step; the clipboard still pastes
    e.update(Message::SelectAll);
    let depth = e.doc.depth();
    assert!(matches!(
        e.update(Message::CutSelection),
        Some(Output::Copy(_))
    ));
    assert!(e.doc.items.is_empty());
    assert_eq!(e.doc.depth(), depth + 1);
    e.update(Message::Undo);
    assert_eq!(e.doc.items.len(), 3);
    assert!(!e.paste_clipboard(None), "no picture: the host tries text");
}

#[test]
fn s1_2_a_foreign_picture_is_a_movable_object_that_merges() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    let foreign = Crop {
        width: 37,
        height: 23,
        rgba: [0, 200, 0, 255].repeat(37 * 23),
    };
    assert!(e.paste_clipboard(Some(foreign)));
    let Shape::Picture { a, b, lock, .. } = e.doc.items[3].shape.clone() else {
        panic!("a pasted picture object")
    };
    assert!(lock, "ratio locked by default");
    assert_eq!((b.x - a.x, b.y - a.y), (37.0, 23.0));
    assert_eq!(e.selection(), vec![3]);
    // it draws (opacity = colour alpha)
    let drawn = render(&e.doc);
    let c = a + Vector::new(3.0, 3.0);
    assert_eq!(px(&drawn, c.x as u32, c.y as u32)[1], 200);
    e.update(Message::Opacity(0));
    assert_eq!(
        px(&render(&e.doc), c.x as u32, c.y as u32),
        [255, 255, 255, 255]
    );
    e.update(Message::Opacity(255));
    // resizing with the ratio locked keeps it
    let from = e.selection_bounds().unwrap();
    e.update(Message::Bounds(Rectangle::new(
        from.position(),
        Size::new(74.0, 10.0),
    )));
    let after = e.selection_bounds().unwrap();
    assert!(
        (after.width / after.height - 37.0 / 23.0).abs() < 0.01,
        "{after:?}"
    );
    // Merge: the pixels become the picture's, the object goes, ONE undo step
    let depth = e.doc.depth();
    e.update(Message::Merge);
    assert!(
        e.doc.items.is_empty(),
        "the three boxes under it went into the picture too"
    );
    assert_eq!(e.doc.depth(), depth + 1);
    let spot = (after.x + 5.0) as u32 + (after.y + 5.0) as u32 * 400;
    assert_eq!(e.doc.image.rgba[spot as usize * 4 + 1], 200);
    e.update(Message::Undo);
    assert_eq!(e.doc.items.len(), 4);
    assert_eq!(e.doc.image.rgba[spot as usize * 4 + 1], 255);
}

#[test]
fn s1_3_duplicate_delete_and_nudge() {
    let mut e = studio();
    click(&mut e, 30.0, 30.0);
    let depth = e.doc.depth();
    assert_eq!(ch(&e, 'd', Modifiers::CTRL), Some(Message::Duplicate));
    e.update(Message::Duplicate);
    assert_eq!(e.doc.items.len(), 4);
    assert_eq!(e.selection(), vec![3], "the copy is selected");
    assert_eq!(e.doc.depth(), depth + 1);
    // arrows: 1 px, Shift 10 px
    let at = e.doc.items[3].frame().position();
    let right = Key::Named(Named::ArrowRight);
    assert_eq!(
        key(&e, right.clone(), Modifiers::empty()),
        Some(Message::Shove((1, 0), false))
    );
    assert_eq!(
        key(&e, right, Modifiers::SHIFT),
        Some(Message::Shove((1, 0), true))
    );
    e.update(Message::Shove((1, 0), false));
    e.update(Message::Shove((0, 1), true));
    assert_eq!(
        e.doc.items[3].frame().position(),
        at + Vector::new(1.0, 10.0)
    );
    assert_eq!(e.doc.depth(), depth + 3, "one undo step per press");
    // Delete takes every selected object, one step
    e.update(Message::SelectAll);
    let depth = e.doc.depth();
    e.update(Message::Delete);
    assert!(e.doc.items.is_empty());
    assert_eq!(e.doc.depth(), depth + 1);
    e.update(Message::Undo);
    assert_eq!(e.doc.items.len(), 4);
    // nothing selected: arrows are not ours
    e.update(Message::Deselect);
    assert_eq!(
        key(&e, Key::Named(Named::ArrowLeft), Modifiers::empty()),
        None
    );
}

#[test]
fn s1_3_alt_drag_moves_a_copy() {
    let mut e = studio();
    let depth = e.doc.depth();
    e.update(Message::PressCopy(Point::new(30.0, 30.0)));
    e.update(Message::Move(Point::new(80.0, 130.0)));
    e.update(Message::Release);
    assert_eq!(e.doc.items.len(), 4);
    assert_eq!(e.doc.items[0].frame().position(), Point::new(10.0, 10.0));
    assert_eq!(e.doc.items[3].frame().position(), Point::new(60.0, 110.0));
    assert_eq!(e.doc.depth(), depth + 1);
    // an Alt+click that does not move makes no copy
    e.update(Message::PressCopy(Point::new(30.0, 30.0)));
    e.update(Message::Release);
    assert_eq!(e.doc.items.len(), 4);
}

#[test]
fn s1_3_z_order_keys_and_moves() {
    let mut e = studio();
    click(&mut e, 30.0, 30.0); // item 0, at the back
    let m = Modifiers::CTRL;
    assert_eq!(ch(&e, ']', m), Some(Message::Order(Order::Forward)));
    assert_eq!(ch(&e, '[', m), Some(Message::Order(Order::Backward)));
    assert_eq!(
        ch(&e, ']', m | Modifiers::SHIFT),
        Some(Message::Order(Order::Front))
    );
    assert_eq!(
        ch(&e, '[', m | Modifiers::SHIFT),
        Some(Message::Order(Order::Back))
    );
    let first = |e: &Annotator| e.doc.items[e.selection()[0]].frame().x;
    let depth = e.doc.depth();
    e.update(Message::Order(Order::Forward));
    assert_eq!(e.selection(), vec![1]);
    assert_eq!(e.doc.depth(), depth + 1);
    e.update(Message::Order(Order::Front));
    assert_eq!(e.selection(), vec![2]);
    assert_eq!(first(&e), 10.0);
    e.update(Message::Order(Order::Backward));
    assert_eq!(e.selection(), vec![1]);
    e.update(Message::Order(Order::Back));
    assert_eq!(e.selection(), vec![0]);
    e.update(Message::Undo);
    assert_eq!(
        e.selection(),
        Vec::<usize>::new(),
        "undo leaves nothing picked"
    );
    assert_eq!(e.doc.items[0].frame().x, 100.0, "back at the step before");
}

#[test]
fn s1_4_options_follow_the_selection() {
    let mut e = studio();
    let green = [52, 199, 89, 255];
    e.doc.items[1].colour = green;
    e.doc.items[1].width = 8.0;
    let next = e.colour;
    click(&mut e, 120.0, 30.0);
    assert_eq!(
        e.colour, green,
        "the bar shows the selected object's colour"
    );
    assert_eq!(e.selection_style().unwrap().colour, Some(green));
    // editing the bar edits the object only; the next drawing's style is kept
    let blue = [10, 132, 255, 255];
    e.update(Message::Colour(blue));
    assert_eq!(e.doc.items[1].colour, blue);
    assert_eq!(e.doc.items[1].width, 8.0, "an untouched value stays");
    assert_eq!(e.doc.items[0].colour, RED);
    assert_eq!(
        e.remembered().colour.as_deref(),
        Some("#FF3B30"),
        "{:?}",
        e.remembered().colour
    );
    // several with different colours: mixed; touching the colour sets all
    e.update(Message::SelectAll);
    let s = e.selection_style().unwrap();
    assert_eq!((s.count, s.colour), (3, None));
    e.update(Message::Colour(green));
    assert!(e.doc.items.iter().all(|i| i.colour == green));
    assert_eq!(e.doc.items[1].width, 8.0, "mixed widths stay mixed");
    // nothing selected: the bar is the next drawing's again
    e.update(Message::Deselect);
    assert_eq!(e.colour, next);
    assert!(e.selection_style().is_none());
}

#[test]
fn s7_snip_widget_keeps_its_keys() {
    // not the Studio: Ctrl+C is the whole picture, Ctrl+D is the eyedropper's letter path
    let mut e = studio();
    e.studio = false;
    assert_eq!(ch(&e, 'c', Modifiers::CTRL), Some(Message::Copy));
    assert_eq!(ch(&e, 'x', Modifiers::CTRL), None);
    click(&mut e, 30.0, 30.0);
    e.update(Message::Shift(true));
    click(&mut e, 120.0, 30.0);
    e.update(Message::Shift(false));
    assert_eq!(e.selection().len(), 1, "the widget selects one item");
}
