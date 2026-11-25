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

fn big() -> Annotator {
    let mut e = Annotator::new(
        super::tests::image_of(1400, 800, |x, y| {
            [(x % 251) as u8, (y % 241) as u8, 200, 255]
        }),
        1.0,
    );
    e.studio = true;
    e.crisp = true;
    e
}

/// The tiles laid over a transparent picture-sized buffer, as straight RGBA.
fn composite(tiles: &[(Rectangle, image::Handle)], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    for (r, handle) in tiles {
        let image::Handle::Rgba { pixels, .. } = handle else {
            panic!()
        };
        let (x0, y0, tw) = (r.x as usize, r.y as usize, r.width as usize);
        for (i, px) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let at = ((y0 + i / tw) * w + x0 + i % tw) * 4;
            out[at..at + 4].copy_from_slice(px);
        }
    }
    out
}

/// A long stroke's tiles hold what one layer holds, after every move (pen and highlighter), and a
/// move repaints only the tiles it touches: the others keep their very image.
#[test]
fn w113_a_long_strokes_tiles_equal_one_layer_and_only_the_touched_ones_repaint() {
    for tool in [Tool::Pen, Tool::Highlighter] {
        let mut e = big();
        e.update(Message::Tool(tool));
        let at = |i: usize| {
            let t = i as f32 / 60.0;
            pt(80.0 + 1240.0 * t, 400.0 + 250.0 * (t * 9.0).sin())
        };
        e.update(Message::Press(at(0)));
        let mut before: Vec<(Rectangle, image::Handle)> = Vec::new();
        for i in 1..=60 {
            e.update(Message::Move(at(i)));
            let tiles = e.live_tiles().expect("tiled");
            let ids = |t: &[(Rectangle, image::Handle)]| -> Vec<_> {
                t.iter()
                    .map(|(r, h)| (r.x as i32, r.y as i32, h.id()))
                    .collect()
            };
            if i > 1 {
                let old = ids(&before);
                let kept = ids(&tiles).iter().filter(|t| old.contains(t)).count();
                assert!(
                    tiles.len() - kept <= 4,
                    "{tool:?} move {i}: {} of {} tiles repainted",
                    tiles.len() - kept,
                    tiles.len()
                );
            }
            if i % 10 == 0 {
                let (r, single) = live_pixels(&e).expect("one layer");
                let (w, h) = (1400, 800);
                let whole = composite(&tiles, w, h);
                let mut differing = 0;
                for (k, px) in single.as_chunks::<4>().0.iter().enumerate() {
                    let (x, y) = (
                        r.x as usize + k % r.width as usize,
                        r.y as usize + k / r.width as usize,
                    );
                    let t = &whole[(y * w + x) * 4..][..4];
                    // (a clipped path is rasterised a hair differently at its anti-aliased edge)
                    let premul = |p: &[u8]| {
                        [0, 1, 2].map(|k| (u32::from(p[k]) * u32::from(p[3]) / 255) as u8)
                    };
                    let alpha = px[3].abs_diff(t[3]);
                    let off = premul(px)
                        .iter()
                        .zip(premul(t))
                        .map(|(a, b)| a.abs_diff(b))
                        .chain([alpha])
                        .max();
                    assert!(
                        off < Some(40),
                        "{tool:?} move {i}: ({x},{y}) {px:?} vs {t:?}"
                    );
                    differing += usize::from(off > Some(1));
                }
                assert!(
                    differing < 400,
                    "{tool:?} move {i}: {differing} pixels differ"
                );
            }
            before = tiles;
        }
    }
}
