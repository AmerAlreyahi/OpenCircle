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

/// Review 5: through an OPAQUE live layer (a magnifier's box cuts a committed Hide's footprint) the
/// Hide's pixels in the box's four corners (outside the lens) equal the committed picture's. Fails
/// with `blur.rs` reading its two colours only from the part the layer holds.
#[test]
fn w113_5_an_opaque_layer_that_cuts_a_hide_leaves_it_as_committed() {
    use super::w111_tests::{hide_one, live_pixels};
    let mut e = hide_one();
    let committed = e.base_pixels();
    e.update(Message::Tool(Tool::Magnifier));
    e.update(Message::Press(pt(240.0, 125.0)));
    e.update(Message::Move(pt(240.0, 155.0)));
    let (r, live) = live_pixels(&e).expect("a live layer");
    let (w, x0, y0) = (r.width as usize, r.x as usize, r.y as usize);
    let (mut seen, mut differing) = (0, 0);
    // (the top corners, 6 wide and 16 deep: outside the lens, over the Hide's first rows)
    for cx in [0, w - 6] {
        for (dx, dy) in (0..16).flat_map(|y| (0..6).map(move |x| (x, y))) {
            let cy = 0;
            let (lx, ly) = (cx + dx, cy + dy);
            let at = ((y0 + ly) * 600 + x0 + lx) * 4;
            // (only the rows the Hide covers)
            if !(100..=110).contains(&(y0 + ly)) {
                continue;
            }
            seen += 1;
            let i = (ly * w + lx) * 4;
            differing += usize::from(live[i..i + 3] != committed[at..at + 3]);
        }
    }
    assert!(seen > 0, "no Hide in the corners of {r:?}");
    assert_eq!(differing, 0, "of {seen} Hide pixels in the corners");
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
