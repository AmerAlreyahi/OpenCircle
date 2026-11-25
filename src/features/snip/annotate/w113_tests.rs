//! W113: fixes from the review of W111.
use super::blur_tests::pt;
use super::w111_tests::{hide_one, live_pixels, page};
use super::*;

/// Review 2: an item of ANY kind is restyled by its own tool's row, and by no other drawing tool's
/// (after a pen stroke, then Shapes, Thick or Fill must not change the stroke), in Snip and Studio.
#[test]
fn w113_2_a_drawing_tool_restyles_only_a_selection_of_its_own_kind() {
    let (a, b) = (pt(40.0, 40.0), pt(100.0, 80.0));
    let line = |head| Shape::Arrow {
        a,
        b,
        bend: pt(70.0, 60.0),
        head,
        text: String::new(),
    };
    let kinds = [
        (Tool::Rect, Shape::Rect { a, b }),
        (Tool::Ellipse, Shape::Ellipse { a, b }),
        (Tool::Line, line(false)),
        (Tool::Arrow, line(true)),
        (Tool::Pen, Shape::Pen(vec![a, b])),
        (Tool::Highlighter, Shape::Highlighter(vec![a, b])),
        (
            Tool::Callout,
            Shape::Callout {
                tip: a,
                at: b,
                text: "hi".into(),
                size: 20.0,
            },
        ),
        (
            Tool::Label,
            Shape::Label {
                at: a,
                text: "hi".into(),
                size: 20.0,
            },
        ),
        (
            Tool::Pointer,
            Shape::Pointer {
                at: a,
                radius: 12.0,
            },
        ),
        (
            Tool::Magnifier,
            Shape::Magnifier {
                at: a,
                radius: 30.0,
            },
        ),
        (
            Tool::Step,
            Shape::Step {
                at: a,
                radius: 12.0,
                note: String::new(),
            },
        ),
    ];
    for studio in [false, true] {
        for (tool, shape) in &kinds {
            assert_eq!(tool_of(shape), *tool, "the kind's tool");
            // (a row of another group: the Shapes row for the drawing, the Draw row for the rest)
            let other = match info(*tool).group == Group::Draw {
                true => Tool::Rect,
                false => Tool::Pen,
            };
            for (row, restyled) in [(*tool, true), (other, false)] {
                let mut e = Annotator::new(page(), 1.0);
                e.studio = studio;
                let item = Item {
                    shape: shape.clone(),
                    colour: [1, 2, 3, 255],
                    width: 5.0,
                    outline: false,
                    fill: false,
                    hidden: false,
                };
                e.doc.exec(Edit::Insert(0, item.clone()));
                e.doc.select_one(Some(0));
                e.update(Message::Tool(row));
                e.update(Message::Colour([200, 100, 50, 255]));
                assert_eq!(
                    e.doc.items[0] != item,
                    restyled,
                    "{tool:?} with the {row:?} row (studio {studio})"
                );
            }
        }
    }
}

/// Review 5: through an OPAQUE live layer (a magnifier's box cuts a committed Hide's footprint) the
/// Hide's pixels in the box's top corners (outside the lens) equal the committed picture's. Fails
/// with `blur.rs` reading its two colours only from the part the layer holds.
#[test]
fn w113_5_an_opaque_layer_that_cuts_a_hide_leaves_it_as_committed() {
    let mut e = hide_one();
    let committed = e.base_pixels();
    e.update(Message::Tool(Tool::Magnifier));
    e.update(Message::Press(pt(240.0, 125.0)));
    e.update(Message::Move(pt(240.0, 155.0)));
    let (r, live) = live_pixels(&e).expect("a live layer");
    let (w, x0, y0) = (r.width as usize, r.x as usize, r.y as usize);
    let (mut seen, mut differing) = (0, 0);
    // (6 wide and 16 deep: outside the lens, over the Hide's first rows)
    for cx in [0, w - 6] {
        for (dx, ly) in (0..16).flat_map(|y| (0..6).map(move |x| (x, y))) {
            let lx = cx + dx;
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

/// Review 3b (Alt+drag copy, Studio): the copy sits on its original, which the base paints; it has
/// no live layer until it has moved.
#[test]
fn w113_3b_an_alt_copy_has_no_live_layer_before_it_moves() {
    let mut e = Annotator::new(page(), 1.0);
    e.studio = true;
    e.update(Message::Tool(Tool::Highlighter));
    e.update(Message::Press(pt(50.0, 50.0)));
    e.update(Message::Move(pt(150.0, 52.0)));
    e.update(Message::Release);
    e.update(Message::Tool(Tool::Select));
    e.update(Message::PressCopy(pt(100.0, 51.0)));
    assert!(e.live_layer().is_none(), "the copy, not yet moved");
    e.update(Message::Move(pt(110.0, 71.0)));
    assert!(e.live_layer().is_some(), "moving");
    e.update(Message::Release);
    assert_eq!(e.doc.items.len(), 2);
}

/// A hand-edited custom strength (0, 255) is clamped to the slider's range when it is read.
#[test]
fn w113_a_remembered_custom_strength_is_clamped() {
    let mut saved = EditorConfig::default();
    for (given, kept) in [(0, 2), (255, 32), (11, 11)] {
        saved.blur_custom = Some(given);
        let e = Annotator::restored(page(), 1.0, saved.clone());
        assert_eq!(e.blur_custom, Some(kept));
    }
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
