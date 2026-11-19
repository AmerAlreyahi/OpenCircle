//! Studio object API for the view (right-click, Library rows, recovery):
//! select at a point / index, hide, reorder, annotations as JSON.
use super::objects_tests::{CLIP, studio};
use super::tests::{RED, item, px};
use super::*;

#[test]
fn s1_1_select_at_a_point_for_the_context_menu() {
    let mut e = studio();
    e.update(Message::Tool(Tool::Rect));
    e.update(Message::SelectAt(Point::new(30.0, 30.0), false));
    assert_eq!(e.selection(), vec![0]);
    assert_eq!(
        e.tool,
        Tool::Select,
        "selecting switches the rail to Select"
    );
    e.update(Message::SelectAt(Point::new(120.0, 30.0), true));
    assert_eq!(e.selection(), vec![0, 1], "add");
    e.update(Message::SelectAt(Point::new(30.0, 30.0), false));
    assert_eq!(
        e.selection(),
        vec![0, 1],
        "right-click inside several keeps them"
    );
    e.update(Message::SelectAt(Point::new(300.0, 250.0), false));
    assert!(e.selection().is_empty(), "an empty spot deselects");
    assert_eq!(e.doc.depth(), 3, "selecting is not an edit");
}

#[test]
fn s1_1_select_index_for_the_objects_list() {
    let mut e = studio();
    e.update(Message::SelectIndex(2, false));
    assert_eq!(e.selection(), vec![2]);
    e.update(Message::SelectIndex(0, true));
    assert_eq!(e.selection(), vec![0, 2]);
    e.update(Message::SelectIndex(2, true));
    assert_eq!(e.selection(), vec![0], "add toggles");
    e.update(Message::SelectIndex(9, false));
    assert_eq!(e.selection(), vec![0], "out of range is nothing");
}

#[test]
fn s1_5_hidden_objects_are_not_drawn_hit_or_copied_and_it_undoes() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    let at = (10u32, 30u32); // on the first box's left edge
    assert_eq!(px(&render(&e.doc), at.0, at.1), RED);
    let depth = e.doc.depth();
    e.update(Message::ToggleHidden(0));
    assert_eq!(e.doc.depth(), depth + 1, "one undo step");
    assert_ne!(px(&render(&e.doc), at.0, at.1), RED, "not drawn / exported");
    e.update(Message::SelectAt(Point::new(30.0, 30.0), false));
    assert!(e.selection().is_empty(), "not hit");
    e.update(Message::SelectIndex(0, false));
    let Some(Output::Copy(c)) = e.update(Message::CopySelection) else {
        panic!("copy")
    };
    assert_eq!(
        (c.width, c.height),
        (400, 300),
        "a hidden object is not copied"
    );
    e.update(Message::Undo);
    assert!(!e.doc.items[0].hidden);
    assert_eq!(px(&render(&e.doc), at.0, at.1), RED);
}

#[test]
fn s1_3_reorder_moves_one_item_in_one_step() {
    let mut e = studio();
    let x = |e: &Annotator, i: usize| e.doc.items[i].frame().x;
    let depth = e.doc.depth();
    e.update(Message::Reorder { from: 0, to: 2 });
    assert_eq!([x(&e, 0), x(&e, 1), x(&e, 2)], [100.0, 200.0, 10.0]);
    assert_eq!(e.selection(), vec![2]);
    assert_eq!(e.doc.depth(), depth + 1);
    e.update(Message::Undo);
    assert_eq!([x(&e, 0), x(&e, 1), x(&e, 2)], [10.0, 100.0, 200.0]);
}

#[test]
fn s1_6_annotations_round_trip_through_json() {
    let _g = CLIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = studio();
    e.doc.exec(Edit::Insert(
        3,
        item(Shape::Pen(vec![
            Point::new(1.0, 2.0),
            Point::new(30.0, 40.0),
        ])),
    ));
    e.doc.exec(Edit::Insert(
        4,
        item(Shape::Arrow {
            a: Point::new(5.0, 5.0),
            b: Point::new(90.0, 60.0),
            bend: Point::new(40.0, 20.0),
            head: true,
            text: "go".into(),
        }),
    ));
    e.doc.exec(Edit::Insert(
        5,
        item(Shape::Text {
            at: Point::new(7.0, 8.0),
            text: "hi".into(),
            size: 14.0,
            style: TextStyle::default(),
        }),
    ));
    e.paste_clipboard(Some(Crop {
        width: 4,
        height: 3,
        rgba: [9, 80, 200, 255].repeat(12),
    }));
    e.doc.items[1].hidden = true;
    let json = e.annotations_json();
    let mut fresh = Annotator::new(e.doc.image.clone(), 1.0);
    fresh.studio = true;
    assert!(fresh.restore_annotations(&json));
    assert_eq!(fresh.doc.items, e.doc.items);
    assert_eq!(
        render(&fresh.doc).rgba,
        render(&e.doc).rgba,
        "draws the same"
    );
    assert!(!fresh.restore_annotations("not json"));
    assert!(!fresh.restore_annotations(r#"{"v":9,"items":[]}"#));
    assert_eq!(fresh.doc.items, e.doc.items, "a bad file changes nothing");
}
