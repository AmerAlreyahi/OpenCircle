//! W113: fixes from the review of W111.
use iced::Point;

use super::{ScreenMap, canvas};
use crate::features::snip::annotate::{Annotator, Image, Message, Tool};
use crate::shared::kit::testing::shot;

/// Review 6: an opaque live layer (a second spotlight reads the picture) is drawn with the base's
/// filter. At zoom 2 with the Studio's crisp (nearest) base, the inside of the second hole shows the
/// picture's stripes pixel for pixel; a linear layer over it blurs every stripe edge.
#[test]
fn w113_6_an_opaque_live_layer_uses_the_bases_filter() {
    let (w, h) = (40u32, 40u32);
    let rgba = (0..w * h)
        .flat_map(|i| match (i % w) % 2 {
            0 => [20, 20, 20, 255],
            _ => [235, 235, 235, 255],
        })
        .collect();
    let mut e = Annotator::new(
        Image {
            width: w,
            height: h,
            rgba,
        },
        2.0,
    );
    e.studio = true;
    e.crisp = true;
    e.update(Message::Tool(Tool::Spotlight));
    // (a first spotlight, committed: the next one is drawn through a layer)
    e.update(Message::Press(Point::new(2.0, 2.0)));
    e.update(Message::Move(Point::new(6.0, 6.0)));
    e.update(Message::Release);
    e.crisp = true; // (the commit repainted the base, which clears it; the Studio's stage sets it again)
    e.update(Message::Press(Point::new(10.0, 10.0)));
    e.update(Message::Move(Point::new(30.0, 30.0)));
    assert!(e.spotlight_hole().is_none());
    assert!(e.live_layer().is_some());
    let size = e.size();
    let live = shot::render(
        canvas(&e, 0.0, ScreenMap::ONE),
        size.width as u32,
        size.height as u32,
    );
    let wrong = (28..52usize)
        .flat_map(|y| (28..52usize).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let want = if (x / 2) % 2 == 0 { 20 } else { 235 };
            live[(y * size.width as usize + x) * 4..][..3]
                .iter()
                .any(|v| v.abs_diff(want) > 2)
        })
        .count();
    assert_eq!(
        wrong, 0,
        "pixels of the second hole that are not the picture's"
    );
}

fn spotlight_scene(with_rect: bool) -> Annotator {
    let mut e = Annotator::new(
        Image {
            width: 800,
            height: 600,
            rgba: [200, 210, 220, 255].repeat(800 * 600),
        },
        1.0,
    );
    e.studio = true;
    if with_rect {
        e.update(Message::Tool(Tool::Rect));
        e.update(Message::Press(Point::new(600.0, 60.0)));
        e.update(Message::Move(Point::new(700.0, 140.0)));
        e.update(Message::Release);
    }
    e.update(Message::Tool(Tool::Spotlight));
    e.update(Message::Press(Point::new(200.0, 150.0)));
    e.update(Message::Move(Point::new(500.0, 400.0)));
    e
}

fn pixel(img: &[u8], x: usize, y: usize) -> Vec<u8> {
    img[(y * 800 + x) * 4..][..3].to_vec()
}

/// The only item, a spotlight, dragged: the canvas dims round its hole and the picture looks as it
/// does once committed (away from the hole's rounded corners).
#[test]
fn w113_a_lone_spotlights_dim_is_drawn_round_its_hole() {
    let mut e = spotlight_scene(false);
    assert!(e.spotlight_hole().is_some());
    let live = shot::render(canvas(&e, 0.0, ScreenMap::ONE), 800, 600);
    e.update(Message::Release);
    assert!(e.spotlight_hole().is_none());
    let done = shot::render(canvas(&e, 0.0, ScreenMap::ONE), 800, 600);
    for (x, y) in [
        (100, 100),
        (700, 500),
        (350, 100),
        (350, 500),
        (100, 275),
        (650, 275),
        (350, 275),
    ] {
        let (a, b) = (pixel(&live, x, y), pixel(&done, x, y));
        assert!(
            a.iter().zip(&b).all(|(p, q)| p.abs_diff(*q) <= 2),
            "({x},{y}): {a:?} vs {b:?}"
        );
    }
    // the hole is the picture itself, the rest is dimmed
    assert_eq!(pixel(&live, 350, 275), [200, 210, 220]);
    assert!(pixel(&live, 100, 100)[0] < 120);
}

/// With another item in the picture the canvas must NOT dim over the base (the committed picture
/// draws items on top of the dim, undimmed): a rect's pixels are the same while the spotlight is
/// dragged as after it is released, and not darkened.
#[test]
fn w113_a_spotlight_over_other_items_leaves_them_undimmed_while_dragged() {
    let mut e = spotlight_scene(true);
    assert!(
        e.spotlight_hole().is_none(),
        "another item: the layer paints"
    );
    let live = shot::render(canvas(&e, 0.0, ScreenMap::ONE), 800, 600);
    e.update(Message::Release);
    let done = shot::render(canvas(&e, 0.0, ScreenMap::ONE), 800, 600);
    let (a, b) = (pixel(&live, 600, 100), pixel(&done, 600, 100));
    assert!(
        a.iter().zip(&b).all(|(p, q)| p.abs_diff(*q) <= 2),
        "{a:?} vs {b:?}"
    );
    assert!(a[0] > 200, "the rect is not darkened: {a:?} (done {b:?})");
}

/// The hole is clipped to the picture: no dim band past its edge (Snip's margin).
#[test]
fn w113_the_spotlight_hole_is_clipped_to_the_picture() {
    let mut e = spotlight_scene(false);
    e.update(Message::Move(Point::new(900.0, 700.0)));
    let hole = e.spotlight_hole().expect("a hole");
    assert!(
        hole.x + hole.width <= 800.0 && hole.y + hole.height <= 600.0,
        "{hole:?}"
    );
}

/// A long stroke drawn as tiles shows no seam between them: the window shows what it shows when the
/// same stroke is one layer (at a zoom where tile edges fall between display pixels too).
#[test]
fn w113_tiles_show_no_seam() {
    for (tool, zoom) in [
        (Tool::Pen, 0.5),
        (Tool::Pen, 0.37),
        (Tool::Highlighter, 0.37),
    ] {
        let mut shown = Vec::new();
        for one_layer in [false, true] {
            let rgba = [200, 210, 220, 255].repeat(1600 * 900);
            let mut e = Annotator::new(
                Image {
                    width: 1600,
                    height: 900,
                    rgba,
                },
                zoom,
            );
            (e.studio, e.crisp, e.one_layer) = (true, true, one_layer);
            e.update(Message::Tool(tool));
            e.update(Message::Press(Point::new(100.0, 400.0)));
            for i in 1..=40 {
                let t = i as f32 / 40.0;
                e.update(Message::Move(Point::new(
                    100.0 + 1400.0 * t,
                    400.0 + 200.0 * (t * 6.0).sin(),
                )));
            }
            let size = e.size();
            assert_eq!(e.live_tiles().is_some_and(|t| t.len() > 2), !one_layer);
            let (w, h) = (size.width.ceil() as u32, size.height.ceil() as u32);
            shown.push(shot::render(canvas(&e, 0.0, ScreenMap::ONE), w, h));
        }
        let differ = (shown[0].as_chunks::<4>().0.iter())
            .zip(shown[1].as_chunks::<4>().0.iter())
            .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 48))
            .count();
        let ink = |i: usize| {
            shown[i]
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] != 200)
                .count()
        };
        let (tiles, one) = (ink(0), ink(1));
        match zoom {
            // (tile edges on whole display pixels: exactly one layer's picture)
            0.5 => assert_eq!(differ, 0, "{tool:?} at {zoom}"),
            // (a nearest sample of a thin line lands on another pixel where a layer starts at a
            // fraction; a seam would take away the ink along a tile's edge)
            _ => {
                assert!(
                    differ * 6 < one,
                    "{tool:?} at {zoom}: {differ} of {one} differ"
                );
                assert!(
                    tiles * 100 > one * 94,
                    "{tool:?} at {zoom}: {tiles} inked, one layer {one}"
                );
            }
        }
    }
}
