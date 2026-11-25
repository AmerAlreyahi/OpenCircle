//! W113: fixes from the review of W111.
use super::blur_tests::pt;
use super::tests::image_of;
use super::*;

fn page() -> Image {
    image_of(600, 300, |x, y| {
        if (y / 10) % 2 == 0 && x % 6 < 3 && (20..150).contains(&x) {
            [30, 30, 40, 255]
        } else {
            [240, 240, 235, 255]
        }
    })
}

/// Review 2: after a pen stroke, then Shapes, the row's controls style the NEXT shape, never the
/// hidden selected stroke (Snip and Studio).
#[test]
fn w113_2_a_drawing_tool_never_restyles_the_old_selection() {
    for studio in [false, true] {
        let mut e = Annotator::new(page(), 1.0);
        e.studio = studio;
        e.update(Message::Tool(Tool::Pen));
        e.update(Message::Press(pt(50.0, 50.0)));
        e.update(Message::Move(pt(120.0, 90.0)));
        e.update(Message::Release);
        let stroke = e.doc.items[0].clone();
        e.update(Message::Slot(Tool::Rect));
        e.update(Message::Width(Width::Thick));
        e.update(Message::Fill(true));
        e.update(Message::Outline(true));
        e.update(Message::Colour([1, 2, 3, 255]));
        assert_eq!(e.doc.items[0], stroke, "studio {studio}");
        // with Select the selection is restyled, as before
        e.update(Message::Tool(Tool::Select));
        e.update(Message::Width(Width::Thin));
        assert_ne!(e.doc.items[0].width, stroke.width, "studio {studio}");
    }
}

/// Review 3: an item picked up (Select press) is still in the base until its first move; a live
/// layer then would paint it a second time (a held highlighter looked darker).
#[test]
fn w113_3_a_held_item_has_no_live_layer_until_it_leaves_the_base() {
    let mut e = Annotator::new(page(), 1.0);
    e.update(Message::Tool(Tool::Highlighter));
    e.update(Message::Press(pt(50.0, 50.0)));
    e.update(Message::Move(pt(150.0, 52.0)));
    e.update(Message::Release);
    e.update(Message::Tool(Tool::Select));
    e.update(Message::Press(pt(100.0, 51.0)));
    assert!(e.live_layer().is_none(), "held, not yet moved");
    e.update(Message::Move(pt(110.0, 61.0)));
    assert!(
        e.live_layer().is_some(),
        "moving: the base no longer has it"
    );
}

/// Review 4: a cancelled press in Snip's bare live area (no items, no base) leaves no layer.
#[test]
fn w113_4_a_cancelled_press_leaves_no_live_layer() {
    let mut e = Annotator::new(page(), 1.0);
    e.movable = true;
    e.update(Message::Tool(Tool::Pen));
    e.update(Message::Press(pt(50.0, 50.0)));
    e.update(Message::Move(pt(120.0, 90.0)));
    assert!(e.live_layer().is_some());
    e.update(Message::Cancel);
    assert!(e.live_layer().is_none());
}
