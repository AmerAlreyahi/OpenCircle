//! F-03: Studio fixes (owner, 2026-11-27).
//!
//! AC1 the bottom-end pill holds Save as a plain item (no raised container round it).
//! AC2 the Steps slider is the design's floating card: margins from the stage's top and edge,
//!     stopping above the pills' band so both pills stay visible, open or pinned.
//! AC3 opening, pinning and closing the card never changes the picture's view or the stage.
//! AC4 the canvas card is rounded on all four corners at every zoom: a picture that fills the
//!     card does not cover them.
use super::d04::{bounds, find};
use super::*;
use crate::shared::kit::motion;
use crate::shared::kit::testing::shot;

fn editor_of(rgba: [u8; 4], w: u32, h: u32) -> Studio {
    let mut s = Studio::new(StudioConfig::default());
    s.open(Picture::Crop(Crop {
        width: w,
        height: h,
        rgba: rgba.repeat((w * h) as usize),
    }));
    s.settle_open();
    s
}

fn pixel(px: &[u8], x: f32, y: f32) -> [u8; 3] {
    let i = (y as usize * SIZE.width as usize + x as usize) * 4;
    [px[i], px[i + 1], px[i + 2]]
}

#[test]
fn f03_ac1_save_is_a_plain_item_of_the_pill() {
    crate::shared::kit::text::load();
    let s = editor_of([255; 4], 300, 200);
    let px = shot::render(view(&s, &[], false), SIZE.width as u32, SIZE.height as u32);
    let all = bounds(&s);
    let pill = find(&all, super::pills::ACTIONS_PILL);
    let words = shot::texts_at(view(&s, &[], false), SIZE);
    let save = words.iter().find(|(_, t)| t == "Save").expect("Save").0;
    // the tray's own colour, inside its start padding; the same lies between Save's button
    // (its label, 14 px each side) and Copy: no raised box reaches past the button
    let mid = pill.y + pill.height / 2.0;
    let tray = pixel(&px, pill.x + 4.0, mid);
    let gap = pixel(&px, save.x + save.width + 14.0 + 3.0, mid);
    assert_eq!(gap, tray, "Save sits on the tray, not in a raised box");
}

#[test]
fn f03_ac2_the_steps_card_floats_with_margins_above_the_pills() {
    motion::set_reduced(true);
    crate::shared::kit::text::load();
    for pinned in [false, true] {
        let mut s = editor_of([255; 4], 300, 200);
        let _ = s.update(Msg::Library);
        if pinned {
            let _ = s.update(Msg::PinSteps);
        }
        let all = bounds(&s);
        let card = find(&all, super::sidebar::CARD);
        let stage = find(&all, super::CANVAS);
        let (size, actions) = (
            find(&all, super::pills::SIZE_PILL),
            find(&all, super::pills::ACTIONS_PILL),
        );
        assert_eq!(card.width, super::SIDEBAR);
        assert!(
            (card.y - stage.y - super::SLIDER_TOP).abs() < 2.0,
            "{card:?} {stage:?}"
        );
        let edge = SIZE.width - 1.0 - (card.x + card.width);
        assert!((edge - super::SLIDER_EDGE).abs() < 2.0, "edge {edge}");
        let room = SIZE.height - 1.0 - (card.y + card.height);
        assert!((room - super::SLIDER_BOTTOM).abs() < 2.0, "bottom {room}");
        let touches = |a: Rectangle, b: Rectangle| a.intersection(&b).is_some();
        assert!(
            !touches(card, actions) && !touches(card, size),
            "pinned {pinned}"
        );
        // the pills stay where they were
        let closed = bounds(&editor_of([255; 4], 300, 200));
        assert_eq!(find(&closed, super::pills::ACTIONS_PILL), actions);
    }
    motion::set_reduced(false);
}

#[test]
fn f03_ac3_the_card_never_moves_the_picture() {
    motion::set_reduced(true);
    let mut s = editor_of([255; 4], 3000, 2000);
    let _ = s.update(Msg::ZoomIn);
    let before = (s.stage(), s.editor.as_ref().unwrap().zoom, s.offset);
    for m in [Msg::Library, Msg::PinSteps, Msg::PinSteps, Msg::Library] {
        let _ = s.update(m);
        assert_eq!(
            (s.stage(), s.editor.as_ref().unwrap().zoom, s.offset),
            before
        );
    }
    motion::set_reduced(false);
}

#[test]
fn f03_ac4_a_zoomed_picture_does_not_cover_the_cards_rounded_corners() {
    crate::shared::kit::text::load();
    let mut s = editor_of([220, 30, 30, 255], 3000, 2000);
    let _ = s.zoom_to(3.0, None);
    let _ = s.take_scroll();
    let all = bounds(&s);
    let card = find(&all, super::CANVAS);
    let px = shot::render(view(&s, &[], false), SIZE.width as u32, SIZE.height as u32);
    let bg = crate::shared::kit::themes::c().bg.into_rgba8();
    let bg = [bg[0], bg[1], bg[2]];
    // the picture is on screen
    assert_eq!(
        pixel(&px, card.x + card.width / 2.0, card.y + card.height / 2.0),
        [220, 30, 30]
    );
    let (r, b) = (card.x + card.width - 2.0, card.y + card.height - 2.0);
    for (x, y) in [
        (card.x + 1.0, card.y + 1.0),
        (r, card.y + 1.0),
        (card.x + 1.0, b),
        (r, b),
    ] {
        assert_eq!(pixel(&px, x, y), bg, "corner at {x},{y}");
    }
}
