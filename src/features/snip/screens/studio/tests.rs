use iced::{Point, Rectangle, Size, Vector};

use super::*;
use crate::features::snip::annotate::{Message, Tool};
use crate::features::snip::screens::steps::Step;
use crate::features::snip::snip_more::Row;
use crate::features::translate::Line;
use crate::shared::config;

mod b01;
mod d04;
mod f03;
mod r16;
mod r18;
mod r19;
mod w123;

/// The Steps panel open and pinned (docked): the frame the older tests were written against.
fn docked() -> StudioConfig {
    StudioConfig {
        library: true,
        pinned: true,
        ..StudioConfig::default()
    }
}

fn crop(width: u32, height: u32) -> Crop {
    Crop {
        width,
        height,
        rgba: vec![255; (width * height * 4) as usize],
    }
}

fn studio_on(width: u32, height: u32) -> Studio {
    let mut s = Studio::new(docked());
    s.open(Picture::Crop(crop(width, height)));
    s.settle_open();
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

/// R-18 / F-03: the Steps card floats over the canvas, so opening, closing and pinning it never
/// changes the stage or the picture's zoom (only an explicit Fit refits).
#[test]
fn the_steps_card_never_changes_the_stage_or_the_zoom() {
    let mut s = studio_on(3840, 2160);
    let zoom = |s: &Studio| s.editor.as_ref().unwrap().zoom;
    let (stage, shown) = (s.stage(), zoom(&s));
    assert!(matches!(s.update(Msg::Library), Outcome::Panels(_, p) if !p.library));
    assert_eq!((s.stage(), zoom(&s)), (stage, shown), "closing");
    let _ = s.update(Msg::Fit);
    let fitted = zoom(&s);
    assert!(matches!(s.update(Msg::Library), Outcome::Panels(_, p) if p.library));
    assert_eq!((s.stage(), zoom(&s)), (stage, fitted), "opening");
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
    let mut s = Studio::new(docked());
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
    let mut empty = Studio::new(docked());
    assert!(matches!(empty.update(Msg::Paste), Outcome::Paste(None)));
    assert!(matches!(empty.update(Msg::Open), Outcome::PickFile));
    let file = std::path::PathBuf::from("/tmp/x.png");
    // a single drop on the empty Studio opens it (after the drop settles)
    assert!(matches!(
        empty.update(Msg::Dropped(file.clone())),
        Outcome::SettleDrop(1)
    ));
    assert!(matches!(empty.update(Msg::DropSettle(1)), Outcome::Read(p) if p == file));
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
        let empty = Studio::new(docked());
        let file = format!("studio-empty-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&empty, &[], false)
        }));
        let rgba = (0..640 * 400)
            .flat_map(|i| match (i % 640 / 40 + i / 640 / 40) % 2 {
                0 => [236, 238, 243, 255],
                _ => [52, 56, 70, 255],
            })
            .collect();
        let mut s = Studio::new(docked());
        s.open(Picture::Crop(Crop {
            width: 640,
            height: 400,
            rgba,
        }));
        draw(&mut s);
        let file = format!("studio-picture-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
        // the Steps tab with three steps, the export bar open on JPG
        let steps: Vec<_> = [(300, 200), (200, 300), (400, 120)]
            .map(|(w, h)| std::sync::Arc::new(Step::new(&crop(w, h)).unwrap()))
            .into();
        let _ = s.update(Msg::Export);
        let _ = s.update(Msg::ExportFormat(export::Format::Jpg));
        let file = format!("studio-steps-export-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &steps, false)
        }));
        // the walkthrough dialog over it: from the Steps, then from the edits, then in Arabic
        let _ = s.update(Msg::ExportWalkthrough);
        let file = format!("studio-walkthrough-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &steps, false)
        }));
        let _ = s.update(Msg::WalkEdits(true));
        let _ = s.update(Msg::WalkNumbered(true));
        let _ = s.update(Msg::WalkFormat(export::Movie::Webm));
        let file = format!("studio-walkthrough-edits-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &steps, false)
        }));
        // encoding, in Arabic
        let _ = s.update(Msg::WalkGo);
        let _ = s.update(Msg::WalkProgress(2, 3));
        crate::shared::app_language::set(crate::shared::app_language::Lang::Ar);
        let file = format!("studio-walkthrough-ar-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &steps, false)
        }));
        crate::shared::app_language::set(crate::shared::app_language::Lang::En);
        let _ = s.update(Msg::WalkEnded(true));
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
    assert!(matches!(s.update(Msg::DropSettle(1)), Outcome::None));
    assert_eq!(s.editor.as_ref().unwrap().doc.state(), before);
}

/// A picture of `px` (grey), with a Redact over (10,10)-(60,30).
fn with_redact(px: [u8; 4]) -> Studio {
    use crate::features::snip::annotate::{Item, Shape};
    let rgba = (0..100 * 60).flat_map(|_| px).collect();
    let mut s = Studio::new(docked());
    s.open(Picture::Crop(Crop {
        width: 100,
        height: 60,
        rgba,
    }));
    s.editor.as_mut().unwrap().doc.items.push(Item {
        shape: Shape::Redact {
            a: Point::new(10.0, 10.0),
            b: Point::new(60.0, 30.0),
            oval: false,
        },
        colour: [0, 0, 0, 255],
        width: 4.0,
        outline: false,
        fill: false,
        hidden: false,
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
    let mut empty = Studio::new(docked());
    assert!(matches!(
        empty.update(Msg::Act(Row::CopyText)),
        Outcome::None
    ));
    draw(&mut s);
    let _ = s.close_requested();
    assert!(matches!(s.update(Msg::Act(Row::AskAi)), Outcome::None));
}

/// The three actions in the design's order; Edit text and Cut out follow them.
#[test]
fn the_panel_rows_are_the_three_actions_in_design_order() {
    assert_eq!(popovers::ACTS, [Row::CopyText, Row::Translate, Row::AskAi]);
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
            view(&s, &[], false)
        }));
        let mut asking = studio_on(640, 400);
        asking.set_card(Some(Fetching {
            which: crate::features::text_reading::model_download::Which::Pair,
            bytes: 78_000_000,
            share: None,
            failed: None,
            provider: true,
            details: false,
        }));
        let file = format!("studio-card-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&asking, &[], false)
        }));
        let _ar = crate::shared::kit::testing::arabic();
        let file = format!("studio-panel-ar-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
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
    let mut s = Studio::new(docked());
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
    let epoch = s.edit_token();
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
    let epoch = s.edit_token();
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
    let epoch = s.edit_token();
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
    let epoch = s.edit_token();
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
    let epoch = s.edit_token();
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
    let epoch = s.edit_token();
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
        let epoch = s.edit_token();
        let other = Line {
            bounds: [16.0, 90.0, 200.0, 22.0],
            text: "Another line".into(),
        };
        let _ = s.set_lines(epoch, Ok(vec![line, other]));
        let _ = s.update(Msg::PickLine(0));
        let file = format!("studio-edit-text-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
        let _ar = crate::shared::kit::testing::arabic();
        let file = format!("studio-edit-text-ar-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
    });
}

/// Picks line 0 of a fresh `lined` picture.
fn picked() -> Studio {
    let (mut s, line) = lined(300, 120);
    let _ = s.update(Msg::EditText);
    let epoch = s.edit_token();
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
    let epoch = s.edit_token();
    let _ = s.set_lines(epoch, Ok(vec![line]));
    assert!(s.edit.as_ref().unwrap().active.is_none());
    let _ = s.update(Msg::PickLine(0));
    let _ = s.update(Msg::Draft("typed".into()));
    assert_eq!(s.edit.as_ref().unwrap().draft, "typed");
}

/// A read from a cancelled session never fills a new one: Edit text, Esc
/// while it reads, Edit text again; the first read's answer is ignored.
#[test]
fn edit_text_a_cancelled_reads_answer_is_ignored_s8() {
    let (mut s, line) = lined(300, 120);
    let _ = s.update(Msg::EditText);
    let old = s.edit_token();
    let _ = s.update(Msg::Cancel);
    let _ = s.update(Msg::EditText);
    let new = s.edit_token();
    assert_ne!(old, new);
    let stale = Line {
        text: "stale".into(),
        ..line.clone()
    };
    assert!(matches!(s.set_lines(old, Ok(vec![stale])), Outcome::None));
    assert!(s.edit.as_ref().unwrap().lines.is_none(), "still reading");
    let _ = s.set_lines(new, Ok(vec![line]));
    assert_eq!(
        s.edit.as_ref().unwrap().lines.as_ref().unwrap()[0].text,
        "Hello there"
    );
}

/// Esc on the unsaved-changes question closes only the question.
#[test]
fn edit_text_esc_on_the_question_keeps_the_session_s8() {
    let mut s = picked();
    let _ = s.close_requested(); // dirty? make it so
    if !s.asking() {
        s.asking = Some(After::Close);
    }
    let _ = s.update(Msg::Cancel);
    assert!(!s.asking());
    assert!(s.edit.is_some(), "the edit goes on");
    let _ = s.update(Msg::Cancel);
    assert!(s.edit.is_none());
}

/// Bold switched off and on again is the matched weight, not forced bold.
#[test]
fn edit_text_bold_back_is_the_matched_weight_s8() {
    let mut s = picked();
    let m = s.edit.as_ref().unwrap().matched.unwrap();
    let style = |bold| crate::features::snip::annotate::TextStyle {
        bold,
        ..Default::default()
    };
    let _ = s.update(Msg::Editor(Message::TextStyle(style(!m.bold))));
    assert_eq!(s.edit.as_ref().unwrap().tweak().bold, Some(!m.bold));
    let _ = s.update(Msg::Editor(Message::TextStyle(style(m.bold))));
    assert_eq!(s.edit.as_ref().unwrap().tweak().bold, None);
}

/// Only the rows round the line are copied, and the edit lands in the right rows.
#[test]
fn edit_text_works_on_a_band_of_rows_s8() {
    let (mut s, line) = lined(300, 400);
    let d = &s.editor.as_ref().unwrap().doc.image;
    let (top, rows, moved) = edit_text::band(&d.rgba, 300, 400, &line);
    assert!(rows.len() < d.rgba.len() / 2, "not the whole frame");
    assert_eq!(moved.bounds[1], line.bounds[1] - top as f32);
    let before = pixels_of(&s);
    let _ = s.update(Msg::EditText);
    let t = s.edit_token();
    let _ = s.set_lines(t, Ok(vec![line]));
    let _ = s.update(Msg::PickLine(0));
    let _ = s.update(Msg::Draft("Hi".into()));
    assert!(matches!(s.update(Msg::EditDone), Outcome::None));
    let after = pixels_of(&s);
    assert_ne!(after, before);
    // nothing outside the band moved
    assert_eq!(after[..top * 300 * 4], before[..top * 300 * 4]);
    let end = top + rows.len() / (300 * 4);
    assert_eq!(after[end * 300 * 4..], before[end * 300 * 4..]);
    let _ = s.update(Msg::Editor(Message::Undo));
    assert_eq!(pixels_of(&s), before);
}

// ---- Cut out (v0.4.6, S9) ----

/// A white picture with a red box in the middle: a flat border.
fn boxed(width: u32, height: u32) -> Studio {
    let mut c = crop(width, height);
    let (w, h) = (width as usize, height as usize);
    for y in h / 3..h * 2 / 3 {
        for x in w * 3 / 10..w * 7 / 10 {
            let i = (y * w + x) * 4;
            c.rgba[i..i + 4].copy_from_slice(&[200, 30, 30, 255]);
        }
    }
    let mut s = Studio::new(docked());
    s.open(Picture::Crop(c));
    s
}

fn alpha_at(s: &Studio, x: usize, y: usize) -> u8 {
    let w = s.editor.as_ref().unwrap().doc.image.width as usize;
    s.editor.as_ref().unwrap().doc.image.rgba[(y * w + x) * 4 + 3]
}

/// The cut as the host does it: take the source, cut, answer.
fn cut_now(s: &mut Studio) -> Outcome {
    let (rgba, w, h, epoch) = s.cut_source().unwrap();
    let done = match crate::features::cutout::quick(&rgba, w, h) {
        Some(a) => CutDone::Alpha(a),
        None => CutDone::NeedsModel,
    };
    s.set_cut(epoch, done)
}

/// S9: the row asks the host; it rests while a line is edited or a cut runs.
#[test]
fn cut_out_row_asks_the_host_s9() {
    let mut s = boxed(100, 60);
    assert!(matches!(s.update(Msg::CutOut), Outcome::CutOut));
    let _ = s.cut_source();
    assert!(matches!(s.update(Msg::CutOut), Outcome::None), "once");
    let mut empty = Studio::new(docked());
    assert!(matches!(empty.update(Msg::CutOut), Outcome::None));
}

/// S9 + brief 1(a): a flat border is cut with no model; the box stays, the
/// background is transparent, and it is one undoable edit (Ctrl+Z brings it back).
#[test]
fn a_flat_background_is_cut_and_undo_brings_it_back_s9() {
    let mut s = boxed(100, 60);
    let before = pixels_of(&s);
    assert!(matches!(cut_now(&mut s), Outcome::None));
    assert_eq!((alpha_at(&s, 2, 2), alpha_at(&s, 50, 30)), (0, 255));
    assert!(s.transparent && s.dirty());
    assert_eq!(s.cut_phase(), CutPhase::Idle);
    let _ = s.update(Msg::Editor(Message::Undo));
    assert_eq!(pixels_of(&s), before);
    assert!(!s.transparent && s.comparing().is_none());
    let _ = s.update(Msg::Editor(Message::Redo));
    assert_eq!(alpha_at(&s, 2, 2), 0);
}

/// S9: Before | After compares without editing; anything that works on the
/// picture (a drawing, Save) sees the cut.
#[test]
fn before_after_compares_and_work_sees_the_cut_s9() {
    let mut s = boxed(100, 60);
    let _ = cut_now(&mut s);
    assert_eq!(s.comparing(), Some(false));
    let _ = s.update(Msg::CutView(true));
    assert_eq!((s.comparing(), alpha_at(&s, 2, 2)), (Some(true), 255));
    assert!(s.dirty(), "the compare view is not an edit");
    let _ = s.update(Msg::CutView(false));
    assert_eq!(alpha_at(&s, 2, 2), 0);
    // Save from Before saves the cut
    let _ = s.update(Msg::CutView(true));
    let Outcome::SaveThen(crop) = s.update(Msg::SaveFirst) else {
        panic!("save");
    };
    assert_eq!(crop.rgba[3], 0, "the cut, not the picture as it was");
    // a drawing ends the compare view
    draw(&mut s);
    assert!(s.comparing().is_none());
    assert_eq!(alpha_at(&s, 2, 2), 0);
}

/// Brief 1(b): a busy background needs the model: the card asks (size shown),
/// nothing is cut; Not now drops it.
#[test]
fn a_busy_background_asks_for_the_model_s9() {
    let mut c = crop(100, 60);
    for (i, p) in c.rgba.chunks_mut(4).enumerate() {
        p[..3].fill(if (i % 100 + i / 100) % 2 == 0 { 0 } else { 255 });
    }
    let mut s = Studio::new(docked());
    s.open(Picture::Crop(c));
    let before = pixels_of(&s);
    assert!(matches!(cut_now(&mut s), Outcome::None));
    assert_eq!(s.cut_phase(), CutPhase::Asking);
    let card = s.card().unwrap();
    assert!(
        card.share.is_none() && card.bytes > 178_000_000,
        "the size is shown"
    );
    assert_eq!(pixels_of(&s), before);
    assert!(
        matches!(s.update(Msg::CutOut), Outcome::None),
        "the card blocks"
    );
    assert!(matches!(s.update(Msg::DismissCard), Outcome::Dismissed));
    assert_eq!(s.cut_phase(), CutPhase::Idle);
}

/// An answer for another picture lands nowhere; a cut with nothing to remove says so.
#[test]
fn a_late_or_empty_cut_changes_nothing_s9() {
    let mut s = boxed(100, 60);
    let (_, _, _, epoch) = s.cut_source().unwrap();
    assert!(matches!(
        s.set_cut(epoch + 1, CutDone::Alpha(vec![0; 6000])),
        Outcome::None
    ));
    assert!(!s.transparent);
    // an all-kept alpha changes nothing: a pill
    assert!(matches!(
        s.set_cut(epoch, CutDone::Alpha(vec![255; 6000])),
        Outcome::Say(_)
    ));
    assert!(!s.dirty());
}

/// Save PNG keeps the alpha; JPEG blends on white.
#[test]
fn png_keeps_alpha_and_jpeg_blends_on_white_s9() {
    use crate::shared::config::Format;
    let mut s = boxed(100, 60);
    let _ = cut_now(&mut s);
    let crop = annotate::render(&s.editor.as_ref().unwrap().doc);
    assert_eq!(crop.rgba[3], 0);
    let png = crate::shared::picture::encode(&crop, Format::Png).unwrap();
    let back = crate::shared::platform::capture::decode_png(std::io::Cursor::new(png)).unwrap();
    assert_eq!((back.rgba[3], back.rgba[(30 * 100 + 50) * 4 + 3]), (0, 255));
    // the background colour under the alpha (red here) must not show in a JPEG
    let mut red = crop.clone();
    red.rgba[..3].copy_from_slice(&[255, 0, 0]);
    let jpg = crate::shared::picture::encode(&red, Format::Jpeg).unwrap();
    let img = ::image::load_from_memory(&jpg).unwrap().to_rgb8();
    let p = img.get_pixel(0, 0);
    assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "white, got {p:?}");
}

/// The panel (with Cut out spinning), the compare bar on a checkerboard, and
/// the card, in English and Arabic.
#[test]
fn cut_out_matches_its_golden_images_s9() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        let mut s = boxed(640, 400);
        let _ = cut_now(&mut s);
        let file = format!("studio-cut-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
        let mut asking = boxed(640, 400);
        asking.set_card(Some(Fetching::cutout()));
        let file = format!("studio-cut-card-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&asking, &[], false)
        }));
        let _ar = crate::shared::kit::testing::arabic();
        let file = format!("studio-cut-ar-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
    });
}

#[test]
fn the_steps_list_opens_reorders_and_deletes_the_strips_own_steps() {
    let mut s = studio_on(200, 100);
    let at = |y: f32| Point::new(40.0, y);
    // a click: open that step on the canvas
    let _ = s.update(Msg::StepMove(at(5.0), 3));
    let _ = s.update(Msg::StepPress(1));
    assert!(matches!(s.update(Msg::StepRelease), Outcome::OpenStep(1)));
    // a drag of the first to the third cell: the strip reorders (no copy here)
    let _ = s.update(Msg::StepMove(at(5.0), 3));
    let _ = s.update(Msg::StepPress(0));
    let third = Point::new(2.0 * (STEP_CELL.width + STEP_GAP) + 5.0, 5.0);
    let _ = s.update(Msg::StepMove(third, 3));
    assert_eq!(
        s.pick.order(3),
        [1, 2, 0],
        "shown at its new place while dragged"
    );
    assert!(matches!(
        s.update(Msg::StepRelease),
        Outcome::Steps(steps::Msg::Reorder(0, 2))
    ));
    assert!(matches!(
        s.update(Msg::StepDelete(2)),
        Outcome::Steps(steps::Msg::Delete(2))
    ));
    assert!(matches!(
        s.update(Msg::CopyAllSteps),
        Outcome::Steps(steps::Msg::CopyAll)
    ));
}

#[test]
fn export_asks_for_the_choices_then_hands_over_the_drawn_picture() {
    // nothing open: the sheet opens, but there is no picture to export
    let mut s = Studio::new(docked());
    let _ = s.update(Msg::Export);
    assert!(matches!(s.update(Msg::ExportGo), Outcome::None));
    let mut s = studio_on(300, 200);
    draw(&mut s);
    let _ = s.update(Msg::Export);
    assert!(s.exporting);
    let _ = s.update(Msg::ExportFormat(export::Format::Jpg));
    let _ = s.update(Msg::ExportQuality(60));
    let _ = s.update(Msg::ExportSize(1024));
    let Outcome::ExportPicture(picture, how) = s.update(Msg::ExportGo) else {
        panic!("no export");
    };
    assert!(!s.exporting);
    assert_eq!((picture.width, picture.height), (300, 200));
    assert_ne!(picture.rgba, crop(300, 200).rgba, "with the drawing");
    let want = export::Picture {
        format: export::Format::Jpg,
        quality: 60,
        max_edge: 1024,
    };
    assert_eq!(how, want);
    assert!(s.dirty(), "an export is not a Save");
}

/// A Studio on a picture with `n` pen strokes (n undoable edits).
fn drawn(n: usize) -> Studio {
    let mut s = studio_on(300, 200);
    for _ in 0..n {
        draw(&mut s);
    }
    s
}

/// W91.3: the walkthrough's transition and its length are remembered between sessions
/// (`snip.studio`), and a new Studio starts from them.
#[test]
fn the_walkthrough_transition_is_remembered() {
    use crate::features::snip::transition::Transition;
    let mut s = Studio::new(docked());
    assert_eq!(s.walk.how.transition, Transition::Crossfade);
    let Outcome::Panels(_, p) = s.update(Msg::WalkTransition(Transition::Wipe)) else {
        panic!("not saved");
    };
    assert_eq!(p.walk_transition, Transition::Wipe.index());
    let Outcome::Panels(_, p) = s.update(Msg::WalkTransitionMs(1000)) else {
        panic!("not saved");
    };
    let again = Studio::new(p);
    assert_eq!(
        (again.walk.how.transition, again.walk.how.transition_ms),
        (Transition::Wipe, 1000)
    );
}

#[test]
fn walkthrough_dialog_hands_its_choices_to_the_host() {
    let mut s = Studio::new(docked());
    let _ = s.update(Msg::ExportWalkthrough);
    assert!(s.exporting && s.walk_tab);
    let Outcome::Panels(_, panels) = s.update(Msg::WalkFormat(export::Movie::Mp4)) else {
        panic!("the format is remembered");
    };
    assert_eq!(panels.walk_format, 1);
    let _ = s.update(Msg::WalkTime(35));
    let _ = s.update(Msg::WalkSize(960));
    let _ = s.update(Msg::WalkNumbered(true));
    let _ = s.update(Msg::WalkLoop(false));
    let Outcome::ExportWalkthrough(how, frames) = s.update(Msg::WalkGo) else {
        panic!("no export");
    };
    assert_eq!(
        how,
        export::Walkthrough {
            movie: export::Movie::Mp4,
            max_edge: 960,
            frame_ms: 3500,
            repeat: false,
            numbered: true,
            ..export::Walkthrough::default()
        }
    );
    assert!(
        matches!(frames, Frames::Steps),
        "from the Steps: the host has them"
    );
    assert_eq!(s.walk.busy, Some((0, 0)));
    // while it runs: progress shows, a second Export does nothing, Cancel stops it
    let _ = s.update(Msg::WalkProgress(2, 5));
    assert_eq!(s.walk.busy, Some((2, 5)));
    assert!(matches!(s.update(Msg::WalkGo), Outcome::None));
    assert!(matches!(
        s.update(Msg::WalkCancel),
        Outcome::CancelWalkthrough
    ));
    let _ = s.update(Msg::WalkEnded(false));
    assert!(
        s.exporting && s.walk.busy.is_none(),
        "cancelled: the sheet stays"
    );
    let _ = s.update(Msg::WalkCancel);
    assert!(!s.exporting, "Cancel closes the idle sheet");
    // the last format is the next Studio's
    let again = Studio::new(panels);
    assert_eq!(again.walk.how.movie, export::Movie::Mp4);
}

/// The edits frames the host draws for `s`'s walkthrough, from the document copy the Studio hands over.
fn edit_frames_of(s: &mut Studio, how: export::Walkthrough) -> Vec<Crop> {
    let _ = s.update(Msg::ExportWalkthrough);
    let _ = s.update(Msg::WalkEdits(true));
    let Outcome::ExportWalkthrough(_, Frames::Edits(mut doc)) = s.update(Msg::WalkGo) else {
        panic!("no edits");
    };
    export::edit_frames(&mut doc, how)
}

#[test]
fn walkthrough_from_edits_replays_the_history_and_leaves_the_document_as_it_was() {
    let mut s = drawn(3);
    let before = s.editor.as_ref().unwrap().doc.steps();
    let state = s.editor.as_ref().unwrap().doc.state();
    let shown = annotate::render(&s.editor.as_ref().unwrap().doc);
    let frames = edit_frames_of(&mut s, export::Walkthrough::default());
    assert_eq!(frames.len(), 4, "the start and each of the 3 edits");
    assert_eq!(
        frames[0].rgba,
        crop(300, 200).rgba,
        "the first is the plain picture"
    );
    assert_ne!(frames[0].rgba, frames[3].rgba);
    assert_ne!(frames[1].rgba, frames[0].rgba);
    let e = s.editor.as_ref().unwrap();
    assert_eq!((e.doc.steps(), e.doc.state()), (before, state));
    assert_eq!(
        annotate::render(&e.doc).rgba,
        shown.rgba,
        "the picture is byte for byte what it was"
    );
    // the replay itself leaves a document as it was, redo list included
    let mut doc = e.doc.clone();
    doc.undo();
    let (steps, before) = (doc.steps(), annotate::render(&doc));
    doc.replay(|_| ());
    assert_eq!(doc.steps(), steps);
    assert_eq!(annotate::render(&doc).rgba, before.rgba);
}

#[test]
fn edit_frames_are_fitted_to_the_chosen_size_as_they_are_drawn() {
    let mut s = drawn(2);
    let how = export::Walkthrough {
        max_edge: 150,
        ..export::Walkthrough::default()
    };
    let frames = edit_frames_of(&mut s, how);
    assert!(frames.iter().all(|f| (f.width, f.height) == (150, 100)));
    // a video's Original is at most 4096 on its long edge
    let video = export::Walkthrough {
        movie: export::Movie::Mp4,
        max_edge: 0,
        ..how
    };
    assert_eq!(video.edge(), export::VIDEO_MAX_EDGE);
    assert_eq!(
        export::Walkthrough { max_edge: 0, ..how }.edge(),
        0,
        "a GIF keeps its size"
    );
}

#[test]
fn a_long_history_is_sampled_to_at_most_the_frame_limit_keeping_both_ends() {
    let mut s = drawn(45);
    let frames = edit_frames_of(&mut s, export::Walkthrough::default());
    assert_eq!(frames.len(), export::EDIT_FRAMES);
    assert_eq!(frames[0].rgba, crop(300, 200).rgba);
    let now = annotate::render(&s.editor.as_ref().unwrap().doc);
    assert_eq!(
        frames.last().unwrap().rgba,
        now.rgba,
        "ends on the current picture"
    );
}

// ---- Studio v3: the frame (STUDIO_V3_SPEC §3 to §7) ----

/// v3.1: at the smallest window (960 x 600) the sidebar stays and the stage keeps room.
#[test]
fn v3_the_sidebar_stays_at_the_smallest_window() {
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Resized(super::MIN));
    assert!(s.sidebar_shown());
    let stage = s.stage();
    assert!(stage.width > 560.0 && stage.height > 300.0, "{stage:?}");
}

/// §6: Alt opens File, the arrows walk, Enter picks, Esc closes.
#[test]
fn v3_the_menu_bar_is_keyboard_driven() {
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::MenuKey(Nav::Alt));
    assert_eq!(s.menu, Some((Menu::File, 0)));
    let _ = s.update(Msg::MenuKey(Nav::Next));
    assert_eq!(s.menu.map(|m| m.0), Some(Menu::Edit));
    let _ = s.update(Msg::MenuKey(Nav::Prev));
    let _ = s.update(Msg::MenuKey(Nav::Prev));
    assert_eq!(s.menu.map(|m| m.0), Some(Menu::View));
    // View > Left panel
    assert!(matches!(s.update(Msg::MenuKey(Nav::Enter)), Outcome::Panels(_, p) if !p.library));
    assert!(s.menu.is_none());
    // Esc closes an open menu before anything else
    let _ = s.update(Msg::Menu(Some(Menu::File)));
    let _ = s.update(Msg::Shortcuts(true));
    let _ = s.update(Msg::Cancel);
    assert!(s.menu.is_none() && s.shortcuts);
    let _ = s.update(Msg::Cancel);
    assert!(!s.shortcuts);
}

/// §6: File > Settings and New snip go to the host; Close asks about unsaved work.
#[test]
fn v3_file_menu_rows_reach_the_host() {
    let mut s = studio_on(640, 400);
    let pick = |s: &mut Studio, name: &str| {
        let rows = menu_bar::items(Menu::File, s);
        let at = rows.iter().position(|r| r.label == name).unwrap();
        s.update(Msg::MenuPick(Menu::File, at))
    };
    assert!(matches!(pick(&mut s, "Preferences…"), Outcome::None));
    assert!(s.prefs.is_some());
    let _ = s.update(Msg::Cancel);
    assert!(matches!(pick(&mut s, "New snip"), Outcome::NewSnip));
    draw(&mut s);
    assert!(matches!(pick(&mut s, "Close"), Outcome::None));
    assert!(s.asking());
}

/// Golden images of the frame: the picture with a drawing selected, menus, the
/// shortcuts sheet, narrow, empty, the panels' other states and the small
/// sheets, in Sapphire and Daylight, English and Arabic.
#[test]
fn v3_the_frame_matches_its_golden_images() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        for ar in [false, true] {
            let _lang = ar.then(crate::shared::kit::testing::arabic);
            let tag = format!("{}{}", theme.id, if ar { "-ar" } else { "" });
            // a rectangle selected with the Select tool: the bar shows ITS groups
            let mut s = two_boxes();
            let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
            let name = format!("studio3-selected-{tag}");
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &[], false)
            }));
            let _ = s.update(Msg::Menu(Some(Menu::View)));
            let name = format!("studio3-view-menu-{tag}");
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &[], false)
            }));
            let _ = s.update(Msg::Menu(Some(Menu::File)));
            let name = format!("studio3-file-menu-{tag}");
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &[], false)
            }));
            let _ = s.update(Msg::Menu(None));
            let _ = s.update(Msg::Shortcuts(true));
            let name = format!("studio3-shortcuts-{tag}");
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &[], false)
            }));
            let empty = Studio::new(docked());
            let name = format!("studio3-empty-{tag}");
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&empty, &[], false)
            }));
        }
        let mut n = studio_on(640, 400);
        let narrow = Size::new(960.0, 600.0);
        let _ = n.update(Msg::Resized(narrow));
        let name = format!("studio3-narrow-{}", theme.id);
        failed.extend(golden::check("screens", &name, narrow, &[1.0], || {
            view(&n, &[], false)
        }));
        // v3.1: a popover (Actions, Position) and the Steps sidebar's walkthrough panel
        let mut s = two_boxes();
        let _ = s.update(Msg::Editor(Message::SelectAll));
        for (pop, tag) in [
            (popovers::Pop::Actions, "actions"),
            (popovers::Pop::Position, "position"),
        ] {
            let _ = s.update(Msg::Pop(Some(pop)));
            let name = format!("studio31-{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &[], false)
            }));
            let _ = s.update(Msg::Pop(Some(pop)));
        }
        let _ = s.update(Msg::ExportWalkthrough);
        let name = format!("studio31-export-{}", theme.id);
        failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
        let r = Restorable {
            name: "Screenshot 2026-11-19".into(),
            size: (1920, 1080),
            png: Vec::new(),
            notes: String::new(),
        };
        let name = format!("studio3-restore-{}", theme.id);
        failed.extend(golden::check(
            "screens",
            &name,
            Size::new(560.0, 200.0),
            &[1.0],
            || sheets::restore(&r, Msg::Menu(None), Msg::Menu(None), Msg::Menu(None), false),
        ));
        let mut c = studio_on(640, 400);
        draw(&mut c);
        if let Some(e) = &mut c.editor {
            e.doc.selected = Some(0);
        }
        let entries = editing::object_menu(c.editor.as_ref().unwrap());
        let name = format!("studio3-context-{}", theme.id);
        failed.extend(golden::check(
            "screens",
            &name,
            Size::new(260.0, 340.0),
            &[1.0],
            || sheets::context(&entries),
        ));
    });
}

/// §5: the right-click menu follows the selection (object rows, else the
/// picture's); a row acts through the editor; Esc closes it first.
#[test]
fn v3_right_click_menu_acts_on_the_selection() {
    let mut s = two_boxes();
    // empty picture: the picture's rows
    let _ = s.update(Msg::Pointer(Point::new(600.0, 40.0)));
    let _ = s.update(Msg::ContextOpen);
    let picture_rows = s.context.as_ref().unwrap().entries.len();
    let _ = s.update(Msg::Cancel);
    assert!(s.context.is_none());
    // on the second box (select it first, as a click would): the object's rows
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    let _ = s.update(Msg::ContextOpen);
    // (the pointer is over nothing, so the menu is the picture's again)
    assert_eq!(s.context.as_ref().unwrap().entries.len(), picture_rows);
    let _ = s.update(Msg::Cancel);
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    let entries = editing::object_menu(s.editor.as_ref().unwrap());
    assert_ne!(
        entries.len(),
        picture_rows,
        "object rows, not the picture's"
    );
    s.context = Some(Context {
        at: Point::ORIGIN,
        entries,
    });
    let at = s
        .context
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .position(|r| r.action == editing::Action::Delete)
        .unwrap();
    let _ = s.update(Msg::ContextPick(at));
    assert!(s.context.is_none());
    assert_eq!(s.editor.as_ref().unwrap().doc.items.len(), 1);
}

/// Board 20: Discard drops the kept copy; Restore with none is nothing.
#[test]
fn v3_restore_sheet_buttons() {
    let mut s = studio_on(640, 400);
    assert!(matches!(s.update(Msg::Restore), Outcome::None));
    s.restorable = Some(Restorable {
        name: "x".into(),
        size: (1, 1),
        png: Vec::new(),
        notes: String::new(),
    });
    let _ = s.update(Msg::DiscardCopy);
    assert!(s.restorable.is_none());
}

/// §7: Space + drag pans the stage (the scroll moves against the drag); the
/// drag ends with the button; a text being typed keeps its spaces.
#[test]
fn v3_space_drag_pans_the_stage() {
    let mut s = studio_on(3840, 2160);
    let _ = s.update(Msg::Scale(1.0));
    let _ = s.update(Msg::Actual);
    let _ = s.update(Msg::Scrolled(Vector::new(300.0, 200.0)));
    let _ = s.update(Msg::Space(true));
    assert!(s.space);
    // moving without a press scrolls nothing
    assert!(matches!(
        s.update(Msg::Pointer(Point::new(50.0, 50.0))),
        Outcome::None
    ));
    let _ = s.update(Msg::PanStart);
    let Outcome::ScrollTo(to) = s.update(Msg::Pointer(Point::new(30.0, 20.0))) else {
        panic!("a drag scrolls");
    };
    assert_eq!((to.x, to.y), (320.0, 230.0));
    let _ = s.update(Msg::PanEnd);
    assert!(matches!(
        s.update(Msg::Pointer(Point::new(0.0, 0.0))),
        Outcome::None
    ));
    let _ = s.update(Msg::Space(false));
    assert!(!s.space);
}

/// §7: Ctrl+Shift+C copies the WHOLE picture, even with an object selected
/// (Ctrl+C then copies the object); the keys come from the editor's table.
#[test]
fn v3_ctrl_shift_c_copies_the_whole_picture() {
    use iced::keyboard::{Key, Modifiers};
    let mut s = studio_on(640, 400);
    draw(&mut s);
    s.editor.as_mut().unwrap().doc.selected = Some(0);
    let e = s.editor.as_ref().unwrap();
    let key = Key::Character("c".into());
    let both = Modifiers::CTRL | Modifiers::SHIFT;
    let whole = e.studio_key(&key, Some('c'), both);
    assert!(matches!(whole, Some(Message::Copy)));
    assert!(matches!(
        e.studio_key(&key, Some('c'), Modifiers::CTRL),
        Some(Message::CopySelection)
    ));
    let Outcome::Copy(crop) = s.update(Msg::Editor(whole.unwrap())) else {
        panic!("copies");
    };
    assert_eq!((crop.width, crop.height), (640, 400));
}

/// §6: the Edit menu has every row (its keys are the editor's own) and Arrange
/// opens a second menu; View has the Snap and Pixel grid checks.
#[test]
fn v3_edit_and_view_menus_are_complete() {
    let mut s = studio_on(640, 400);
    draw(&mut s);
    s.editor.as_mut().unwrap().doc.selected = Some(0);
    let edit = menu_bar::items(Menu::Edit, &s);
    let names: Vec<_> = edit.iter().map(|r| r.label).collect();
    for want in [
        "Undo",
        "Redo",
        "Cut",
        "Copy",
        "Paste",
        "Duplicate",
        "Delete",
        "Select all",
        "Deselect",
        "Arrange",
    ] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    let cut = edit.iter().find(|r| r.label == "Cut").unwrap();
    assert_eq!(cut.keys, "Ctrl+X");
    let arrange = edit.iter().position(|r| r.label == "Arrange").unwrap();
    assert_eq!(edit[arrange].sub, Some(Sub::Arrange));
    let _ = s.update(Msg::Menu(Some(Menu::Edit)));
    let _ = s.update(Msg::MenuPick(Menu::Edit, arrange));
    assert_eq!(s.sub, Some(Sub::Arrange));
    assert!(s.menu.is_some(), "the menu stays open for its second menu");
    assert_eq!(menu_bar::sub_items(Sub::Arrange, &s).len(), 4);
    let _ = s.update(Msg::Cancel);
    assert!(
        s.sub.is_none() && s.menu.is_some(),
        "Esc closes the second menu first"
    );
    let view = menu_bar::items(Menu::View, &s);
    assert!(view.iter().any(|r| r.label == "Snap to edges and objects"));
    assert!(view.iter().any(|r| r.label == "Pixel grid at high zoom"));
}

/// C3: Ctrl+= / Ctrl+- zoom the picture and Ctrl+0 fits it; the UI size has no key but is
/// in the View menu and Preferences > Appearance.
#[test]
fn c3_ctrl_zoom_keys_move_the_picture_and_the_ui_size_has_menu_rows() {
    assert_eq!(keys::of(keys::Cmd::ZoomIn), "Ctrl+=");
    assert_eq!(keys::of(keys::Cmd::ZoomOut), "Ctrl+-");
    assert_eq!(keys::of(keys::Cmd::Fit), "Ctrl+0");
    assert!(matches!(keys::Cmd::ZoomIn.msg(), Msg::ZoomIn));
    assert!(matches!(keys::Cmd::Fit.msg(), Msg::Fit));
    let s = studio_on(640, 400);
    let view = menu_bar::items(Menu::View, &s);
    for l in [
        "Interface larger",
        "Interface smaller",
        "Interface size: reset",
    ] {
        assert!(view.iter().any(|r| r.label == l), "{l}");
    }
    // the Preferences row sets the size and the Studio keeps it
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::OpenPrefs);
    let out = s.update(Msg::Prefs(preferences::Msg::UiZoom(125)));
    assert!(matches!(out, Outcome::Panels(_, p) if p.ui_zoom == 125));
    assert_eq!(s.ui_zoom, 125);
}

/// C3: Ctrl+Shift+= is zoom in's alias in the key table (it acts, and no other command may take
/// it) while zoom in keeps its key; rebinding zoom in moves both.
#[test]
fn c3_ctrl_shift_equals_is_zoom_ins_alias() {
    use super::keys::{Cmd, Taken, ZOOM_IN_ALIAS, command_for, taken};
    assert_eq!(command_for(ZOOM_IN_ALIAS), Some(Cmd::ZoomIn));
    assert_eq!(
        taken(Cmd::Fit, ZOOM_IN_ALIAS),
        Some(Taken::Command(Cmd::ZoomIn))
    );
    assert!(taken(Cmd::ZoomIn, ZOOM_IN_ALIAS).is_none());
    let mut cfg = config::Config::default();
    cfg.studio
        .shortcuts
        .insert("zoom_in".into(), "Ctrl+Alt+9".into());
    config::with_test_config(cfg, || {
        assert_eq!(command_for("Ctrl+Alt+9"), Some(Cmd::ZoomIn));
        assert_eq!(
            command_for(ZOOM_IN_ALIAS),
            None,
            "rebound: the alias goes too"
        );
    });
}

/// C3: a config from before (the UI size had keys `ui_larger` / `ui_smaller` / `ui_reset`) loads
/// without a warning; the three lines are ignored.
#[test]
fn c3_an_old_config_with_the_ui_size_keys_loads_without_error() {
    let text = "[studio.shortcuts]\nui_larger = \"Ctrl+=\"\nui_smaller = \"Ctrl+-\"\nui_reset = \"Ctrl+0\"\nopen = \"Ctrl+O\"\n";
    let (cfg, warnings) = config::Config::parse(text);
    assert!(warnings.is_empty(), "{warnings:?}");
    config::with_test_config(cfg, || {
        assert_eq!(keys::of(keys::Cmd::ZoomIn), "Ctrl+=");
        assert_eq!(keys::of(keys::Cmd::Fit), "Ctrl+0");
    });
}

/// §5: Open recent lists the recent files; the title carries the document name;
/// Clear empties the list.
#[test]
fn v3_recent_menu_title_and_clear() {
    let mut s = studio_on(64, 64);
    assert_eq!(s.title, "Untitled - OpenCircle Studio");
    s.next_name(std::path::Path::new("/x/holiday.png"));
    s.open(Picture::Crop(crop(8, 8)));
    assert_eq!(s.title, "holiday.png - OpenCircle Studio");
    let h = image::Handle::from_rgba(1, 1, vec![0; 4]);
    let _ = s.update(Msg::Recent(
        vec![(PathBuf::from("/x/a.png"), h)],
        vec![String::new()],
    ));
    let rows = menu_bar::sub_items(Sub::Recent, &s);
    assert_eq!(rows[0].label, "a.png");
    assert!(matches!(s.update(Msg::ClearRecent), Outcome::ClearRecent));
    assert!(s.recent.is_empty());
}

/// §6: with Snap on, a rectangle drawn up to the picture's edge lands on it, a
/// moved one lands its edge on another's; with Snap off nothing moves.
#[test]
fn v3_snap_lands_drawing_and_moving_on_edges() {
    let draw_rect = |snap: bool| {
        let mut s = studio_on(640, 400);
        if s.snap != snap {
            let _ = s.update(Msg::Snap);
        }
        assert_eq!(s.snap, snap);
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
        for m in [
            Message::Press(Point::new(100.0, 100.0)),
            Message::Move(Point::new(300.0, 200.0)),
            Message::Move(Point::new(637.0, 200.0)),
            Message::Release,
        ] {
            let _ = s.update(Msg::Editor(m));
        }
        let item = &s.editor.as_ref().unwrap().doc.items[0];
        let f = item.frame();
        // (the picture cannot grow: the line's own half-width stays inside, #200)
        f.x + f.width + item.width / 2.0
    };
    assert!(
        (draw_rect(true) - 639.0).abs() < 0.5,
        "lands on the edge, 1 px in for its line"
    );
    assert!(
        (draw_rect(false) - 639.0).abs() < 0.5,
        "off: where the pointer was (held inside by its line)"
    );
}

/// Two rectangles on a 640 x 400 picture (the first at the back).
fn two_boxes() -> Studio {
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Resized(SIZE));
    let _ = s.update(Msg::Actual);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
    for (a, b) in [
        ((20.0, 20.0), (120.0, 90.0)),
        ((300.0, 200.0), (400.0, 280.0)),
    ] {
        for m in [
            Message::Press(Point::new(a.0, a.1)),
            Message::Move(Point::new(b.0 - 20.0, b.1 - 20.0)),
            Message::Move(Point::new(b.0, b.1)),
            Message::Release,
        ] {
            let _ = s.update(Msg::Editor(m));
        }
    }
    let _ = s.update(Msg::Editor(Message::Deselect));
    assert_eq!(s.editor.as_ref().unwrap().doc.items.len(), 2);
    s
}

/// A right-click selects the object under the pointer first.
#[test]
fn v3_right_click_selects_under_the_pointer() {
    let mut s = two_boxes();
    // over the second box's edge (its stroke), then over empty picture
    let corner = |s: &Studio, p: Point| {
        let e = s.editor.as_ref().unwrap();
        let (room, shown) = (s.stage(), e.size());
        let c = Vector::new(
            ((room.width - 2.0).max(shown.width) - shown.width) / 2.0,
            ((room.height - 2.0).max(shown.height) - shown.height) / 2.0,
        );
        Point::new(p.x * e.zoom + c.x, p.y * e.zoom + c.y)
    };
    let on = corner(&s, Point::new(300.0, 240.0));
    let _ = s.update(Msg::Pointer(on));
    let _ = s.update(Msg::ContextOpen);
    assert_eq!(s.editor.as_ref().unwrap().selection(), vec![1]);
    let _ = s.update(Msg::Cancel);
    let off = corner(&s, Point::new(600.0, 40.0));
    let _ = s.update(Msg::Pointer(off));
    let _ = s.update(Msg::ContextOpen);
    assert!(s.editor.as_ref().unwrap().selection().is_empty());
}

/// Recovery keeps the drawings: the JSON round-trips onto the picture.
#[test]
fn v3_recovery_notes_restore_the_drawings() {
    let s = two_boxes();
    let json = s.editor.as_ref().unwrap().annotations_json();
    let mut t = studio_on(640, 400);
    assert!(t.editor.as_mut().unwrap().restore_annotations(&json));
    assert_eq!(t.editor.as_ref().unwrap().doc.items.len(), 2);
}

/// Alt opens the menu bar on its release when nothing came between (Windows'
/// way); another key or a click in between, and it opens nothing.
#[test]
fn v3_alt_opens_the_menu_bar_on_release_only() {
    let mut s = studio_on(64, 64);
    let _ = s.update(Msg::AltDown);
    assert!(s.menu.is_none(), "not on the press");
    let _ = s.update(Msg::AltUp);
    assert_eq!(s.menu, Some((Menu::File, 0)));
    let _ = s.update(Msg::Menu(None));
    // Alt+A (a key between): nothing opens
    let _ = s.update(Msg::AltDown);
    let _ = s.update(Msg::AltBreak);
    let _ = s.update(Msg::AltUp);
    assert!(s.menu.is_none());
    // a click between
    let _ = s.update(Msg::AltDown);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Pen)));
    let _ = s.update(Msg::AltUp);
    assert!(s.menu.is_none());
    // pointer moves and modifier changes do not count as another key
    let _ = s.update(Msg::AltDown);
    let _ = s.update(Msg::Mods(false, false));
    let _ = s.update(Msg::AltUp);
    assert!(s.menu.is_some());
}

/// Restore with unsaved work on screen asks first; Cancel keeps the copy (the
/// sheet comes back), Don't save restores it.
#[test]
fn v3_restore_waits_for_the_unsaved_question_and_cancel_keeps_the_copy() {
    let png = crate::shared::picture::encode_png(4, 3, &[200; 4 * 3 * 4]).unwrap();
    let notes = empty_notes();
    let copy = || Restorable {
        name: "Unsaved".into(),
        size: (4, 3),
        png: png.clone(),
        notes: notes.clone(),
    };
    let mut s = studio_on(64, 64);
    draw(&mut s);
    s.restorable = Some(copy());
    let _ = s.update(Msg::Restore);
    assert!(s.asking(), "the question comes first");
    assert!(s.restorable.is_none() && s.restoring.is_some());
    assert_eq!(
        s.editor.as_ref().unwrap().doc.image.width,
        64,
        "nothing replaced yet"
    );
    let _ = s.update(Msg::Cancel);
    assert!(s.restorable.is_some(), "Cancel gives the copy back");
    assert!(s.restoring.is_none() && !s.asking());
    let _ = s.update(Msg::Restore);
    let _ = s.update(Msg::DontSave);
    let e = s.editor.as_ref().unwrap();
    assert_eq!((e.doc.image.width, e.doc.image.height), (4, 3), "restored");
    assert!(s.restorable.is_none() && s.restoring.is_none());
}

/// The clock only runs while an edit waits for its copy; the copy's bookkeeping
/// ends when the picture is clean again.
#[test]
fn v3_the_recovery_clock_runs_only_while_an_edit_waits() {
    let mut s = studio_on(64, 64);
    assert!(!s.waiting());
    s.quiet.changed(std::time::Instant::now());
    assert!(s.waiting());
    s.drop_copy();
    assert!(!s.waiting());
}

/// The right-click menu stays inside the window at the right / bottom edges, and
/// in Arabic is placed from the mirrored layout.
#[test]
fn v3_context_menu_stays_inside_the_window_and_mirrors() {
    for arabic in [false, true] {
        let _lang = arabic.then(crate::shared::kit::testing::arabic);
        let mut s = studio_on(640, 400);
        let _ = s.update(Msg::Resized(SIZE));
        let stage = s.stage();
        let _ = s.update(Msg::Pointer(Point::new(
            stage.width - 2.0,
            stage.height - 2.0,
        )));
        let _ = s.update(Msg::ContextOpen);
        let c = s.context.as_ref().unwrap();
        let w = crate::shared::kit::option_list::MENU_WIDTH;
        assert!(
            c.at.x >= 0.0 && c.at.x + w <= SIZE.width,
            "{arabic}: x {}",
            c.at.x
        );
        assert!(
            c.at.y >= 0.0 && c.at.y < SIZE.height,
            "{arabic}: y {}",
            c.at.y
        );
        let _ = s.update(Msg::Cancel);
        let _ = s.update(Msg::Pointer(Point::new(10.0, 10.0)));
        let _ = s.update(Msg::ContextOpen);
        let at = s.context.as_ref().unwrap().at;
        let left = match arabic {
            true => 0.0,
            false => RAIL + 2.0,
        };
        assert!(
            (at.x - (left + 10.0)).abs() < 1.5,
            "{arabic}: {} vs {}",
            at.x,
            left + 10.0
        );
    }
}

/// ONE source for key names: the editor's right-click rows and the menus read
/// the same constants (`keys`).
#[test]
fn v3_key_names_have_one_source() {
    use editing::Action as A;
    let mut s = studio_on(64, 64);
    draw(&mut s);
    s.editor.as_mut().unwrap().doc.selected = Some(0);
    let e = s.editor.as_ref().unwrap();
    let rows: Vec<_> = editing::object_menu(e)
        .into_iter()
        .chain(editing::picture_menu(e))
        .collect();
    let want = |a: A| match a {
        A::Cut => keys::CUT,
        A::Copy => keys::COPY,
        A::Paste => keys::PASTE,
        A::Duplicate => keys::DUPLICATE,
        A::Delete => keys::DELETE,
        A::Forward => keys::FORWARD,
        A::Backward => keys::BACKWARD,
        A::Front => keys::FRONT,
        A::Back => keys::BACK,
        A::SelectAll => keys::SELECT_ALL,
        A::CopyPicture => keys::COPY_PICTURE,
        A::Fit => keys::FIT,
        A::Shortcuts => keys::SHORTCUTS,
        A::HideShape | A::HideOutline => "", // no key
    };
    for r in rows {
        assert_eq!(
            keys::chord_caps(r.keys).join("+"),
            want(r.action),
            "{:?}",
            r.action
        );
    }
}

/// While a menu, the right-click menu, a sheet or the narrow overlay is up the
/// canvas takes no key (the real path: the editor's `key` answers nothing, so the
/// window's own key map sees the arrows and Esc); closing it gives the keys back.
#[test]
fn v3_open_menus_and_sheets_block_the_canvas_keys() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut s = two_boxes();
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    let arrow = |s: &Studio| {
        s.editor.as_ref().unwrap().key(
            &Key::Named(Named::ArrowLeft),
            None,
            Modifiers::empty(),
            None,
        )
    };
    assert!(arrow(&s).is_some(), "an open canvas nudges the selection");
    let ups: [(Msg, Msg); 3] = [
        (Msg::Menu(Some(Menu::Edit)), Msg::Menu(None)),
        (Msg::Shortcuts(true), Msg::Shortcuts(false)),
        (Msg::ContextOpen, Msg::Cancel),
    ];
    for (up, down) in ups {
        let _ = s.update(up);
        assert!(s.editor.as_ref().unwrap().keys_blocked, "{:?}", s.menu);
        assert!(
            arrow(&s).is_none(),
            "the canvas takes no key while it is up"
        );
        let _ = s.update(down);
        assert!(!s.editor.as_ref().unwrap().keys_blocked);
    }
    // the menu's own Esc: closes the menu, never touches the selection
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    let _ = s.update(Msg::Menu(Some(Menu::File)));
    let _ = s.update(Msg::Cancel);
    assert!(s.menu.is_none());
    assert_eq!(s.editor.as_ref().unwrap().selection(), vec![1]);
}

/// Add to steps (the Properties button, Ctrl+Enter) hands the host the picture.
#[test]
fn v3_add_to_steps_reaches_the_host() {
    let mut s = two_boxes();
    assert!(matches!(
        s.update(Msg::Editor(Message::AddStep)),
        Outcome::AddStep(c) if (c.width, c.height) == (640, 400)
    ));
}

/// A typed X / Y / W / H is made on Enter or when the user moves on to something
/// that is not typing: the key the field took (`AltBreak` follows every key) must
/// not commit it on each keystroke.
#[test]
fn v3_typed_box_is_not_committed_by_the_keys_it_is_typed_with() {
    let mut s = two_boxes();
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    let depth = s.editor.as_ref().unwrap().doc.depth();
    for typed in ["1", "12", "120"] {
        let _ = s.update(Msg::BoundsEdit(2, typed.into()));
        let _ = s.update(Msg::AltBreak); // (the key the field took)
    }
    let _ = s.update(Msg::BoundsEdit(2, String::new()));
    let _ = s.update(Msg::AltBreak);
    // a Space typed in the field: its release is typing too
    let _ = s.update(Msg::Space(false));
    assert_eq!(
        s.editor.as_ref().unwrap().doc.depth(),
        depth,
        "nothing made while typing"
    );
    assert!(s.bounds_draft.is_some());
    let _ = s.update(Msg::BoundsEdit(2, "150".into()));
    let _ = s.update(Msg::BoundsCommit);
    let e = s.editor.as_ref().unwrap();
    assert_eq!(e.doc.depth(), depth + 1, "one undo step");
    assert_eq!(e.selection_bounds().unwrap().width.round(), 150.0);
    // moving on to another action also commits
    let _ = s.update(Msg::BoundsEdit(3, "90".into()));
    let _ = s.update(Msg::Editor(Message::Deselect));
    assert_eq!(s.editor.as_ref().unwrap().doc.depth(), depth + 2);
}

/// Properties' style groups (several objects selected) show the whole swatch row,
/// not one swatch per selected object.
#[test]
fn v3_several_selected_objects_show_every_swatch() {
    let mut s = two_boxes();
    let _ = s.update(Msg::Editor(Message::SelectAll));
    assert_eq!(s.editor.as_ref().unwrap().selection().len(), 2);
    let e = s.editor.as_ref().unwrap();
    let colours = options_bar::colour_group(e, 6);
    let picked = (0..8)
        .filter(|i| {
            let at = Point::new(2.0 + 14.0 + 30.0 * *i as f32, 16.0);
            let clicked = crate::shared::kit::testing::shot::click(
                colours_again(e),
                Size::new(400.0, 40.0),
                at,
            );
            clicked
                .iter()
                .any(|m| matches!(m, Msg::Editor(Message::Colour(_))))
        })
        .count();
    let _ = colours;
    assert_eq!(picked, 6, "six swatches, however many objects");
}

fn colours_again(e: &annotate::Annotator) -> iced::Element<'_, Msg> {
    options_bar::colour_group(e, 6)
}

/// A kept copy in this test's own folder, as the window's `Restorable` (`notes`
/// are the drawings' JSON, or whatever is given).
fn kept_copy(tag: &str, notes: &str) -> Restorable {
    recovery::use_dir(crate::shared::paths::test_dir(tag).join("studio-recovery"));
    let png = crate::shared::picture::encode_png(4, 3, &[200; 4 * 3 * 4]).unwrap();
    assert!(recovery::write_in(&recovery::dir(), true, &png, notes).unwrap());
    let p = recovery::pending().unwrap();
    Restorable {
        name: "Unsaved".into(),
        size: (4, 3),
        png: p.png,
        notes: p.notes,
    }
}

fn empty_notes() -> String {
    studio_on(4, 3).editor.as_ref().unwrap().annotations_json()
}

/// Restore does not delete the copy on load: the restored document is unsaved
/// (it asks before it is lost), a new copy is scheduled, and the old one goes only
/// when it is saved.
#[test]
fn v3_restore_keeps_the_copy_until_the_document_is_saved() {
    let r = kept_copy("w87-restore-keep", &empty_notes());
    let mut s = studio_on(64, 64);
    s.restorable = Some(r);
    let _ = s.update(Msg::Restore);
    assert_eq!(s.editor.as_ref().unwrap().doc.image.width, 4, "restored");
    assert!(recovery::pending().is_some(), "the copy stays on disk");
    assert!(s.dirty(), "the restored document is unsaved");
    assert!(s.waiting(), "a new copy is scheduled");
    s.saved();
    assert!(recovery::pending().is_none(), "saved: the copy goes");
}

/// A copy whose drawings can't be read is kept and the sheet says so.
#[test]
fn v3_a_copy_that_cannot_be_restored_is_kept_and_says_so() {
    let r = kept_copy("w87-restore-fail", "not json");
    let mut s = studio_on(64, 64);
    s.restorable = Some(r);
    let _ = s.update(Msg::Restore);
    assert!(s.restorable.is_some() && s.restore_failed);
    assert!(recovery::pending().is_some());
}

/// Reopen last stands down for any picture that was asked for, even before it has
/// been read (a file from the command line, a drop, a recent file, Open, Paste).
#[test]
fn v3_a_requested_file_always_wins_over_reopen_last() {
    let h = image::Handle::from_rgba(1, 1, vec![0; 4]);
    let recent = || vec![(PathBuf::from("/x/last.png"), h.clone())];
    let mut fresh = Studio::new(docked());
    let _ = fresh.update(Msg::Recent(recent(), vec![String::new()]));
    assert_eq!(fresh.reopen(true), Some(PathBuf::from("/x/last.png")));
    assert_eq!(fresh.reopen(false), None, "the setting off");
    type Ask = Box<dyn Fn(&mut Studio)>;
    let asks: [Ask; 5] = [
        Box::new(|s| s.next_name(std::path::Path::new("/x/asked.png"))),
        Box::new(|s| drop(s.update(Msg::Picked(PathBuf::from("/x/d.png"))))),
        Box::new(|s| drop(s.update(Msg::OpenRecent(PathBuf::from("/x/r.png"))))),
        Box::new(|s| drop(s.update(Msg::Open))),
        Box::new(|s| drop(s.update(Msg::Paste))),
    ];
    for ask in asks {
        let mut s = Studio::new(docked());
        ask(&mut s);
        let out = s.update(Msg::Recent(recent(), vec![String::new()]));
        assert!(
            matches!(out, Outcome::None),
            "the list arriving brings nothing back"
        );
        assert_eq!(s.reopen(true), None);
    }
}

/// The canvas's keys follow every state change the host makes without a message:
/// a card, the unsaved question and its answer.
#[test]
fn v3_blocked_keys_follow_the_hosts_calls_too() {
    let blocked = |s: &Studio| s.editor.as_ref().unwrap().keys_blocked;
    let mut s = studio_on(64, 64);
    draw(&mut s);
    assert!(!blocked(&s));
    s.set_card(Some(Fetching::cutout()));
    assert!(blocked(&s), "a download card is up");
    s.set_card(None);
    assert!(!blocked(&s));
    let _ = s.close_requested();
    assert!(
        s.asking() && blocked(&s),
        "the unsaved question has the keys"
    );
    let _ = s.proceed();
    assert!(!blocked(&s), "answered");
    draw(&mut s);
    let _ = s.quit_requested();
    assert!(blocked(&s));
    s.saved();
    let _ = s.proceed();
    assert!(!blocked(&s));
}

/// "Grow the canvas past the edge" saying no gives a plain pill; a grow that is made
/// refreshes the see-through flag (the new area has no pixels yet).
#[test]
fn v3_a_refused_or_made_grow_is_said_and_refreshes_the_transparent_flag() {
    let mut s = two_boxes();
    s.editor.as_mut().unwrap().grow_canvas = true;
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    let r = s.editor.as_ref().unwrap().selection_bounds().unwrap();
    let far = Rectangle::new(Point::new(1.0e9, 0.0), r.size());
    let out = s.update(Msg::Editor(Message::Bounds(far)));
    assert!(
        matches!(&out, Outcome::Say(w) if w == "The picture can't grow that far"),
        "{:?}",
        matches!(out, Outcome::Say(_))
    );
    // a small grow past the right edge: the canvas gets wider, the flag follows
    let r = s.editor.as_ref().unwrap().selection_bounds().unwrap();
    let past = Rectangle::new(Point::new(630.0, r.y), r.size());
    let _ = s.update(Msg::Editor(Message::Bounds(past)));
    let e = s.editor.as_ref().unwrap();
    assert!(e.doc.image.width > 640, "grown: {}", e.doc.image.width);
    assert_eq!(s.transparent, cut::has_alpha(&e.doc.image.rgba));
}

/// Esc closes the export sheet, then the restore sheet (Later: the copy stays).
#[test]
fn v3_esc_closes_the_export_and_restore_sheets() {
    let mut s = studio_on(64, 64);
    let _ = s.update(Msg::Export);
    assert!(s.exporting);
    let _ = s.update(Msg::Cancel);
    assert!(!s.exporting, "Esc closes the sheet");
    s.restorable = Some(Restorable {
        name: "x".into(),
        size: (1, 1),
        png: Vec::new(),
        notes: String::new(),
    });
    let _ = s.update(Msg::Cancel);
    assert!(
        s.later && s.restorable.is_some(),
        "Later: put away, the copy stays"
    );
}

/// Restyle through Properties: with the Select tool and a rectangle selected, a
/// swatch of the panel's own style groups recolours THAT object.
#[test]
fn v3_properties_restyle_the_selected_object() {
    let mut s = two_boxes();
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    assert_eq!(s.editor.as_ref().unwrap().tool, Tool::Select);
    let before = s.editor.as_ref().unwrap().doc.items[1].colour;
    let other = s.editor.as_ref().unwrap().doc.items[0].colour;
    let e = s.editor.as_ref().unwrap();
    // the second swatch of the first group (a swatch is 28 wide with 2 gaps and the 2 px pad)
    let at = Point::new(2.0 + 14.0 + 30.0, 16.0);
    let msgs = crate::shared::kit::testing::shot::click(
        options_bar::colour_group(e, 6),
        Size::new(400.0, 40.0),
        at,
    );
    let pick = msgs
        .into_iter()
        .find(|m| matches!(m, Msg::Editor(Message::Colour(_))))
        .unwrap();
    let _ = s.update(pick);
    let e = s.editor.as_ref().unwrap();
    assert_ne!(e.doc.items[1].colour, before, "the selected object took it");
    assert_eq!(e.doc.items[0].colour, other, "the other did not");
}

/// The shortcuts sheet fits a narrow window (960 x 600): it narrows and scrolls.
#[test]
fn v3_the_shortcuts_sheet_fits_a_small_window() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    let small = Size::new(960.0, 600.0);
    golden::each_theme(|theme, failed| {
        let mut s = studio_on(640, 400);
        let _ = s.update(Msg::Resized(small));
        let _ = s.update(Msg::Shortcuts(true));
        let name = format!("studio3-shortcuts-narrow-{}", theme.id);
        failed.extend(golden::check("screens", &name, small, &[1.0], || {
            view(&s, &[], false)
        }));
    });
}

/// The start screen shows the recent pictures as big thumbnails (Settings > Studio's
/// count); with none yet it shows no such list.
#[test]
fn v3_the_start_screen_lists_recent_pictures() {
    use crate::shared::kit::testing::shot;
    let mut s = Studio::new(docked());
    let words = |s: &Studio| shot::texts(start::view(s).map(|_| ()));
    assert!(
        !words(&s)
            .iter()
            .any(|t| t.eq_ignore_ascii_case("Recent pictures"))
    );
    let picture = iced::widget::image::Handle::from_rgba(1, 1, vec![9; 4]);
    let _ = s.update(Msg::Recent(
        vec![(std::path::PathBuf::from("/tmp/holiday.png"), picture)],
        vec![String::new()],
    ));
    let got = words(&s);
    assert!(
        got.iter()
            .any(|t| t.eq_ignore_ascii_case("Recent pictures")),
        "{got:?}"
    );
    assert!(got.iter().any(|t| t == "holiday.png"), "{got:?}");
}

// ---- Studio v3.1: the window, popovers, the Steps sidebar, input ----

fn click_view(s: &Studio, at: (f32, f32)) -> Vec<Msg> {
    crate::shared::kit::testing::shot::click(view(s, &[], false), SIZE, Point::new(at.0, at.1))
}

/// v3.1 item 6: a press anywhere outside an open menu closes it (before, it
/// stayed until its opener was pressed again); the press goes no further.
#[test]
fn v31_an_open_menu_closes_on_an_outside_click() {
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Menu(Some(Menu::Edit)));
    let got = click_view(&s, (700.0, 500.0));
    assert!(matches!(got.as_slice(), [Msg::Menu(None)]), "{got:?}");
    let _ = s.update(Msg::Menu(None));
    assert!(s.menu.is_none());
}

/// W106 (owner, 2026-11-25): with a menu open, a click on another title opens that menu at once
/// (it used to close the first and need a second click); the pointer over another title switches
/// to it (menu mode); a click on the open menu's own title or Esc closes it; F10 enters menu mode
/// and Left / Right walk the menus. Also in Arabic (the bar keeps its order).
#[test]
fn w106_the_menu_bar_switches_menus_by_click_and_hover() {
    use crate::shared::app_language::{self, Lang};
    use crate::shared::kit::testing::shot;
    for lang in [Lang::En, Lang::Ar] {
        app_language::set(lang);
        let title = |m: Menu| Point::new(super::menu_bar::offset(m) + 12.0, 20.0);
        let mut s = studio_on(640, 400);
        // no menu open: hovering a title does nothing
        let got = shot::hover(view(&s, &[], false), SIZE, title(Menu::Edit));
        assert!(got.is_empty(), "{lang:?}: {got:?}");
        let _ = s.update(Msg::Menu(Some(Menu::File)));
        // File open: a click on Edit opens Edit at once
        let got = click_view(&s, (title(Menu::Edit).x, title(Menu::Edit).y));
        // (the pointer arriving switches too, then the press: both say Edit, never a close)
        assert!(
            !got.is_empty() && got.iter().all(|m| matches!(m, Msg::Menu(Some(Menu::Edit)))),
            "{lang:?}: {got:?}"
        );
        // and the pointer over it switches
        let got = shot::hover(view(&s, &[], false), SIZE, title(Menu::View));
        assert!(
            got.iter().any(|m| matches!(m, Msg::Menu(Some(Menu::View)))),
            "{lang:?}: {got:?}"
        );
        // the open menu's own title closes it
        let got = click_view(&s, (title(Menu::File).x, title(Menu::File).y));
        assert!(
            matches!(got.as_slice(), [Msg::Menu(None)]),
            "{lang:?}: {got:?}"
        );
        // Esc closes; Left / Right walk the menus (wrapping)
        let _ = s.update(Msg::Menu(Some(Menu::Edit)));
        let _ = s.update(Msg::MenuKey(Nav::Next));
        assert_eq!(s.menu.map(|m| m.0), Some(Menu::View));
        let _ = s.update(Msg::MenuKey(Nav::Prev));
        let _ = s.update(Msg::MenuKey(Nav::Prev));
        assert_eq!(s.menu.map(|m| m.0), Some(Menu::File));
        let _ = s.update(Msg::Cancel);
        assert!(s.menu.is_none());
        // F10 and Alt enter menu mode
        let _ = s.update(Msg::MenuKey(Nav::Alt));
        assert_eq!(s.menu.map(|m| m.0), Some(Menu::File));
    }
    app_language::set(Lang::En);
}

/// v3.1 item 5: a popover closes on an outside press and on Esc; a press on its
/// own row acts.
#[test]
fn v31_a_popover_closes_on_an_outside_click_and_esc() {
    let mut s = two_boxes();
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Actions)));
    assert_eq!(s.popover, Some(popovers::Pop::Actions));
    let got = click_view(&s, (300.0, 600.0));
    assert!(matches!(got.as_slice(), [Msg::Pop(None)]), "{got:?}");
    // the "Cut out" row (the fifth) acts
    let got = click_view(&s, (1010.0, 247.0));
    assert!(got.iter().any(|m| matches!(m, Msg::CutOut)), "{got:?}");
    let _ = s.update(Msg::Cancel);
    assert!(s.popover.is_none());
    // the same opener twice closes it
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Position)));
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Position)));
    assert!(s.popover.is_none());
}

/// v3.1 item 2: the bar always shows every control; ones the tool can't use do
/// nothing (dimmed), the tool's own ones act.
#[test]
fn v31_the_options_bar_dims_what_the_tool_cannot_use() {
    let colours = |s: &Studio| {
        let e = s.editor.as_ref().unwrap();
        let size = Size::new(1227.0, 48.0);
        (0..130)
            .filter(|i| {
                let at = Point::new(60.0 + 8.0 * *i as f32, 24.0);
                let bar = options_bar::view(s, e);
                crate::shared::kit::testing::shot::click(bar, size, at)
                    .iter()
                    .any(|m| matches!(m, Msg::Editor(Message::Colour(_))))
            })
            .count()
    };
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Select)));
    assert_eq!(colours(&s), 0, "Select has no colour: dimmed, not hidden");
    let bar = options_bar::view(&s, s.editor.as_ref().unwrap());
    let words = crate::shared::kit::testing::shot::texts(bar);
    for w in ["Size", "Opacity", "Outline", "Fill", "Position", "Actions"] {
        assert!(words.iter().any(|t| t == w), "{w} shown: {words:?}");
    }
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
    assert!(colours(&s) > 3, "Rect takes a colour");
}

/// v3.1 item 1: the caption buttons keep their place (right) in Arabic too, the
/// rail sits at the window's edge and the Steps sidebar on the other side.
#[test]
fn v31_the_caption_buttons_stay_right_in_arabic() {
    let s = studio_on(640, 400);
    let is_close = |got: Vec<Msg>| {
        got.iter()
            .any(|m| matches!(m, Msg::Caption(Caption::Close)))
    };
    assert!(is_close(click_view(&s, (1256.0, 20.0))));
    let _ar = crate::shared::kit::testing::arabic();
    assert!(is_close(click_view(&s, (1256.0, 20.0))));
    assert!(!is_close(click_view(&s, (24.0, 20.0))));
    // the rail (Rectangle tool) is at the right edge, the sidebar's Add at the left
    let rect = click_view(&s, (1252.0, 192.0));
    assert!(
        rect.iter()
            .any(|m| matches!(m, Msg::Editor(Message::Tool(Tool::Rect)))),
        "{rect:?}"
    );
}

/// v3.1 item 3: Add pictures asks for several files; files dropped together (or onto an
/// open picture) become steps; a single drop on the empty Studio opens it.
#[test]
fn v31_pictures_join_the_steps() {
    let mut s = studio_on(640, 400);
    assert!(matches!(s.update(Msg::PickFiles), Outcome::PickFiles));
    let files = vec![PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.png")];
    assert!(matches!(s.update(Msg::AddFiles(files)), Outcome::AddPictures(v) if v.len() == 2));
    assert!(matches!(s.update(Msg::AddFiles(Vec::new())), Outcome::None));
    assert!(matches!(s.update(Msg::PasteStep), Outcome::PasteStep));
    // (the drops are judged in `v31_drops_follow_the_owner_rules`)
}

/// v3.1 review 5: one file dropped on an open picture is placed on it as a movable
/// picture; on the Steps sidebar it is a step; several files anywhere are all steps
/// (the empty Studio also opens the first); a single one on the empty Studio opens.
#[test]
fn v31_drops_follow_the_owner_rules() {
    let (a, b) = (PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.png"));
    let drop = |s: &mut Studio, files: &[&PathBuf]| {
        let mut last = Outcome::None;
        for f in files {
            last = s.update(Msg::Dropped((*f).clone()));
        }
        // (a settle that was voided by a later file does nothing)
        let n = files.len();
        assert!(
            matches!(
                s.update(Msg::DropSettle(n.saturating_sub(1))),
                Outcome::None
            ) || n == 1
        );
        let _ = last;
        s.update(Msg::DropSettle(n))
    };
    // open picture: one = placed
    let mut s = studio_on(640, 400);
    assert!(matches!(drop(&mut s, &[&a]), Outcome::Place(p) if p == a));
    // over the Steps sidebar: one = a step
    let _ = s.update(Msg::OverSidebar(true));
    assert!(matches!(drop(&mut s, &[&a]), Outcome::AddPictures(v) if v == vec![a.clone()]));
    let _ = s.update(Msg::OverSidebar(false));
    // several: all steps
    assert!(matches!(drop(&mut s, &[&a, &b]), Outcome::AddPictures(v) if v.len() == 2));
    // empty Studio: one opens; several open the first AND are all steps
    let mut e = Studio::new(docked());
    assert!(matches!(drop(&mut e, &[&a]), Outcome::Read(p) if p == a));
    assert!(matches!(drop(&mut e, &[&a, &b]), Outcome::OpenAndAdd(v) if v.len() == 2));
    // the Open dialog's file opens at once
    assert!(matches!(e.update(Msg::Picked(a.clone())), Outcome::Read(p) if p == a));
}

/// v3.1 review 6: the UI scale is remembered like the AI chat's text zoom.
#[test]
fn v31_the_ui_scale_is_remembered() {
    let mut s = studio_on(640, 400);
    let out = s.update(Msg::UiScale(1));
    assert!(matches!(out, Outcome::Panels(_, p) if p.ui_zoom == 110));
    let again = Studio::new(StudioConfig {
        ui_zoom: 125,
        ..StudioConfig::default()
    });
    assert_eq!(again.ui_zoom, 125);
}

/// v3.1 item 4: the empty Studio shows its recent pictures big, and a click opens one.
#[test]
fn v31_the_start_screen_opens_a_recent_picture() {
    let mut s = Studio::new(docked());
    let picture = iced::widget::image::Handle::from_rgba(1, 1, vec![9; 4]);
    let path = PathBuf::from("/tmp/holiday.png");
    let _ = s.update(Msg::Recent(
        vec![(path.clone(), picture)],
        vec![String::new()],
    ));
    let got: Vec<Msg> = (0..14 * 12)
        .flat_map(|i| {
            let at = (
                380.0 + 40.0 * (i % 14) as f32,
                400.0 + 30.0 * (i / 14) as f32,
            );
            click_view(&s, at)
        })
        .collect();
    assert!(
        got.iter()
            .any(|m| matches!(m, Msg::OpenRecent(p) if *p == path)),
        "{got:?}"
    );
}

/// v3.1 item 7: the wheel zooms by default (Ctrl+wheel too), the middle button pans,
/// Ctrl+= / Ctrl+- / Ctrl+0 scale the UI.
#[test]
fn v31_wheel_middle_button_and_ui_scale() {
    assert_eq!(
        crate::shared::config::StudioPrefs::default().wheel,
        crate::shared::config::StudioWheel::Zoom
    );
    let mut s = studio_on(1600, 900);
    let before = s.editor.as_ref().unwrap().zoom;
    let _ = s.update(Msg::Wheel {
        at: Point::new(300.0, 300.0),
        notches: 2.0,
    });
    assert!(s.editor.as_ref().unwrap().zoom > before);
    // the middle button: a move pans while it is down, none after
    assert!(matches!(
        s.update(Msg::Pointer(Point::new(10.0, 10.0))),
        Outcome::None
    ));
    let _ = s.update(Msg::Middle(true));
    assert!(matches!(
        s.update(Msg::Pointer(Point::new(12.0, 12.0))),
        Outcome::ScrollTo(_)
    ));
    let _ = s.update(Msg::Middle(false));
    assert!(matches!(
        s.update(Msg::Pointer(Point::new(14.0, 14.0))),
        Outcome::None
    ));
    // the UI scale steps like the AI chat's
    assert_eq!(s.ui_zoom, 100);
    let _ = s.update(Msg::UiScale(1));
    assert_eq!(s.ui_zoom, 110);
    let _ = s.update(Msg::UiScale(-1));
    let _ = s.update(Msg::UiScale(-1));
    assert_eq!(s.ui_zoom, 90);
    let _ = s.update(Msg::UiScale(0));
    assert_eq!(s.ui_zoom, 100);
}

/// v3.1 item 5: the walkthrough opens in the slide-over (the header stays free: the
/// caption buttons still work); Esc closes the panel.
#[test]
fn v31_big_tasks_slide_in_and_the_header_stays_free() {
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::ExportWalkthrough);
    assert!(s.exporting);
    let words = crate::shared::kit::testing::shot::texts(view(&s, &[], false));
    assert!(words.iter().any(|t| t == "Walkthrough"), "{words:?}");
    let close = click_view(&s, (1256.0, 20.0));
    assert!(
        close
            .iter()
            .any(|m| matches!(m, Msg::Caption(Caption::Close))),
        "{close:?}"
    );
    let _ = s.update(Msg::Cancel);
    assert!(!s.exporting);
}

/// v3.1 review 1: a pick in a popover closes it, so the next press reaches the
/// picture (Edit text's first pick), and no popover shows over a slide-over.
#[test]
fn v31_a_pick_in_a_popover_closes_it_and_the_canvas_gets_the_next_click() {
    let mut s = two_boxes();
    for (pop, pick) in [
        (popovers::Pop::Actions, Msg::EditText),
        (popovers::Pop::Actions, Msg::CutOut),
        (
            popovers::Pop::Position,
            Msg::Editor(Message::Order(annotate::Order::Front)),
        ),
    ] {
        let _ = s.update(Msg::Pop(Some(pop)));
        let _ = s.update(Msg::Cancel);
        let _ = s.update(Msg::Pop(Some(pop)));
        assert_eq!(s.popover, Some(pop));
        let _ = s.update(pick);
        assert!(s.popover.is_none(), "{pop:?}");
        let _ = s.update(Msg::Cancel);
    }
    // Edit text from Actions: the picture takes the next click
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Actions)));
    let _ = s.update(Msg::EditText);
    assert!(s.popover.is_none() && s.edit.is_some());
    let got = click_view(&s, (500.0, 400.0));
    assert!(!got.is_empty(), "the picture got the click");
    assert!(!got.iter().any(|m| matches!(m, Msg::Pop(_))), "{got:?}");
}

/// v3.1 review 3: a narrow bar never hides a control: below the room for eight
/// colour dots they become ONE swatch button that opens the Colour popover; the
/// plan follows the bar's own room (window, UI scale), not the window alone.
#[test]
fn v31_a_narrow_bar_keeps_every_control_and_folds_the_colours() {
    let probe = |s: &Studio| {
        let e = s.editor.as_ref().unwrap();
        let size = Size::new(s.window.width - 54.0, 48.0);
        let (mut colours, mut button) = (0, 0);
        for i in 0..(size.width / 8.0) as usize {
            let at = Point::new(8.0 * i as f32, 24.0);
            let got = crate::shared::kit::testing::shot::click(options_bar::view(s, e), size, at);
            colours += got
                .iter()
                .filter(|m| matches!(m, Msg::Editor(Message::Colour(_))))
                .count();
            button += got
                .iter()
                .filter(|m| matches!(m, Msg::Pop(Some(popovers::Pop::Colours))))
                .count();
        }
        let words = crate::shared::kit::testing::shot::texts(options_bar::view(s, e));
        (colours, button, words)
    };
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Text)));
    let _ = s.update(Msg::Resized(SIZE));
    let (colours, button, _) = probe(&s);
    assert!(
        colours >= 6 && button == 0,
        "wide: dots {colours}, button {button}"
    );
    let _ = s.update(Msg::Resized(super::MIN));
    let (colours, button, words) = probe(&s);
    assert_eq!(colours, 0, "narrow: no loose dots");
    assert!(button >= 1, "narrow: the colour button");
    for w in ["Outline", "Fill", "Position", "Actions", "More", "100 %"] {
        assert!(words.iter().any(|t| t == w), "{w} kept: {words:?}");
    }
    // the plan follows the UI scale too: at 1280 a large UI folds the dots
    let _ = s.update(Msg::Resized(SIZE));
    for _ in 0..4 {
        let _ = s.update(Msg::UiScale(1));
    }
    let (colours, button, _) = probe(&s);
    assert!(colours == 0 && button >= 1, "scaled: {colours} {button}");
}

/// v3.1 review 4: while Edit text runs its own bar replaces the options bar (one
/// colour row, never the shared groups beside it); no second panel shows.
#[test]
fn v31_edit_text_replaces_the_options_bar() {
    let mut s = studio_on(640, 400);
    let words = |s: &Studio| crate::shared::kit::testing::shot::texts(view(s, &[], false));
    assert!(words(&s).iter().any(|t| t == "Opacity"));
    let _ = s.update(Msg::EditText);
    let got = words(&s);
    assert!(
        !got.iter().any(|t| t == "Opacity" || t == "Position"),
        "{got:?}"
    );
    assert!(got.iter().any(|t| t == "Cancel"), "{got:?}");
    let _ = s.update(Msg::Cancel);
    assert!(words(&s).iter().any(|t| t == "Opacity"));
}

/// W91 wiring: File > Preferences (Ctrl+,) opens the dialog over the Studio; a change in
/// it asks the host to write the config; Esc closes it; its Save folder button opens General.
#[test]
fn v31_the_preferences_dialog_opens_over_the_studio_and_closes_on_esc() {
    let mut s = studio_on(640, 400);
    assert!(s.prefs.is_none());
    let rows = menu_bar::items(Menu::File, &s);
    let at = rows.iter().position(|r| r.label == "Preferences…").unwrap();
    assert!(matches!(
        s.update(Msg::MenuPick(Menu::File, at)),
        Outcome::None
    ));
    assert!(s.prefs.is_some());
    // it blocks the canvas keys and shows its sections over the window
    assert!(s.editor.as_ref().unwrap().keys_blocked);
    let words = crate::shared::kit::testing::shot::texts(view(&s, &[], false));
    assert!(words.iter().any(|t| t == "Keyboard shortcuts"), "{words:?}");
    // a change: the host writes it
    let mut p = config::current().studio;
    p.snap = !p.snap;
    assert!(matches!(
        s.update(Msg::Prefs(preferences::Msg::Prefs(Box::new(p)))),
        Outcome::ApplyPrefs
    ));
    // F11: the Save folder's button opens Settings > General
    assert!(matches!(
        s.update(Msg::Prefs(preferences::Msg::OpenGeneral)),
        Outcome::PrefsOpenGeneral
    ));
    let _ = s.update(Msg::Cancel);
    assert!(s.prefs.is_none());
    assert!(!s.editor.as_ref().unwrap().keys_blocked);
    // the dialog's own Close button
    let _ = s.update(Msg::OpenPrefs);
    assert!(matches!(
        s.update(Msg::Prefs(preferences::Msg::Close)),
        Outcome::None
    ));
    assert!(s.prefs.is_none());
}

/// W91 wiring: the sidebar's side follows Settings > Studio (the rail takes the other
/// edge), and the default is the end (rail at the window's start edge).
#[test]
fn v31_the_steps_sidebar_side_is_a_setting() {
    use crate::shared::config::StudioSide;
    assert_eq!(config::current().studio.steps_side, StudioSide::End);
    let s = studio_on(640, 400);
    assert!(!s.sidebar_at_start());
    // End: the sidebar's Add at the right, the rail's Rectangle at the left
    let rect = click_view(&s, (27.0, 192.0));
    assert!(
        rect.iter()
            .any(|m| matches!(m, Msg::Editor(Message::Tool(Tool::Rect))))
    );
}

/// Review follow-up: the panels write only the keys that changed, in order, so a stale
/// copy never undoes what Snip or the dialog wrote meanwhile (`hint_seen` here).
#[test]
fn v31_panels_write_only_what_changed() {
    let mut s = studio_on(640, 400);
    let Outcome::Panels(old, new) = s.update(Msg::UiScale(1)) else {
        panic!("a UI scale is a panels change");
    };
    assert_eq!((old.ui_zoom, new.ui_zoom), (100, 110));
    // the file meanwhile: Snip showed its hint, the walkthrough length changed
    let mut file = StudioConfig {
        hint_seen: true,
        walk_ms: 1000,
        ..StudioConfig::default()
    };
    file.apply_changes(&old, &new);
    assert_eq!(file.ui_zoom, 110);
    assert!(
        file.hint_seen && file.walk_ms == 1000,
        "what was not changed stays"
    );
}

/// Review follow-up: a preference changed in the dialog reaches the open picture
/// (snap, pixel grid, Ctrl+C, paste, grow, drag type), and "Reset tool styles" resets
/// the open editor's own options.
#[test]
fn v31_preferences_reach_the_open_picture() {
    let mut s = studio_on(640, 400);
    {
        let e = s.editor.as_mut().unwrap();
        (e.snap, e.grid, e.grow_canvas, e.copy_picture_when_empty) = (false, false, true, false);
    }
    s.snap = false;
    s.prefs_applied();
    let d = config::current().studio;
    let e = s.editor.as_ref().unwrap();
    assert_eq!((e.snap, e.grid), (d.snap, d.pixel_grid));
    assert_eq!(e.grow_canvas, d.grow_canvas);
    assert_eq!(e.copy_picture_when_empty, d.ctrl_c_picture);
    assert_eq!(s.snap, d.snap);
    // Reset tool styles: the colour the user picked goes back to the default
    let red = [255, 0, 0, 255];
    let _ = s.update(Msg::Editor(Message::Colour([1, 2, 3, 255])));
    assert_ne!(s.editor.as_ref().unwrap().colour, red);
    let _ = s.update(Msg::OpenPrefs);
    let _ = s.update(Msg::Prefs(preferences::Msg::ResetToolStyles));
    s.prefs_applied();
    let now = s.editor.as_ref().unwrap().colour;
    assert_ne!(now, [1, 2, 3, 255], "the editor took the defaults");
}

/// Review follow-up: the unsaved question takes over the files that were still arriving.
#[test]
fn v31_the_question_takes_the_pending_drops() {
    let mut s = studio_on(64, 64);
    draw(&mut s);
    assert!(matches!(
        s.update(Msg::Dropped(PathBuf::from("/tmp/a.png"))),
        Outcome::SettleDrop(1)
    ));
    let _ = s.close_requested();
    assert!(s.asking());
    assert!(matches!(s.update(Msg::DropSettle(1)), Outcome::None));
    let _ = s.update(Msg::Cancel);
    assert!(!s.asking());
    assert!(
        matches!(s.update(Msg::DropSettle(1)), Outcome::None),
        "nothing is left to place"
    );
}

/// Owner note 7: the rubber band is drawn from the press to the pointer at any zoom
/// (16 % and 300 %): the box's corners are the pointer's, in canvas px.
#[test]
fn w90b_the_rubber_band_follows_the_pointer_at_any_zoom() {
    for zoom in [0.16_f32, 1.0, 3.0] {
        let mut s = studio_on(4000, 2500);
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Select)));
        let _ = s.zoom_to(zoom, None);
        s.editor.as_mut().unwrap().refresh();
        let (from, to) = (Point::new(40.0, 30.0), Point::new(300.0, 200.0));
        let size = s.editor.as_ref().unwrap().size();
        let msgs = {
            let e = s.editor.as_ref().unwrap();
            crate::shared::kit::testing::shot::press_move(
                crate::features::snip::screens::snip_toolbar::canvas::canvas(
                    e,
                    1.0,
                    crate::features::snip::screens::snip_toolbar::canvas::ScreenMap::ONE,
                ),
                size,
                from,
                to,
            )
        };
        for m in msgs {
            let _ = s.update(Msg::Editor(m));
        }
        let band = s.editor.as_ref().unwrap().band_rect().expect("a band");
        assert!(
            (band.x - from.x).abs() < 1.5 && (band.y - from.y).abs() < 1.5,
            "zoom {zoom}: {band:?}"
        );
        assert!(
            (band.x + band.width - to.x).abs() < 1.5 && (band.y + band.height - to.y).abs() < 1.5,
            "zoom {zoom}: {band:?}"
        );
    }
}

/// Owner note 7 through the whole window (stage, scroll area, centring, UI scale): the
/// band's size is the pointer's travel, its end is where the pointer is.
#[test]
fn w90b_the_rubber_band_in_the_window_matches_the_pointer() {
    for (scale, zoom) in [(1.0_f32, 0.16_f32), (1.5, 0.16), (1.0, 3.0), (2.0, 1.0)] {
        let mut s = studio_on(4000, 2500);
        let _ = s.update(Msg::Scale(scale));
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Select)));
        let _ = s.zoom_to(zoom, None);
        s.editor.as_mut().unwrap().refresh();
        let (from, to) = (Point::new(400.0, 300.0), Point::new(600.0, 450.0));
        let msgs =
            crate::shared::kit::testing::shot::press_move(view(&s, &[], false), SIZE, from, to);
        for m in msgs {
            let _ = s.update(m);
        }
        let band = s.editor.as_ref().unwrap().band_rect().expect("a band");
        assert!(
            (band.width - 200.0).abs() < 2.0 && (band.height - 150.0).abs() < 2.0,
            "scale {scale} zoom {zoom}: {band:?}"
        );
    }
}

/// Owner note 7, zoomed in AND scrolled (the picture bigger than the stage): the band
/// is where the pointer is, at the scroll offset of the stage.
#[test]
fn w90b_the_rubber_band_is_right_after_a_scroll() {
    for zoom in [1.0_f32, 3.0] {
        let mut s = studio_on(4000, 2500);
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Select)));
        let _ = s.zoom_to(zoom, None);
        s.editor.as_mut().unwrap().refresh();
        let scrolled = (CANVAS, Vector::new(700.0, 500.0));
        let (from, to) = (Point::new(400.0, 300.0), Point::new(600.0, 450.0));
        let msgs = crate::shared::kit::testing::shot::press_move_scrolled(
            view(&s, &[], false),
            SIZE,
            scrolled,
            from,
            to,
        );
        // what the pointer travelled, in picture px, is what the band spans
        for m in msgs {
            let _ = s.update(m);
        }
        let e = s.editor.as_ref().unwrap();
        let band = e.band_rect().expect("a band");
        assert!(
            (band.width - 200.0).abs() < 2.0 && (band.height - 150.0).abs() < 2.0,
            "zoom {zoom}: {band:?}"
        );
        // and it starts where the picture point under the first press is
        let start = e.to_display_point(e.to_image(Point::new(band.x, band.y)));
        assert!((start.x - band.x).abs() < 1.0);
    }
}

/// Owner notes 9 and 4: the unsaved question is a solid modal over a dimmed window (the
/// picture does not show through), the buttons are one size, Save is the accent primary;
/// Esc cancels, Enter saves, the X and a press outside cancel.
#[test]
fn w90b_the_question_is_a_solid_modal() {
    let mut s = studio_on(640, 400);
    draw(&mut s);
    let _ = s.close_requested();
    assert!(s.asking());
    // a press outside the card (the dimmed window) cancels; the picture takes no press
    let got = click_view(&s, (60.0, 400.0));
    assert!(matches!(got.as_slice(), [Msg::Cancel]), "{got:?}");
    // the card's words and buttons
    let words = crate::shared::kit::testing::shot::texts(view(&s, &[], false));
    for w in ["Don't save", "Cancel", "Save"] {
        assert!(words.iter().any(|t| t == w), "{w}: {words:?}");
    }
    assert!(
        !words.iter().any(|t| t == "Close"),
        "no Close text button: {words:?}"
    );
    // solid: a point inside the card, away from its words, is the widget colour
    let size = SIZE;
    let pixels = {
        let mut shown = Some(view(&s, &[], false));
        crate::shared::kit::testing::shot::redrawn(
            |i| (i == 0).then(|| shown.take()).flatten(),
            size,
            1.0,
        )
        .0
    };
    let at = ((400.0 * size.width + 430.0) * 4.0) as usize;
    let want = crate::shared::kit::themes::c().widget;
    let got = [pixels[at], pixels[at + 1], pixels[at + 2]];
    let want = [want.r, want.g, want.b].map(|v| (v * 255.0).round() as u8);
    assert_eq!(
        got, want,
        "the card is solid, the picture does not show through"
    );
    // Enter saves, Esc cancels
    assert!(matches!(
        s.update(Msg::MenuKey(Nav::Enter)),
        Outcome::SaveThen(_)
    ));
    let _ = s.close_requested();
    let _ = s.update(Msg::Cancel);
    assert!(!s.asking());
}

/// Owner note 4: the shortcuts sheet and the restore sheet are the same modal: an X
/// (never a Close text button), a press outside closes it.
#[test]
fn w90b_every_studio_sheet_closes_by_x_outside_press_and_esc() {
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Shortcuts(true));
    let words = crate::shared::kit::testing::shot::texts(view(&s, &[], false));
    assert!(!words.iter().any(|t| t == "Close"), "{words:?}");
    let got = click_view(&s, (4.0, 400.0));
    assert!(matches!(got.as_slice(), [Msg::Shortcuts(false)]), "{got:?}");
    let _ = s.update(Msg::Cancel);
    assert!(!s.shortcuts, "Esc closes it");
}

/// R-18: the Steps slider has ONE control (the edge handle + its header arrow and Ctrl+B);
/// the extra toggle button in the options bar is gone.
#[test]
fn w90b_the_sidebar_toggle_is_at_the_end_of_the_options_bar() {
    let mut s = studio_on(640, 400);
    assert!(s.sidebar_shown());
    let head = |x: f32| {
        crate::shared::kit::testing::shot::click(
            sidebar::view(&s, &[]),
            Size::new(super::SIDEBAR, 700.0),
            Point::new(x, 20.0),
        )
    };
    assert!(
        head(super::SIDEBAR - 24.0)
            .iter()
            .any(|m| matches!(m, Msg::Library))
    );
    assert!(
        head(super::SIDEBAR - 64.0)
            .iter()
            .any(|m| matches!(m, Msg::PinSteps))
    );
    let _ = s.update(Msg::Library);
    assert!(!s.sidebar_shown());
}

/// Draws `s` once, so each tracked opener knows where it is.
fn draw_once(s: &Studio) {
    let mut shown = Some(view(s, &[], false));
    let _ = crate::shared::kit::testing::shot::redrawn(
        |i| (i == 0).then(|| shown.take()).flatten(),
        SIZE,
        1.0,
    );
}

/// Owner note 2: a popover hangs directly under its own button, end edge on the button's
/// end edge, kept inside the window (above when there is no room below); Arabic mirrored.
#[test]
fn w90b_a_popover_opens_under_its_own_button() {
    use crate::shared::kit::popover::place;
    let button = iced::Rectangle::new(Point::new(1000.0, 50.0), Size::new(90.0, 32.0));
    let card = Size::new(324.0, 200.0);
    let win = Size::new(1280.0, 800.0);
    let at = place(button, card, true, win);
    assert_eq!(
        at.x + card.width,
        button.x + button.width,
        "end edge on the button's end edge"
    );
    assert_eq!(at.y, button.y + button.height + 4.0, "directly under it");
    // no room to the right: kept inside the window
    let edge = iced::Rectangle::new(Point::new(1250.0, 50.0), Size::new(30.0, 32.0));
    assert!(place(edge, card, false, win).x + card.width <= win.width);
    // no room below: above
    let low = iced::Rectangle::new(Point::new(300.0, 700.0), Size::new(90.0, 32.0));
    assert!(place(low, card, true, win).y + card.height <= low.y);
    // Arabic: the start edge becomes the right one, so an `end` card hangs by the left edge
    {
        let _ar = crate::shared::kit::testing::arabic();
        let left = iced::Rectangle::new(Point::new(300.0, 50.0), Size::new(90.0, 32.0));
        assert_eq!(place(left, card, true, win).x, left.x);
    }
    // the real window: draw it once, open Actions, its first row is right under the button
    let mut s = two_boxes();
    draw_once(&s);
    let anchor = s.anchors.of(popovers::Pop::Actions).get();
    assert!(
        anchor.width > 0.0 && anchor.y > 40.0,
        "the opener knows its place: {anchor:?}"
    );
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Actions)));
    let (size, end) = popovers::Pop::Actions.card();
    let at = crate::shared::kit::popover::place_in(anchor, size, end, s.popover_room());
    let got = click_view(&s, (at.x + 60.0, at.y + 12.0 + 16.0));
    assert!(
        got.iter().any(|m| matches!(m, Msg::Act(Row::CopyText))),
        "the first row sits under the button: {got:?} at {at:?}"
    );
}

/// Owner note 3: Studio settings are a familiar settings window: a gear in the header, a
/// list of sections on the left, the section's title and one-line subtitle, groups with
/// small headings, rows (label + grey text, control), an X at the top, and an outside
/// press closes it (Esc too).
#[test]
fn w90b_studio_settings_are_a_familiar_settings_window() {
    let mut s = studio_on(640, 400);
    // the gear: a visible button in the header row, near the menus
    let gear = (0..80)
        .map(|i| Point::new(180.0 + 2.0 * i as f32, 20.0))
        .any(|at| {
            click_view(&s, (at.x, at.y))
                .iter()
                .any(|m| matches!(m, Msg::OpenPrefs))
        });
    assert!(gear, "a gear button opens the preferences");
    let _ = s.update(Msg::OpenPrefs);
    let words = crate::shared::kit::testing::shot::texts(view(&s, &[], false));
    for w in [
        "General",
        "Appearance",
        "Saving and export",
        "Copy and paste",
        "Editing",
        "Keyboard shortcuts",
    ] {
        assert!(words.iter().any(|t| t == w), "section {w}: {words:?}");
    }
    // the section's subtitle and a small group heading
    assert!(
        words.iter().any(|t| t.starts_with("How the Studio opens")),
        "{words:?}"
    );
    assert!(
        words.iter().any(|t| t.eq_ignore_ascii_case("Opening")),
        "{words:?}"
    );
    assert!(
        !words.iter().any(|t| t == "Close"),
        "an X, not a Close button: {words:?}"
    );
    // a press on the dim outside the dialog closes it
    let got = click_view(&s, (3.0, 400.0));
    assert!(
        got.iter()
            .any(|m| matches!(m, Msg::Prefs(preferences::Msg::Close))),
        "{got:?}"
    );
    // Esc closes it too
    let _ = s.update(Msg::Cancel);
    assert!(s.prefs.is_none());
}

/// Owner note 5: the Studio's keys are editable in its preferences: record a new key
/// (Esc stops), conflicts are refused and said, Reset gives the default back, and the
/// key handler, menus and sheet all read the same table.
#[test]
fn w90b_studio_shortcuts_can_be_recorded_refused_and_reset() {
    use super::keys::{Cmd, Taken};
    let mut d = preferences::Dialog::open();
    // record Open as Ctrl+Shift+O
    assert!(matches!(
        d.update(preferences::Msg::Record(Cmd::Open)),
        preferences::Out::None
    ));
    assert!(d.recording());
    assert!(matches!(
        d.update(preferences::Msg::Recorded("Ctrl+Shift+O".into())),
        preferences::Out::Changed
    ));
    assert!(!d.recording());
    let mut c = config::Config::default();
    d.apply(&mut c);
    assert_eq!(
        c.studio.shortcuts.get("open").map(String::as_str),
        Some("Ctrl+Shift+O")
    );
    // a chord another command has is refused, and so is an editing key
    let _ = d.update(preferences::Msg::Record(Cmd::Fit));
    assert!(matches!(
        d.update(preferences::Msg::Recorded("Ctrl+B".into())),
        preferences::Out::None
    ));
    assert_eq!(
        keys::taken(Cmd::Fit, "Ctrl+B"),
        Some(Taken::Command(Cmd::StepsSidebar))
    );
    assert_eq!(keys::taken(Cmd::Fit, "Ctrl+Z"), Some(Taken::Fixed));
    assert!(keys::taken(Cmd::Fit, "Ctrl+Alt+9").is_none());
    // Esc stops waiting
    let _ = d.update(preferences::Msg::Record(Cmd::Fit));
    d.stop_recording();
    assert!(!d.recording());
    // Reset: Open has its own key again
    assert!(matches!(
        d.update(preferences::Msg::ResetKey(Cmd::Open)),
        preferences::Out::Changed
    ));
    let mut c = config::Config::default();
    d.apply(&mut c);
    assert!(c.studio.shortcuts.is_empty());
    // the file: a chord that is none is dropped, a good one kept
    let (c, warnings) = config::Config::parse(
        "[studio.shortcuts]\nopen = \"Ctrl+Shift+O\"\nfit = \"nonsense++\"\n",
    );
    assert_eq!(c.studio.shortcuts.len(), 1, "{:?}", c.studio.shortcuts);
    assert!(!warnings.is_empty());
    // the keys the table gives: defaults, and the chord text of a pressed key
    assert_eq!(keys::of(Cmd::Open), "Ctrl+O");
    assert_eq!(keys::command_for("Ctrl+B"), Some(Cmd::StepsSidebar));
    use iced::keyboard::{Key, Modifiers, key::Code, key::Physical};
    let chord = keys::chord_text(
        Modifiers::CTRL | Modifiers::SHIFT,
        &Key::Character("o".into()),
        Physical::Code(Code::KeyO),
    );
    assert_eq!(chord.as_deref(), Some("Ctrl+Shift+O"));
    assert!(
        keys::chord_text(
            Modifiers::empty(),
            &Key::Character("a".into()),
            Physical::Code(Code::KeyA)
        )
        .is_none()
    );
}

/// Runs `body` with `change` applied to the running config on this test's own thread
/// (no other test sees it).
fn with_config<R>(change: impl FnOnce(&mut config::Config), body: impl FnOnce() -> R) -> R {
    let mut changed = config::current();
    change(&mut changed);
    config::with_test_config(changed, body)
}

/// Owner note 6: every Studio preference acts in the open Studio (or at its next
/// opening, where that is what the row says). One check per row.
#[test]
fn w90b_every_studio_preference_acts() {
    use crate::shared::config::{Format, StudioBackdrop, StudioSide, StudioWheel, StudioZoom};
    let texts = |s: &Studio| crate::shared::kit::testing::shot::texts(view(s, &[], false));

    // Start screen: off = no Open / Paste / recent files, only the drop zone's words
    with_config(
        |c| c.studio.start_screen = false,
        || {
            let words = texts(&Studio::new(docked()));
            assert!(
                !words.iter().any(|t| t == "Open…" || t == "Paste"),
                "{words:?}"
            );
        },
    );
    let words = texts(&Studio::new(docked()));
    assert!(words.iter().any(|t| t == "Open…"), "on: Open is there");

    // Show Steps when the Studio opens
    with_config(
        |c| c.studio.steps_on_start = true,
        || assert!(Studio::new(StudioConfig::default()).sidebar_shown()),
    );
    assert!(
        !Studio::new(StudioConfig::default()).sidebar_shown(),
        "closed by default: a handle on the edge"
    );

    // Steps sidebar side: the sidebar's Add at the left, the rail at the right
    with_config(
        |c| c.studio.steps_side = StudioSide::Start,
        || {
            let s = studio_on(640, 400);
            assert!(s.sidebar_at_start());
            let rail = click_view(&s, (1252.0, 192.0));
            assert!(
                rail.iter()
                    .any(|m| matches!(m, Msg::Editor(Message::Tool(Tool::Rect))))
            );
        },
    );

    // Ask before closing with unsaved changes
    let dirty = || {
        let mut s = studio_on(64, 64);
        draw(&mut s);
        s
    };
    assert!(dirty().asks());
    with_config(
        |c| c.studio.ask_unsaved = false,
        || assert!(!dirty().asks()),
    );

    // Keep unsaved work: an edit waits for its copy only when it is on
    with_config(
        |c| c.studio.keep_unsaved = true,
        || assert!(dirty().quiet.waiting()),
    );
    assert!(!dirty().quiet.waiting());

    // Reopen the last picture
    let mut fresh = Studio::new(docked());
    let _ = fresh.update(Msg::Recent(
        vec![(
            PathBuf::from("/x/last.png"),
            iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]),
        )],
        vec![String::new()],
    ));
    with_config(
        |c| c.studio.reopen_last = true,
        || {
            let mut s = Studio::new(docked());
            let out = s.update(Msg::Recent(
                vec![(
                    PathBuf::from("/x/last.png"),
                    iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]),
                )],
                vec![String::new()],
            ));
            assert!(matches!(out, Outcome::Read(_)));
        },
    );

    // Recent files shown: 0 hides the list on the start screen
    with_config(
        |c| c.studio.recent_files = 0,
        || {
            let words = texts(&fresh);
            assert!(!words.iter().any(|t| t == "last.png"), "{words:?}");
        },
    );
    assert!(texts(&fresh).iter().any(|t| t == "last.png"));

    // Zoom on a new picture
    with_config(
        |c| c.studio.zoom_on_open = StudioZoom::Actual,
        || {
            let s = studio_on(4000, 2500);
            assert!((s.editor.as_ref().unwrap().zoom - 1.0).abs() < 0.001);
        },
    );
    assert!(studio_on(4000, 2500).editor.as_ref().unwrap().zoom < 0.5);

    // Canvas background: the stage behind the picture changes
    let stage_pixel = |s: &Studio| {
        let mut shown = Some(view(s, &[], false));
        let (pixels, _) = crate::shared::kit::testing::shot::redrawn(
            |i| (i == 0).then(|| shown.take()).flatten(),
            SIZE,
            1.0,
        );
        let at = ((150.0 * SIZE.width + 100.0) * 4.0) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    };
    let plain = stage_pixel(&studio_on(64, 64));
    let others: Vec<[u8; 3]> = [StudioBackdrop::Checkerboard, StudioBackdrop::Dark]
        .into_iter()
        .map(|b| {
            with_config(
                |c| c.studio.background = b,
                || stage_pixel(&studio_on(64, 64)),
            )
        })
        .collect();
    assert!(
        others.iter().any(|p| *p != plain),
        "{plain:?} vs {others:?}"
    );

    // Mouse wheel: it zooms by default; set to scroll it leaves the wheel to the scroll area
    let wheel = |s: &Studio| {
        crate::shared::kit::testing::shot::wheel(
            view(s, &[], false),
            SIZE,
            Point::new(500.0, 300.0),
            1.0,
        )
        .iter()
        .any(|m| matches!(m, Msg::Wheel { .. }))
    };
    assert!(
        wheel(&studio_on(2000, 1500)),
        "zoom: the wheel is the picture's zoom"
    );
    with_config(
        |c| c.studio.wheel = StudioWheel::Scroll,
        || assert!(!wheel(&studio_on(2000, 1500)), "scroll: the wheel scrolls"),
    );

    // Studio theme: its own theme (None = follow OpenCircle's)
    let mut prefs = config::current().studio;
    assert!(preferences::theme_of(&prefs).is_none());
    prefs.theme = "obsidian".into();
    assert_eq!(
        preferences::theme_of(&prefs).map(|t| t.id),
        Some("obsidian")
    );

    // Format / Quality / Save size: the Save as sheet starts on them after the dialog writes
    with_config(
        |c| {
            c.studio.format = Format::Jpeg;
            c.studio.quality = 95;
            c.snip.max_edge = 1568;
        },
        || {
            let mut s = studio_on(64, 64);
            s.prefs_applied();
            assert_eq!(
                (s.export.format, s.export.quality, s.export.max_edge),
                (crate::features::snip::export::Format::Jpg, 95, 1568)
            );
        },
    );

    // Copy / paste / grow / drag type / snap / grid reach the open picture
    with_config(
        |c| {
            c.studio.ctrl_c_picture = false;
            c.studio.paste_movable = false;
            c.studio.grow_canvas = true;
            c.studio.drag_type = Format::Jpeg;
            c.studio.snap = false;
            c.studio.pixel_grid = false;
        },
        || {
            let mut s = studio_on(64, 64);
            s.prefs_applied();
            let e = s.editor.as_ref().unwrap();
            assert!(!e.copy_picture_when_empty && !e.paste_movable && e.grow_canvas && e.drag_jpeg);
            assert!(!e.snap && !e.grid && !s.snap && !s.grid);
        },
    );
}

/// Review 1: resetting the last custom key empties `[studio.shortcuts]`; the write must
/// remove the table from a file that already has it.
#[test]
fn w90c_resetting_every_shortcut_leaves_the_file() {
    use super::keys::Cmd;
    let mut file = config::Config::default();
    file.studio
        .shortcuts
        .insert("open".into(), "Ctrl+Shift+O".into());
    with_config(
        |c| *c = file.clone(),
        || {
            let mut d = preferences::Dialog::open();
            let _ = d.update(preferences::Msg::ResetKey(Cmd::Open));
            let mut c = file.clone();
            d.apply(&mut c);
            assert!(c.studio.shortcuts.is_empty(), "{:?}", c.studio.shortcuts);
        },
    );
}

/// Review 2: a hand-edited chord is written the one way (Ctrl, Alt, Shift, key); one on a
/// fixed key or a menu Alt key never shadows it.
#[test]
fn w90c_hand_edited_chords_are_canonical_and_never_shadow_fixed_keys() {
    use super::keys::{self, Cmd, Taken};
    let (c, _) = config::Config::parse(
        "[studio.shortcuts]\nopen = \"Shift+Ctrl+O\"\nfit = \"Shift+Alt+9\"\nactual = \"Ctrl+W\"\nsave_as = \"Alt+F\"\n",
    );
    assert_eq!(
        c.studio.shortcuts.get("open").map(String::as_str),
        Some("Ctrl+Shift+O")
    );
    assert_eq!(
        c.studio.shortcuts.get("fit").map(String::as_str),
        Some("Alt+Shift+9")
    );
    with_config(
        |cfg| *cfg = c.clone(),
        || {
            assert_eq!(keys::of(Cmd::Open), "Ctrl+Shift+O");
            assert_eq!(keys::of(Cmd::Actual), Cmd::Actual.default_chord());
            assert_eq!(keys::of(Cmd::SaveAs), Cmd::SaveAs.default_chord());
            assert_eq!(keys::taken(Cmd::Fit, "Alt+V"), Some(Taken::Fixed));
        },
    );
}

/// Review 5: a command on an Alt chord leaves no pending Alt (its release opens no menu).
#[test]
fn w90c_an_alt_chord_does_not_open_the_menu_bar_on_release() {
    let mut file = config::Config::default();
    file.studio.shortcuts.insert("fit".into(), "Alt+9".into());
    with_config(
        |c| *c = file.clone(),
        || {
            let mut s = studio_on(640, 400);
            let _ = s.update(Msg::AltDown);
            let cmd = super::keys::command_for("Alt+9").expect("the chord is Fit's");
            let _ = s.update(cmd.msg());
            let _ = s.update(Msg::AltUp);
            assert!(s.menu.is_none(), "the menu bar stayed shut");
        },
    );
}

/// W90c item 2: the empty Studio keeps the options bar's row with its picture controls
/// R-18: the empty Studio dims the picture controls (Position / Actions) and has no extra
/// Steps toggle button in the options bar.
#[test]
fn w90c_the_empty_studio_has_the_steps_toggle_at_the_same_spot() {
    let empty = Studio::new(docked());
    assert!(empty.editor.is_none());
    let got = click_view(
        &empty,
        (SIZE.width - 60.0, super::HEADER + super::OPTIONS / 2.0),
    );
    assert!(!got.iter().any(|m| matches!(m, Msg::Pop(_))), "{got:?}");
    assert!(!got.iter().any(|m| matches!(m, Msg::Library)), "{got:?}");
}

/// W90d.5: where the time goes, measured (not a gate): `cargo test --release --bin opencircle
/// studio_delay -- --ignored --nocapture`. At 3440x1440, per action: the update on the UI
/// thread, and the frame that shows the result (layout + draw, raster of the damaged part).
#[test]
#[ignore = "a measurement, not a gate"]
fn studio_delay_by_action() {
    use crate::shared::kit::testing::shot;
    crate::shared::kit::text::load();
    let win = Size::new(3440.0, 1440.0);
    let mut sized = Studio::new(docked());
    let _ = sized.update(Msg::Resized(win));
    drop(sized);
    let make = |w: u32, h: u32| {
        let mut s = Studio::new(docked());
        let _ = s.update(Msg::Resized(win));
        s.open(Picture::Crop(crop(w, h)));
        s.settle_open();
        // a big picture's smaller copies are made on a thread: wait for them, as a user
        // would have by the time a menu is opened
        for _ in 0..1000 {
            let _ = s.update(Msg::Fit);
            if w * h < 3_000_000 || s.editor.as_ref().is_some_and(|e| e.display.is_some()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        s
    };
    let frames = |states: &[&Studio], steps: &[std::sync::Arc<Step>]| -> Vec<(f64, f64)> {
        shot::frame_times(|i| states.get(i).map(|s| view::view(s, steps, false)), win)
    };
    let line = |what: &str, ms: f64, f: &[(f64, f64)]| {
        let shown: Vec<String> = f.iter().map(|(a, b)| format!("{a:.1}+{b:.1}")).collect();
        println!(
            "{what:<34} update {ms:>7.1} ms | frames (layout+draw + raster ms): {}",
            shown.join("  ")
        );
    };
    for (name, w, h) in [
        ("1080p", 1920, 1080),
        ("4K", 3840, 2160),
        ("8K", 7680, 4320),
    ] {
        let empty = {
            let mut s = Studio::new(docked());
            let _ = s.update(Msg::Resized(win));
            s
        };
        let t = std::time::Instant::now();
        let a = make(w, h);
        let open_ms = t.elapsed().as_secs_f64() * 1e3;
        let f = frames(&[&empty, &a, &a, &a], &[]);
        line(
            &format!("open {name} (empty -> picture, +2 idle)"),
            open_ms,
            &f,
        );
        // pick a tool
        let mut b = make(w, h);
        let t = std::time::Instant::now();
        let _ = b.update(Msg::Editor(Message::Tool(Tool::Rect)));
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let f = frames(&[&a, &b, &b], &[]);
        line(&format!("pick Rect tool on {name}"), ms, &f);
        // a stroke: 4 moves
        let mut moves = Vec::new();
        let mut s = make(w, h);
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Pen)));
        let _ = s.update(Msg::Editor(Message::Press(Point::new(10.0, 10.0))));
        let mut upd = 0.0;
        for k in 1..=4 {
            let t = std::time::Instant::now();
            let _ = s.update(Msg::Editor(Message::Move(Point::new(
                10.0 + 40.0 * k as f32,
                10.0 + 25.0 * k as f32,
            ))));
            upd += t.elapsed().as_secs_f64() * 1e3;
            let mut again = make(w, h);
            let _ = again.update(Msg::Editor(Message::Tool(Tool::Pen)));
            let _ = again.update(Msg::Editor(Message::Press(Point::new(10.0, 10.0))));
            for j in 1..=k {
                let _ = again.update(Msg::Editor(Message::Move(Point::new(
                    10.0 + 40.0 * j as f32,
                    10.0 + 25.0 * j as f32,
                ))));
            }
            moves.push(again);
        }
        let mut st: Vec<&Studio> = vec![&a];
        st.extend(moves.iter());
        let f = frames(&st, &[]);
        line(&format!("pen stroke, 4 moves on {name}"), upd / 4.0, &f);
        // menu, popover, sidebar
        let mut m = make(w, h);
        let _ = m.update(Msg::Menu(Some(menu_bar::Menu::File)));
        let mut p = make(w, h);
        let _ = p.update(Msg::Pop(Some(popovers::Pop::Actions)));
        let mut sb = make(w, h);
        let _ = sb.update(Msg::Library);
        for (what, x) in [
            ("tool", &b),
            ("menu", &m),
            ("popover", &p),
            ("sidebar", &sb),
        ] {
            let d = shot::damage(|i| [&a, x].get(i).map(|s| view::view(s, &[], false)), win);
            println!("damage after {what} on {name}: {:?}", d.get(1));
        }
        let f = frames(&[&a, &m, &a, &p, &a, &sb], &[]);
        line(&format!("menu / popover / sidebar on {name}"), 0.0, &f);
    }
    // "add this picture as a step": the flatten on the UI thread, then the step (PNG + thumbnail) off it
    for (name, w, h) in [
        ("1080p", 1920, 1080),
        ("4K", 3840, 2160),
        ("8K", 7680, 4320),
    ] {
        let s = make(w, h);
        let t = std::time::Instant::now();
        let flat = annotate::render(&s.editor.as_ref().unwrap().doc);
        let flatten = t.elapsed().as_secs_f64() * 1e3;
        let t = std::time::Instant::now();
        let _step = Step::new(&flat).unwrap();
        let make_step = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "add step {name}: flatten (UI thread) {flatten:.1} ms, step made off-thread {make_step:.1} ms"
        );
    }
    // the canvas itself moving across the stage (a pan frame): the picture drawn at a new
    // offset each frame, at the fit zoom and at 100 %
    for (name, w, h) in [
        ("1080p", 1920, 1080),
        ("4K", 3840, 2160),
        ("8K", 7680, 4320),
    ] {
        for (zoom, label) in [(None, "fit"), (Some(1.0f32), "100 %")] {
            let mut s = make(w, h);
            if zoom.is_some() {
                let _ = s.zoom_to(1.0 / s.scale, None);
            }
            let e = s.editor.as_ref().unwrap();
            let f = shot::frame_times(
                |i| {
                    (i < 6).then(|| {
                        iced::widget::container(
                            iced::widget::pin(crate::features::snip::screens::snip_toolbar::canvas::canvas(
                                e,
                                1.0,
                                crate::features::snip::screens::snip_toolbar::canvas::ScreenMap::ONE,
                            ))
                            .position(Point::new(-(i as f32) * 9.0, -(i as f32) * 5.0)),
                        )
                        .width(iced::Fill)
                        .height(iced::Fill)
                        .into()
                    })
                },
                win,
            );
            let shown: Vec<String> = f.iter().map(|(a, b)| format!("{a:.1}+{b:.1}")).collect();
            println!("pan frames {name} at {label:<6}: {}", shown.join("  "));
        }
    }
}

/// W90d.8 / W90d.10: the Steps sidebar's two pages and the slide between them, the unsaved
/// question, in English and Arabic (look at each after an update); Esc and the back arrow go back.
#[test]
fn the_sidebar_pages_and_the_question_match_their_golden_images() {
    use crate::shared::app_language::{self, Lang};
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    let steps: Vec<std::sync::Arc<Step>> = [(300, 200), (200, 300), (400, 120)]
        .map(|(w, h)| std::sync::Arc::new(Step::new(&crop(w, h)).unwrap()))
        .into();
    golden::each_theme(|theme, failed| {
        crate::shared::kit::motion::set_reduced(false); // (the slide is the thing shown)
        for (lang, tag) in [(Lang::En, ""), (Lang::Ar, "-ar")] {
            app_language::set(lang);
            let mut s = studio_on(300, 200);
            let file = format!("studio-sidebar-steps{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                view(&s, &steps, false)
            }));
            let _ = s.update(Msg::SidebarPage(Page::Export));
            assert!(s.moving(), "the page slides");
            let start = s.slide.unwrap().start;
            // halfway through the slide, and at its end
            let _ = s.update(Msg::Frame(start + crate::shared::kit::motion::GLIDE / 2));
            assert!(s.moving() && s.slide_at() > 0.0 && s.slide_at() < 1.0);
            let file = format!("studio-sidebar-slide{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                view(&s, &steps, false)
            }));
            let _ = s.update(Msg::Frame(start + crate::shared::kit::motion::GLIDE * 2));
            assert!(!s.moving(), "the clock stops when it lands");
            let file = format!("studio-sidebar-export{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                view(&s, &steps, false)
            }));
            // Esc goes back (a slide again), and the question's buttons
            let _ = s.update(Msg::Cancel);
            assert_eq!(s.page, Page::Steps);
            draw(&mut s);
            let _ = s.close_requested();
            assert!(s.asking());
            let file = format!("studio-question{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
                view(&s, &steps, false)
            }));
        }
        app_language::set(Lang::En);
    });
}

/// W90d.5: a big picture shown small is drawn from a smaller copy (the full pixels cost 25 ms a
/// frame on the CPU renderer); a small one, or one near its size, from itself; an edit keeps it.
#[test]
fn a_big_picture_shown_small_is_drawn_from_a_smaller_copy() {
    // (the copies are made on a thread: the next event picks them up)
    let ready = |s: &mut Studio| {
        for _ in 0..500 {
            let _ = s.update(Msg::Fit);
            if s.editor.as_ref().unwrap().display.is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    };
    let mut s = studio_on(3000, 1500);
    assert!(
        s.editor.as_ref().unwrap().display.is_none(),
        "not before the thread is done"
    );
    assert!(ready(&mut s), "4.5 MP at the fit zoom: a copy");
    let e = s.editor.as_ref().unwrap();
    let iced::widget::image::Handle::Rgba { width, .. } = e.display.as_ref().unwrap() else {
        panic!("pixels")
    };
    let want = 3000.0 * e.zoom * s.scale;
    assert!(
        (*width as f32 - want).abs() <= 1.0 && *width < 3000,
        "{width} vs {want}"
    );
    assert!(e.crisp, "drawn pixel for pixel");
    // an edit repaints the base: the copy is made again from the new base
    draw(&mut s);
    assert!(ready(&mut s));
    // a small picture never gets one
    let small = studio_on(800, 500);
    assert!(small.editor.as_ref().unwrap().display.is_none());
}

/// W90d.C: a menu, a popover or another tool inserts primitives in the middle of the frame; the
/// picture and the bars that did not change are not redrawn (paired by index, everything after the
/// insertion differed and the whole window was repainted: 15-35 ms a frame at 3440x1440).
#[test]
fn an_action_redraws_only_what_changed() {
    use crate::shared::kit::testing::shot;
    crate::shared::kit::text::load();
    let win = Size::new(1920.0, 1080.0);
    let mut s = studio_on(1600, 900);
    let _ = s.update(Msg::Resized(win));
    let acts = [
        Msg::Menu(Some(menu_bar::Menu::File)),
        Msg::Menu(None),
        Msg::Pop(Some(popovers::Pop::Actions)),
        Msg::Pop(None),
        // (a tool change repaints the picture too when the Steps card is not docked: it did on
        // origin/main with the card closed; the docked fixture hid it. Follow-up, see F-03 report)
    ];
    let frames = shot::frames_of(win, acts.len() + 2, |i, show| {
        // frame 0 draws the picture; frame 1 is idle; then one action each
        if i >= 2 {
            let _ = s.update(acts[i - 2].clone());
        }
        show(view::view(&s, &[], false));
    });
    let area = |rs: &[Rectangle]| rs.iter().map(|r| r.width * r.height).sum::<f32>();
    assert_eq!(area(&frames[1].damage), 0.0, "idle: {:?}", frames[1].damage);
    for (i, f) in frames.iter().enumerate().skip(2) {
        let hit = area(&f.damage);
        assert!(
            hit < win.width * win.height * 0.25,
            "action {i}: {hit} px of {} redrawn: {:?}",
            win.width * win.height,
            f.damage
        );
    }
}

/// W90d.C: per-action frame times with the interface kept between frames (as a window does):
/// `cargo test --release --bin opencircle studio_frames -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a gate"]
fn studio_frames_by_action() {
    use crate::shared::kit::testing::shot;
    crate::shared::kit::text::load();
    let win = Size::new(3440.0, 1440.0);
    for (name, w, h) in [
        ("1080p", 1920, 1080),
        ("4K", 3840, 2160),
        ("8K", 7680, 4320),
    ] {
        let mut s = Studio::new(docked());
        s.at_rest = false;
        let _ = s.update(Msg::Resized(win));
        crate::shared::kit::motion::set_reduced(false);
        let steps: Vec<std::sync::Arc<Step>> = [(300, 200), (200, 300), (400, 120)]
            .map(|(w, h)| std::sync::Arc::new(Step::new(&crop(w, h)).unwrap()))
            .into();
        // each: (label, what it does to the Studio before the frame is drawn)
        type Act<'a> = (&'a str, Box<dyn Fn(&mut Studio)>);
        let big = w * h >= 3_000_000;
        let acts: Vec<Act> = vec![
            (
                "open (spinner)",
                Box::new(move |s| s.open(Picture::Crop(crop(w, h)))),
            ),
            (
                "open (picture)",
                Box::new(move |s| {
                    s.settle_open();
                    for _ in 0..1000 {
                        let _ = s.update(Msg::Fit);
                        if !big
                            || s.editor
                                .as_ref()
                                .is_some_and(|e| e.display.is_some() || e.crisp)
                        {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }),
            ),
            ("idle", Box::new(|_| {})),
            (
                "pick Rect",
                Box::new(|s| {
                    let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
                }),
            ),
            (
                "pick Pen",
                Box::new(|s| {
                    let _ = s.update(Msg::Editor(Message::Tool(Tool::Pen)));
                }),
            ),
            (
                "stroke press",
                Box::new(|s| {
                    let _ = s.update(Msg::Editor(Message::Press(Point::new(10.0, 10.0))));
                }),
            ),
            (
                "stroke move 1",
                Box::new(|s| {
                    let _ = s.update(Msg::Editor(Message::Move(Point::new(60.0, 40.0))));
                }),
            ),
            (
                "stroke move 2",
                Box::new(|s| {
                    let _ = s.update(Msg::Editor(Message::Move(Point::new(120.0, 80.0))));
                }),
            ),
            (
                "stroke release",
                Box::new(|s| {
                    let _ = s.update(Msg::Editor(Message::Release));
                }),
            ),
            (
                "menu (rising 0)",
                Box::new(|s| {
                    let _ = s.update(Msg::Menu(Some(menu_bar::Menu::File)));
                }),
            ),
            (
                "menu (rising 1/3)",
                Box::new(|s| {
                    let t = s.rise_from.unwrap();
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(70)));
                }),
            ),
            (
                "menu (rising 2/3)",
                Box::new(|s| {
                    let t = s.rise_from.unwrap();
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(140)));
                }),
            ),
            (
                "menu (risen)",
                Box::new(|s| {
                    let t = s.rise_from.unwrap();
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(300)));
                }),
            ),
            (
                "menu closed",
                Box::new(|s| {
                    let _ = s.update(Msg::Menu(None));
                }),
            ),
            (
                "popover (rising 0)",
                Box::new(|s| {
                    let _ = s.update(Msg::Pop(Some(popovers::Pop::Actions)));
                }),
            ),
            (
                "popover (risen)",
                Box::new(|s| {
                    let t = s.rise_from.unwrap();
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(300)));
                }),
            ),
            (
                "popover closed",
                Box::new(|s| {
                    let _ = s.update(Msg::Pop(None));
                }),
            ),
            (
                "sidebar page (slide 0)",
                Box::new(|s| {
                    let _ = s.update(Msg::SidebarPage(Page::Export));
                }),
            ),
            (
                "sidebar page (slide 1/3)",
                Box::new(|s| {
                    let t = s.slide.unwrap().start;
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(67)));
                }),
            ),
            (
                "sidebar page (slide 2/3)",
                Box::new(|s| {
                    let t = s.slide.unwrap().start;
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(133)));
                }),
            ),
            (
                "sidebar page (landed)",
                Box::new(|s| {
                    let t = s.slide.unwrap().start;
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(300)));
                }),
            ),
            (
                "sidebar page back",
                Box::new(|s| {
                    let _ = s.update(Msg::SidebarPage(Page::Steps));
                    let t = s.slide.unwrap().start;
                    let _ = s.update(Msg::Frame(t + std::time::Duration::from_millis(300)));
                }),
            ),
            (
                "sidebar toggle",
                Box::new(|s| {
                    let _ = s.update(Msg::Library);
                }),
            ),
            (
                "zoom in (instant)",
                Box::new(|s| {
                    let _ = s.update(Msg::ZoomIn);
                }),
            ),
            (
                "zoom in (instant)",
                Box::new(|s| {
                    let _ = s.update(Msg::ZoomIn);
                }),
            ),
        ];
        let mut stamps: Vec<f64> = Vec::new();
        let frames = shot::frames_of(win, acts.len() + 1, |i, show| {
            let t = std::time::Instant::now();
            if i > 0 {
                (acts[i - 1].1)(&mut s);
            }
            stamps.push(t.elapsed().as_secs_f64() * 1e3);
            show(view::view(&s, &steps, false));
        });
        for (i, f) in frames.iter().enumerate().skip(1) {
            let area: f32 = f.damage.iter().map(|r| r.width * r.height).sum();
            let frame = f.draw_ms + f.raster_ms;
            println!(
                "{name:<6} {:<24} update {:>6.1} ms | layout+draw {:>5.1} + raster {:>5.1} = {:>5.1} ms ({:>3.0} % of a core at 60 fps) | damage {:>3.0} %",
                acts[i - 1].0,
                stamps[i],
                f.draw_ms,
                f.raster_ms,
                frame,
                frame / 16.7 * 100.0,
                area / (win.width * win.height) * 100.0
            );
        }
    }
}

/// W90d.A: what the UI thread pays to open a picture, before (all in place) and after (the thread
/// starts, the opened picture is taken): `cargo test --release --bin opencircle studio_open_cost
/// -- --ignored --nocapture`. The crop is built outside the timing (a decoder hands it over).
#[test]
#[ignore = "a measurement, not a gate"]
fn studio_open_cost() {
    crate::shared::kit::text::load();
    for (name, w, h) in [
        ("1080p", 1920, 1080),
        ("4K", 3840, 2160),
        ("8K", 7680, 4320),
    ] {
        let mut before = Vec::new();
        let mut start = Vec::new();
        let mut finish = Vec::new();
        for _ in 0..3 {
            let mut s = Studio::new(docked());
            let c = crop(w, h);
            let t = std::time::Instant::now();
            s.open_in_place(c);
            before.push(t.elapsed().as_secs_f64() * 1e3);
            let mut s = Studio::new(docked());
            let c = crop(w, h);
            let t = std::time::Instant::now();
            s.open(Picture::Crop(c));
            start.push(t.elapsed().as_secs_f64() * 1e3);
            loop {
                if let Some(ms) = s.poll_open_timed() {
                    finish.push(ms);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(|a, b| a.total_cmp(b));
            v[v.len() / 2]
        };
        // Add to steps: before = flatten on the UI thread; after = a copy of the document
        let mut s = Studio::new(docked());
        s.open(Picture::Crop(crop(w, h)));
        s.settle_open();
        let t = std::time::Instant::now();
        let flat = annotate::render(&s.editor.as_ref().unwrap().doc);
        let step_before = t.elapsed().as_secs_f64() * 1e3;
        drop(flat);
        let t = std::time::Instant::now();
        s.editor.as_mut().unwrap().refresh();
        println!(
            "{name:<6} refresh alone (flatten + base handle): {:.1} ms",
            t.elapsed().as_secs_f64() * 1e3
        );
        let t = std::time::Instant::now();
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Pen)));
        for m in [
            Message::Press(Point::new(10.0, 10.0)),
            Message::Move(Point::new(60.0, 40.0)),
        ] {
            let _ = s.update(Msg::Editor(m));
        }
        let t2 = std::time::Instant::now();
        let _ = s.update(Msg::Editor(Message::Release));
        println!(
            "{name:<6} stroke release alone: {:.1} ms (stroke before it {:.1} ms)",
            t2.elapsed().as_secs_f64() * 1e3,
            (t2 - t).as_secs_f64() * 1e3
        );
        let t = std::time::Instant::now();
        let out = s.update(Msg::Editor(Message::AddToSteps));
        let step_after = t.elapsed().as_secs_f64() * 1e3;
        assert!(matches!(out, Outcome::AddStepDoc(_)));
        println!(
            "{name:<6} add as step, UI thread: before {step_before:>6.1} ms | after {step_after:>5.1} ms (the flatten and the PNG run on a thread)"
        );
        println!(
            "{name:<6} UI thread: before {:>6.1} ms | after: start {:>5.1} ms + take {:>5.1} ms",
            med(&mut before),
            med(&mut start),
            med(&mut finish)
        );
    }
}

/// W90d.A: a big picture is prepared on a thread: `open` only starts it (a spinner shows, the frame
/// clock runs), the picture is there when the thread is done, with its base painted and its
/// see-through pixels known. (The 8K stall was 157 ms on the UI thread.)
#[test]
fn a_big_picture_opens_off_the_ui_thread() {
    let mut s = Studio::new(docked());
    s.open(Picture::Crop(crop(3000, 2000)));
    assert!(s.editor.is_none(), "not on the UI thread");
    assert!(
        s.opening_since().is_some() && s.moving(),
        "a spinner and the clock"
    );
    s.settle_open();
    let e = s.editor.as_ref().expect("opened when the thread is done");
    let iced::widget::image::Handle::Rgba { width, .. } = &e.base else {
        panic!("pixels")
    };
    assert_eq!(
        *width, 3000,
        "its base is the picture, not a 1 px placeholder"
    );
    // see-through pixels are found by the thread
    let mut c = crop(2000, 1000);
    c.rgba[3] = 10;
    s.open(Picture::Crop(c));
    s.settle_open();
    assert!(s.transparent);
    // a small picture opens at once
    s.open(Picture::Crop(crop(300, 200)));
    assert!(s.opening_since().is_none() && s.editor.as_ref().unwrap().doc.image.width == 300);
}

/// W90d.B: Add to steps hands the host the document to flatten on a thread (an 8K flatten + PNG
/// was 143 + 126 ms), and the sidebar shows a numbered place for each step being made.
#[test]
fn adding_a_step_flattens_off_the_ui_thread_and_shows_its_place() {
    use crate::shared::kit::testing::shot;
    crate::shared::kit::text::load();
    let mut s = studio_on(300, 200);
    assert!(
        matches!(
            s.update(Msg::Editor(Message::AddToSteps)),
            Outcome::AddStepDoc(_)
        ),
        "the document, not a flattened copy"
    );
    let texts = |s: &Studio| shot::texts(view::view(s, &[], false));
    let before = texts(&s);
    s.step_pending(1);
    s.step_pending(1);
    let after = texts(&s);
    assert!(
        !before.iter().any(|t| t == "1")
            && after.iter().any(|t| t == "1")
            && after.iter().any(|t| t == "2"),
        "two numbered places: {after:?}"
    );
    s.step_pending(-1);
    s.step_pending(-1);
    s.step_pending(-1); // (never below none)
    assert_eq!(s.steps_pending, 0);
}

/// W90d.D: at 100 % and magnified 200 % and more the picture draws pixel for pixel (the renderer's
/// fast path copies only the visible rows: 40 ms down to 1-2 ms for 4K / 8K); between, smooth.
#[test]
fn a_picture_at_100_percent_draws_pixel_for_pixel() {
    let mut s = studio_on(3000, 2000);
    let _ = s.update(Msg::Resized(Size::new(3440.0, 1440.0)));
    let _ = s.zoom_to(1.0 / s.scale, None);
    let e = s.editor.as_ref().unwrap();
    assert!(e.crisp && e.display.is_none(), "100 %: nearest, no copy");
    let _ = s.zoom_to(4.0 / s.scale, None);
    assert!(s.editor.as_ref().unwrap().crisp, "magnified 4x: pixels");
    let _ = s.zoom_to(1.5 / s.scale, None);
    assert!(!s.editor.as_ref().unwrap().crisp, "1.5x stays smooth");
}

/// W90d.F: a menu, a popover or a dialog that opens rises into place over `motion::OPEN` (the
/// frame clock runs only while it does); with motion off it is simply there.
#[test]
fn menus_popovers_and_dialogs_rise_as_they_open() {
    use crate::shared::kit::motion::{self, OPEN, RISE};
    motion::set_reduced(false);
    let mut s = studio_on(300, 200);
    s.at_rest = false;
    assert_eq!(s.rise(), 0.0);
    assert!(!s.moving());
    for open in [
        Msg::Menu(Some(menu_bar::Menu::File)),
        Msg::Pop(Some(popovers::Pop::Actions)),
        Msg::OpenPrefs,
    ] {
        let _ = s.update(Msg::Cancel);
        let _ = s.update(Msg::Frame(std::time::Instant::now() + OPEN * 2));
        let _ = s.update(open.clone());
        assert!(s.rise() > RISE * 0.9, "{open:?}: starts low: {}", s.rise());
        assert!(s.moving(), "{open:?}: the clock runs");
        let start = s.rise_from.unwrap();
        let _ = s.update(Msg::Frame(start + OPEN / 2));
        assert!(s.rise() > 0.0 && s.rise() < RISE, "{open:?}: half way");
        let _ = s.update(Msg::Frame(start + OPEN * 2));
        assert_eq!(s.rise(), 0.0);
        assert!(!s.moving(), "{open:?}: the clock stops");
    }
    // motion off: nothing to wait for
    motion::set_reduced(true);
    let _ = s.update(Msg::Cancel);
    let _ = s.update(Msg::Menu(Some(menu_bar::Menu::Edit)));
    assert_eq!(s.rise(), 0.0);
    motion::set_reduced(false);
}

/// The Studio's title row is laid out the same in Arabic as in English (logo, File, Edit, View,
/// gear at the left, title centred, buttons at the right); a menu opens under its opener, kept in
/// the window, its rows reading right to left. At 1280 and at 3440 wide, both themes.
#[test]
fn the_title_row_and_menus_are_the_same_places_in_arabic_and_english() {
    use crate::shared::app_language::{self, Lang};
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        for (w, h) in [(1280.0, 800.0), (3440.0, 1440.0)] {
            for (lang, tag) in [(Lang::En, ""), (Lang::Ar, "-ar")] {
                app_language::set(lang);
                let mut s = studio_on(300, 200);
                let _ = s.update(Msg::Resized(Size::new(w, h)));
                let _ = s.update(Msg::Menu(Some(menu_bar::Menu::View)));
                let size = Size::new(w, h);
                let file = format!("studio-titlerow-{w}{tag}-{}", theme.id);
                failed.extend(golden::check("screens", &file, size, &[1.0], || {
                    view(&s, &[], false)
                }));
            }
        }
        app_language::set(Lang::En);
    });
}

/// F8 (#213): opened with no steps, the Steps sidebar is closed (no empty panel); the first
/// step opens it; with steps it stays as the user has it.
#[test]
fn f8_the_sidebar_starts_closed_without_steps_and_opens_with_the_first() {
    let mut s = studio_on(100, 100);
    assert!(s.sidebar_shown(), "the user's own panels");
    s.steps_at_open(2);
    assert!(s.sidebar_shown(), "steps: as the user has it");
    s.steps_at_open(0);
    assert!(!s.sidebar_shown(), "no steps: closed");
    s.step_pending(1);
    assert!(s.sidebar_shown(), "the first step opens it");
    // the user closes it (Ctrl+B): the next step does not reopen it
    let _ = s.update(Msg::Library);
    assert!(!s.sidebar_shown());
    s.step_pending(1);
    assert!(
        !s.sidebar_shown(),
        "only the first step after an empty start opens it"
    );
}

/// F8: Ctrl+B before any step is the user's own say: the first step then leaves it closed.
#[test]
fn f8_toggling_the_sidebar_by_hand_ends_the_auto_open() {
    let mut s = studio_on(100, 100);
    s.steps_at_open(0);
    let _ = s.update(Msg::Library); // open by hand
    let _ = s.update(Msg::Library); // closed by hand
    s.step_pending(1);
    assert!(!s.sidebar_shown());
}

/// F8: one Add step button instead of three look-alike ones; its menu has Pictures, Paste
/// and This picture (the last only with a picture open).
#[test]
fn f8_the_sidebar_has_one_add_step_button() {
    use crate::shared::kit::testing::shot;
    let s = studio_on(100, 100);
    let texts = shot::texts(sidebar::view(&s, &[]));
    for gone in ["Add pictures…", "Add to steps", "Paste as step"] {
        assert!(!texts.iter().any(|t| t == gone), "{gone}: {texts:?}");
    }
    let labels =
        |s: &Studio| -> Vec<String> { sidebar::add_rows(s).into_iter().map(|r| r.0).collect() };
    assert_eq!(labels(&s), ["Pictures…", "Paste", "This picture"]);
    let empty = Studio::new(docked());
    assert_eq!(
        labels(&empty),
        ["Pictures…", "Paste"],
        "no picture: no This picture"
    );
}

/// F15 (#220): a popover from the options bar hangs inside the window less the Steps sidebar
/// (the Actions card covered the sidebar), on either side; Copy text and Edit text no longer
/// share the "T" glyph.
#[test]
fn f15_popovers_stay_off_the_sidebar_and_edit_text_has_its_own_icon() {
    use crate::shared::config::StudioSide;
    let s = studio_on(640, 400);
    let room = s.popover_room();
    // the default: the sidebar at the end, the room ends where it starts
    assert_eq!(room.x, 0.0);
    assert_eq!(
        room.x + room.width,
        s.window.width - (SIDEBAR + SLIDER_EDGE)
    );
    // the end-hung Actions card (324 px) is placed inside it
    let button = iced::Rectangle::new(Point::new(room.width - 60.0, 50.0), Size::new(90.0, 32.0));
    let at = crate::shared::kit::popover::place_in(button, Size::new(324.0, 200.0), true, room);
    assert!(at.x + 324.0 <= room.x + room.width, "{at:?}");
    with_config(
        |c| c.studio.steps_side = StudioSide::Start,
        || {
            let s = studio_on(640, 400);
            let room = s.popover_room();
            assert_eq!(room.x, SIDEBAR + SLIDER_EDGE, "the sidebar at the start");
            assert_eq!(room.x + room.width, s.window.width);
        },
    );
    let mut closed = studio_on(640, 400);
    closed.steps_at_open(0);
    assert_eq!(
        closed.popover_room().width,
        closed.window.width,
        "no sidebar"
    );
    assert_ne!(
        popovers::EDIT_TEXT_ICON,
        crate::features::snip::snip_more::Row::CopyText.glyph()
    );
}

/// W98 #202 (criterion 202.6): Ctrl+wheel over the selected box resizes it; off it, it zooms.
#[test]
fn w98_202_6_studio_ctrl_wheel_resizes_over_the_selection_and_zooms_elsewhere() {
    let mut s = two_boxes();
    let _ = s.update(Msg::Editor(Message::SelectIndex(1, false)));
    let _ = s.update(Msg::Ctrl(true));
    let (zoom, depth) = {
        let e = s.editor.as_ref().unwrap();
        (e.zoom, e.doc.depth())
    };
    let origin = s.image_point(Point::ORIGIN);
    let at = Point::new((350.0 - origin.x) * zoom, (240.0 - origin.y) * zoom);
    let width = |s: &Studio| s.editor.as_ref().unwrap().selection_bounds().unwrap().width;
    let w = width(&s);
    let _ = s.update(Msg::Wheel { at, notches: 2.0 });
    assert!(width(&s) > w, "resized");
    assert_eq!(
        s.editor.as_ref().unwrap().zoom,
        zoom,
        "the picture did not zoom"
    );
    assert_eq!(s.editor.as_ref().unwrap().doc.depth(), depth + 1);
    // off the selection (the first box) the wheel zooms the picture
    let off = Point::new((60.0 - origin.x) * zoom, (50.0 - origin.y) * zoom);
    let _ = s.update(Msg::Wheel {
        at: off,
        notches: 1.0,
    });
    assert!(s.editor.as_ref().unwrap().zoom > zoom, "zoomed");
}

/// C5: in the Studio the first drag that starts on a shape (drawing tool) says "Ctrl+drag moves".
#[test]
fn c5_the_studio_says_ctrl_drag_moves_on_the_first_drag_over_a_shape() {
    use crate::shared::config::{self, Config};
    config::with_test_config(Config::default(), || {
        let mut s = studio_on(640, 400);
        draw(&mut s);
        let _ = s.update(Msg::Editor(Message::Deselect));
        let out = s.update(Msg::Editor(Message::Press(Point::new(60.0, 40.0))));
        assert!(!matches!(&out, Outcome::Say(_)), "a press alone is no drag");
        let out = s.update(Msg::Editor(Message::Move(Point::new(70.0, 50.0))));
        assert!(matches!(&out, Outcome::Say(t) if t == "Ctrl+drag moves"));
    });
}

/// W102: "Hide this shape" lives in the Actions menu and the right-click menu of a selected
/// shape (not in the option bar, which stays as approved); a pick adds the Hide and closes the menu.
#[test]
fn hide_this_shape_is_in_the_actions_menu_and_the_right_click_menu() {
    use crate::features::snip::annotate::{Edit, Item, Shape};
    let mut s = boxed(300, 200);
    let ed = s.editor.as_mut().unwrap();
    let a = Point::new(40.0, 40.0);
    ed.doc.exec(Edit::Insert(
        0,
        Item {
            shape: Shape::Ellipse {
                a,
                b: Point::new(120.0, 100.0),
            },
            colour: [255, 0, 0, 255],
            width: 4.0,
            outline: false,
            fill: false,
            hidden: false,
        },
    ));
    ed.doc.select_one(Some(0));
    let actions = editing::object_menu(ed)
        .iter()
        .map(|r| r.action)
        .collect::<Vec<_>>();
    assert!(actions.contains(&editing::Action::HideShape));
    assert!(actions.contains(&editing::Action::HideOutline));
    s.popover = Some(popovers::Pop::Actions);
    let _ = s.update(Msg::Editor(annotate::Message::HideShape(true)));
    assert!(s.popover.is_none(), "a pick closes the Actions menu");
    let ed = s.editor.as_ref().unwrap();
    assert!(matches!(
        ed.doc.items.last().unwrap().shape,
        Shape::Blur { .. }
    ));
    // a text object has neither row
    let mut s = boxed(300, 200);
    assert!(
        !editing::object_menu(s.editor.as_ref().unwrap())
            .iter()
            .any(|r| r.action == editing::Action::HideShape)
    );
    let _ = &mut s;
}

/// W107-4: the FIRST crop is as smooth as the later ones: a picture of a typical snip's size (1640 x
/// 1270, over 1 Mpx) shown smaller than itself is drawn from a stage copy made on a thread when it opens
/// (not whole through the linear pipeline on every frame of the crop drag, which cost 6.1 ms a frame in
/// release against 0.4 for the already-cropped, smaller picture), as a 4K one already was; a small one
/// is itself. The timings: `studio_crop_frame_times`.
#[test]
fn w107_4_a_snip_sized_picture_gets_its_stage_copy_before_the_first_crop() {
    for (w, h, copy) in [(1640, 1270, true), (3840, 2160, true), (1200, 800, false)] {
        let mut s = studio_on(w, h);
        // the copy is made on a thread: a slow CI runner needs more than a second for 4K (CI 45588669323)
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < until {
            let _ = s.update(Msg::Frame(std::time::Instant::now()));
            if s.mips.as_ref().is_none_or(|m| m.pending.is_none()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let e = s.editor.as_ref().unwrap();
        assert!(e.zoom < 0.9, "{w}x{h} is shown smaller: {}", e.zoom);
        assert_eq!(e.display.is_some(), copy, "{w}x{h}");
        assert_eq!(e.crisp, copy, "{w}x{h}");
    }
}

/// W107 fix 5: opening a picture below the stage-copy line drops the earlier picture's copy.
#[test]
fn w107_a_small_picture_drops_the_old_stage_copy() {
    let mut s = studio_on(1640, 1270);
    for _ in 0..500 {
        let _ = s.update(Msg::Frame(std::time::Instant::now()));
        if s.mips.as_ref().is_some_and(|m| m.pending.is_none()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(s.mips.is_some(), "the big picture has its copy");
    s.open(Picture::Crop(crop(300, 200)));
    s.settle_open();
    assert!(s.mips.is_none(), "a small picture frees it");
}

/// A picture with detail (not flat: a flat one hides what costs).
fn photo(width: u32, height: u32) -> Crop {
    let mut c = crop(width, height);
    for (i, px) in c.rgba.chunks_mut(4).enumerate() {
        let (x, y) = (i as u32 % width, i as u32 / width);
        let n = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 24;
        px.copy_from_slice(&[
            (x * 255 / width) as u8,
            (y * 255 / height) as u8,
            n as u8,
            255,
        ]);
    }
    c
}

/// The Studio's crop, the FIRST one of a picture and the next ones (W107-4): each frame of a drag
/// and of the release that applies it, in milliseconds (state change + layout + draw + raster).
/// Measurement: `cargo test --release studio_crop_frame_times -- --ignored --nocapture`.
#[test]
#[ignore = "timing: run by hand, in release"]
fn studio_crop_frame_times() {
    for (w, h) in [(1640, 1270), (3840, 2160)] {
        let mut s = Studio::new(docked());
        s.open(Picture::Crop(photo(w, h)));
        s.settle_open();
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Crop)));
        // (the stage copy a big picture draws from is made on a thread: the app's frames collect it)
        // the copy is made on a thread: a slow CI runner needs more than a second for 4K (CI 45588669323)
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < until {
            let _ = s.update(Msg::Frame(std::time::Instant::now()));
            if s.mips.as_ref().is_none_or(|m| m.pending.is_none()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let window = s.window;
        let mut report = Vec::new();
        for round in 0..3 {
            let (x, y) = (200.0 + 40.0 * round as f32, 150.0 + 30.0 * round as f32);
            let frames = crate::shared::kit::testing::shot::frames_of(window, 34, |i, show| {
                let m = match i {
                    0 => Some(Message::Press(Point::new(x, y))),
                    1..=24 => Some(Message::Move(Point::new(
                        x + 20.0 * i as f32,
                        y + 14.0 * i as f32,
                    ))),
                    25 => Some(Message::Release),
                    _ => None,
                };
                if let Some(m) = m {
                    let _ = s.update(Msg::Editor(m));
                }
                if i > 25 {
                    let _ = s.update(Msg::Tick);
                }
                show(view(&s, &[], false));
            });
            let ms = |f: &crate::shared::kit::testing::shot::Frame| f.draw_ms + f.raster_ms;
            let drag = frames[1..25].iter().map(ms).collect::<Vec<_>>();
            let mean = drag.iter().sum::<f64>() / drag.len() as f64;
            let worst = drag.iter().cloned().fold(0.0, f64::max);
            let apply = ms(&frames[25]);
            let after = frames[26..].iter().map(ms).fold(0.0, f64::max);
            let area: f32 = frames[1..25]
                .iter()
                .map(|f| f.damage.iter().map(|r| r.width * r.height).sum::<f32>())
                .sum::<f32>()
                / 24.0;
            report.push(format!(
                "(crop set: {}, damage/frame {:.0} px, window {:?})\n  crop {}: first drag frame {:.1} ms, drag mean {mean:.1} / worst {worst:.1}, release {apply:.1}, after worst {after:.1}",
                s.editor.as_ref().unwrap().doc.crop.is_some(),
                area,
                window,
                round + 1,
                ms(&frames[1])
            ));
        }
        println!("{w}x{h}:\n  {}", report.join("\n  "));
    }
}

/// W108 (owner round 3): the colour row ends with a custom colour ("+") that opens the SAME colour
/// picker Snip's toolbar has (HEX field, eyedropper, presets): one component; a colour typed there
/// is the drawing colour, and the row then shows it as picked.
#[test]
fn w108_the_colour_row_ends_with_a_custom_colour_that_opens_snips_picker() {
    use crate::shared::kit::testing::shot;
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = s.update(Msg::Resized(SIZE));
    let opens = |s: &Studio| {
        let e = s.editor.as_ref().unwrap();
        let size = Size::new(s.window.width - 54.0, 48.0);
        (0..(size.width / 4.0) as usize).any(|i| {
            let at = Point::new(4.0 * i as f32, 24.0);
            shot::click(options_bar::view(s, e), size, at)
                .iter()
                .any(|m| matches!(m, Msg::Pop(Some(popovers::Pop::Custom))))
        })
    };
    assert!(opens(&s), "the \"+\" is on the bar");
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Custom)));
    // the one picker: the square and its recent colours, not the style tools (W112)
    let words = shot::texts(popovers::content(&s, popovers::Pop::Custom));
    assert!(
        !words.iter().any(|w| w == "Copy style" || w == "Presets"),
        "no style tools in the picker: {words:?}"
    );
    let _ = s.update(Msg::Editor(Message::Hex("#12AB34".into())));
    let e = s.editor.as_ref().unwrap();
    assert_eq!(e.colour, [0x12, 0xAB, 0x34, 255], "the typed colour draws");
    assert!(opens(&s), "the custom swatch still opens the picker");
    // the eyedropper closes the popover: the next click picks from the picture
    let _ = s.update(Msg::Editor(Message::Eyedropper));
    assert_eq!(s.popover, None);
}

/// Narrow bar: the Colour popover holds the eight and the custom one.
#[test]
fn w108_the_narrow_colour_popover_has_the_custom_colour_too() {
    use crate::shared::kit::testing::shot;
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = s.update(Msg::Resized(super::MIN));
    let size = Size::new(400.0, 48.0);
    let got = (0..50).any(|i| {
        let at = Point::new(8.0 * i as f32, 24.0);
        shot::click(popovers::content(&s, popovers::Pop::Colours), size, at)
            .iter()
            .any(|m| matches!(m, Msg::Pop(Some(popovers::Pop::Custom))))
    });
    assert!(got);
}

/// W117: in Studio (Pop::Custom anchored under the custom colour button, at non-100 % UI scale),
/// pressing and dragging known local points on the square and hue bar puts the handle right under
/// the pointer.
#[test]
fn w117_studio_magnifier_options_enable_colour_stroke_outline_zoom_and_shape() {
    use crate::features::snip::screens::options_row::can;
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Magnifier)));
    let e = s.editor.as_ref().unwrap();
    let c = can(e, Tool::Magnifier);
    assert!(c.colour && c.stroke && c.outline && !c.fill && !c.opacity);
    // Default 1280 width: Outline inline, tool-specific groups in More (tool_group)
    let bar = options_bar::view(&s, e);
    let words = crate::shared::kit::testing::shot::texts(bar);
    assert!(
        words.iter().any(|w| w == "Outline"),
        "missing Outline: {words:?}"
    );
    let more_words = crate::shared::kit::testing::shot::texts(options_bar::tool_group(e));
    for expected in ["2×", "3×", "4×"] {
        assert!(
            more_words.iter().any(|w| w == expected),
            "missing {expected} in More: {more_words:?}"
        );
    }
    // Wide window: 2×/3×/4× shown inline on the bar
    let _ = s.update(Msg::Resized(Size::new(1600.0, 820.0)));
    let wide = options_bar::view(&s, s.editor.as_ref().unwrap());
    let wide_words = crate::shared::kit::testing::shot::texts(wide);
    for expected in ["Outline", "2×", "3×", "4×"] {
        assert!(
            wide_words.iter().any(|w| w == expected),
            "missing {expected} inline: {wide_words:?}"
        );
    }
}

#[test]
fn w117_studio_colour_picker_puts_handle_under_pointer() {
    use crate::shared::kit::testing::shot;
    use crate::shared::kit::tokens::space;
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = s.update(Msg::Resized(SIZE));
    let _ = s.update(Msg::UiScale(2)); // 125 % UI zoom
    // Render once so the custom-colour anchor records its bounds, then open Pop::Custom
    let _ = shot::render(view(&s, &[], false), SIZE.width as u32, SIZE.height as u32);
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Custom)));
    s.slide = None;
    let anchor = s.anchors.of(popovers::Pop::Custom).get();
    assert!(anchor.width > 0.0, "anchor tracked");
    let (card_sz, end) = popovers::Pop::Custom.card();
    let card_at = crate::shared::kit::popover::place_in(anchor, card_sz, end, s.popover_room());
    // W121: no card around the picker now; only colour_picker's own PAD = 10.0
    let pad = 10.0;
    let sq_w = crate::shared::kit::colour::PICKER_W - 20.0;
    let sq_h = 150.0_f32;
    let hue_y = card_at.y + pad + sq_h + space::GAP + 7.0;

    let from = Point::new(card_at.x + pad + 0.20 * sq_w, card_at.y + pad + 0.25 * sq_h);
    let to = Point::new(card_at.x + pad + 0.65 * sq_w, card_at.y + pad + 0.70 * sq_h);
    for m in shot::press_move(view(&s, &[], false), SIZE, from, to) {
        let _ = s.update(m);
    }
    let [_, sat, val] = s.editor.as_ref().unwrap().hsv().expect("studio hsv square");
    assert!(
        (sat - 0.65).abs() < 0.02 && (val - 0.30).abs() < 0.02,
        "studio square: ({sat:.3}, {val:.3})"
    );

    let h_from = Point::new(card_at.x + pad + 0.15 * sq_w, hue_y);
    let h_to = Point::new(card_at.x + pad + 0.55 * sq_w, hue_y);
    for m in shot::press_move(view(&s, &[], false), SIZE, h_from, h_to) {
        let _ = s.update(m);
    }
    let [h, _, _] = s.editor.as_ref().unwrap().hsv().expect("studio hsv hue");
    assert!((h - 0.55 * 360.0).abs() < 2.0, "studio hue: {h:.1}");
}

/// W121 (owner, 2026-11-26): the Studio's colour picker is Snip's picker panel alone: no card
/// around it and no X of its own. Its size is the kit picker's (`colour::PICKER_W`); a press just
/// past its end edge is outside it (it closes the popover), where the old card's padding and X were.
#[test]
fn w121_the_studio_picker_is_snips_panel_with_no_card_and_no_x() {
    use crate::shared::kit::testing::shot;
    let mut s = studio_on(640, 400);
    let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
    let _ = s.update(Msg::Pop(Some(popovers::Pop::Custom)));
    assert!(!shot::texts(popovers::content(&s, popovers::Pop::Custom)).is_empty());
    assert_eq!(
        popovers::Pop::Custom.card().0.width,
        crate::shared::kit::colour::PICKER_W,
        "the card is the picker's own width: nothing around it"
    );
    // not drawn yet: the fallback place, the panel's start edge at `left`
    let left = if s.sidebar_at_start() {
        8.0
    } else {
        super::RAIL + 40.0
    };
    let y = super::HEADER + super::OPTIONS + 4.0 + 120.0;
    let at = |dx: f32| Point::new(left + dx, y);
    let closes = |dx: f32| {
        shot::click(view(&s, &[], false), SIZE, at(dx))
            .iter()
            .any(|m| matches!(m, Msg::Pop(None)))
    };
    assert!(
        closes(crate::shared::kit::colour::PICKER_W + 3.0),
        "a press 3 px past the picker is outside it (no card padding, no X)"
    );
    assert!(!closes(20.0), "a press inside the picker does not close it");
}

/// W121: the Studio's picker popover, light and dark, English and Arabic.
#[test]
fn w121_the_studio_picker_matches_its_golden_images() {
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    golden::each_theme(|theme, failed| {
        let mut s = studio_on(640, 400);
        let _ = s.update(Msg::Editor(Message::Tool(Tool::Rect)));
        let _ = s.update(Msg::Pop(Some(popovers::Pop::Custom)));
        let file = format!("studio-picker-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
        let _ar = crate::shared::kit::testing::arabic();
        let file = format!("studio-picker-ar-{}", theme.id);
        failed.extend(golden::check("screens", &file, SIZE, &[1.0], || {
            view(&s, &[], false)
        }));
    });
}
