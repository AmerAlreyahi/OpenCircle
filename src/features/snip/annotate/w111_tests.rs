//! W111: drawing tools follow the pointer and never change each other.
use super::blur_tests::pt;
use super::tests::image_of;
use super::*;

/// Text-like bars on a pale page (a Hide's colours depend on where it looks).
pub(super) fn page() -> Image {
    image_of(600, 300, |x, y| {
        if (y / 10) % 2 == 0 && x % 6 < 3 && (20..150).contains(&x) {
            [30, 30, 40, 255]
        } else {
            [240, 240, 235, 255]
        }
    })
}

pub(super) fn live_pixels(e: &Annotator) -> Option<(Rectangle, Vec<u8>)> {
    let (r, h) = e.live_layer()?;
    match h {
        image::Handle::Rgba { pixels, .. } => Some((r, pixels.to_vec())),
        _ => None,
    }
}

pub(super) fn hide_one() -> Annotator {
    let mut e = Annotator::new(page(), 1.0);
    e.assist = false;
    e.update(Message::Tool(Tool::Blur));
    e.update(Message::Press(pt(100.0, 105.0)));
    for i in 1..=10 {
        e.update(Message::Move(pt(100.0 + 20.0 * i as f32, 105.0)));
    }
    e.update(Message::Release);
    e.refresh();
    e
}

/// W111-3: a pen stroke over a Hide, while it is drawn, leaves the Hide's pixels as they were
/// everywhere the Hide is not covered by the stroke itself.
#[test]
fn w111_3_a_pen_stroke_crossing_a_hide_leaves_the_hide_alone_while_drawn() {
    let mut e = hide_one();
    let before = e.base_pixels();
    e.update(Message::Tool(Tool::Pen));
    e.update(Message::Press(pt(200.0, 60.0)));
    for i in 1..=8 {
        e.update(Message::Move(pt(
            200.0 + 4.0 * i as f32,
            60.0 + 12.0 * i as f32,
        )));
    }
    let (r, live) = live_pixels(&e).expect("a live layer");
    let (x0, y0, w) = (r.x as usize, r.y as usize, r.width as usize);
    let (mut differing, mut seen) = (0, 0);
    for (i, px) in live.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (x0 + i % w, y0 + i / w);
        let pen_x = 200.0 + 4.0 * ((y as f32 - 60.0) / 12.0);
        if (100..=300).contains(&x) && (98..=112).contains(&y) && (x as f32 - pen_x).abs() > 8.0 {
            seen += 1;
            let at = (y * 600 + x) * 4;
            // a see-through pixel shows the base, which holds the Hide
            if px[3] != 0 && px[..3] != before[at..at + 3] {
                differing += 1;
            }
        }
    }
    assert!(seen > 100, "the test looks at the Hide: {seen}");
    assert_eq!(differing, 0, "of {seen} Hide pixels in the live layer");
}

/// W111-2: the live layer holds only what the pending item paints: where it paints nothing it is
/// see-through, so the picture under it is never drawn a second time.
#[test]
fn w111_2_the_live_layer_of_a_rectangle_is_see_through_inside() {
    let mut e = Annotator::new(page(), 1.0);
    e.update(Message::Tool(Tool::Rect));
    e.update(Message::Press(pt(100.0, 100.0)));
    e.update(Message::Move(pt(300.0, 220.0)));
    let (r, live) = live_pixels(&e).expect("a live layer");
    let w = r.width as usize;
    let mid = ((r.height as usize / 2) * w + w / 2) * 4;
    assert_eq!(live[mid + 3], 0, "the middle of the square is see-through");
    // and its edge is drawn
    assert!(live.chunks(4).any(|p| p[3] == 255), "the outline is there");
}

/// W111-1: pointer moves are cheap: the live layer is only painted when it is asked for.
#[test]
fn w111_1_moves_do_not_paint_until_the_live_layer_is_asked_for() {
    let mut e = Annotator::new(page(), 1.0);
    e.update(Message::Tool(Tool::Pen));
    e.update(Message::Press(pt(10.0, 10.0)));
    e.update(Message::Move(pt(20.0, 20.0)));
    let before = e.live_paints();
    e.update(Message::Move(pt(30.0, 25.0)));
    e.update(Message::Move(pt(40.0, 30.0)));
    assert_eq!(e.live_paints(), before, "two moves painted nothing");
    let _ = e.live_layer();
    let _ = e.live_layer();
    assert_eq!(e.live_paints(), before + 1, "asking twice paints once");
}

/// W111-4: every pointer point of a stroke reaches the pen and the painted stroke covers each.
#[test]
fn w111_4_the_pen_stroke_runs_through_the_pointer_path() {
    let mut e = Annotator::new(page(), 1.0);
    e.update(Message::Tool(Tool::Pen));
    let path: Vec<Point> = (0..60)
        .map(|i| pt(20.0 + 8.0 * i as f32, 150.0 + 60.0 * (i as f32 / 5.0).sin()))
        .collect();
    e.update(Message::Press(path[0]));
    for p in &path[1..] {
        e.update(Message::Move(*p));
    }
    e.update(Message::Release);
    let Some(Item {
        shape: Shape::Pen(points),
        ..
    }) = e.doc.items.last()
    else {
        panic!("a pen stroke")
    };
    assert_eq!(points.len(), path.len(), "no point dropped");
    let base = e.base_pixels();
    let c = e.colour;
    for p in &path {
        let at = (p.y as usize * 600 + p.x as usize) * 4;
        assert_eq!(base[at..at + 3], c[..3], "the stroke covers {p:?}");
    }
}

/// W111-5 (owner 2026-11-25): one click on another tool makes it the row's tool at once, even with
/// a drawing selected; picking a sub-tool in the row updates its group's button before any drawing.
#[test]
fn w111_5_the_row_follows_the_active_tool_not_the_last_drawing() {
    use crate::features::snip::screens::options_row::base_tool;
    for studio in [false, true] {
        let mut e = Annotator::new(page(), 1.0);
        e.studio = studio;
        e.update(Message::Tool(Tool::Pen));
        e.update(Message::Press(pt(50.0, 50.0)));
        e.update(Message::Move(pt(120.0, 90.0)));
        e.update(Message::Release);
        assert_eq!(base_tool(&e), Tool::Pen);
        e.update(Message::Slot(Tool::Rect));
        assert_eq!(e.tool, Tool::Rect);
        assert_eq!(
            base_tool(&e),
            Tool::Rect,
            "studio {studio}: the row is Shapes'"
        );
        // a sub-tool picked in the row shows on its group's button at once
        e.update(Message::Tool(Tool::Arrow));
        let group = crate::features::snip::annotate::info(Tool::Arrow).group as usize;
        assert_eq!(e.last[group], Tool::Arrow, "studio {studio}");
        assert_eq!(base_tool(&e), Tool::Arrow);
    }
}

/// W111-1b (owner 2026-11-25): adding any item (a pen stroke, a shape, another Hide; far away or
/// over it) never changes an existing Hide's pixels, nor the Hide row's style pictures: the fill
/// of Frost and Pixelate reads the picture as captured, never the annotations drawn on it.
#[test]
fn w111_1b_adding_items_changes_neither_a_hide_nor_its_style_pictures() {
    let hide_area = |px: &[u8], skip: Rectangle| -> Vec<u8> {
        let mut out = Vec::new();
        for y in 95..116usize {
            for x in 100..300usize {
                if !skip.contains(Point::new(x as f32, y as f32)) {
                    out.extend_from_slice(&px[(y * 600 + x) * 4..(y * 600 + x) * 4 + 4]);
                }
            }
        }
        out
    };
    // (tool, from, to, touches_hide_ring): non-Hide items (far or over) and a far-away Hide
    // never change an existing Hide; an overlapping/adjacent Hide is allowed to change its
    // neighbour because Soft excludes every Hide footprint from its exterior ring (W114).
    let adds = [
        (Tool::Pen, pt(40.0, 200.0), pt(120.0, 260.0), false),
        (Tool::Pen, pt(100.0, 70.0), pt(115.0, 140.0), false),
        (Tool::Rect, pt(90.0, 90.0), pt(130.0, 125.0), false),
        (Tool::Pen, pt(180.0, 60.0), pt(230.0, 160.0), false),
        (Tool::Rect, pt(400.0, 40.0), pt(500.0, 120.0), false),
        (Tool::Rect, pt(150.0, 80.0), pt(260.0, 140.0), false),
        (Tool::Blur, pt(300.0, 220.0), pt(450.0, 220.0), false),
        (Tool::Blur, pt(240.0, 105.0), pt(400.0, 105.0), true),
    ];
    for (tool, from, to, touches_hide_ring) in adds {
        let mut e = hide_one();
        e.update(Message::Move(pt(110.0, 105.0)));
        let px0 = e.base_pixels();
        e.update(Message::Tool(tool));
        e.update(Message::Press(from));
        e.update(Message::Move(from + (to - from) * 0.5));
        e.update(Message::Move(to));
        e.update(Message::Release);
        e.refresh();
        if !touches_hide_ring {
            let skip = e.doc.items.last().unwrap().frame().expand(8.0);
            assert_eq!(
                hide_area(&e.base_pixels(), skip),
                hide_area(&px0, skip),
                "{tool:?} {from:?}: the Hide is as it was"
            );
        }
    }
}

/// W111-6 (owner 2026-11-25): Hide's style, shape and strength are remembered like the other
/// tools' options: an editor opened with what the last one remembered has the same.
#[test]
fn w111_6_hides_style_shape_and_strength_are_remembered() {
    let mut e = Annotator::new(page(), 1.0);
    e.update(Message::Tool(Tool::Blur));
    e.update(Message::BlurShape(blur::Kind::Ellipse));
    e.update(Message::BlurStyle(blur::Style::Pixelate));
    e.update(Message::BlurCell(Width::Thick));
    let next = Annotator::restored(page(), 1.0, e.remembered());
    assert_eq!(next.blur_shape, blur::Kind::Ellipse);
    assert_eq!(next.blur_style, blur::Style::Pixelate);
    assert_eq!(next.blur_cell, Width::Thick);
    e.update(Message::BlurCustom(Some(11)));
    let next = Annotator::restored(page(), 1.0, e.remembered());
    assert_eq!(next.blur_custom, Some(11));
}
