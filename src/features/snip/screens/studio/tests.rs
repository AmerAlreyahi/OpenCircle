use iced::{Point, Rectangle, Size, Vector};

use super::*;
use crate::features::snip::annotate::{Message, Tool};
use crate::features::snip::snip_more::Row;
use crate::features::translate::Line;

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

/// Quit with unsaved drawings asks (Save / Don't save / Cancel); answered, it goes.
#[test]
fn quitting_with_unsaved_drawings_asks_first() {
    let mut s = studio_on(400, 300);
    assert!(matches!(s.quit_requested(), Outcome::Quit), "clean: goes");
    draw(&mut s);
    assert!(matches!(s.quit_requested(), Outcome::None) && s.asking());
    // Cancel: stays, still unsaved
    assert!(matches!(s.update(Msg::Cancel), Outcome::None));
    assert!(!s.asking() && s.dirty());
    // Don't save: the quit goes on
    let _ = s.quit_requested();
    assert!(matches!(s.update(Msg::DontSave), Outcome::Quit));
    assert!(!s.dirty());
    // Save: the Save runs, then the quit goes on
    let mut s = studio_on(400, 300);
    draw(&mut s);
    let _ = s.quit_requested();
    assert!(matches!(s.update(Msg::SaveFirst), Outcome::SaveThen(_)));
    s.saved();
    assert!(matches!(s.proceed(), Outcome::Quit));
}

/// The question blocks the picture: no drawing, open or paste while it is up.
#[test]
fn the_question_blocks_editing() {
    let mut s = studio_on(400, 300);
    draw(&mut s);
    let _ = s.close_requested();
    let before = s.editor.as_ref().unwrap().doc.state();
    draw(&mut s);
    assert!(matches!(s.update(Msg::Open), Outcome::None));
    assert!(matches!(s.update(Msg::Paste), Outcome::None));
    assert!(matches!(
        s.update(Msg::Dropped("/x.png".into())),
        Outcome::None
    ));
    assert_eq!(s.editor.as_ref().unwrap().doc.state(), before);
}

/// A picture of `px` (grey), with a Redact over (10,10)-(60,30).
fn with_redact(px: [u8; 4]) -> Studio {
    use crate::features::snip::annotate::{Item, Shape};
    let rgba = (0..100 * 60).flat_map(|_| px).collect();
    let mut s = Studio::new(StudioConfig::default());
    s.open(Picture::Crop(Crop {
        width: 100,
        height: 60,
        rgba,
    }));
    s.editor.as_mut().unwrap().doc.items.push(Item {
        shape: Shape::Redact {
            a: Point::new(10.0, 10.0),
            b: Point::new(60.0, 30.0),
        },
        colour: [0, 0, 0, 255],
        width: 4.0,
        outline: false,
        fill: false,
    });
    s
}

/// A picture-panel row asks the host; with no picture, or under the unsaved
/// question, nothing runs.
#[test]
fn a_panel_row_asks_the_host() {
    let mut s = studio_on(100, 60);
    for row in Row::ALL {
        assert!(matches!(s.update(Msg::Act(row)), Outcome::Act(r) if r == row));
    }
    let mut empty = Studio::new(StudioConfig::default());
    assert!(matches!(
        empty.update(Msg::Act(Row::CopyText)),
        Outcome::None
    ));
    draw(&mut s);
    let _ = s.close_requested();
    assert!(matches!(s.update(Msg::Act(Row::AskAi)), Outcome::None));
}

/// The three actions in the design's order; Edit text follows them (Cut out comes later).
#[test]
fn the_panel_rows_are_the_three_actions_in_design_order() {
    assert_eq!(view::ACTS, [Row::CopyText, Row::Translate, Row::AskAi]);
}

/// Copy text reads the clean capture with the hidden part painted out; Ask AI
/// gets the annotated picture (the drawing is on it).
#[test]
fn copy_text_reads_the_clean_picture_ask_ai_the_annotated_one() {
    let s = with_redact([200, 120, 40, 255]);
    let (read, hidden) = s.read_picture().unwrap();
    let at = |c: &Crop, x: usize, y: usize| c.rgba[(y * c.width as usize + x) * 4..][..4].to_vec();
    assert_eq!(
        at(&read, 30, 20),
        [255; 4],
        "the redacted part is painted out"
    );
    assert_eq!(
        at(&read, 80, 50),
        [200, 120, 40, 255],
        "the rest is the capture"
    );
    assert!(hidden.covers(30.0, 20.0) && !hidden.covers(80.0, 50.0));
    let asked = s.annotated().unwrap();
    assert_eq!(at(&asked, 30, 20), [0, 0, 0, 255], "the drawing is on it");
    assert_eq!(at(&asked, 80, 50), [200, 120, 40, 255]);
}

/// Translate sends the picture with the hidden part painted out; the
/// translation lands under the drawing, the hidden part stays as it was, and
/// Show original brings the picture back exactly.
#[test]
fn translate_keeps_hidden_parts_and_show_original_restores() {
    let mut s = with_redact([200, 120, 40, 255]);
    let before = s.editor.as_ref().unwrap().doc.image.rgba.clone();
    let (frame, area) = s.translate_frame().unwrap();
    assert_eq!(area, [0, 0, 100, 60]);
    let image::Handle::Rgba { pixels, .. } = &frame else {
        panic!("a frame");
    };
    assert_eq!(
        &pixels[(20 * 100 + 30) * 4..][..4],
        [255; 4],
        "masked for the translator"
    );
    assert!(s.translated());
    // the translator's answer: all blue
    let answer = image::Handle::from_rgba(100, 60, [0, 0, 255, 255].repeat(100 * 60));
    s.show_translation(&answer, area, true);
    let now = &s.editor.as_ref().unwrap().doc.image.rgba;
    let at = |x: usize, y: usize| now[(y * 100 + x) * 4..][..4].to_vec();
    assert_eq!(at(80, 50), [0, 0, 255, 255], "translated");
    assert_eq!(at(30, 20), [200, 120, 40, 255], "the hidden part as it was");
    s.show_original();
    assert!(!s.translated());
    assert_eq!(s.editor.as_ref().unwrap().doc.image.rgba, before);
}

/// A translation for an older picture never lands on a newer one.
#[test]
fn a_new_picture_forgets_the_translation() {
    let mut s = studio_on(100, 60);
    let epoch = s.epoch();
    let _ = s.translate_frame();
    s.open(Picture::Crop(crop(50, 50)));
    assert!(!s.translated() && s.epoch() != epoch);
}

/// The quick-action keys act here only while the window has the keyboard.
#[test]
fn quick_keys_need_the_keyboard() {
    let mut s = studio_on(100, 60);
    assert!(!s.takes_quick_keys());
    let _ = s.update(Msg::Focus(true));
    assert!(s.takes_quick_keys());
    let _ = s.update(Msg::Focus(false));
    assert!(!s.takes_quick_keys());
}

/// The rows show the user's own keys (never hard-coded) and Show original while translated.
#[test]
fn the_rows_show_the_users_keys() {
    let mut s = studio_on(100, 60);
    let info = s.info();
    assert_eq!(info.caps[0], ["Ctrl", "Shift", "X"]);
    assert!(!info.translated);
    let _ = s.translate_frame();
    assert!(s.info().translated);
}

/// The download card: Download starts it, Not now drops it, and no row acts
/// while it is up; a reading Copy text is not read twice.
#[test]
fn the_download_card_blocks_rows_and_answers_its_buttons() {
    let mut s = studio_on(100, 60);
    s.set_card(Some(Fetching::reader()));
    assert!(matches!(s.update(Msg::Act(Row::Translate)), Outcome::None));
    assert!(matches!(s.update(Msg::Download), Outcome::Download));
    assert_eq!(s.card().and_then(|c| c.share), Some(0.0));
    assert!(matches!(s.update(Msg::DismissCard), Outcome::Dismissed));
    assert!(s.card().is_none());
    s.set_busy(Some(Row::CopyText));
    assert!(matches!(s.update(Msg::Act(Row::CopyText)), Outcome::None));
    assert!(matches!(
        s.update(Msg::Act(Row::Translate)),
        Outcome::Act(_)
    ));
}

/// The panel with a spinning row, and the card, in English and Arabic.
#[test]
fn the_panel_and_card_match_their_golden_images() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        let mut s = studio_on(640, 400);
        s.set_busy(Some(Row::Translate));
        let file = format!("studio-busy-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, false)
        }));
        let mut asking = studio_on(640, 400);
        asking.set_card(Some(Fetching {
            which: crate::features::text_reading::model_download::Which::Pair,
            bytes: 78_000_000,
            share: None,
            failed: None,
            provider: true,
        }));
        let file = format!("studio-card-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&asking, false)
        }));
        let _ar = crate::shared::kit::testing::arabic();
        let file = format!("studio-panel-ar-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, false)
        }));
    });
}

// ---- Edit text (v0.4.5, S8) ----

/// A white picture with a dark bar of "text" rows at `y` (a line a reader would box).
fn lined(width: u32, height: u32) -> (Studio, Line) {
    let mut c = crop(width, height);
    for y in 30..44usize {
        for x in 20..140usize {
            let on = (x / 3 + y / 2) % 3 != 0; // strokes, not a block
            if on {
                let i = (y * width as usize + x) * 4;
                c.rgba[i..i + 4].copy_from_slice(&[20, 20, 30, 255]);
            }
        }
    }
    let mut s = Studio::new(StudioConfig::default());
    s.open(Picture::Crop(c));
    let line = Line {
        bounds: [16.0, 26.0, 130.0, 22.0],
        text: "Hello there".into(),
    };
    (s, line)
}

fn pixels_of(s: &Studio) -> Vec<u8> {
    s.editor.as_ref().unwrap().doc.image.rgba.clone()
}

/// Edit text: the panel row asks the host to read the lines; a second click
/// and the other rows rest meanwhile; Cancel (Esc) ends it with the picture as it was.
#[test]
fn edit_text_routes_reads_and_cancels_s8() {
    let (mut s, line) = lined(300, 120);
    let before = pixels_of(&s);
    assert!(matches!(s.update(Msg::EditText), Outcome::ReadLines));
    assert!(matches!(s.update(Msg::EditText), Outcome::None), "once");
    assert!(matches!(s.update(Msg::Act(Row::Translate)), Outcome::None));
    let epoch = s.epoch();
    // an answer for another picture lands nowhere
    assert!(matches!(
        s.set_lines(epoch + 1, Ok(vec![line.clone()])),
        Outcome::None
    ));
    assert!(s.edit.as_ref().unwrap().lines.is_none());
    assert!(matches!(s.set_lines(epoch, Ok(vec![line])), Outcome::None));
    let _ = s.update(Msg::PickLine(0));
    let _ = s.update(Msg::Draft("Bye".into()));
    let _ = s.update(Msg::Cancel);
    assert!(s.edit.is_none());
    assert_eq!(pixels_of(&s), before);
    assert!(!s.dirty());
}

/// No lines found: a pill says so, nothing is on.
#[test]
fn edit_text_without_lines_says_so_s8() {
    let (mut s, _) = lined(300, 120);
    let _ = s.update(Msg::EditText);
    let epoch = s.epoch();
    assert!(matches!(
        s.set_lines(epoch, Ok(Vec::new())),
        Outcome::Say(_)
    ));
    assert!(s.edit.is_none());
}

/// Lines read from a cropped view come back in image px.
#[test]
fn edit_text_lines_follow_the_crop_s8() {
    let (mut s, line) = lined(300, 120);
    s.editor.as_mut().unwrap().doc.crop = Some(Rectangle::new(
        Point::new(10.0, 5.0),
        Size::new(200.0, 100.0),
    ));
    let _ = s.update(Msg::EditText);
    let epoch = s.epoch();
    let _ = s.set_lines(epoch, Ok(vec![line]));
    let l = &s.edit.as_ref().unwrap().lines.as_ref().unwrap()[0];
    assert_eq!(l.bounds[..2], [26.0, 31.0]);
}

/// Done replaces the line in the pixels (the old strokes are gone, new ones
/// drawn), as one undoable, unsaved document edit; Undo brings the bytes back
/// exactly and Redo again.
#[test]
fn edit_text_done_is_one_undoable_edit_s8() {
    let (mut s, line) = lined(300, 120);
    let before = pixels_of(&s);
    let _ = s.update(Msg::EditText);
    let epoch = s.epoch();
    let _ = s.set_lines(epoch, Ok(vec![line]));
    let _ = s.update(Msg::PickLine(0));
    let _ = s.update(Msg::Draft("Hi".into()));
    let out = s.update(Msg::EditDone);
    assert!(
        matches!(out, Outcome::None),
        "{:?}",
        matches!(out, Outcome::Say(_))
    );
    assert!(s.edit.is_none());
    let after = pixels_of(&s);
    assert_ne!(after, before);
    assert!(s.dirty(), "an unsaved edit");
    assert_eq!(s.editor.as_ref().unwrap().doc.steps(), (1, 1));
    // the right part of the old line (past "Hi") is background again
    let at = |x: usize, y: usize| after[(y * 300 + x) * 4..][..3].to_vec();
    assert_eq!(at(130, 37), [255; 3], "old strokes erased");
    // Undo / Redo through the editor's own messages
    let _ = s.update(Msg::Editor(Message::Undo));
    assert_eq!(pixels_of(&s), before);
    assert!(!s.dirty());
    let _ = s.update(Msg::Editor(Message::Redo));
    assert_eq!(pixels_of(&s), after);
    // Save and Copy get the edit
    let saved = annotate::render(&s.editor.as_ref().unwrap().doc);
    assert_eq!(saved.rgba, after);
}

/// An unchanged or empty text changes nothing (no history entry).
#[test]
fn edit_text_with_the_same_text_is_no_edit_s8() {
    let (mut s, line) = lined(300, 120);
    let _ = s.update(Msg::EditText);
    let epoch = s.epoch();
    let _ = s.set_lines(epoch, Ok(vec![line]));
    let _ = s.update(Msg::PickLine(0));
    let _ = s.update(Msg::EditDone);
    assert!(s.edit.is_none());
    assert_eq!(s.editor.as_ref().unwrap().doc.steps(), (0, 0));
}

/// Arabic text goes right to left: it is drawn against the line box's right edge.
#[test]
fn edit_text_arabic_is_right_aligned_s8() {
    let (mut s, line) = lined(300, 120);
    let _ = s.update(Msg::EditText);
    let epoch = s.epoch();
    let _ = s.set_lines(epoch, Ok(vec![line]));
    let _ = s.update(Msg::PickLine(0));
    let _ = s.update(Msg::Draft("مرحبا".into()));
    assert!(matches!(s.update(Msg::EditDone), Outcome::None));
    let px = pixels_of(&s);
    let ink: Vec<usize> = (0..300usize)
        .filter(|&x| (26..48usize).any(|y| px[(y * 300 + x) * 4] < 128))
        .collect();
    let (lo, hi) = (*ink.first().unwrap(), *ink.last().unwrap());
    assert!(hi > 100 && lo > 60, "against the right edge: {lo}..{hi}");
}

/// The lines over the picture, a picked one, and the bar, in English and Arabic.
#[test]
fn edit_text_matches_its_golden_images_s8() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        let (mut s, line) = lined(640, 400);
        let _ = s.update(Msg::EditText);
        let epoch = s.epoch();
        let other = Line {
            bounds: [16.0, 90.0, 200.0, 22.0],
            text: "Another line".into(),
        };
        let _ = s.set_lines(epoch, Ok(vec![line, other]));
        let _ = s.update(Msg::PickLine(0));
        let file = format!("studio-edit-text-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, false)
        }));
        let _ar = crate::shared::kit::testing::arabic();
        let file = format!("studio-edit-text-ar-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, false)
        }));
    });
}

/// Picks line 0 of a fresh `lined` picture.
fn picked() -> Studio {
    let (mut s, line) = lined(300, 120);
    let _ = s.update(Msg::EditText);
    let epoch = s.epoch();
    let _ = s.set_lines(epoch, Ok(vec![line]));
    assert!(matches!(s.update(Msg::PickLine(0)), Outcome::FocusField));
    s
}

/// The dark-on-white bars: (ink pixels, ink rows) in the line's box.
fn ink(px: &[u8]) -> (usize, usize) {
    let on = |x: usize, y: usize| px[(y * 300 + x) * 4..][..3].iter().any(|&v| v < 200);
    let rows = (20..60usize)
        .filter(|&y| (10..160usize).any(|x| on(x, y)))
        .count();
    let n = (20..60usize)
        .map(|y| (10..160usize).filter(|&x| on(x, y)).count())
        .sum();
    (n, rows)
}

/// Picking a line reads its matched look; the bar's controls (the editor's own
/// messages) set size, colour and Bold on the line, not on the editor; picking
/// again starts over.
#[test]
fn edit_text_controls_start_matched_and_change_s8() {
    let mut s = picked();
    let x = s.edit.as_ref().unwrap();
    let m = x.matched.unwrap();
    assert_eq!((x.size(), x.bold()), (m.em, m.bold));
    assert_eq!(x.colour()[..3], m.colour);
    let tool_colour = s.editor.as_ref().unwrap().colour;
    let _ = s.update(Msg::Editor(Message::TextSize(30.0)));
    let _ = s.update(Msg::Editor(Message::Colour([200, 30, 30, 255])));
    let flipped = crate::features::snip::annotate::TextStyle {
        bold: !m.bold,
        ..Default::default()
    };
    let _ = s.update(Msg::Editor(Message::TextStyle(flipped)));
    let x = s.edit.as_ref().unwrap();
    assert_eq!((x.size(), x.bold()), (30.0, !m.bold));
    assert_eq!(x.colour(), [200, 30, 30, 255]);
    let e = s.editor.as_ref().unwrap();
    assert_eq!(
        e.colour, tool_colour,
        "the editor's own colour is untouched"
    );
    let _ = s.update(Msg::PickLine(0));
    assert_eq!(s.edit.as_ref().unwrap().size(), m.em, "starts over");
}

/// Colour, size and Bold reach the pixels (render byte checks).
#[test]
fn edit_text_colour_size_and_bold_draw_s8() {
    let done = |f: &dyn Fn(&mut Studio)| {
        let mut s = picked();
        let _ = s.update(Msg::Draft("Hello".into()));
        f(&mut s);
        assert!(matches!(s.update(Msg::EditDone), Outcome::None));
        pixels_of(&s)
    };
    let plain = done(&|_| {});
    // colour: red ink where the matched colour was dark
    let red = done(&|s| {
        let _ = s.update(Msg::Editor(Message::Colour([220, 20, 20, 255])));
    });
    let reddest = |px: &[u8]| {
        px.chunks(4)
            .filter(|p| p[0] > 150 && p[1] < 100 && p[2] < 100)
            .count()
    };
    assert_eq!(reddest(&plain), 0);
    assert!(reddest(&red) > 20, "{}", reddest(&red));
    // size: smaller text covers fewer rows
    let small = done(&|s| {
        let _ = s.update(Msg::Editor(Message::TextSize(9.0)));
    });
    assert!(
        ink(&small).1 < ink(&plain).1,
        "{:?} {:?}",
        ink(&small),
        ink(&plain)
    );
    // Bold: more ink than regular at the same size
    let weight = |bold: bool| {
        done(&|s| {
            let _ = s.update(Msg::Editor(Message::TextSize(14.0)));
            let st = crate::features::snip::annotate::TextStyle {
                bold,
                ..Default::default()
            };
            let _ = s.update(Msg::Editor(Message::TextStyle(st)));
        })
    };
    assert!(ink(&weight(true)).0 > ink(&weight(false)).0);
}

/// The matched look: the dark ink colour of the line.
#[test]
fn edit_text_matched_colour_is_the_inks_s8() {
    let s = picked();
    let c = s.edit.as_ref().unwrap().matched.unwrap().colour;
    assert!(c.iter().all(|&v| v < 80), "{c:?}");
}

/// The inline field is on the picture only once a line is picked.
#[test]
fn edit_text_inline_field_follows_the_pick_s8() {
    let (mut s, line) = lined(300, 120);
    let _ = s.update(Msg::EditText);
    let epoch = s.epoch();
    let _ = s.set_lines(epoch, Ok(vec![line]));
    assert!(s.edit.as_ref().unwrap().active.is_none());
    let _ = s.update(Msg::PickLine(0));
    let _ = s.update(Msg::Draft("typed".into()));
    assert_eq!(s.edit.as_ref().unwrap().draft, "typed");
}
