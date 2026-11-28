//! The pick screen's Picture | Video | Audio switch (`pick::Mode`): what it shows, its keys, and
//! that each choice runs the existing flows (the R1 record bar, the audio start).
use iced::{Point, Rectangle};

use crate::app::App;
use crate::app::message::Purpose;
use crate::app::tests::overlay_key;
use crate::app::tests::snipping_on;
use crate::features::snip::record::state::State;
use crate::features::snip::screens::pick::{Mode, Next, Pick, Snip};
use crate::features::snip::screens::record_bar::Phase;
use crate::features::snip::screens::tools::Which;
use crate::shared::kit::testing::shot;
use crate::shared::kit::tooltip::tests_support;

const AREA: Rectangle = Rectangle {
    x: 100.0,
    y: 100.0,
    width: 300.0,
    height: 200.0,
};

fn app() -> App {
    snipping_on(1280, 720, AREA)
}

fn snip(app: &App) -> &Snip {
    match &app.o.purpose {
        Purpose::Snip(s) => s,
        _ => unreachable!(),
    }
}

fn texts(s: &Snip, picking: bool) -> Vec<String> {
    shot::texts(s.view_with(picking))
}

/// The real recorder starts (a machine without the GStreamer plugins answers Error(Encoder):
/// the flow still reached `record_begin`).
fn assert_audio_started(app: &App) {
    assert!(
        matches!(
            app.rec.state,
            State::Recording { audio_only: true } | State::Error(_)
        ),
        "{:?}",
        app.rec.state
    );
}

/// PM-1: the switch is on the pick screen and gone once something is picked, dragged, or the
/// pick is the recording bar's Hide / a live pin.
#[test]
fn pm_1_the_switch_shows_only_while_picking() {
    let mut app = app();
    let all = |t: &[String]| {
        ["Picture", "Video", "Audio"]
            .iter()
            .all(|w| t.iter().any(|x| x == w))
    };
    assert!(all(&texts(snip(&app), true)), "on the pick screen");
    assert!(!all(&texts(snip(&app), false)), "gone once picked");
    let Purpose::Snip(s) = &mut app.o.purpose else {
        unreachable!()
    };
    s.hide = Some(false);
    assert!(!all(&texts(s, true)), "not for Hide's pick");
    s.hide = None;
    s.live_pin = true;
    assert!(!all(&texts(s, true)), "not for a live pin");
    s.live_pin = false;
    let _ = s.press(Point::new(10.0, 10.0));
    assert!(!all(&texts(s, true)), "not while dragging an area");
    let mut timed = self::app();
    let Purpose::Snip(s) = &mut timed.o.purpose else {
        unreachable!()
    };
    let _ = s.delay(3);
    assert!(!all(&texts(s, true)), "not during a countdown");
    let mut tooled = self::app();
    let _ = tooled.start_tool(Which::Picker);
    assert!(!all(&texts(snip(&tooled), true)), "not with a tool open");
    assert!(!tooled.pick_switch_on());
}

/// PM-2: every pick starts on Picture (never remembered); the record-video key's pick on Video.
#[test]
fn pm_2_every_pick_starts_on_picture() {
    let place = app().place();
    let mut first = Snip::new(vec![Pick::window(AREA, "App", 1)], place, true, None);
    assert_eq!(first.mode(), Mode::Picture);
    first.set_mode(Mode::Video);
    assert_eq!(first.mode(), Mode::Video);
    let next = Snip::new(vec![Pick::window(AREA, "App", 1)], place, true, None);
    assert_eq!(next.mode(), Mode::Picture, "a new pick forgets it");
    let mut keyed = Snip::new(vec![], place, true, None);
    keyed.record = true; // what Alt+Shift+R sets
    assert_eq!(
        keyed.mode(),
        Mode::Video,
        "opened by the record key: Video selected"
    );
}

/// PM-3: Video, then a pick, ends in the R1 Ready bar (the same `record_picked` as Alt+Shift+R).
#[test]
fn pm_3_video_then_pick_opens_the_ready_bar() {
    let mut app = app();
    let _ = app.on_pick_mode(Mode::Video);
    assert!(snip(&app).record);
    let _ = app.snip_next(Some(Next::Edit(AREA)));
    assert_eq!(
        app.windows.record_bar().map(|b| b.phase),
        Some(Phase::Ready)
    );
}

/// PM-4: Audio closes the pick and starts the audio recording at once (Alt+Shift+M's flow).
#[test]
fn pm_4_audio_starts_recording_and_closes_the_pick() {
    let mut app = app();
    let _ = app.on_pick_mode(Mode::Audio);
    assert_audio_started(&app);
    assert!(
        !matches!(app.o.purpose, Purpose::Snip(_)),
        "the pick is closed"
    );
}

/// PM-5: P / V / A pick a mode while picking, Tab and Shift+Tab cycle; with a pick made
/// the letters are not the switch's.
#[test]
fn pm_5_keys_drive_the_switch() {
    let mut app = app();
    overlay_key(&mut app, "v");
    assert_eq!(snip(&app).mode(), Mode::Video);
    overlay_key(&mut app, "p");
    assert_eq!(snip(&app).mode(), Mode::Picture);
    overlay_key(&mut app, "tab");
    assert_eq!(snip(&app).mode(), Mode::Video);
    overlay_key(&mut app, "shift+tab");
    assert_eq!(snip(&app).mode(), Mode::Picture);
    overlay_key(&mut app, "a");
    assert_audio_started(&app);
    assert_eq!(Mode::Audio.cycled(false), Mode::Picture, "Tab wraps");
}

/// PM-6: Alt+A, click, Ctrl+C still copies the picked window at once, the switch untouched.
#[test]
fn pm_6_the_alt_a_fast_path_is_unchanged() {
    let mut app = app();
    overlay_key(&mut app, "ctrl+c");
    assert!(app.o.edit.is_some(), "picked: the Snip toolbar");
    assert!(!snip(&app).record, "still Picture");
}

/// PM-7: each segment's tooltip carries its key.
#[test]
fn pm_7_each_segment_shows_its_key() {
    let app = app();
    let _ = tests_support::shown();
    let _ = snip(&app).view_with(true);
    let tips = tests_support::shown();
    for want in ["Picture · P", "Video · V", "Audio · A"] {
        assert!(tips.iter().any(|t| t == want), "{want} in {tips:?}");
    }
}

/// PM-8: with an area picked, a countdown running or a tool open, P / V / A and Tab are not the
/// switch's (the editor, the timer and the tools keep their keys).
#[test]
fn pm_8_the_keys_are_the_switchs_only_while_picking() {
    let mut picked = app();
    let _ = picked.snip_next(Some(Next::Edit(AREA)));
    assert!(picked.o.edit.is_some(), "picked: the Snip toolbar");
    for key in ["v", "a", "tab"] {
        overlay_key(&mut picked, key);
    }
    assert_eq!(
        snip(&picked).mode(),
        Mode::Picture,
        "the letters reach the editor"
    );
    assert_eq!(picked.rec.state, State::Idle, "no recording started");
    let mut timed = app();
    let Purpose::Snip(s) = &mut timed.o.purpose else {
        unreachable!()
    };
    let _ = s.delay(3);
    overlay_key(&mut timed, "v");
    assert_eq!(snip(&timed).mode(), Mode::Picture, "not during a countdown");
    let mut tooled = app();
    let _ = tooled.start_tool(Which::Picker);
    overlay_key(&mut tooled, "a");
    assert_eq!(tooled.rec.state, State::Idle, "not with a tool open");
}

/// PM-9: Audio while a recording runs does nothing: the pick stays, nothing is lost.
#[test]
fn pm_9_audio_during_a_recording_keeps_the_pick() {
    let mut app = app();
    app.rec.state = State::Recording { audio_only: false };
    let _ = app.on_pick_mode(Mode::Audio);
    assert!(matches!(app.o.purpose, Purpose::Snip(_)), "the pick stays");
    assert_eq!(snip(&app).mode(), Mode::Picture);
    assert_eq!(app.rec.state, State::Recording { audio_only: false });
}
