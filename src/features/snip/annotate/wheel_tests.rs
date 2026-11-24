//! #202 Ctrl+wheel resize (W98): over the selection it resizes round the
//! centre, one undo step per burst, in the Studio and the Snip editor alike.
use std::time::{Duration, Instant};

use super::objects_tests::studio;
use super::*;

const OVER: Point = Point::new(35.0, 35.0); // inside the first box (10..60)

fn selected() -> Annotator {
    let mut e = studio();
    e.update(Message::SelectIndex(0, false));
    e
}

fn width(e: &Annotator) -> f32 {
    e.selection_bounds().unwrap().width
}

#[test]
fn w98_202_1_over_the_selection_it_resizes_round_the_centre() {
    let mut e = selected();
    let (w, c) = (width(&e), e.selection_bounds().unwrap().center());
    assert!(e.wheel_resize(OVER, 3.0, Instant::now()));
    assert!(width(&e) > w);
    let after = e.selection_bounds().unwrap();
    assert!((after.center().x - c.x).abs() < 0.01 && (after.center().y - c.y).abs() < 0.01);
    assert_eq!(e.selection(), vec![0], "still selected");
    let later = Instant::now() + Duration::from_secs(5);
    assert!(e.wheel_resize(after.center(), -300.0, later));
    assert!(width(&e) >= 3.9, "never smaller than a few px");
}

#[test]
fn w98_202_2_elsewhere_or_nothing_selected_it_leaves_the_wheel() {
    let mut e = selected();
    let depth = e.doc.depth();
    assert!(!e.wheel_resize(Point::new(300.0, 250.0), 2.0, Instant::now()));
    let mut none = studio();
    assert!(!none.wheel_resize(OVER, 2.0, Instant::now()));
    assert_eq!(e.doc.depth(), depth);
}

#[test]
fn w98_202_3_one_undo_step_per_burst_and_undo_restores_the_size() {
    let mut e = selected();
    let (depth, w) = (e.doc.depth(), width(&e));
    let t = Instant::now();
    for i in 0..4 {
        assert!(e.wheel_resize(OVER, 1.0, t + Duration::from_millis(100 * i)));
    }
    assert_eq!(e.doc.depth(), depth + 1, "one step for the burst");
    assert!((width(&e) - w * 1.06f32.powi(4)).abs() < 0.05);
    // a pause starts a new gesture
    assert!(e.wheel_resize(OVER, 1.0, t + Duration::from_secs(3)));
    assert_eq!(e.doc.depth(), depth + 2);
    e.doc.undo();
    e.doc.undo();
    e.doc.select(&[0]);
    assert!((width(&e) - w).abs() < 0.01, "undo restores the size");
}

#[test]
fn w98_202_4_the_snip_editor_takes_it_through_the_same_message() {
    let mut e = selected();
    e.studio = false;
    let w = width(&e);
    e.update(Message::WheelResize(OVER, 2.0));
    assert!(width(&e) > w);
}

#[test]
fn w98_202_5_another_edit_between_ends_the_burst() {
    let mut e = selected();
    let t = Instant::now();
    e.wheel_resize(OVER, 1.0, t);
    e.update(Message::Duplicate);
    let depth = e.doc.depth();
    let over = e.selection_bounds().unwrap().center();
    e.wheel_resize(over, 1.0, t + Duration::from_millis(50));
    assert_eq!(e.doc.depth(), depth + 1, "its own step");
}
