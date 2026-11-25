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
