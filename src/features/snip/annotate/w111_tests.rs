//! W111: drawing tools follow the pointer and never change each other.
use super::blur_tests::pt;
use super::tests::image_of;
use super::*;

/// Text-like bars on a pale page (a Hide's colours depend on where it looks).
fn page() -> Image {
    image_of(600, 300, |x, y| {
        if (y / 10) % 2 == 0 && x % 6 < 3 && (20..150).contains(&x) {
            [30, 30, 40, 255]
        } else {
            [240, 240, 235, 255]
        }
    })
}

fn live_pixels(e: &Annotator) -> Option<(Rectangle, Vec<u8>)> {
    let (r, h) = e.live_layer()?;
    match h {
        image::Handle::Rgba { pixels, .. } => Some((r, pixels.to_vec())),
        _ => None,
    }
}

fn hide_one() -> Annotator {
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
