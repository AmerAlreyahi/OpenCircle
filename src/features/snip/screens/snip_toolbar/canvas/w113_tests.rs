//! W113: fixes from the review of W111.
use iced::Point;

use super::{ScreenMap, canvas, margin};
use crate::features::snip::annotate::{Annotator, Image, Message, Tool};
use crate::shared::kit::testing::shot;

/// Review 6: an opaque live layer (Spotlight reads the picture) is drawn with the base's filter. At
/// zoom 2 with the Studio's crisp (nearest) base, the inside of the spotlight's hole must show the
/// same pixels as the base alone; a linear layer over it blurs every stripe edge.
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
    let size = e.size();
    let (pw, ph) = (
        size.width as u32 + 2 * margin(&e) as u32,
        size.height as u32 + 2 * margin(&e) as u32,
    );
    let alone = shot::render(canvas(&e, 0.0, ScreenMap::ONE), pw, ph);
    e.update(Message::Tool(Tool::Spotlight));
    e.update(Message::Press(Point::new(10.0, 10.0)));
    e.update(Message::Move(Point::new(30.0, 30.0)));
    assert!(e.live_layer().is_some());
    let live = shot::render(canvas(&e, 0.0, ScreenMap::ONE), pw, ph);
    let m = margin(&e) as u32;
    let differing = (28..52)
        .flat_map(|y| (28..52).map(move |x| ((y + m) * pw + x + m) as usize * 4))
        .filter(|&at| alone[at..at + 3] != live[at..at + 3])
        .count();
    assert_eq!(differing, 0, "pixels of the hole that differ from the base");
}

/// A first spotlight's dim, while it is dragged, is drawn by the canvas round its hole and looks as it
/// does once committed (away from the hole's rounded corners); with a spotlight in the picture already,
/// the live layer paints instead.
#[test]
fn w113_a_first_spotlights_dim_is_drawn_round_its_hole() {
    let mut e = Annotator::new(
        Image {
            width: 800,
            height: 600,
            rgba: [200, 210, 220, 255].repeat(800 * 600),
        },
        1.0,
    );
    e.studio = true;
    e.update(Message::Tool(Tool::Spotlight));
    e.update(Message::Press(Point::new(200.0, 150.0)));
    e.update(Message::Move(Point::new(500.0, 400.0)));
    assert!(e.spotlight_hole().is_some());
    let live = shot::render(canvas(&e, 0.0, ScreenMap::ONE), 800, 600);
    e.update(Message::Release);
    assert!(e.spotlight_hole().is_none());
    let done = shot::render(canvas(&e, 0.0, ScreenMap::ONE), 800, 600);
    let at = |img: &[u8], x: usize, y: usize| img[(y * 800 + x) * 4..][..3].to_vec();
    for (x, y) in [
        (100, 100),
        (700, 500),
        (350, 100),
        (350, 500),
        (100, 275),
        (650, 275),
        (350, 275),
    ] {
        let (a, b) = (at(&live, x, y), at(&done, x, y));
        assert!(
            a.iter().zip(&b).all(|(p, q)| p.abs_diff(*q) <= 2),
            "({x},{y}): {a:?} vs {b:?}"
        );
    }
    // the hole is the picture itself, the rest is dimmed
    assert_eq!(at(&live, 350, 275), [200, 210, 220]);
    assert!(at(&live, 100, 100)[0] < 120);
    // a second spotlight: the committed one dims the base, the live layer paints the union
    e.update(Message::Press(Point::new(50.0, 50.0)));
    e.update(Message::Move(Point::new(120.0, 120.0)));
    assert!(e.spotlight_hole().is_none());
    assert!(e.live_layer().is_some());
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
