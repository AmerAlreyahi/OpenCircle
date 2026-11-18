use iced::{Point, Rectangle, Size, Vector};

use super::*;
use crate::features::snip::annotate::{Message, Tool};

fn crop(width: u32, height: u32) -> Crop {
    Crop {
        width,
        height,
        rgba: vec![255; (width * height * 4) as usize],
    }
}

fn studio_on(width: u32, height: u32) -> Studio {
    let mut s = Studio::new(StudioConfig::default());
    s.open(Picture::Crop(crop(width, height)));
    s
}

/// A pen stroke across the picture: one undoable drawing.
fn draw(s: &mut Studio) {
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Pen)));
    for m in [
        Message::Press(Point::new(10.0, 10.0)),
        Message::Move(Point::new(60.0, 40.0)),
        Message::Move(Point::new(120.0, 80.0)),
        Message::Release,
    ] {
        let _ = s.update(Msg::Editor(m));
    }
}

#[test]
fn a_picture_fits_the_stage_never_above_100_percent() {
    // big: scaled down into the stage with room around it
    let mut s = studio_on(3840, 2160);
    let _ = s.update(Msg::Resized(SIZE));
    let stage = s.stage();
    let shown = s.editor.as_ref().unwrap().size();
    assert!(
        shown.width <= stage.width - 2.0 * STAGE_PAD + 0.5,
        "{shown:?} {stage:?}"
    );
    assert!(shown.height <= stage.height - 2.0 * STAGE_PAD + 0.5);
    // small: 100 % (one picture px per screen px), never enlarged
    let mut s = studio_on(200, 100);
    let _ = s.update(Msg::Scale(1.5));
    assert_eq!(s.editor.as_ref().unwrap().zoom, 1.0 / 1.5);
    assert_eq!(s.percent(), Some(100));
}

#[test]
fn hiding_the_panels_gives_the_picture_their_room() {
    let mut s = studio_on(3840, 2160);
    let zoom = |s: &Studio| s.editor.as_ref().unwrap().zoom;
    let both = zoom(&s);
    assert!(matches!(s.update(Msg::Library), Outcome::Panels(p) if !p.library && p.options));
    let one = zoom(&s);
    // Ctrl+\: with one still shown, both go; again, both come back
    assert!(matches!(s.update(Msg::Both), Outcome::Panels(p) if !p.library && !p.options));
    let none = zoom(&s);
    assert!(both < one && one < none, "{both} {one} {none}");
    assert!(matches!(s.update(Msg::Both), Outcome::Panels(p) if p.library && p.options));
    assert_eq!(zoom(&s), both);
}

#[test]
fn a_user_zoom_stays_until_fit() {
    let mut s = studio_on(400, 300);
    let fitted = s.editor.as_ref().unwrap().zoom;
    let _ = s.update(Msg::ZoomIn);
    let zoomed = s.editor.as_ref().unwrap().zoom;
    assert!((zoomed - fitted * ZOOM_STEP).abs() < 1e-5);
    let _ = s.update(Msg::Resized(Size::new(2000.0, 1200.0)));
    assert_eq!(s.editor.as_ref().unwrap().zoom, zoomed, "a resize keeps it");
    let _ = s.update(Msg::Fit);
    assert_eq!(s.editor.as_ref().unwrap().zoom, fitted);
}

#[test]
fn closing_with_unsaved_drawings_asks_first() {
    let mut s = studio_on(300, 200);
    assert!(!s.dirty());
    assert!(
        matches!(s.close_requested(), Outcome::Close),
        "nothing to lose"
    );
    draw(&mut s);
    assert!(s.dirty());
    // the x asks; Cancel keeps it open; Don't save closes
    assert!(matches!(
        s.update(Msg::Caption(Caption::Close)),
        Outcome::None
    ));
    assert!(s.asking());
    let _ = s.update(Msg::Cancel);
    assert!(!s.asking() && s.dirty());
    let _ = s.close_requested();
    assert!(matches!(s.update(Msg::DontSave), Outcome::Close));
    // Save: the picture to save, then (once saved) the close
    let _ = s.close_requested();
    let Outcome::SaveThen(flat) = s.update(Msg::SaveFirst) else {
        panic!("Save asks the host to save first");
    };
    assert_eq!((flat.width, flat.height), (300, 200));
    s.saved();
    assert!(!s.dirty());
    assert!(matches!(s.proceed(), Outcome::Close));
    // undo back past the save: unsaved again
    let _ = s.update(Msg::Editor(Message::Undo));
    assert!(s.dirty());
}

/// Undo past the save, then a new edit: the history is as long as when saved
/// but holds other edits, so it is unsaved (it read as saved before).
#[test]
fn undo_past_the_save_then_a_new_edit_is_unsaved() {
    let mut s = studio_on(300, 200);
    draw(&mut s);
    s.saved();
    assert!(!s.dirty());
    let _ = s.update(Msg::Editor(Message::Undo));
    let _ = s.update(Msg::Editor(Message::Redo));
    assert!(!s.dirty(), "undo then redo: the saved picture again");
    let _ = s.update(Msg::Editor(Message::Undo));
    draw(&mut s);
    assert!(
        s.dirty(),
        "a different drawing in the same place of the history"
    );
}

#[test]
fn another_picture_over_unsaved_drawings_asks_then_opens() {
    let mut s = studio_on(300, 200);
    draw(&mut s);
    s.open(Picture::Crop(crop(50, 40)));
    assert!(s.asking());
    assert_eq!(s.editor.as_ref().unwrap().doc.image.width, 300, "not yet");
    assert!(matches!(s.update(Msg::DontSave), Outcome::None));
    let e = s.editor.as_ref().unwrap();
    assert_eq!((e.doc.image.width, e.doc.items.len()), (50, 0));
    assert!(!s.dirty() && !s.asking());
}

#[test]
fn the_snip_widgets_editor_moves_in_with_its_drawings() {
    // Snip's live area: the whole frozen monitor, the area its crop
    let mut snip = Annotator::new(
        Image {
            width: 800,
            height: 600,
            rgba: vec![200; 800 * 600 * 4],
        },
        1.0,
    );
    snip.doc.crop = Some(Rectangle::new(
        Point::new(100.0, 50.0),
        Size::new(300.0, 200.0),
    ));
    (snip.movable, snip.scroll, snip.steps) = (true, true, Some(2));
    let mut s = Studio::new(StudioConfig::default());
    s.open(Picture::Editor(Box::new(snip)));
    let e = s.editor.as_ref().unwrap();
    assert!(!e.movable && !e.scroll && e.steps.is_none());
    assert_eq!(
        e.doc.view().size(),
        Size::new(300.0, 200.0),
        "the area alone"
    );
    assert!(!s.dirty(), "no drawing yet: nothing to lose");
    // with a drawing it is unsaved work from the first frame
    let mut snip = Annotator::new(
        Image {
            width: 200,
            height: 200,
            rgba: vec![200; 200 * 200 * 4],
        },
        1.0,
    );
    let _ = snip.update(Message::Tool(Tool::Rect));
    for m in [
        Message::Press(Point::new(10.0, 10.0)),
        Message::Move(Point::new(80.0, 80.0)),
        Message::Release,
    ] {
        let _ = snip.update(m);
    }
    assert_eq!(snip.doc.items.len(), 1);
    s.open(Picture::Editor(Box::new(snip)));
    let _ = s.update(Msg::Fit);
    assert!(s.dirty());
    assert_eq!(
        s.editor.as_ref().unwrap().doc.depth(),
        1,
        "its undo history came along"
    );
}

#[test]
fn ctrl_v_asks_the_host_for_a_picture_first() {
    let mut s = studio_on(100, 100);
    let at = Some(Point::new(5.0, 5.0));
    assert!(matches!(
        s.update(Msg::Editor(Message::Paste(at))),
        Outcome::Paste(p) if p == at
    ));
    // no picture there: the editor's own paste (text, none on the test clipboard)
    assert!(matches!(s.paste_text(at), Outcome::None));
    // the empty Studio's Paste button / Ctrl+V
    let mut empty = Studio::new(StudioConfig::default());
    assert!(matches!(empty.update(Msg::Paste), Outcome::Paste(None)));
    assert!(matches!(empty.update(Msg::Open), Outcome::PickFile));
    let file = std::path::PathBuf::from("/tmp/x.png");
    assert!(matches!(empty.update(Msg::Dropped(file.clone())), Outcome::Read(p) if p == file));
}

#[test]
fn copy_and_save_hand_over_the_flattened_picture() {
    let mut s = studio_on(120, 80);
    assert!(matches!(s.update(Msg::Editor(Message::Copy)), Outcome::Copy(c) if c.width == 120));
    assert!(matches!(s.update(Msg::Editor(Message::Save)), Outcome::Save(c) if c.height == 80));
    // Esc never closes the Studio
    assert!(matches!(
        s.update(Msg::Editor(Message::Back)),
        Outcome::None
    ));
}

/// The Studio at its opening size: empty, and on a picture with a drawing
/// (Pen chosen: the options bar shows its colours and size), in Sapphire and
/// Daylight. Look at each one after an update.
#[test]
fn the_studio_matches_its_golden_images() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        let empty = Studio::new(StudioConfig::default());
        let file = format!("studio-empty-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&empty, false)
        }));
        let rgba = (0..640 * 400)
            .flat_map(|i| match (i % 640 / 40 + i / 640 / 40) % 2 {
                0 => [236, 238, 243, 255],
                _ => [52, 56, 70, 255],
            })
            .collect();
        let mut s = Studio::new(StudioConfig::default());
        s.open(Picture::Crop(Crop {
            width: 640,
            height: 400,
            rgba,
        }));
        draw(&mut s);
        let file = format!("studio-picture-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, false)
        }));
    });
}

/// The picture point under `at` for a scroll `offset` (display px from the picture's corner).
fn under(s: &Studio, at: Point, offset: Vector) -> Vector {
    let e = s.editor.as_ref().unwrap();
    let (room, shown) = (s.stage(), e.size());
    let corner = Vector::new(
        ((room.width - 2.0).max(shown.width) - shown.width) / 2.0,
        ((room.height - 2.0).max(shown.height) - shown.height) / 2.0,
    );
    (Vector::new(at.x, at.y) + offset - corner) * (1.0 / e.zoom)
}

#[test]
fn ctrl_wheel_zooms_by_the_buttons_step_round_the_pointer() {
    let mut s = studio_on(4000, 3000);
    let _ = s.update(Msg::Resized(SIZE));
    let _ = s.update(Msg::ZoomIn); // past fit: both ways scroll
    let _ = s.update(Msg::ZoomIn);
    let _ = s.update(Msg::ZoomIn);
    let _ = s.update(Msg::Scrolled(Vector::new(300.0, 200.0)));
    let (before, at) = (s.editor.as_ref().unwrap().zoom, Point::new(400.0, 250.0));
    let point = under(&s, at, Vector::new(300.0, 200.0));
    let Outcome::ScrollTo(to) = s.update(Msg::Wheel { at, notches: 1.0 }) else {
        panic!("a zoom scrolls the stage");
    };
    let e = s.editor.as_ref().unwrap();
    assert!(
        (e.zoom - before * ZOOM_STEP).abs() < 1e-4,
        "one notch = one step"
    );
    let after = under(&s, at, Vector::new(to.x, to.y));
    assert!(
        (after.x - point.x).abs() < 0.5 && (after.y - point.y).abs() < 0.5,
        "the picture point under the pointer stays: {point:?} -> {after:?}"
    );
    // out again, and the range holds
    let _ = s.update(Msg::Wheel { at, notches: -1.0 });
    assert!((s.editor.as_ref().unwrap().zoom - before).abs() < 1e-4);
    let _ = s.update(Msg::Wheel { at, notches: 100.0 });
    assert!((s.editor.as_ref().unwrap().zoom - ZOOM_RANGE.end() / s.scale).abs() < 1e-4);
    assert!(
        matches!(s.update(Msg::Ctrl(true)), Outcome::None) && s.ctrl,
        "Ctrl is tracked for the stage"
    );
}
