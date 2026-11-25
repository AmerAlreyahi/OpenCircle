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
