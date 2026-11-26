//! W123: the Steps panel is a slider.
//!
//! AC1 closed by default: a slim handle on the edge (the end of the reading direction), the
//!     step count and a tooltip.
//! AC2 opening slides it OVER the canvas (the stage does not move).
//! AC3 pin = docked: the stage makes room and refits.
//! AC4 it closes with the handle, the header arrow, Ctrl+B, Esc and a press outside (not
//!     pinned, never mid-drag).
//! AC5 motion: 220 ms in / 180 ms out, reduced = none, the frame clock only while it slides.
//! AC6 open and pinned are remembered (config).
//! AC7 mirrored in Arabic.
//! AC8 contents: four thumbnails to a row at 320 px.
use super::*;
use crate::shared::kit::motion;
use std::sync::Arc;
use std::time::Duration;

fn slider() -> Studio {
    slider_of(300, 200)
}

fn slider_of(w: u32, h: u32) -> Studio {
    let mut s = Studio::new(StudioConfig::default());
    s.open(Picture::Crop(crop(w, h)));
    s.settle_open();
    s
}

fn three_steps() -> Vec<Arc<Step>> {
    [(300, 200), (200, 300), (400, 120)]
        .map(|(w, h)| Arc::new(Step::new(&crop(w, h)).unwrap()))
        .into()
}

fn click_with(s: &Studio, steps: &[Arc<Step>], at: (f32, f32)) -> Vec<Msg> {
    crate::shared::kit::testing::shot::click(view(s, steps, false), SIZE, Point::new(at.0, at.1))
}

fn word_x(s: &Studio, steps: &[Arc<Step>], word: &str) -> Option<f32> {
    crate::shared::kit::testing::shot::texts_at(view(s, steps, false), SIZE)
        .into_iter()
        .find(|(_, t)| t.trim_start_matches('\u{200f}') == word)
        .map(|(r, _)| r.x)
}

#[test]
fn w123_ac1_the_panel_is_closed_by_default_with_a_handle_on_the_edge() {
    crate::shared::kit::text::load();
    let (s, steps) = (slider(), three_steps());
    assert!(!s.sidebar_shown() && !s.steps_overlay() && !s.sidebar_docked());
    assert!(!s.moving(), "nothing runs at rest");
    // the handle (right edge, mid-height) opens it; the count is on it
    let got = click_with(&s, &steps, (SIZE.width - 8.0, 420.0));
    assert!(matches!(got.as_slice(), [Msg::Library]), "{got:?}");
    assert!(word_x(&s, &steps, "3").is_some(), "the step count");
    assert!(word_x(&s, &steps, "Steps").is_none(), "no panel");
    // Arabic: the handle is on the left edge
    let _ar = crate::shared::kit::testing::arabic();
    let got = click_with(&s, &steps, (8.0, 420.0));
    assert!(matches!(got.as_slice(), [Msg::Library]), "{got:?}");
}

#[test]
fn w123_ac2_opening_slides_over_the_canvas_without_moving_it() {
    motion::set_reduced(false);
    crate::shared::kit::text::load();
    let (mut s, steps) = (slider(), three_steps());
    let stage = s.stage();
    let zoom = s.editor.as_ref().unwrap().zoom;
    let out = s.update(Msg::Library);
    assert!(matches!(out, Outcome::Panels(_, p) if p.library && !p.pinned));
    assert!(s.moving(), "the frame clock runs while it slides");
    assert!(s.steps_overlay() && !s.sidebar_docked());
    assert_eq!((s.stage(), s.editor.as_ref().unwrap().zoom), (stage, zoom));
    let start = s.steps_slide.unwrap().0;
    let _ = s.update(Msg::Frame(start + Duration::from_millis(60)));
    let mid = word_x(&s, &steps, "Steps").unwrap();
    let _ = s.update(Msg::Frame(start + Duration::from_millis(400)));
    assert!(!s.moving(), "the frame clock stops when it lands");
    let rest = word_x(&s, &steps, "Steps").unwrap();
    assert!(mid > rest, "still on its way in: {mid} vs {rest}");
    assert_eq!((s.stage(), s.editor.as_ref().unwrap().zoom), (stage, zoom));
}

#[test]
fn w123_ac3_pinned_docks_and_the_canvas_makes_room() {
    motion::set_reduced(true);
    let mut s = slider_of(3840, 2160);
    let wide = (s.stage(), s.editor.as_ref().unwrap().zoom);
    let _ = s.update(Msg::Library);
    assert_eq!(
        (s.stage(), s.editor.as_ref().unwrap().zoom),
        wide,
        "over it"
    );
    let out = s.update(Msg::PinSteps);
    assert!(matches!(out, Outcome::Panels(_, p) if p.library && p.pinned));
    assert!(s.sidebar_docked() && !s.steps_overlay());
    let (stage, zoom) = (s.stage(), s.editor.as_ref().unwrap().zoom);
    assert_eq!(stage.width, wide.0.width - (super::SIDEBAR + 1.0));
    assert_eq!(zoom, wide.1, "R-18: pinning keeps the zoom unchanged");
    // unpinned again: over the canvas, the picture gets its room back
    let _ = s.update(Msg::PinSteps);
    assert_eq!(s.stage(), wide.0);
    motion::set_reduced(false);
}

#[test]
fn w123_ac4_it_closes_by_every_way_it_may() {
    motion::set_reduced(true);
    let steps = three_steps();
    // the header arrow sends Library (so do the handle and Ctrl+B)
    let mut s = slider();
    let _ = s.update(Msg::Library);
    let arrow = click_with(
        &s,
        &steps,
        (SIZE.width - 24.0, super::HEADER + super::OPTIONS + 22.0),
    );
    assert!(arrow.iter().any(|m| matches!(m, Msg::Library)), "{arrow:?}");
    assert!(matches!(s.update(Msg::Library), Outcome::Panels(_, p) if !p.library));
    // Esc closes the open panel
    let _ = s.update(Msg::Library);
    assert!(matches!(s.update(Msg::Cancel), Outcome::Panels(_, p) if !p.library));
    // a press outside: the catcher is over the canvas, not over the panel
    let _ = s.update(Msg::Library);
    let outside = click_with(&s, &steps, (300.0, 300.0));
    assert!(
        matches!(outside.as_slice(), [Msg::StepsAway]),
        "{outside:?}"
    );
    let inside = click_with(&s, &steps, (SIZE.width - 150.0, 600.0));
    assert!(
        !inside.iter().any(|m| matches!(m, Msg::StepsAway)),
        "{inside:?}"
    );
    // never while a step is being dragged
    let _ = s.update(Msg::StepPress(0));
    let _ = s.update(Msg::StepMove(Point::new(200.0, 40.0), 3));
    assert!(s.pick.dragging(), "a step is being dragged");
    assert!(matches!(s.update(Msg::StepsAway), Outcome::None));
    assert!(s.sidebar_shown());
    let _ = s.update(Msg::StepRelease);
    assert!(matches!(s.update(Msg::StepsAway), Outcome::Panels(_, p) if !p.library));
    // pinned: a press outside leaves it docked (no catcher is drawn), and Esc leaves it too
    let _ = s.update(Msg::Library);
    let _ = s.update(Msg::PinSteps);
    assert!(
        matches!(s.update(Msg::Cancel), Outcome::None),
        "Esc keeps a pinned panel"
    );
    assert!(s.sidebar_docked());
    let outside = click_with(&s, &steps, (300.0, 300.0));
    assert!(
        !outside.iter().any(|m| matches!(m, Msg::StepsAway)),
        "{outside:?}"
    );
    motion::set_reduced(false);
}

#[test]
fn w123_ac5_it_slides_220_in_180_out_and_not_at_all_when_motion_is_reduced() {
    assert_eq!(motion::PANEL_OPEN.as_millis(), 220);
    assert_eq!(motion::PANEL_CLOSE.as_millis(), 180);
    motion::set_reduced(false);
    let mut s = slider();
    let _ = s.update(Msg::Library);
    let t0 = s.steps_slide.unwrap().0;
    let _ = s.update(Msg::Frame(t0 + Duration::from_millis(210)));
    assert!(s.moving() && s.steps_at() < 1.0 + f32::EPSILON);
    let _ = s.update(Msg::Frame(t0 + Duration::from_millis(221)));
    assert!(!s.moving() && s.steps_at() == 1.0);
    let _ = s.update(Msg::Library);
    let t1 = s.steps_slide.unwrap().0;
    let _ = s.update(Msg::Frame(t1 + Duration::from_millis(170)));
    assert!(s.moving() && s.steps_at() >= 0.0);
    let _ = s.update(Msg::Frame(t1 + Duration::from_millis(181)));
    assert!(!s.moving() && !s.steps_overlay() && s.steps_at() == 0.0);
    // reduced motion: there at once, nothing runs
    motion::set_reduced(true);
    let _ = s.update(Msg::Library);
    assert!(!s.moving() && s.steps_at() == 1.0);
    let _ = s.update(Msg::Library);
    assert!(!s.moving() && s.steps_at() == 0.0 && !s.steps_overlay());
    motion::set_reduced(false);
}

/// A slide reversed half way goes on from where the panel is (not from the edge).
#[test]
fn w123_reversing_mid_slide_starts_from_the_current_position() {
    motion::set_reduced(false);
    let mut s = slider();
    let _ = s.update(Msg::Library);
    let t0 = s.steps_slide.unwrap().0;
    let _ = s.update(Msg::Frame(t0 + Duration::from_millis(60)));
    let here = s.steps_at();
    assert!(here > 0.05 && here < 0.95, "{here}");
    let _ = s.update(Msg::Library); // closes: from `here`, not from fully open
    assert!(
        (s.steps_at() - here).abs() < 0.02,
        "{} vs {here}",
        s.steps_at()
    );
    let t1 = s.steps_slide.unwrap().0;
    let _ = s.update(Msg::Frame(t1 + Duration::from_millis(40)));
    assert!(s.steps_at() < here);
    let _ = s.update(Msg::Frame(t1 + Duration::from_millis(181)));
    assert!(!s.moving() && !s.sidebar_shown());
}

#[test]
fn w123_ac6_open_and_pinned_are_remembered() {
    motion::set_reduced(true);
    let mut s = slider();
    let Outcome::Panels(old, new) = s.update(Msg::Library) else {
        panic!("the open state is written");
    };
    let Outcome::Panels(_, new2) = s.update(Msg::PinSteps) else {
        panic!("the pin is written");
    };
    // what the host writes: only what changed, onto the file's own copy
    let mut file = StudioConfig::default();
    file.apply_changes(&old, &new);
    file.apply_changes(&new, &new2);
    assert!(file.library && file.pinned);
    // a Studio opened from it comes up as it was left
    let mut again = Studio::new(file);
    again.steps_at_open(3);
    assert!(again.sidebar_docked());
    // (an empty start closes a docked panel: the first step brings it back)
    let mut empty = Studio::new(file);
    empty.steps_at_open(0);
    assert!(!empty.sidebar_shown());
    empty.step_pending(1);
    assert!(empty.sidebar_docked());
    motion::set_reduced(false);
}

#[test]
fn w123_ac7_the_panel_is_mirrored_in_arabic() {
    motion::set_reduced(true);
    crate::shared::kit::text::load();
    let (mut s, steps) = (slider(), three_steps());
    let _ = s.update(Msg::Library);
    let en = word_x(&s, &steps, "Steps").unwrap();
    let ar = {
        let _ar = crate::shared::kit::testing::arabic();
        word_x(&s, &steps, "الخطوات").unwrap()
    };
    assert!(en > SIZE.width / 2.0, "English: at the right edge ({en})");
    assert!(ar < SIZE.width / 2.0, "Arabic: at the left edge ({ar})");
    motion::set_reduced(false);
}

#[test]
fn w123_ac8_four_thumbnails_to_a_row_at_320_px() {
    assert_eq!((super::SIDEBAR, super::STEP_COLS), (320.0, 4));
    let row = 4.0 * super::STEP_CELL.width + 3.0 * super::STEP_GAP;
    assert!(row <= super::SIDEBAR - 2.0 * sidebar::PAD, "{row} fits");
}

/// The closed handle, the open overlay and the pinned panel, in both themes, English and Arabic.
#[test]
fn w123_the_slider_matches_its_golden_images() {
    use crate::shared::app_language::{self, Lang};
    use crate::shared::kit::testing::golden;
    crate::shared::kit::text::load();
    let steps = three_steps();
    golden::each_theme(|theme, failed| {
        motion::set_reduced(true); // (things at rest)
        for (lang, tag) in [(Lang::En, ""), (Lang::Ar, "-ar")] {
            app_language::set(lang);
            let mut s = slider();
            let name = format!("studio-slider-closed{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &steps, false)
            }));
            let _ = s.update(Msg::Library);
            let name = format!("studio-slider-open{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &steps, false)
            }));
            let _ = s.update(Msg::PinSteps);
            let name = format!("studio-slider-pinned{tag}-{}", theme.id);
            failed.extend(golden::check("screens", &name, SIZE, &[1.0], || {
                view(&s, &steps, false)
            }));
        }
        app_language::set(Lang::En);
        motion::set_reduced(false);
    });
}
