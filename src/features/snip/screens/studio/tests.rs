use iced::{Point, Rectangle, Size, Vector};

use super::*;
use crate::features::snip::annotate::{Message, Tool};
use crate::features::snip::screens::steps::Step;

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
            view(&empty, &[], false)
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
    assert_eq!(s.editor.as_ref().unwrap().doc.state(), before);
}

#[test]
fn the_steps_list_opens_reorders_and_deletes_the_strips_own_steps() {
    let mut s = studio_on(200, 100);
    let at = |y: f32| Point::new(40.0, y);
    // a click: open that step on the canvas
    let _ = s.update(Msg::StepMove(at(5.0), 3));
    let _ = s.update(Msg::StepPress(1));
    assert!(matches!(s.update(Msg::StepRelease), Outcome::OpenStep(1)));
    // a drag of the first to the third row: the strip reorders (no copy here)
    let _ = s.update(Msg::StepMove(at(5.0), 3));
    let _ = s.update(Msg::StepPress(0));
    let _ = s.update(Msg::StepMove(at(2.0 * STEP_PITCH + 5.0), 3));
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
    let mut s = Studio::new(StudioConfig::default());
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

#[test]
fn walkthrough_dialog_hands_its_choices_to_the_host() {
    let mut s = Studio::new(StudioConfig::default());
    let _ = s.update(Msg::ExportWalkthrough);
    assert!(s.exporting && s.walk_tab);
    let Outcome::Panels(panels) = s.update(Msg::WalkFormat(export::Movie::Mp4)) else {
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
            numbered: true
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
