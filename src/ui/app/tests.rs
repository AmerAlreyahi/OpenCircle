use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::{self};
use iced::widget::{container, image, space};
use iced::window::{self};
use iced::{Element, Fill, Point, Rectangle, Size, Task, Vector};

use crate::Trigger;
use crate::actions::{self, Crop, copy, lens, qr, text};
use crate::capture::CaptureError;
use crate::config::{self, After, QuickAction, SearchWith, SelectStyle, SelectionConfig, Target};
use crate::editor::{self};
use crate::labs::{self, BarButton};
use crate::ocr::{self, TextBlock};

use crate::ui::app::message::{BarKeys, Hiding, Message, Purpose, Shot, WordRead};
use crate::ui::app::opening::Opening;
use crate::ui::app::quick::Quick;
use crate::ui::app::{keys, word};
use crate::ui::edit::Editing;
use crate::ui::kit::panel_frame as card;
use crate::ui::screens::bar::geometry as bar;
use crate::ui::snip::Snip;
use crate::ui::{border, glass, overlay, panel, snip, song, translate, widgets, windows};

use super::actions::auto_targets;
use super::view::chips_at;
use super::*;
use crate::ui::kit::field;
use crate::ui::screens::bar::view::ask_placeholder;

use crate::actions::crop::STACK_GAP;
use crate::config::{BarAnchor, TextReader};
use crate::labs::Labs;

fn closing_app() -> App {
    let mut app = App::idle(window::Id::unique());
    app.o.start(Opening::default()); // open (each test sets what it shows)
    app.o.labs = Labs::all_on(); // (read at open)
    app
}

fn a_shot() -> Result<Shot, CaptureError> {
    Ok(Shot {
        image: image::Handle::from_rgba(4, 4, vec![9; 4 * 4 * 4]),
        scale: 1.0,
        origin: Point::ORIGIN,
        size: Size::new(4.0, 4.0),
        snip: None,
    })
}

#[test]
fn an_update_waits_until_nothing_is_open() {
    use crate::update::{Event, Install, Staged};
    let staged = Staged {
        version: "9.9.9".into(),
        file: "missing".into(),
        sha256: [0; 32],
        dir: std::env::temp_dir().join("oc-update-test-never-made"),
        install: Install::Manual, // even if it ran, it could not touch anything
        notes: String::new(),
        url: String::new(),
    };
    let mut app = App::idle(window::Id::unique());
    assert!(app.is_idle());
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    assert!(!app.is_idle(), "a capture is out");
    let _ = app.update(Message::Trigger(Trigger::Update(Event::Ready(
        staged.clone(),
    ))));
    assert_eq!(app.update, Some(staged.clone()), "kept for later");
    let _ = app.update(Message::Captured(
        app.capturing.unwrap(),
        Instant::now(),
        a_shot(),
    ));
    assert!(!app.is_idle(), "the overlay is open");
    let _ = app.update(Message::UpdateTick);
    assert_eq!(app.update, Some(staged), "still waiting");
    let _ = app.close();
    app.hiding = Hiding::No; // the hidden window drew its last frames
    assert!(app.is_idle());
    let _ = app.update(Message::UpdateTick);
    assert_eq!(app.update, None, "tried once idle");
}

#[test]
fn a_shortcut_while_open_brings_the_overlay_back_and_says_so() {
    let mut app = App::idle(window::Id::unique());
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    let n = app.capturing.expect("pending");
    let said = |line: &str| crate::log::tail(200).iter().any(|l| l.contains(line));
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    assert!(
        said("a capture is still on its way"),
        "a press left no line"
    );
    let _ = app.update(Message::Captured(n, Instant::now(), a_shot()));
    assert!(app.o.frame.is_some());
    let again = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    assert!(said("brought to the front"), "a press left no line");
    assert!(again.units() > 0, "the overlay takes the focus back");
    assert!(app.o.frame.is_some(), "still open, same frame");
}

#[test]
fn a_timed_out_capture_frees_the_trigger_and_its_late_answer_is_dropped() {
    let mut app = App::idle(window::Id::unique());
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    let first = app.capturing.expect("pending");
    // A second press while it is out does nothing.
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    assert_eq!(app.capturing, Some(first));
    let _ = app.update(Message::CaptureTimedOut(first));
    assert_eq!(app.capturing, None, "the next trigger may start");
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    let second = app.capturing.expect("a new capture started");
    assert_ne!(first, second);
    // The first capture's answer arrives late: ignored, the new one still waits.
    let _ = app.update(Message::Captured(first, Instant::now(), a_shot()));
    assert!(app.o.frame.is_none());
    assert_eq!(app.capturing, Some(second));
    // A stale timeout doesn't touch the new capture.
    let _ = app.update(Message::CaptureTimedOut(first));
    assert_eq!(app.capturing, Some(second));
    let _ = app.update(Message::Captured(second, Instant::now(), a_shot()));
    assert!(app.o.frame.is_some(), "the live one opens");
    assert_eq!(app.capturing, None);
}

#[test]
fn esc_cancels_a_pending_capture() {
    let mut app = App::idle(window::Id::unique());
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    let n = app.capturing.expect("pending");
    let _ = app.update(Message::Escape);
    assert_eq!(app.capturing, None);
    let _ = app.update(Message::Captured(n, Instant::now(), a_shot()));
    assert!(app.o.frame.is_none(), "the overlay doesn't open");
    assert_eq!(app.hiding, Hiding::No);
}

/// Trigger, capture, Esc: the overlay's window id while shown, and after.
fn open_and_escape(app: &mut App) -> (window::Id, window::Id) {
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    let n = app.capturing.expect("pending");
    let _ = app.update(Message::Captured(n, Instant::now(), a_shot()));
    assert!(app.o.frame.is_some() && app.overlay_open);
    let shown = app.overlay;
    let _ = app.update(Message::Escape);
    assert!(app.o.frame.is_none());
    (shown, app.overlay)
}

#[test]
fn where_windows_cannot_hide_each_opening_has_its_own_window_closed_after() {
    let mut app = App {
        can_hide: false,
        overlay_open: false,
        ..App::idle(window::Id::unique())
    };
    let mut ids = Vec::new();
    for _ in 0..3 {
        let (shown, _) = open_and_escape(&mut app);
        assert!(!app.overlay_open, "closed, not hidden");
        assert_eq!(app.hiding, Hiding::No, "no hide steps on a closed window");
        assert!(!ids.contains(&shown), "a fresh window each time");
        ids.push(shown);
    }
    // closing again (or a Copy that never showed it) closes nothing
    let _ = app.close();
    assert!(!app.overlay_open);
    assert_eq!(app.hiding, Hiding::No);
}

#[test]
fn where_windows_can_hide_the_one_pre_created_window_is_reused() {
    let first = window::Id::unique();
    let mut app = App::idle(first);
    for _ in 0..3 {
        let (shown, after) = open_and_escape(&mut app);
        assert_eq!((shown, after), (first, first));
        assert!(app.overlay_open, "kept, to be hidden");
        assert_eq!(app.hiding, Hiding::Empty);
        app.hiding = Hiding::No; // (the hide steps run on frames)
    }
}

/// The owner's gray flash: Esc drew the empty view (the theme's background)
/// over the whole screen, then the window left full-screen, still shown.
/// The window is cleared in the very update that closes it, before any of
/// those frames (a `Task` would land after one may be on screen).
#[test]
fn esc_clears_the_overlay_before_its_last_frames() {
    let first = window::Id::unique();
    let mut app = App::idle(first);
    let _ = open_and_escape(&mut app);
    let cleared = windows::CLEARED.with(|c| std::mem::take(&mut *c.borrow_mut()));
    assert_eq!(cleared, vec![first], "not cleared at the close");
    assert_eq!(app.hiding, Hiding::Empty, "then hidden by the usual steps");
}

#[test]
fn close_shrinks_then_hides() {
    let mut app = closing_app();
    let _ = app.close();
    let _ = app.update(Message::Drawn);
    let _ = app.update(Message::Drawn); // a full-size frame before the resize lands
    let _ = app.update(Message::Resized(Size::new(3440.0, 1440.0)));
    assert_eq!(app.hiding, Hiding::Shrinking(1));
    let _ = app.update(Message::Resized(Size::new(1.0, 1.0)));
    let _ = app.update(Message::Drawn);
    assert_eq!(app.hiding, Hiding::No);
}

#[test]
fn close_hides_even_if_the_window_never_shrinks() {
    let mut app = closing_app();
    let _ = app.close();
    for _ in 0..=SHRINK_FRAMES {
        let _ = app.update(Message::Drawn);
    }
    assert_ne!(app.hiding, Hiding::No);
    let _ = app.update(Message::Drawn);
    assert_eq!(app.hiding, Hiding::No);
}

#[test]
fn a_translation_ends_with_its_overlay() {
    let mut app = closing_app();
    let frame = image::Handle::from_rgba(40, 20, vec![0; 40 * 20 * 4]);
    // no models: the session waits for consent (no thread starts)
    let (session, _) =
        translate::Session::start(frame.clone(), [0, 0, 40, 20], 1.0, app.cache.clone(), false);
    app.o.frame = Some(frame);
    app.hiding = Hiding::No;
    app.o.translate = Some((7, session));
    let _ = app.update(Message::Translate(6, translate::Msg::Close));
    assert!(
        app.o.frame.is_some(),
        "a stale session's message is ignored"
    );
    let _ = app.update(Message::Peek(true));
    // declining the download closes the overlay and drops the session
    let _ = app.update(Message::Translate(7, translate::Msg::Close));
    assert!(app.o.translate.is_none() && app.o.frame.is_none());
    assert_eq!(app.hiding, Hiding::Empty);
}

/// An open overlay over `frame` with all of it selected (nothing scanned yet).
fn selected_app(frame: &Crop) -> (App, Rectangle) {
    let mut app = closing_app();
    let area = Rectangle::with_size(Size::new(frame.width as f32, frame.height as f32));
    app.o.frame = Some(image::Handle::from_rgba(
        frame.width,
        frame.height,
        frame.rgba.clone(),
    ));
    app.hiding = Hiding::No;
    app.o.screen = area.size();
    app.o.view = overlay::View::new(frame.width, frame.height, app.o.screen);
    app.o.selection = Some(overlay::Selection {
        area,
        kind: overlay::Kind::Drag,
    });
    (app, area)
}

fn overlay_texts(app: &App) -> Vec<String> {
    widgets::shot::texts(app.view(app.overlay))
}

#[test]
fn a_qr_code_in_the_selection_gets_a_chip_and_nothing_opens() {
    // A code (selected) on the right of a wider grey screen.
    let code = qr::tests::qr_crop("https://example.com/menu", 5, 0);
    let mut frame = Crop {
        width: 1200,
        height: 600,
        rgba: [140, 140, 140, 255].repeat(1200 * 600),
    };
    for y in 0..code.height as usize {
        let from = y * code.width as usize * 4;
        let to = ((y + 20) * 1200 + 700) * 4;
        let row = code.width as usize * 4;
        frame.rgba[to..to + row].copy_from_slice(&code.rgba[from..from + row]);
    }
    let (mut app, _) = selected_app(&frame);
    let area = Rectangle::new(
        Point::new(700.0, 20.0),
        Size::new(code.width as f32, code.height as f32),
    );
    app.o.selection = Some(overlay::Selection {
        area,
        kind: overlay::Kind::Drag,
    });
    // The scan of a selection that has since moved on is dropped.
    let moved = Rectangle::with_size(area.size() * 0.5);
    let _ = app.update(Message::Scanned(moved, qr::scan(&frame)));
    assert!(app.o.scan.is_none());
    assert!(!overlay_texts(&app).iter().any(|t| t.starts_with("QR code")));

    let scan = qr::scan(&app.crop().unwrap());
    let _ = app.update(Message::Scanned(area, scan));
    let texts = overlay_texts(&app);
    assert!(
        texts.contains(&"QR code: https://example.com/menu".into()),
        "{texts:?}"
    );
    assert!(texts.contains(&"Open link · example.com".into()));
    assert!(
        app.o.toast.is_none() && app.o.frame.is_some(),
        "nothing happened by itself"
    );
    let size = app.o.screen;
    widgets::shot::save("overlay-qr-chip", app.view(app.overlay), size);

    // A new press forgets the old code.
    let _ = app.update(Message::Overlay(overlay::Event::Press {
        at: Point::new(5.0, 5.0),
        shift: false,
    }));
    assert!(app.o.scan.is_none());
}

/// What Search in browser hands to Lens is exactly the selected pixels, never
/// blank: through the real opening (`open`), at 100 % on the primary screen,
/// at 150 % on a monitor left of (and above) it, and at 125 % on its right.
#[test]
fn the_crop_lens_gets_is_the_selection_at_any_scale_and_monitor() {
    // pixel (x, y) of the frame = [x, y, x >> 8 | (y >> 8) << 4, 255]
    let px = |x: u32, y: u32| [x as u8, y as u8, (x >> 8 | (y >> 8) << 4) as u8, 255];
    for (scale, origin) in [
        (1.0, Point::ORIGIN),
        (1.5, Point::new(-1280.0, -100.0)),
        (1.25, Point::new(1920.0, 0.0)),
    ] {
        let (w, h) = ((1280.0 * scale) as u32, (720.0 * scale) as u32);
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| px(x, y)))
            .collect();
        let mut app = App::idle(window::Id::unique());
        let shot = Shot {
            image: image::Handle::from_rgba(w, h, rgba),
            scale,
            origin,
            size: Size::new(1280.0, 720.0),
            snip: None,
        };
        let _ = app.open(Instant::now(), Ok(shot));
        app.o.behaviour = config::Config::default().selection;
        drag(
            &mut app,
            Point::new(100.0, 120.0),
            Point::new(300.0, 220.0),
            false,
        );
        assert!(
            app.o.panel.is_none(),
            "{scale}: nothing sent before a choice"
        );
        let crop = app.crop().expect("a crop");
        let (x0, y0) = ((100.0 * scale) as u32, (120.0 * scale) as u32);
        assert_eq!(
            (crop.width, crop.height),
            ((200.0 * scale) as u32, (100.0 * scale) as u32),
            "{scale}"
        );
        for (i, p) in crop.rgba.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (x0 + i as u32 % crop.width, y0 + i as u32 / crop.width);
            assert_eq!(*p, px(x, y), "{scale}: pixel ({x}, {y})");
        }
        assert!(
            lens::lens_png(&crop).is_ok(),
            "{scale}: never refused as blank"
        );
    }
}

#[test]
fn a_black_capture_says_it_is_protected_and_keeps_the_bar() {
    let black = Crop {
        width: 600,
        height: 360,
        rgba: [0, 0, 0, 255].repeat(600 * 360),
    };
    let (mut app, area) = selected_app(&black);
    assert!(!overlay_texts(&app).contains(&PROTECTED.to_string()));
    let _ = app.update(Message::Scanned(area, qr::scan(&app.crop().unwrap())));
    let texts = overlay_texts(&app);
    assert!(texts.contains(&PROTECTED.to_string()), "{texts:?}");
    let texts = overlay_texts(&app);
    assert!(texts.contains(&lens::ACTION.icon.to_string()), "{texts:?}");
    widgets::shot::save("overlay-protected", app.view(app.overlay), area.size());
    // An ordinary capture shows no such pill.
    let grey = Crop {
        rgba: [90, 90, 90, 255].repeat(600 * 360),
        ..black
    };
    let (mut app, area) = selected_app(&grey);
    let _ = app.update(Message::Scanned(area, qr::scan(&app.crop().unwrap())));
    assert!(!overlay_texts(&app).contains(&PROTECTED.to_string()));
}

/// An open overlay on a `w` x `h` frame at `scale`; pixel (x, y) is
/// [x % 256, y % 256, x / 256, 255].
pub(in crate::ui) fn open_app(w: u32, h: u32, scale: f32) -> App {
    let mut app = App::idle(window::Id::unique());
    app.o.start(Opening::default());
    app.o.labs = Labs::all_on();
    let rgba: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, (x >> 8) as u8, 255]))
        .collect();
    let frame = image::Handle::from_rgba(w, h, rgba);
    app.o.tiles = overlay::tiles(&frame).map(|t| (frame.id(), t));
    app.o.frame = Some(frame);
    app.o.scale = scale;
    app.o.screen = Size::new(w as f32 / scale, h as f32 / scale);
    app.o.view = overlay::View::new(w, h, app.o.screen);
    app.o.bar_keys = BarKeys::of(&config::Config::default());
    app.o.appear = Some((Instant::now(), 1.0)); // opened
    app
}

fn event(app: &mut App, e: overlay::Event) {
    let _ = app.on_overlay(e);
}

/// Runs `task` to its end (its threads' work done) and hands its messages
/// to the app (their own tasks are dropped).
fn run_task(app: &mut App, task: Task<Message>) {
    let actions = iced_runtime::task::into_stream(task)
        .map(iced::futures::executor::block_on_stream)
        .into_iter()
        .flatten();
    for action in actions {
        if let iced_runtime::Action::Output(message) = action {
            let _ = app.update(message);
        }
    }
}

/// What this test's background work copied, once it has (within 10 s).
fn copied_image() -> Option<(u32, u32)> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        match copy::copied::image() {
            Some(size) => return Some(size),
            None if Instant::now() > until => return None,
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// A press-drag-release on screen from `a` to `b`.
pub(in crate::ui) fn drag(app: &mut App, a: Point, b: Point, shift: bool) {
    event(app, overlay::Event::Press { at: a, shift });
    for i in 1..=10 {
        event(app, overlay::Event::Move(a + (b - a) * (i as f32 / 10.0)));
    }
    event(app, overlay::Event::Release);
}

fn ring(cx: f32, cy: f32, r: f32) -> Vec<Point> {
    (0..=32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            Point::new(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn circle(app: &mut App, c: Point, r: f32, shift: bool) {
    let ring = ring(c.x, c.y, r);
    event(app, overlay::Event::Press { at: ring[0], shift });
    ring[1..]
        .iter()
        .for_each(|&p| event(app, overlay::Event::Move(p)));
    event(app, overlay::Event::Release);
}

fn close_to(a: Rectangle, b: Rectangle) -> bool {
    [
        (a.x, b.x),
        (a.y, b.y),
        (a.width, b.width),
        (a.height, b.height),
    ]
    .iter()
    .all(|(p, q)| (p - q).abs() < 0.01)
}

/// A snip over a 800x600 screen with one window (`area`) the pointer is on.
fn snipping(area: Rectangle) -> App {
    let mut app = open_app(800, 600, 1.0);
    let window = snip::Pick::window(area, "App", 1);
    app.o.purpose = Purpose::Snip(Box::new(Snip::new(vec![window], app.place(), true, None)));
    event(&mut app, overlay::Event::Hover(area.center()));
    app
}

#[test]
fn snip_ctrl_c_copies_the_area_under_the_pointer_as_an_image() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    let _ = copy::copied::image();
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'c',
            ctrl: true,
        },
    );
    assert_eq!(copied_image(), Some((300, 200)), "that area");
    assert!(copy::copied::text().is_none(), "the picture only, no text");
    assert!(app.snip_toast(), "and the toast says so");
    // off every window it is what a click takes there: the whole screen
    let mut app = snipping(area);
    event(&mut app, overlay::Event::Hover(Point::new(700.0, 500.0)));
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'c',
            ctrl: true,
        },
    );
    assert_eq!(copied_image(), Some((800, 600)));
}

/// The two shortcuts of #52: Ctrl+C copies the selection's picture, Ctrl+S saves
/// it in the save folder, each with a pill; a failure shows the pill without a dot.
#[test]
fn ctrl_c_and_ctrl_s_confirm_with_a_pill_and_failures_say_so() {
    let dir = std::env::temp_dir().join(format!("oc-pill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let before = config::current();
    let mut cfg = before.clone();
    cfg.general.save_folder = dir.to_string_lossy().into_owned();
    config::set_current(cfg);
    let key = |letter| overlay::Event::Key { letter, ctrl: true };
    let selected = || {
        let mut app = open_app(800, 600, 1.0);
        app.select(Rectangle::new(
            Point::new(100.0, 80.0),
            Size::new(300.0, 200.0),
        ));
        app
    };
    // Ctrl+C: the picture on the clipboard and the "Copied" pill
    // (#123: the encoding and the clipboard off the UI thread: the overlay
    // closes at once, the pill comes once the copy is done)
    let mut app = selected();
    let _ = copy::copied::image();
    let task = app.on_overlay(key('c'));
    assert!(app.o.frame.is_none() && app.windows.toasts().is_empty());
    run_task(&mut app, task);
    assert_eq!(copy::copied::image(), Some((300, 200)));
    assert_eq!(app.windows.toasts(), [("Copied".to_string(), true)]);
    // Ctrl+S: a file in the save folder and "Saved to <folder>"
    let mut app = selected();
    let task = app.on_overlay(key('s'));
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "not yet");
    run_task(&mut app, task);
    let toasts = app.windows.toasts();
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert!(
        toasts[0].1 && toasts[0].0.starts_with("Saved to "),
        "{toasts:?}"
    );
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    assert!(
        files[0]
            .file_name()
            .to_string_lossy()
            .starts_with("OpenCircle-")
    );
    // an error: the same pill, the error text, no dot
    let mut app = selected();
    let place = (app.o.origin, app.o.screen, app.o.scale);
    let failed = Err("Couldn't save. Pick a folder you can write to.".into());
    let _ = app.update(Message::Confirm(place, failed));
    assert_eq!(
        app.windows.toasts(),
        [(
            "Couldn't save. Pick a folder you can write to.".to_string(),
            false
        )]
    );
    // it goes after the design's 1800 ms
    assert_eq!(windows::TOAST_MS, 1800);
    config::set_current(before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_snip_timer_comes_from_settings_and_runs_once_per_snip() {
    let shot = || {
        let mut shot = a_shot().unwrap();
        shot.snip = Some((snip::Kind::Region, Vec::new(), None, Vec::new()));
        Ok(shot)
    };
    // (only the timer changes: other tests share the config)
    let with_delay = |secs| {
        let mut cfg = config::current();
        cfg.snip.delay = secs;
        config::set_current(cfg);
    };
    with_delay(3);
    let mut app = App::idle(window::Id::unique());
    let _ = app.open(Instant::now(), shot());
    assert!(app.snip_pill(), "counting down, the overlay is only a pill");
    // the capture the timer waited for opens plain
    app.timed = true;
    let _ = app.open(Instant::now(), shot());
    assert!(!app.snip_pill() && !app.timed);
    // no timer in Settings: no pill
    with_delay(0);
    let mut app = App::idle(window::Id::unique());
    let _ = app.open(Instant::now(), shot());
    assert!(!app.snip_pill());
}

#[test]
fn the_editors_scroll_button_starts_a_scrolling_capture_of_the_area() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    app.select(area);
    let _ = app.open_editor();
    let _ = app.update(Message::Editor(editor::Message::Scroll));
    assert!(app.o.edit.is_none(), "the editor steps aside");
    assert!(app.scroll().is_some(), "scrolling and stitching has begun");
    // no auto-scroll here (Wayland) or no snip (Edit in place): the button is dimmed
    let mut app = open_app(800, 600, 1.0);
    app.select(area);
    let _ = app.open_editor();
    let _ = app.update(Message::Editor(editor::Message::Scroll));
    assert!(app.o.edit.is_some() && app.scroll().is_none());
}

#[test]
fn after_a_drag_out_the_editor_is_still_there_or_the_overlay_closes_whole() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    // nothing took the drop: the overlay is back, toolbar and all
    let mut app = snipping(area);
    app.select(area);
    let _ = app.open_editor();
    editor::dragout::TEST_DROP.set(Some(false));
    let _ = app.update(Message::Editor(editor::Message::DragOut));
    assert!(app.editing() && app.o.frame.is_some());
    // an app took it: closed like Copy, nothing of the editor or the frame left
    editor::dragout::TEST_DROP.set(Some(true));
    let _ = app.update(Message::Editor(editor::Message::DragOut));
    assert!(app.o.edit.is_none() && app.o.frame.is_none() && !app.editing());
    assert_eq!(app.hiding, Hiding::Empty, "hides by the usual steps");
}

#[test]
fn handles_resize_and_moving_drags_at_any_zoom_and_scale() {
    // scale 2 (a 400x300 overlay on 800x600 pixels), zoomed in 1.5625x
    let mut app = open_app(800, 600, 2.0);
    event(
        &mut app,
        overlay::Event::Zoom {
            at: Point::new(200.0, 150.0),
            notches: 2.0,
        },
    );
    let v = app.o.view;
    assert!(v.zoom() > 1.5);
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(200.0, 180.0),
        false,
    );
    let sel = app.o.selection.expect("a rectangle");
    assert!(
        app.selected().units() > 0,
        "a drawn selection is scanned (QR)"
    );
    assert_eq!(sel.kind, overlay::Kind::Drag);
    let on_screen = v.rect_to_screen(sel.area);
    let want = Rectangle::new(Point::new(100.0, 100.0), Size::new(100.0, 80.0));
    assert!(close_to(on_screen, want), "{on_screen:?}");
    assert!(app.handles());

    // the bottom-right handle, 20 px right and 10 down on screen
    drag(
        &mut app,
        Point::new(200.0, 180.0),
        Point::new(220.0, 190.0),
        false,
    );
    let resized = v.rect_to_screen(app.o.selection.unwrap().area);
    let want = Rectangle::new(Point::new(100.0, 100.0), Size::new(120.0, 90.0));
    assert!(close_to(resized, want), "{resized:?}");

    // drag inside: it moves, same size
    drag(
        &mut app,
        Point::new(150.0, 150.0),
        Point::new(130.0, 160.0),
        false,
    );
    let moved = v.rect_to_screen(app.o.selection.unwrap().area);
    let want = Rectangle::new(Point::new(80.0, 110.0), Size::new(120.0, 90.0));
    assert!(close_to(moved, want), "{moved:?}");
    // the crop is what is under it: image px x 2, the top-left pixel included
    let crop = app.crop().unwrap();
    let area = app.o.selection.unwrap().area;
    let first = [area.x * 2.0, area.y * 2.0].map(|v| v.floor() as u32);
    assert_eq!(crop.rgba[..2], [first[0] as u8, first[1] as u8]);
    assert!((crop.width as f32 - area.width * 2.0).abs() <= 1.0);

    // a freehand loop resized by a corner scales its stroke with the box
    circle(&mut app, Point::new(300.0, 150.0), 40.0, false);
    let lp = app.o.selection.unwrap();
    assert_eq!(lp.kind, overlay::Kind::Loop);
    let box_ = v.rect_to_screen(lp.area);
    let corner = Point::new(box_.x, box_.y);
    drag(&mut app, corner, corner - Vector::new(40.0, 0.0), false);
    let grown = app.o.selection.unwrap();
    assert_eq!(grown.kind, overlay::Kind::Loop, "still the drawn shape");
    assert!((grown.area.width - lp.area.width * 1.5).abs() < 0.5);
    let xs = app.o.stroke.iter().map(|p| p.x);
    let min = xs.fold(f32::MAX, f32::min);
    assert!(
        (min - grown.area.x).abs() < 0.01,
        "the stroke fills the new box"
    );

    // double right-click: back to 1x
    event(&mut app, overlay::Event::ZoomReset);
    assert_eq!(app.o.view.zoom(), 1.0);
}

#[test]
fn shift_adds_selections_and_actions_get_all_of_them() {
    let mut app = open_app(400, 300, 1.0);
    circle(&mut app, Point::new(60.0, 60.0), 30.0, false);
    circle(&mut app, Point::new(300.0, 200.0), 40.0, true);
    assert_eq!(app.o.others.len(), 1, "two selections");
    // a Shift tap adds nothing and keeps both
    drag(
        &mut app,
        Point::new(10.0, 290.0),
        Point::new(11.0, 290.0),
        true,
    );
    assert_eq!(app.o.others.len(), 1);
    assert!(app.o.selection.is_some());
    let (a, b) = (app.o.others[0].0.area, app.o.selection.unwrap().area);

    // Lens / Copy: the box around all of them
    let union = app.crop().unwrap();
    assert_eq!(
        (union.width, union.height),
        (
            (b.x + b.width).ceil() as u32 - a.x.floor() as u32,
            (b.y + b.height).ceil() as u32 - a.y.floor() as u32
        )
    );
    // Ask AI / Text: each one, stacked in the order made
    let stacked = app.stacked().unwrap();
    let (ca, cb) = (
        actions::crop(&pixels(&app), 400, 300, 1.0, a).unwrap(),
        actions::crop(&pixels(&app), 400, 300, 1.0, b).unwrap(),
    );
    assert_eq!(stacked.width, ca.width.max(cb.width));
    assert_eq!(stacked.height, ca.height + STACK_GAP + cb.height);
    assert_eq!(stacked.rgba[..4], ca.rgba[..4]);
    let second = (ca.height + STACK_GAP) as usize * stacked.width as usize * 4;
    assert_eq!(stacked.rgba[second..second + 4], cb.rgba[..4]);
    assert_eq!(
        stacked.rgba[(ca.width as usize) * 4..][..4],
        [255; 4],
        "white beside"
    );

    // a new selection without Shift starts over
    circle(&mut app, Point::new(200.0, 100.0), 30.0, false);
    assert!(app.o.others.is_empty());
    assert_eq!(app.stacked().unwrap().height, app.crop().unwrap().height);
    let _ = app.close();
    assert!(app.o.others.is_empty() && app.o.word.is_none() && app.o.ask.is_empty());
}

fn pixels(app: &App) -> Vec<u8> {
    match &app.o.frame {
        Some(image::Handle::Rgba { pixels, .. }) => pixels.to_vec(),
        _ => Vec::new(),
    }
}

#[test]
fn a_tapped_word_is_the_selection() {
    let mut app = open_app(400, 300, 2.0);
    // OCR read one line in a crop at frame px (100, 40): "Press any key", 10 image px a char
    let block = TextBlock {
        text: "Press any key".into(),
        bounds: [20.0, 20.0, 260.0, 40.0],
        confidence: 1.0,
    };
    app.taps = 7;
    let at = Point::new(135.0, 40.0); // image px: on "any" (image x 120..150)
    let read = |id| WordRead {
        id,
        at,
        origin: [100, 40],
        region: Rectangle::new(Point::new(50.0, 20.0), Size::new(100.0, 80.0)),
        extend: false,
    };
    let stale = app.on_tapped(read(6), Ok(vec![block.clone()]));
    assert!(
        stale.units() == 0 && app.o.selection.is_none(),
        "an old tap"
    );
    let scan = app.on_tapped(read(7), Ok(vec![block]));
    assert_eq!(scan.units(), 0, "a word is not scanned for a QR code");
    let sel = app.o.selection.expect("the word");
    assert_eq!(sel.kind, overlay::Kind::Word);
    assert_eq!(app.o.word.as_ref().unwrap().text(), "any");
}

const TEXT: [&str; 6] = [
    "Circle to Search keeps the page",
    "frozen while you drag a finger",
    "across the words you want to",
    "copy, translate or look up.",
    "Press Shift to add a second",
    "selection anywhere on screen.",
];
/// Monospace, 22 px: about this many px per character.
const CHAR: f32 = 13.2;

/// What OCR would read of `TEXT` drawn by `text_frame`, as one block per line.
fn text_blocks() -> Vec<TextBlock> {
    (TEXT.iter().enumerate())
        .map(|(i, t)| TextBlock {
            text: (*t).into(),
            bounds: [60.0, 43.0 + 32.0 * i as f32, t.len() as f32 * CHAR, 26.0],
            confidence: 1.0,
        })
        .collect()
}

/// A word on line `i`: image px at its middle (`c` = the character it starts at).
fn at_word(i: usize, c: f32) -> Point {
    Point::new(60.0 + (c + 2.0) * CHAR, 43.0 + 32.0 * i as f32 + 13.0)
}

/// The caret before character `c` of line `i` (image px, mid-height).
fn caret(i: usize, c: f32) -> Point {
    Point::new(60.0 + c * CHAR, 43.0 + 32.0 * i as f32 + 13.0)
}

/// An overlay on a 960x420 frame with `TEXT` drawn on it.
fn text_app() -> App {
    use iced::widget::{column, text};
    let lines = TEXT.map(|t| -> Element<'_, Message> {
        let line = text(t).size(22).font(iced::Font::MONOSPACE);
        line.line_height(iced::Pixels(32.0)).into()
    });
    let page = container(column(lines)).padding([40, 60]);
    let rgba = widgets::shot::render(Element::from(page), 960, 420);
    let mut app = open_app(960, 420, 1.0);
    app.o.frame = Some(image::Handle::from_rgba(960, 420, rgba));
    app
}

/// OCR read `blocks` for the read `extend`/`at`, over `region`, for the latest press.
fn read_back(app: &mut App, at: Point, region: Rectangle, extend: bool) {
    let what = WordRead {
        id: app.taps,
        at,
        origin: [0, 0],
        region,
        extend,
    };
    // OCR reads the lines that lie in the region it was given
    let inside = |b: &TextBlock| {
        region.y <= b.bounds[1] && b.bounds[1] + b.bounds[3] <= region.y + region.height
    };
    let blocks = text_blocks().into_iter().filter(inside).collect();
    let _ = app.on_tapped(what, Ok(blocks));
}

fn page() -> Rectangle {
    Rectangle::new(Point::ORIGIN, Size::new(960.0, 420.0))
}

#[test]
fn a_shift_click_past_the_lines_read_reads_a_band_first() {
    let mut app = text_app();
    let start = at_word(1, 7.0);
    let first = Rectangle::new(Point::new(0.0, 60.0), Size::new(960.0, 80.0)); // lines 1-2
    app.taps += 1;
    read_back(&mut app, start, first, false);
    let end = caret(5, 18.0); // after "anywhere": not read yet
    drag(&mut app, end, end, true);
    let word = app.o.word.as_ref().unwrap();
    assert_eq!(word.text(), "while", "unchanged until that is read");
    assert!(!word.covers(end), "the end is past what was read");
    // the band OCR reads for it: whole width, every line the drag crosses
    let band = word.band(end, app.o.screen);
    assert!(band.width == 960.0 && band.y <= 60.0 && band.y + band.height >= end.y);
    // its answer (not stale: nothing pressed since) stretches the words there
    read_back(&mut app, end, band, true);
    let word = app.o.word.as_ref().unwrap();
    assert_eq!(word.text().lines().count(), 5);
    assert!(word.text().starts_with("while you drag"));
    assert!(word.text().ends_with("selection anywhere"));
    assert_eq!(app.o.stroke.len(), 10);
    // a read for an earlier press is dropped
    let before = word.text();
    app.taps += 1;
    let stale = WordRead {
        id: app.taps - 1,
        at: at_word(0, 1.0),
        origin: [0, 0],
        region: band,
        extend: true,
    };
    let _ = app.on_tapped(stale, Ok(text_blocks()));
    assert_eq!(app.o.word.as_ref().unwrap().text(), before);
}

#[test]
fn shift_click_stretches_the_words_across_lines() {
    let mut app = text_app();
    app.taps += 1;
    read_back(&mut app, at_word(1, 7.0), page(), false);
    let end = caret(4, 18.0); // after "add"
    drag(&mut app, end, end, true);
    assert_eq!(
        app.o.word.as_ref().unwrap().text(),
        "while you drag a finger\nacross the words you want to\ncopy, translate or look up.\nPress Shift to add"
    );
    assert!(app.o.others.is_empty(), "stretched, not a second selection");
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Word);
    // Shift+click on the first line: from there to where it began ("while")
    drag(&mut app, caret(0, 10.0), caret(0, 10.0), true);
    let text = app.o.word.as_ref().unwrap().text();
    assert_eq!(text, "Search keeps the page\nfrozen");
    // a Shift drag still adds a second selection
    circle(&mut app, Point::new(700.0, 300.0), 40.0, true);
    assert_eq!(app.o.others.len(), 1);
    assert!(app.o.word.is_none());
}

#[test]
fn a_scribble_selects_a_padded_hull_that_resizes_as_one() {
    let mut app = open_app(960, 420, 1.0);
    let zig: Vec<Point> = (0..=100)
        .map(|i| {
            let t = i as f32 / 100.0;
            let leg = (t * 4.0).floor();
            let x = if leg % 2.0 == 0.0 {
                (t * 4.0).fract()
            } else {
                1.0 - (t * 4.0).fract()
            };
            Point::new(300.0 + 200.0 * x, 150.0 + 80.0 * t)
        })
        .collect();
    event(
        &mut app,
        overlay::Event::Press {
            at: zig[0],
            shift: false,
        },
    );
    zig[1..]
        .iter()
        .for_each(|&p| event(&mut app, overlay::Event::Move(p)));
    event(&mut app, overlay::Event::Release);
    let sel = app.o.selection.expect("a scribble selects");
    assert_eq!(sel.kind, overlay::Kind::Scribble);
    assert!(app.o.word.is_none() && app.handles());
    // the kept stroke is the hull and the area its box, grown past the scribble's own box
    assert!(app.o.stroke.len() > 8);
    assert!(sel.area.x < 300.0 && sel.area.y < 150.0);
    assert!(sel.area.x + sel.area.width > 500.0 && sel.area.y + sel.area.height > 230.0);
    assert!(app.selected().units() > 0, "scanned like a drawn area");
    // the crop is that box
    let crop = app.crop().unwrap();
    assert!((crop.width as f32 - sel.area.width).abs() <= 2.0);
    // a corner drag resizes it and keeps the shape
    let corner = Point::new(sel.area.x + sel.area.width, sel.area.y + sel.area.height);
    drag(&mut app, corner, corner + Vector::new(30.0, 20.0), false);
    let grown = app.o.selection.unwrap();
    assert_eq!(grown.kind, overlay::Kind::Scribble);
    assert!((grown.area.width - sel.area.width - 30.0).abs() < 0.5);
    let xs = app.o.stroke.iter().map(|p| p.x);
    assert!((xs.fold(f32::MIN, f32::max) - (grown.area.x + grown.area.width)).abs() < 0.01);
    // a loop or a drag next to it still work as before
    circle(&mut app, Point::new(200.0, 300.0), 40.0, false);
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Loop);
    drag(
        &mut app,
        Point::new(600.0, 50.0),
        Point::new(800.0, 120.0),
        false,
    );
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Drag);
}

/// `OPENCIRCLE_SELECTION_PNG=<dir> cargo test selection_shots -- --ignored`
/// writes the overlay as drawn (headless tiny-skia, 960x420): words dragged
/// across three lines, a scribble over the text, and the same two in the
/// idle state for comparison.
#[test]
#[ignore = "writes PNGs to look at"]
fn selection_shots() {
    let dir = std::path::PathBuf::from(
        std::env::var("OPENCIRCLE_SELECTION_PNG").expect("OPENCIRCLE_SELECTION_PNG=<dir>"),
    );
    let shot = |app: &App, name: &str| {
        let rgba = widgets::shot::render(app.view(app.overlay), 960, 420);
        actions::write_png(&dir.join(name), 960, 420, &rgba).unwrap();
    };
    let mut app = text_app();
    shot(&app, "0-page.png");
    let start = at_word(1, 7.0);
    app.taps += 1;
    read_back(&mut app, start, page(), false);
    shot(&app, "1-word.png");
    drag(&mut app, start, at_word(3, 6.0), false);
    shot(&app, "2-highlight-across-lines.png");

    let mut app = text_app();
    let wave: Vec<Point> = (0..=140)
        .map(|i| {
            let t = i as f32 / 140.0;
            let x = ((t * 5.0).fract() - 0.5).abs() * 2.0; // a triangle wave
            Point::new(150.0 + 460.0 * x, 100.0 + 70.0 * t + 6.0 * (i % 3) as f32)
        })
        .collect();
    event(
        &mut app,
        overlay::Event::Press {
            at: wave[0],
            shift: false,
        },
    );
    wave[1..]
        .iter()
        .for_each(|&p| event(&mut app, overlay::Event::Move(p)));
    shot(&app, "3-scribble-drawing.png");
    event(&mut app, overlay::Event::Release);
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Scribble);
    shot(&app, "4-scribble-selected.png");
}

#[test]
fn the_ask_bar_shows_with_a_selection_and_opens_ask_ai() {
    let mut app = open_app(400, 300, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let _ = app.update(Message::AskInput("what is this?".into()));
    let texts = widgets::shot::texts(app.view(app.overlay));
    assert!(texts.contains(&"what is this?".to_string()), "{texts:?}");
    // The design: an empty field opens Ask AI (its presets), asking nothing.
    let _ = app.update(Message::AskInput("   ".into()));
    let _ = app.update(Message::AskSubmit);
    let (_, panel) = app.o.panel.as_mut().expect("Ask AI opens");
    assert_eq!(panel.kind(), actions::Panel::Ask);
    assert!(panel.take_input().is_empty(), "no question");
    app.o.panel = None;
    let _ = app.update(Message::AskInput("what is this?".into()));
    let _ = app.update(Message::AskSubmit);
    let (_, panel) = app.o.panel.as_ref().expect("Ask AI opens");
    assert_eq!(panel.kind(), actions::Panel::Ask);
    assert!(app.o.ask.is_empty());
}

/// `text_app` with its text reader (the OCR models) here: main mode reads
/// the screen's text on demand.
fn reader_app() -> App {
    let mut app = text_app();
    app.o.ocr_ready = true;
    app
}

/// OCR answers the band being read (`band`) of `text_app`'s page.
fn answer(app: &mut App, band: Rectangle) -> Task<Message> {
    let inside =
        |b: &TextBlock| band.y <= b.bounds[1] && b.bounds[1] + b.bounds[3] <= band.y + band.height;
    let blocks = text_blocks().into_iter().filter(inside).collect();
    let lines = word::lines(blocks, [0, 0], 1.0);
    app.update(Message::ScreenText(app.opening, band, Ok(lines)))
}

/// `reader_app` with all its screen read.
fn read_app() -> App {
    let mut app = reader_app();
    let _ = answer(&mut app, page());
    assert!(app.o.screen_text.is_whole(420.0));
    app
}

/// A press-drag-release through `points` (image px = screen px at 1x).
fn stroke(app: &mut App, points: &[Point]) {
    let at = points[0];
    event(app, overlay::Event::Press { at, shift: false });
    for pair in points.windows(2) {
        for i in 1..=8 {
            let p = pair[0] + (pair[1] - pair[0]) * (i as f32 / 8.0);
            event(app, overlay::Event::Move(p));
        }
    }
    event(app, overlay::Event::Release);
}

fn picked_text(app: &App) -> Option<String> {
    let word = app.o.selection.filter(|s| s.kind == overlay::Kind::Word);
    word.and(app.o.word.as_ref()).map(word::Picked::text)
}

/// A click (press and release, no move) at `p`.
fn click_at(app: &mut App, p: Point, shift: bool) {
    event(app, overlay::Event::Press { at: p, shift });
    event(app, overlay::Event::Release);
}

#[test]
fn with_the_screen_read_text_is_selected_like_in_a_document() {
    // a drag selects from the caret under the press to the one under the
    // pointer: characters, not whole words; a flat highlight, not a stroke
    let mut app = read_app();
    stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
    assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
    assert!(app.o.text_press.is_none() && !app.o.drawing);
    stroke(&mut app, &[caret(1, 9.4), caret(1, 18.6)]);
    assert_eq!(picked_text(&app).as_deref(), Some("ile you dr"));
    // to a line's end and on to the next lines: reading order, middle lines whole
    let end_of_1 = caret(1, 30.0);
    stroke(
        &mut app,
        &[caret(1, 7.0), end_of_1, caret(3, 0.0), caret(3, 15.0)],
    );
    assert_eq!(
        picked_text(&app).as_deref(),
        Some("while you drag a finger\nacross the words you want to\ncopy, translate")
    );
    let rects = app.o.word.as_ref().unwrap().rects();
    assert_eq!(rects.len(), 3);
    assert_eq!(rects[1].x, 60.0, "the middle line from its start");
    assert!((rects[1].x + rects[1].width - (60.0 + 28.0 * CHAR)).abs() < 0.01);
    // one click on text selects nothing; a double click the word, a triple the line
    // (with text selected before, as on the VM)
    let shift = caret(4, 8.0); // in "Shift"
    click_at(&mut app, shift, false);
    assert!(app.o.selection.is_none() && app.o.word.is_none());
    click_at(&mut app, shift, false);
    assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
    click_at(&mut app, shift, false);
    assert_eq!(
        picked_text(&app).as_deref(),
        Some("Press Shift to add a second")
    );
    // a loop that starts on text is a circle
    let mut app = read_app();
    let start = at_word(2, 11.0);
    let around: Vec<Point> = (0..=40)
        .map(|i| {
            let a = i as f32 / 40.0 * std::f32::consts::TAU + std::f32::consts::PI;
            start + Vector::new(80.0 + 80.0 * a.cos(), 60.0 * a.sin())
        })
        .collect();
    stroke(&mut app, &around);
    let sel = app.o.selection.expect("a circle");
    assert_eq!(sel.kind, overlay::Kind::Loop);
    assert!(app.o.word.is_none());
    // a press off the text draws as before
    let mut app = read_app();
    stroke(
        &mut app,
        &[Point::new(600.0, 250.0), Point::new(800.0, 400.0)],
    );
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Drag);
    // before the screen is read, a drag along text is a stroke too
    let mut app = text_app();
    stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
    assert!(picked_text(&app).is_none());
}

#[test]
fn selected_text_gets_the_text_actions_shift_extends_ctrl_a_takes_all() {
    let mut app = read_app();
    stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]); // "while you drag"
    // the bottom bar's Copy and Translate take the text (no pill)
    let texts = overlay_texts(&app);
    assert!(
        !texts.contains(&"Copy".to_string()),
        "no floating pill: {texts:?}"
    );
    let click = |p| widgets::shot::click(app.view(app.overlay), app.o.screen, p);
    let copy = click(bar_pill(&app, 1));
    assert!(
        copy.iter()
            .any(|m| matches!(m, Message::Pill(BarButton::CopyText)))
    );
    assert!(
        click(bar_pill(&app, 2))
            .iter()
            .any(|m| matches!(m, Message::Pill(BarButton::Translate)))
    );
    assert!(app.picked_text().is_some(), "the pills take the text");
    assert!(
        !app.handles() || app.grab_at(caret(1, 21.0)).is_none(),
        "no handles"
    );
    // Shift+click stretches it from where it began
    click_at(&mut app, caret(2, 16.0), true);
    assert_eq!(
        picked_text(&app).as_deref(),
        Some("while you drag a finger\nacross the words")
    );
    assert!(app.o.others.is_empty());
    // Ctrl+A: all the text on the screen
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'a',
            ctrl: true,
        },
    );
    assert_eq!(picked_text(&app).as_deref(), Some(TEXT.join("\n").as_str()));
    // A (Ask AI) asks about the text's box, still the selection
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'a',
            ctrl: false,
        },
    );
    let (_, panel) = app.o.panel.as_ref().expect("Ask AI opens");
    assert_eq!(panel.kind(), actions::Panel::Ask);
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Word);
}

#[test]
fn the_bottom_bar_is_there_from_the_first_frame() {
    let mut app = open_app(800, 600, 1.0);
    let _ = app.update(Message::AskInput("what is on my screen?".into()));
    let has_field = |app: &App| overlay_texts(app).contains(&app.o.ask);
    assert!(has_field(&app), "before any selection");
    let clicked = widgets::shot::click(app.view(app.overlay), app.o.screen, bar_pill(&app, 2));
    assert!(
        clicked
            .iter()
            .any(|m| matches!(m, Message::Pill(BarButton::Translate)))
    );
    // while drawing, and with a selection
    event(
        &mut app,
        overlay::Event::Press {
            at: Point::new(50.0, 50.0),
            shift: false,
        },
    );
    event(&mut app, overlay::Event::Move(Point::new(150.0, 120.0)));
    assert!(has_field(&app), "while drawing");
    event(&mut app, overlay::Event::Release);
    assert!(app.o.selection.is_some() && has_field(&app));
    // Enter with no selection asks about the whole screen
    let mut app = open_app(800, 600, 1.0);
    let _ = app.update(Message::AskInput("what is this?".into()));
    let _ = app.update(Message::AskSubmit);
    assert_eq!(
        app.o.panel.as_ref().map(|(_, p)| p.kind()),
        Some(actions::Panel::Ask)
    );
    let bottom = |t: &String| ASK_PLACEHOLDERS.contains(&t.as_str());
    assert!(
        !overlay_texts(&app).iter().any(bottom),
        "the card's own field takes over"
    );
    // Translate translates the whole screen
    let mut app = open_app(800, 600, 1.0);
    let _ = app.update(Message::Pill(BarButton::Translate));
    assert!(app.o.translate.is_some() && app.o.selection.is_none());
}

/// Where the bottom bar's pills start (x), and its middle (y), as laid out.
fn bar_row(app: &App) -> (f32, f32) {
    let (field, size) = app.bar_size();
    let at = app.bar_at();
    let x = at.x + bar::PAD + bar::HANDLE + bar::SPACING + field;
    (
        x + bar::SPACING + bar::DIVIDER + bar::SPACING,
        at.y + size.height / 2.0,
    )
}

/// The bar's pills' widths as laid out.
fn pill_widths(app: &App) -> [f32; 3] {
    let icons = labs::bar(&app.o.labs)
        .iter()
        .filter(|b| !b.is_pill())
        .count();
    bar::pill_widths(app.o.screen.width, icons as f32)
}

/// The centre of the bottom bar's pill `k` (0 = Search in browser, 1 = Copy
/// text, 2 = Translate).
fn bar_pill(app: &App, k: usize) -> Point {
    let (x, y) = bar_row(app);
    let widths = pill_widths(app);
    let before: f32 = widths[..k].iter().map(|w| w + bar::PILL_GAP).sum();
    Point::new(x + before + widths[k] / 2.0, y)
}

/// The centre of the bottom bar's round button `k` (0 = Live translate,
/// then Song search and Edit when on), after the pills and their divider.
fn bar_button(app: &App, k: usize) -> Point {
    let (x, y) = bar_row(app);
    let pills: f32 = pill_widths(app).iter().sum::<f32>() + 2.0 * bar::PILL_GAP;
    let first = x + pills + bar::SPACING + bar::DIVIDER + bar::SPACING;
    let x = first + k as f32 * (bar::BUBBLE + bar::SPACING) + bar::BUBBLE / 2.0;
    Point::new(x, y)
}

/// A point in the bottom bar's field.
fn bar_field(app: &App) -> Point {
    let (_, size) = app.bar_size();
    let at = app.bar_at();
    let x = at.x + bar::PAD + bar::HANDLE + bar::SPACING + 60.0;
    Point::new(x, at.y + size.height / 2.0)
}

/// One selection, the old action bar gone: only the bottom bar, whose pills
/// act on the selection.
#[test]
fn a_selection_brings_its_actions_into_the_bottom_bar_not_a_second_bar() {
    let mut app = open_app(1280, 800, 1.0);
    let pill =
        |app: &App, k| widgets::shot::click(app.view(app.overlay), app.o.screen, bar_pill(app, k));
    // before a selection: Search in browser and Copy text are dimmed
    let pressed = |got: Vec<Message>| got.iter().any(|m| matches!(m, Message::Pill(_)));
    assert!(!pressed(pill(&app, 0)) && !pressed(pill(&app, 1)));
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    assert!(app.o.selection.is_some());
    let texts = overlay_texts(&app);
    for label in ["Search in browser", "Copy text", "Translate"] {
        assert!(texts.iter().any(|t| t == label), "{label}: {texts:?}");
    }
    for label in ["Search", "Ask AI", "Text", "Edit", "Copy", "Save"] {
        assert!(
            !texts.iter().any(|t| t == label),
            "no labelled action bar: {texts:?}"
        );
    }
    for (k, which) in [
        (0, BarButton::Search),
        (1, BarButton::CopyText),
        (2, BarButton::Translate),
    ] {
        let sent = pill(&app, k);
        assert!(
            sent.iter()
                .any(|m| matches!(m, Message::Pill(b) if *b == which)),
            "{which:?}: {sent:?}"
        );
    }
    let live = widgets::shot::click(app.view(app.overlay), app.o.screen, bar_button(&app, 0));
    assert!(live.iter().any(|m| matches!(m, Message::LiveToggle)));
    // the pressed pill is the highlighted one; Translate takes the selection
    let _ = app.update(Message::Pill(BarButton::Translate));
    assert_eq!(app.o.pill, BarButton::Translate);
    assert!(app.o.translate.is_some() && app.o.selection.is_some());
    // picked words in a snapped box: the pills take their text
    let mut words = select_text_app();
    drag(&mut words, caret(1, 7.0), caret(2, 16.0), false);
    assert!(words.picked_text().is_some_and(|t| !t.trim().is_empty()));
}

/// What a click on the reader's note (over the bottom bar, on a row across
/// its width) sends.
fn note_clicks(app: &App) -> Vec<Message> {
    let (_, size) = app.bar_size();
    let at = app.bar_at();
    let y = at.y - bar::SPACING - bar::HEIGHT / 2.0;
    (0..(size.width as usize / 8))
        .flat_map(|i| {
            let p = Point::new(at.x + 8.0 * i as f32, y);
            widgets::shot::click(app.view(app.overlay), app.o.screen, p)
        })
        .collect()
}

/// A fresh install has no text reader (OCR models): a drag on text can only
/// draw. The overlay says so over the bar, with the size, and downloads
/// only on its button; then its progress, a failure to try again, and
/// "Reading text…" while the screen is read.
#[test]
fn without_the_text_reader_the_bar_offers_its_download() {
    let mut app = open_app(1280, 800, 1.0);
    app.note_reader(); // as at open (the tests have no models)
    let Reader::Missing(bytes) = app.o.reader else {
        panic!("{:?}", app.o.reader);
    };
    assert!(bytes > 1_000_000, "{bytes}");
    let texts = overlay_texts(&app);
    let mb = bytes.div_ceil(1_000_000);
    let say = format!("Text selection needs the text reader (~{mb} MB).");
    assert!(texts.contains(&say), "{texts:?}");
    assert!(texts.contains(&"Download".to_string()), "{texts:?}");
    widgets::shot::save("reader-missing", app.view(app.overlay), app.o.screen);
    // nothing fetched by itself: drawing, selecting, frames leave it offered
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    settle_bar(&mut app);
    assert!(matches!(app.o.reader, Reader::Missing(_)));
    // one click on Download
    let clicks = note_clicks(&app);
    assert!(
        clicks.iter().any(|m| matches!(m, Message::ReaderDownload)),
        "{clicks:?}"
    );
    let _ = app.update(Message::ReaderDownload);
    assert_eq!(app.o.reader, Reader::Downloading(0.0));
    let opening = app.opening;
    let step = move |p| Message::Reader(opening, p);
    let _ = app.update(step(Ok(text::Progress::Download(0.42))));
    let texts = overlay_texts(&app);
    assert!(
        texts.contains(&"Downloading the text reader… 42%".to_string()),
        "{texts:?}"
    );
    widgets::shot::save("reader-downloading", app.view(app.overlay), app.o.screen);
    // another opening's download says nothing here
    let stale = Message::Reader(app.opening.wrapping_add(1), Err("x".into()));
    let _ = app.update(stale);
    assert_eq!(app.o.reader, Reader::Downloading(0.42));
    let _ = app.update(step(Err("no network".into())));
    let texts = overlay_texts(&app);
    assert!(
        texts.contains(&"The text reader didn't download: no network".to_string()),
        "{texts:?}"
    );
    assert!(
        note_clicks(&app)
            .iter()
            .any(|m| matches!(m, Message::ReaderDownload))
    );
    // done: the screen is read (here, still no models: offered again)
    let _ = app.update(step(Ok(text::Progress::Reading)));
    assert!(
        matches!(app.o.reader, Reader::Missing(_)),
        "{:?}",
        app.o.reader
    );
}

/// A drag on text that began before the screen's text was read becomes a
/// text selection as soon as it is (not a stroke snapped to a box).
#[test]
fn a_drag_on_text_selects_it_once_the_words_arrive() {
    let mut app = reader_app();
    event(
        &mut app,
        overlay::Event::Press {
            at: caret(1, 7.0),
            shift: false,
        },
    );
    for i in 1..=8 {
        let p = caret(1, 7.0) + (caret(1, 15.0) - caret(1, 7.0)) * (i as f32 / 8.0);
        event(&mut app, overlay::Event::Move(p));
    }
    assert!(app.o.drawing, "a stroke until the words are read");
    let band = app.o.reading.expect("the band under the press is read");
    let _ = answer(&mut app, band);
    assert!(!app.o.drawing && app.o.text_press.is_some());
    event(&mut app, overlay::Event::Move(caret(1, 21.0)));
    event(&mut app, overlay::Event::Release);
    assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
    // off the text it stays a stroke
    let mut app = reader_app();
    event(
        &mut app,
        overlay::Event::Press {
            at: Point::new(600.0, 250.0),
            shift: false,
        },
    );
    event(&mut app, overlay::Event::Move(Point::new(700.0, 300.0)));
    let band = app.o.reading.expect("a press may be on text");
    let _ = answer(&mut app, band);
    assert!(app.o.drawing && app.o.word.is_none());
    assert!(
        !app.o.text_used && app.o.reading.is_none(),
        "no text: no more is read"
    );
}

/// #123: opening the overlay reads no text and starts no helper while the
/// opening animation runs (it was a full-screen OCR read 150 ms in, every
/// core busy); the helper starts once it ends. A press reads the band
/// under it first; once text is used, the rest follows, nearest first.
#[test]
fn the_screen_is_read_on_demand_the_touched_band_first() {
    text::MODELS_HERE.set(Some(true));
    let page = text_app();
    let mut app = App::idle(window::Id::unique());
    let shot = Shot {
        image: page.o.frame.clone().unwrap(),
        scale: 1.0,
        origin: Point::ORIGIN,
        size: page.o.screen,
        snip: None,
    };
    let _ = app.open(Instant::now(), Ok(shot));
    assert!(app.o.ocr_ready && app.o.reader == Reader::Idle);
    assert_eq!(app.o.reading, None, "nothing read at open");
    // the opening animation: nothing read, no helper started
    let start = Instant::now();
    let mut ms = 0;
    let mut last = Task::none();
    while app.o.appear.is_none() || app.appearing() {
        let revealing = app.o.reveal.is_some();
        last = app.update(Message::Frame(start + Duration::from_millis(ms)));
        ms += 16;
        // the reveal's frame fades the window in (Windows, #23): not work
        if app.appearing() && !(revealing && app.o.reveal.is_none()) {
            assert_eq!(last.units(), 0, "work at {ms} ms of the opening");
        }
        assert_eq!(app.o.reading, None);
    }
    assert!(last.units() > 0, "its end starts the helper");
    // a press on text: the band under it is read first, not the screen
    let at = caret(1, 7.0);
    event(&mut app, overlay::Event::Press { at, shift: false });
    let first = app.o.reading.expect("the touched band");
    assert_eq!(first, word::Screen::band(at.y, app.o.screen));
    assert!(first.height < app.o.screen.height / 1.5 && first.contains(at));
    for i in 1..=8 {
        let p = at + (caret(1, 21.0) - at) * (i as f32 / 8.0);
        event(&mut app, overlay::Event::Move(p));
    }
    let _ = answer(&mut app, first);
    event(&mut app, overlay::Event::Release);
    assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
    // text was used: the rest of the screen, band by band
    let next = app.o.reading.expect("the rest, in the background");
    assert_ne!(next, first);
    let mut bands = 1;
    while let Some(band) = app.o.reading {
        let _ = answer(&mut app, band);
        bands += 1;
        assert!(bands < 10);
    }
    assert!(app.o.screen_text.is_whole(app.o.screen.height));
    assert_eq!(app.o.screen_text.lines().len(), TEXT.len());
    assert_eq!(picked_text(&app).as_deref(), Some("while you drag"), "kept");
    text::MODELS_HERE.set(None);
}

/// Off Windows (no Windows OCR): once the helper has started after the
/// opening animation, the whole screen is read gently, band by band from
/// the pointer out, with no press: the I-beam shows over its text (the
/// overlay gets its lines), as on Windows. A late `Warmed` of an earlier
/// opening reads nothing.
#[test]
#[cfg(not(windows))]
fn off_windows_the_screen_is_read_once_the_helper_is_warm() {
    let mut app = reader_app();
    app.o.pointer = caret(2, 3.0);
    let _ = app.update(Message::Warmed(app.opening.wrapping_sub(1)));
    assert_eq!(app.o.reading, None, "an earlier opening's helper");
    let _ = app.update(Message::Warmed(app.opening));
    let first = app.o.reading.expect("read with no press");
    assert!(first.contains(app.o.pointer), "the pointer's band first");
    let mut bands = 0;
    while let Some(band) = app.o.reading {
        let _ = answer(&mut app, band);
        bands += 1;
        assert!(bands < 10);
    }
    assert!(app.o.screen_text.is_whole(app.o.screen.height));
    assert_eq!(app.o.screen_text.lines().len(), TEXT.len());
    assert!(
        app.o.word.is_none() && app.o.selection.is_none(),
        "nothing selected"
    );
}

/// #62: with Windows OCR (no PP-OCR models needed: no download note) the
/// whole screen is read in one go from the capture on (while the overlay
/// comes up, one read only); a drag begun on text before it arrives
/// becomes a text selection when it does. Nothing more is read after it
/// (no bands, no helper).
#[test]
fn windows_ocr_reads_the_whole_screen_from_the_capture() {
    text::MODELS_HERE.set(Some(false));
    ocr::system::LANGUAGE_HERE.set(Some(Some("en-US")));
    let page = text_app();
    let mut app = App::idle(window::Id::unique());
    let shot = Shot {
        image: page.o.frame.clone().unwrap(),
        scale: 1.0,
        origin: Point::ORIGIN,
        size: page.o.screen,
        snip: None,
    };
    let opened = app.open(Instant::now(), Ok(shot));
    assert_eq!(app.o.reader, Reader::Idle, "no download offered");
    let whole = Rectangle::with_size(app.o.screen);
    assert_eq!(
        app.o.reading,
        Some(whole),
        "the whole screen, at the capture"
    );
    assert_eq!(app.o.text_by, Some(ocr::Source::System));
    assert!(opened.units() > 0);
    let start = Instant::now();
    let mut ms = 0;
    while app.o.appear.is_none() || app.appearing() {
        let _ = app.update(Message::Frame(start + Duration::from_millis(ms)));
        ms += 16;
        assert_eq!(app.o.reading, Some(whole), "one read at {ms} ms");
    }
    // a drag on text while it is read: a stroke for now
    let at = caret(1, 7.0);
    event(&mut app, overlay::Event::Press { at, shift: false });
    for i in 1..=8 {
        let p = at + (caret(1, 21.0) - at) * (i as f32 / 8.0);
        event(&mut app, overlay::Event::Move(p));
    }
    assert!(app.o.drawing && app.o.word.is_none());
    let _ = answer(&mut app, whole);
    assert!(app.o.screen_text.is_whole(app.o.screen.height));
    event(&mut app, overlay::Event::Release);
    assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
    assert_eq!(app.o.reading, None, "nothing more to read");
    // every word is selectable now, at once
    click_at(&mut app, at_word(4, 6.0), false);
    click_at(&mut app, at_word(4, 6.0), false);
    assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
    assert_eq!(app.o.reading, None);
    // the closer (doubled) read replaces the quick one; the selection stays
    let blocks: Vec<TextBlock> = text_blocks().into_iter().skip(1).collect();
    let closer = word::lines(blocks, [0, 0], 1.0);
    let _ = app.update(Message::ScreenText(app.opening, whole, Ok(closer)));
    assert_eq!(app.o.screen_text.lines().len(), TEXT.len() - 1, "replaced");
    assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
    ocr::system::LANGUAGE_HERE.set(None);
    text::MODELS_HERE.set(None);
}

/// #63: Windows OCR can't read (no recognizer for the user's languages;
/// here: no Windows at all): the helper reads instead, band by band, and
/// without its models the bar offers their download, as before.
#[test]
#[cfg(not(windows))] // Windows' real OCR would read
fn without_windows_ocr_the_helper_reads() {
    ocr::system::LANGUAGE_HERE.set(Some(Some("en-US"))); // as if it were there
    let mut app = text_app();
    assert!(app.reads_text() && app.o.reader == Reader::Idle);
    app.o.helper = Some(Arc::new(ocr::hold())); // open, as `open` does
    let task = app.warm();
    assert_eq!(app.o.text_by, Some(ocr::Source::System));
    run_task(&mut app, task); // the real read: there is none here
    assert_eq!(app.o.text_by, Some(ocr::Source::Helper));
    assert!(
        matches!(app.o.reader, Reader::Missing(_)),
        "{:?}",
        app.o.reader
    );
    assert!(!app.reads_text() && app.o.screen_text.lines().is_empty());
    // with the models, a press reads the band under it (#123)
    let mut app = reader_app();
    app.o.text_by = Some(ocr::Source::Helper);
    let at = caret(1, 7.0);
    event(&mut app, overlay::Event::Press { at, shift: false });
    assert_eq!(app.o.reading, Some(word::Screen::band(at.y, app.o.screen)));
    ocr::system::LANGUAGE_HERE.set(None);
}

/// Windows OCR reads the frame in physical px: the whole screen's band is
/// the whole frame and its lines land on their text at 100/150/175 %
/// (the frame is the screen's own, so on a second screen too, #47).
#[test]
fn windows_ocr_lines_land_on_the_frame_at_any_scale() {
    for scale in [1.0, 1.5, 1.75] {
        let screen = Size::new(1920.0 / scale, 1080.0 / scale);
        let whole = Rectangle::with_size(screen);
        let crop = actions::crop_bounds(1920, 1080, scale, whole);
        assert_eq!(crop, Some([0, 0, 1920, 1080]), "{scale}x");
        // "Hello" at image (100, 40), 60 x 20, as Windows reads it (frame px)
        let px = |v: f32| v * scale;
        let word = (
            "Hello".to_string(),
            [px(100.0), px(40.0), px(60.0), px(20.0)],
        );
        let lines = word::lines(ocr::system::blocks(vec![vec![word]]), [0, 0], scale);
        let b = lines[0].bounds;
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(
            near(b.x, 100.0) && near(b.y, 40.0) && near(b.width, 60.0) && near(b.height, 20.0),
            "{scale}x: {b:?}"
        );
    }
}

/// A quick drag on text let go (and Ctrl+C pressed) before the band under
/// its press is read: both wait for it, and the text is what is copied.
/// Off text, the held drag is a stroke as before.
#[test]
fn a_drag_let_go_before_its_band_is_read_waits_for_it() {
    let mut app = reader_app();
    stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
    let band = app.o.reading.expect("the band under the press");
    assert!(app.o.held.is_some() && app.o.selection.is_none(), "held");
    let ctrl_c = overlay::Event::Key {
        letter: 'c',
        ctrl: true,
    };
    event(&mut app, ctrl_c);
    let _ = copy::copied::text();
    let task = answer(&mut app, band);
    run_task(&mut app, task);
    assert_eq!(copy::copied::text().as_deref(), Some("while you drag"));
    assert!(app.o.held.is_none());
    // off text: a stroke, snapped to a box as before
    let mut app = reader_app();
    stroke(
        &mut app,
        &[Point::new(600.0, 250.0), Point::new(800.0, 400.0)],
    );
    let band = app.o.reading.expect("the band under the press");
    let _ = answer(&mut app, band);
    assert!(app.o.held.is_none() && app.o.word.is_none());
    assert_eq!(app.o.selection.map(|s| s.kind), Some(overlay::Kind::Drag));
}

/// A tap, a Shift+click and Ctrl+A before their text is read wait for it.
#[test]
fn taps_shift_clicks_and_ctrl_a_wait_for_their_band() {
    // a tap: the word there once its band is read
    let mut app = reader_app();
    let tap = at_word(4, 6.0); // "Shift"
    click_at(&mut app, tap, false);
    let band = app.o.reading.expect("the tapped band");
    assert!(band.contains(tap) && band.height < 420.0);
    let _ = answer(&mut app, band);
    assert_eq!(picked_text(&app).as_deref(), Some("Shift"));
    // Shift+click past the rows read: that band, then the words stretch there
    let mut app = reader_app();
    let band = word::Screen::band(caret(0, 0.0).y, app.o.screen);
    let _ = answer(&mut app, band);
    stroke(&mut app, &[caret(0, 7.0), caret(0, 16.0)]);
    assert_eq!(picked_text(&app).as_deref(), Some("to Search"));
    let end = caret(5, 9.0);
    assert!(!app.o.screen_text.knows(end.y));
    click_at(&mut app, end, true);
    while let Some(band) = app.o.reading {
        let _ = answer(&mut app, band); // the background band, then the clicked one
    }
    let text = picked_text(&app).unwrap();
    assert!(
        text.starts_with("to Search") && text.ends_with("selection"),
        "{text}"
    );
    // Ctrl+A: everything, once all is read
    let mut app = reader_app();
    event(&mut app, overlay::Event::Hover(caret(3, 0.0)));
    let ctrl_a = overlay::Event::Key {
        letter: 'a',
        ctrl: true,
    };
    event(&mut app, ctrl_a);
    let first = app.o.reading.expect("from the pointer");
    assert!(first.contains(caret(3, 0.0)));
    while let Some(band) = app.o.reading {
        assert!(app.o.selection.is_none(), "not before all is read");
        let _ = answer(&mut app, band);
    }
    assert_eq!(picked_text(&app).unwrap(), TEXT.join("\n"));
}

/// The bottom bar (owner, 2026-10-25): no Search, Ask AI or Text buttons
/// (a selection searches by itself, Enter asks, text is selected directly);
/// their keys L, A, T still work. In "wait" mode Enter in the empty field
/// is the search, and the field says so.
/// The names of the bottom bar's buttons (their tooltips' first words).
fn bar_names(app: &App) -> Vec<String> {
    let _ = widgets::tests_support::shown();
    let _ = overlay_texts(app);
    let mut names: Vec<String> = widgets::tests_support::shown()
        .iter()
        .map(|t| t.split(" · ").next().unwrap_or_default().to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn hidden_features_have_no_bar_button_key_or_editor_and_turning_them_on_brings_them_back() {
    let mut app = open_app(1280, 800, 1.0);
    let (_, full) = app.bar_size();
    app.o.labs = Labs::default();
    let (_, small) = app.bar_size();
    assert!(
        small.width < full.width,
        "two buttons fewer: {small:?} < {full:?}"
    );
    let names = bar_names(&app);
    assert_eq!(
        names,
        ["Ask AI", "Open OpenCircle AI chat"],
        "the design's bar, no Live translate, no Song search, no Edit"
    );
    let texts = overlay_texts(&app);
    for pill in ["Search in browser", "Copy text", "Translate"] {
        assert!(texts.iter().any(|t| t == pill), "{pill}: {texts:?}");
    }
    // E does nothing without a selection and with one (no editor, no whole-screen editor).
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'e',
            ctrl: false,
        },
    );
    assert!(app.o.edit.is_none() && !app.editing());
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'e',
            ctrl: false,
        },
    );
    let _ = app.update(Message::Run(&actions::edit::ACTION));
    let _ = app.update(Message::EditScreen);
    assert!(app.o.edit.is_none(), "the editor never opens");
    // Pins and a snip's editor are off too.
    assert!(!app.o.labs.has(labs::Feature::Pins));
    // Song search on: its button is back (one more), the editor still off.
    app.o.labs.set(labs::Feature::Song, true);
    assert!(bar_names(&app).iter().any(|n| n == "Song search"));
    // (the first icon also brings the divider before the icons)
    assert!(app.bar_size().1.width >= small.width + bar::BUBBLE + bar::SPACING);
    // Live translate on: its button is back.
    app.o.labs.set(labs::Feature::Live, true);
    assert!(bar_names(&app).iter().any(|n| n == "Live translate (Beta)"));
    // Editor on: Edit is back, and E opens it.
    app.o.labs.set(labs::Feature::Editor, true);
    assert!(bar_names(&app).iter().any(|n| n.starts_with("Edit")));
    let _ = app.update(Message::Run(&actions::edit::ACTION));
    assert!(app.editing());
}

#[test]
fn a_pin_needs_its_labs_switch() {
    let mut app = open_app(1280, 800, 1.0);
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    let _ = app.open_editor();
    app.o.labs.set(labs::Feature::Pins, false);
    let _ = app.update(Message::Editor(editor::Message::Pin));
    assert!(
        app.o.frame.is_some() && app.o.edit.is_some(),
        "the overlay stays: nothing was pinned"
    );
    app.o.labs.set(labs::Feature::Pins, true);
    let _ = app.update(Message::Editor(editor::Message::Pin));
    assert!(app.hiding != Hiding::No, "pinned: the overlay closes");
}

#[test]
fn a_hidden_features_trigger_does_nothing() {
    // The shared config is the defaults here: Snip and Song are off.
    let mut app = open_app(1280, 800, 1.0);
    app.o.frame = None;
    let qa = |action: &str| QuickAction {
        action: action.into(),
        shortcut: String::new(),
        target: Target::Circle,
        live: false,
    };
    for id in ["snip", "song_search"] {
        let _ = app.update(Message::Trigger(Trigger::Quick(Instant::now(), qa(id))));
        assert!(app.capturing.is_none(), "{id} did not start a capture");
    }
}

/// #144: the ask field's placeholder fits its text area at every bar width,
/// in the UI font on this machine (Inter here, Segoe UI on Windows).
#[test]
fn ask_placeholder_always_fits_its_field() {
    for icons in 0..=5 {
        for screen in (600..=3840).step_by(40) {
            let (field, _) = bar::bottom_size(screen as f32, icons as f32);
            let room = bar::text_room(field);
            let p = ask_placeholder(field);
            assert!(
                p == "Ask AI…" || widgets::text_width(p, field::SLOTS_TEXT) <= room,
                "{p:?} is cut in a {field} px field"
            );
        }
    }
    // the design's 270 px field keeps the sentence's meaning
    assert!(ask_placeholder(bar::FIELD).ends_with("screen…"));
}

#[test]
fn the_bar_is_the_designs_and_nothing_is_sent_before_a_choice() {
    // The default (and every migrated) config: nothing runs after a selection.
    let mut app = open_app(1280, 800, 1.0);
    app.o.labs = Labs::all_on();
    app.o.behaviour = config::Config::default().selection;
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    assert!(app.o.selection.is_some());
    assert!(app.auto_actions().is_empty(), "nothing runs by itself");
    assert!(app.o.panel.is_none(), "no Lens, no Ask AI before a choice");
    assert!(app.o.lens_pages.is_empty(), "nothing handed to the browser");
    let _ = widgets::tests_support::shown();
    let texts = overlay_texts(&app);
    let tips = widgets::tests_support::shown();
    let labels = |t: &String| t.split(" · ").next().unwrap_or_default().to_string();
    let mut names: Vec<String> = tips.iter().map(labels).collect();
    names.sort();
    names.dedup();
    assert_eq!(
        names,
        [
            "Ask AI",
            "Edit image",
            "Live translate (Beta)",
            "Open OpenCircle AI chat",
            "Song search",
        ],
        "{tips:?}"
    );
    assert!(
        texts.iter().any(|t| ASK_PLACEHOLDERS.contains(&t.as_str())),
        "{texts:?}"
    );
    // the three labelled pills, in the design's order
    let pills: Vec<&String> = (texts.iter())
        .filter(|t| ["Search in browser", "Copy text", "Translate"].contains(&t.as_str()))
        .collect();
    assert_eq!(pills, ["Search in browser", "Copy text", "Translate"]);
    // Enter with nothing typed: Ask AI opens (the design), nothing is searched
    let _ = app.update(Message::AskInput("  ".into()));
    let _ = app.update(Message::AskSubmit);
    assert_eq!(
        app.o.panel.as_ref().map(|(_, p)| p.kind()),
        Some(actions::Panel::Ask)
    );
    // "Auto action" opted back in: the selection searches by itself
    let mut app = open_app(1280, 800, 1.0);
    app.o.behaviour = behaviour(SelectStyle::Snap, After::Search, SearchWith::Lens);
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    assert_eq!(
        app.o.panel.as_ref().map(|(_, p)| p.kind()),
        Some(actions::Panel::Lens)
    );
    // the keys still work: T reads the text, A asks, L searches
    for (letter, kind) in [
        ('t', actions::Panel::Text),
        ('a', actions::Panel::Ask),
        ('l', actions::Panel::Lens),
    ] {
        app.o.panel = None;
        event(
            &mut app,
            overlay::Event::Key {
                letter,
                ctrl: false,
            },
        );
        assert_eq!(app.o.panel.as_ref().map(|(_, p)| p.kind()), Some(kind));
    }
}

/// Frames until the bar is where it goes (it glides there).
pub(in crate::ui) fn settle_bar(app: &mut App) {
    let start = Instant::now();
    for i in 0..100 {
        let _ = app.update(Message::Frame(start + Duration::from_millis(16 * i)));
    }
    assert!(!app.bar_gliding());
}

/// The bar's bounds, and whether its Edit slot (the last) is Edit, once settled.
fn bar_box(app: &mut App) -> (Rectangle, bool) {
    settle_bar(app);
    let edit = widgets::shot::click(app.view(app.overlay), app.o.screen, bar_button(app, 2));
    let is_edit = |m: &Message| match m {
        Message::EditScreen => true,
        Message::Run(a) => a.id == actions::edit::ACTION.id,
        _ => false,
    };
    let edits = edit.iter().any(is_edit);
    (Rectangle::new(app.bar_at(), app.bar_size().1), edits)
}

/// The bottom bar is exactly the same size in every state (owner,
/// 2026-10-25): no selection, a selection, after a result card closed.
/// The selection's buttons are always there, dimmed until it exists.
#[test]
fn the_bottom_bar_never_changes_size() {
    let mut app = open_app(1280, 800, 1.0);
    let (before, edits) = bar_box(&mut app);
    assert!(edits);
    // nothing selected: Search in browser and Copy text are there but do nothing
    for k in 0..=1 {
        let at = bar_pill(&app, k);
        let got = widgets::shot::click(app.view(app.overlay), app.o.screen, at);
        assert!(
            !got.iter().any(|m| matches!(m, Message::Pill(_))),
            "{k}: {got:?}"
        );
    }
    // a selection (away from the bar): the same box, the same slots
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    app.o.panel = None; // the auto-search's card
    let (selected, edits) = bar_box(&mut app);
    assert_eq!(selected, before);
    assert!(edits);
    // Ask AI's card opens (the bar steps aside) and closes: the same again
    let _ = app.update(Message::AskButton);
    let (id, _) = app.o.panel.as_ref().expect("Ask AI opens");
    let id = *id;
    assert!(!app.bottom_bar_shown());
    let _ = app.update(Message::Panel(id, panel::Msg::Card(card::Event::Close)));
    assert!(app.bottom_bar_shown());
    let (after, edits) = bar_box(&mut app);
    assert_eq!(after, before);
    assert!(edits);
}

/// Edit turns the bar into the editing toolbar in its place (same anchor,
/// a short morph between them); the toolbar drags and docks like the bar,
/// remembered; Back turns it into the bar again where the toolbar was.
#[test]
fn the_editing_toolbar_takes_the_bars_place_and_docks_like_it() {
    let mut app = open_app(1280, 800, 1.0);
    app.o.work_inset = 40.0;
    // clear of both spots, even for a two-row toolbar
    drag(
        &mut app,
        Point::new(200.0, 300.0),
        Point::new(500.0, 450.0),
        false,
    );
    let (bar_box, _) = bar_box(&mut app);
    let spot = |app: &App, size| bar::anchor_at(app.o.bar_anchor, app.o.screen, size, 40.0);
    assert_eq!(bar_box.position(), spot(&app, bar_box.size()));
    let tool = Editing::toolbar_size(app.o.screen.width);
    let _ = app.update(Message::Run(&actions::edit::ACTION));
    assert!(app.editing());
    // on its way: one shape between the two sizes, at the same spot
    let start = Instant::now();
    let _ = app.update(Message::Frame(start));
    let _ = app.update(Message::Frame(start + Duration::from_millis(40)));
    let mid = app.dock_size();
    assert!(app.swapping(), "a morph, not a jump");
    assert!(mid.width > bar_box.width.min(tool.width));
    assert!(mid.width < bar_box.width.max(tool.width));
    assert_eq!(app.bar_at(), spot(&app, mid));
    // ...drawn at its whole size (inside it differs from just outside it)
    // (over a plain frame)
    app.o.frame = Some(image::Handle::from_rgba(4, 4, vec![9; 4 * 4 * 4]));
    let px = widgets::shot::save("bar-morph", app.view(app.overlay), app.o.screen);
    let at = app.bar_at();
    let y = (at.y + mid.height / 2.0) as usize;
    let pixel = |x: f32| {
        let i = (y * 1280 + x as usize) * 4;
        px[i..i + 4].to_vec()
    };
    assert_ne!(
        pixel(at.x + mid.width - 30.0),
        pixel(at.x + mid.width + 30.0),
        "the morph's shape is there"
    );
    // there: the toolbar at the bar's anchor (bottom edge, centre the same)
    settle_bar(&mut app);
    assert!(!app.swapping());
    let at = app.bar_at();
    assert_eq!(at, spot(&app, tool));
    assert_eq!(at.y + tool.height, bar_box.y + bar_box.height);
    assert_eq!(at.x + tool.width / 2.0, bar_box.center_x());
    // its first row (two rows where drag-out makes it too wide)
    let pad = widgets::SURFACE_PADDING;
    let row = pad + widgets::CONTROL / 2.0;
    let back = at + Vector::new(row, row);
    let hit = |app: &App, p| widgets::shot::click(app.view(app.overlay), app.o.screen, p);
    assert!(
        hit(&app, back)
            .iter()
            .any(|m| matches!(m, Message::Editor(editor::Message::Back))),
        "its Back arrow is there"
    );
    // dragged by its edge, it docks at the nearest spot, remembered
    let edge = at + Vector::new(pad / 2.0, row);
    assert!(
        hit(&app, edge)
            .iter()
            .any(|m| matches!(m, Message::BarGrab))
    );
    let _ = app.update(Message::BarGrab);
    let _ = app.update(Message::BarMoved(edge));
    let _ = app.update(Message::BarMoved(edge - Vector::new(0.0, 560.0)));
    let _ = app.update(Message::BarReleased);
    assert_eq!(app.o.bar_anchor, BarAnchor::Top);
    assert_eq!(config::current().bar.anchor, BarAnchor::Top, "remembered");
    settle_bar(&mut app);
    assert_eq!(app.bar_at(), spot(&app, tool));
    assert_eq!(app.bar_at().y, bar::TOP);
    // Back: the bar again, at the toolbar's spot
    let _ = app.update(Message::Editor(editor::Message::Back));
    assert!(!app.editing());
    // Esc did it: the same key's Escape doesn't close the overlay too
    let _ = app.update(Message::Escape);
    assert!(app.o.frame.is_some(), "still open, at the bar");
    settle_bar(&mut app);
    let (_, size) = app.bar_size();
    assert_eq!(app.bar_at(), spot(&app, size));
    assert_eq!(app.bar_at().y, bar::TOP);
    let edit = hit(&app, bar_button(&app, 2));
    assert!(
        edit.iter()
            .any(|m| matches!(m, Message::Run(a) if a.id == actions::edit::ACTION.id)),
        "{edit:?}"
    );
    let mut cfg = config::current();
    cfg.bar.anchor = BarAnchor::Bottom;
    config::set_current(cfg);
}

/// The bar: by default a clear gap above the taskbar; dragged by its
/// handle it follows the pointer, and let go it glides to the nearest
/// spot, remembered for the next run; a selection under it sends it to a
/// free spot until the space is free again.
#[test]
fn the_bar_drags_by_its_handle_and_docks_at_the_nearest_spot() {
    let mut app = open_app(1280, 800, 1.0);
    app.o.work_inset = 40.0; // a taskbar
    let (_, size) = app.bar_size();
    assert_eq!(app.bar_at().y + size.height, 800.0 - 40.0 - bar::BOTTOM_GAP);
    let handle = app.bar_at() + Vector::new(bar::PAD + bar::HANDLE / 2.0, size.height / 2.0);
    let grabbed = widgets::shot::click(app.view(app.overlay), app.o.screen, handle);
    assert!(grabbed.iter().any(|m| matches!(m, Message::BarGrab)));
    // the field and the buttons don't drag it
    let field = widgets::shot::click(app.view(app.overlay), app.o.screen, bar_field(&app));
    assert!(!field.iter().any(|m| matches!(m, Message::BarGrab)));
    // held, it follows the pointer
    let before = app.bar_at();
    let _ = app.update(Message::BarGrab);
    let _ = app.update(Message::BarMoved(handle));
    let _ = app.update(Message::BarMoved(handle - Vector::new(100.0, 560.0)));
    let held = before - Vector::new(100.0, 560.0);
    assert_eq!(app.bar_target(), held);
    // ...eased there on the frame clock, never jumping per mouse event
    assert_eq!(app.bar_at(), before, "no jump before a frame");
    assert!(app.animating(), "frames while it is behind the pointer");
    let now = Instant::now();
    let _ = app.update(Message::Frame(now));
    let first = app.bar_at();
    assert!(first.y < before.y && first.y > held.y, "{first:?}");
    let _ = app.update(Message::Frame(now + bar::FOLLOW));
    let off = app.bar_at().distance(held) / before.distance(held);
    assert!(off < 0.06, "95 % of the way in FOLLOW: {off}");
    for i in 1..=6 {
        let _ = app.update(Message::Frame(
            now + bar::FOLLOW + Duration::from_millis(16 * i),
        ));
    }
    assert_eq!(app.bar_at(), held, "there");
    assert!(
        !app.animating(),
        "caught up: no more frames while held still"
    );
    // let go near the top: it glides to the top spot, remembered
    let _ = app.update(Message::BarReleased);
    assert_eq!(app.o.bar_anchor, BarAnchor::Top);
    assert_eq!(config::current().bar.anchor, BarAnchor::Top, "remembered");
    let now = Instant::now();
    let _ = app.update(Message::Frame(now));
    let _ = app.update(Message::Frame(now + Duration::from_millis(16)));
    let step = app.bar_at();
    assert!(
        step != before - Vector::new(100.0, 560.0) && app.bar_gliding(),
        "gliding"
    );
    settle_bar(&mut app);
    assert_eq!(app.bar_at().y, bar::TOP);
    // the next opening finds it there
    let mut next = open_app(1280, 800, 1.0);
    next.o.bar_anchor = config::current().bar.anchor;
    assert_eq!(next.bar_at().y, bar::TOP);
    // a selection where the bar is: it moves to a free spot, and comes back
    let bar_box = Rectangle::new(next.bar_at(), size);
    drag(
        &mut next,
        bar_box.position() + Vector::new(50.0, -20.0),
        bar_box.position() + Vector::new(300.0, 120.0),
        false,
    );
    assert!(next.o.selection.is_some());
    settle_bar(&mut next);
    let sel = next.o.view.rect_to_screen(next.o.selection.unwrap().area);
    let (_, size) = next.bar_size();
    assert!(
        !Rectangle::new(next.bar_at(), size).intersects(&sel),
        "never covers it"
    );
    let _ = next.update(Message::Escape); // closes the overlay
    let mut back = open_app(1280, 800, 1.0);
    back.o.bar_anchor = BarAnchor::Top;
    settle_bar(&mut back);
    assert_eq!(back.bar_at().y, bar::TOP, "free again: home");
    // dropped far off screen: the nearest spot, inside the work area
    let _ = back.update(Message::BarGrab);
    let _ = back.update(Message::BarMoved(Point::new(10.0, 10.0)));
    let _ = back.update(Message::BarMoved(Point::new(5000.0, 5000.0)));
    let _ = back.update(Message::BarReleased);
    assert_eq!(back.o.bar_anchor, BarAnchor::BottomRight);
    let mut cfg = config::current();
    cfg.bar.anchor = BarAnchor::Bottom;
    config::set_current(cfg);
}

#[test]
fn the_bottom_bar_has_every_main_action_on_any_screen() {
    for (w, h) in [(1280, 800), (800, 600), (640, 480)] {
        let app = open_app(w, h, 1.0);
        let _ = widgets::tests_support::shown(); // drop older records
        let click = |at| widgets::shot::click(app.view(app.overlay), app.o.screen, at);
        let translate = click(bar_pill(&app, 2));
        let messages: Vec<Vec<Message>> = (0..3).map(|k| click(bar_button(&app, k))).collect();
        let is = |k: usize, f: fn(&Message) -> bool| messages[k].iter().any(f);
        assert!(
            (translate.iter()).any(|m| matches!(m, Message::Pill(BarButton::Translate))),
            "{w}: Translate"
        );
        assert!(is(0, |m| matches!(m, Message::LiveToggle)), "{w}: Live");
        assert!(
            is(1, |m| matches!(m, Message::SongStart)),
            "{w}: music, key or not"
        );
        assert!(is(2, |m| matches!(m, Message::EditScreen)), "{w}: Edit");
        // each names itself (icons) and its shortcut in its tooltip
        let tips = widgets::tests_support::shown();
        for want in [
            "Live translate (Beta) · Ctrl+Shift+L",
            "Song search",
            "Edit the screen · E",
        ] {
            assert!(tips.iter().any(|t| t == want), "{want}: {tips:?}");
        }
        widgets::tests_support::assert_tips_add_something("bottom bar");
    }
}

#[test]
fn the_bottom_bar_buttons_work_on_the_whole_screen() {
    // without a song key the music button opens its setup: nothing is recorded
    let mut app = open_app(800, 600, 1.0);
    let _ = app.song(false);
    assert!(
        app.o.song.is_none() && app.o.frame.is_none(),
        "closed, not listening"
    );
    assert!(app.windows.busy(), "Settings opened (Song search)");
    // Ask AI with an empty field: the panel with its presets, on the whole screen
    let mut app = open_app(800, 600, 1.0);
    let _ = app.update(Message::AskButton);
    assert_eq!(
        app.o.panel.as_ref().map(|(_, p)| p.kind()),
        Some(actions::Panel::Ask)
    );
    // the same keys with nothing selected: A asks, E edits, R translates, all the screen
    let mut app = open_app(800, 600, 1.0);
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'e',
            ctrl: false,
        },
    );
    assert!(app.o.edit.is_some(), "the whole screen in the editor");
    assert_eq!(
        app.o.selection.map(|s| s.area),
        Some(Rectangle::with_size(app.o.screen))
    );
    let mut app = open_app(800, 600, 1.0);
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'r',
            ctrl: false,
        },
    );
    assert!(app.o.translate.is_some());
    let mut app = open_app(800, 600, 1.0);
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'a',
            ctrl: false,
        },
    );
    assert!(app.o.panel.is_some());
}

/// The opening as drawn `ms` after its first frame (`None`: the first frame).
/// (A frame every 10 ms.)
fn opening_at(ms: Option<u64>) -> App {
    let mut app = text_app();
    app.o.appear = None;
    if let Some(ms) = ms {
        let start = Instant::now();
        for at in (0..=ms).step_by(10).chain([ms]) {
            let _ = app.update(Message::Frame(start + Duration::from_millis(at)));
        }
    }
    app
}

#[test]
fn the_opening_is_one_eased_animation_from_its_first_frame() {
    // frame 1 is t = 0, already dimmed: the window fades in as a whole
    // (`windows::fade`), the dim never fades over the whole screen
    let first = opening_at(None);
    assert_eq!(first.appeared(), 0.0);
    let shot = |app: &App| widgets::shot::render(app.view(app.overlay), 960, 420);
    let bright = |rgba: &[u8]| {
        let at = (300 * 960 + 700) * 4; // an empty part of the page
        u32::from(rgba[at]) + u32::from(rgba[at + 1]) + u32::from(rgba[at + 2])
    };
    let frame = match &first.o.frame {
        Some(image::Handle::Rgba { pixels, .. }) => pixels.to_vec(),
        _ => unreachable!(),
    };
    let dimmed = bright(&shot(&first));
    assert!(dimmed < bright(&frame), "dimmed on frame 1");
    // then it only ever moves one way, to rest by APPEAR; the dim stays
    let steps: Vec<(f32, u32)> = (0..=300)
        .step_by(25)
        .map(|ms| {
            let app = opening_at(Some(ms));
            (app.appeared(), bright(&shot(&app)))
        })
        .collect();
    for pair in steps.windows(2) {
        assert!(pair[1].0 >= pair[0].0 && pair[1].1 == dimmed, "{steps:?}");
    }
    assert!(
        steps[1].0 > 0.0 && steps[1].0 < 1.0,
        "eased in steps, no jump"
    );
    let done = opening_at(Some(overlay::APPEAR.as_millis() as u64));
    assert_eq!(done.appeared(), 1.0);
    assert!(!done.appearing(), "no more frames asked for");
    assert!(opening_at(Some(10)).appearing());
    // by the clock from the first frame: one slow frame (a busy machine)
    // lands where 60 fps would be, neither ahead nor behind
    let mut slow = opening_at(Some(0));
    let start = slow.o.appear.unwrap().0;
    let _ = slow.update(Message::Frame(start + Duration::from_millis(100)));
    let smooth = opening_at(Some(100));
    assert_eq!(slow.appeared(), smooth.appeared());
    assert!(
        slow.appeared() > 0.5 && slow.appearing(),
        "{}",
        slow.appeared()
    );
    // a new opening starts again at t = 0
    let mut again = done;
    let _ = again.open(Instant::now(), a_shot());
    assert_eq!(again.appeared(), 0.0);
}

/// `OPENCIRCLE_RENDER_DIR=<dir> cargo test opening_frames -- --ignored`:
/// the overlay's first 300 ms, a PNG every 25 ms; and the time a full
/// frame of the opening takes at 3440x1440 (the first frame: every pixel
/// drawn, while the window is still clear).
#[test]
#[ignore = "writes PNGs and prints timings"]
fn opening_frames() {
    for ms in (0..=300).step_by(25) {
        let app = opening_at(Some(ms));
        let name = format!("opening-{ms:03}ms");
        widgets::shot::save(&name, app.view(app.overlay), Size::new(960.0, 420.0));
    }
    let mut app = open_app(3440, 1440, 1.0);
    app.o.appear = None;
    app.o.appear = Some((Instant::now(), 0.4));
    let t = Instant::now();
    for _ in 0..5 {
        widgets::shot::render(app.view(app.overlay), 3440, 1440);
    }
    println!("3440x1440 opening frame: {:?}", t.elapsed() / 5);
    let t = Instant::now();
    for _ in 0..5 {
        widgets::shot::render(Element::<'_, Message>::from(space()), 3440, 1440);
    }
    println!("3440x1440 empty frame (the harness): {:?}", t.elapsed() / 5);
    let frame = app.o.frame.clone().unwrap();
    let t = Instant::now();
    for _ in 0..5 {
        let only = image(frame.clone())
            .width(Fill)
            .height(Fill)
            .filter_method(image::FilterMethod::Nearest);
        widgets::shot::render(Element::<'_, Message>::from(only), 3440, 1440);
    }
    println!("3440x1440 the frame alone: {:?}", t.elapsed() / 5);
}

/// What one key press (letter `c`) on the overlay produces, after the
/// widget operations `task` asks for (focusing a field, say) ran on it.
fn key_after(app: &App, task: Task<Message>, click: Option<Point>, c: char) -> Vec<Message> {
    use iced::advanced::clipboard;
    use iced::keyboard::{Key, Location, Modifiers, key};
    use iced_runtime::user_interface::{Cache, UserInterface};
    let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
    let view = app.view(app.overlay);
    let mut ui = UserInterface::build(view, app.o.screen, Cache::default(), &mut renderer);
    let actions = iced_runtime::task::into_stream(task)
        .map(iced::futures::executor::block_on_stream)
        .into_iter()
        .flatten();
    for action in actions {
        if let iced_runtime::Action::Widget(mut op) = action {
            ui.operate(&renderer, op.as_mut());
        }
    }
    let letter = Key::Character(c.to_string().into());
    let press = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: letter.clone(),
        modified_key: letter,
        physical_key: key::Physical::Code(key::Code::KeyT),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: Some(c.to_string().into()),
        repeat: false,
    });
    let mut events = Vec::new();
    if let Some(at) = click {
        use iced::mouse::{Button, Event as Mouse};
        events.extend([
            iced::Event::Mouse(Mouse::CursorMoved { position: at }),
            iced::Event::Mouse(Mouse::ButtonPressed(Button::Left)),
            iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)),
        ]);
    }
    events.push(press);
    let mut messages = Vec::new();
    let cursor = click.map_or(
        iced::mouse::Cursor::Unavailable,
        iced::mouse::Cursor::Available,
    );
    ui.update(
        &events,
        cursor,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    messages
}

fn is_key(m: &Message, c: char) -> bool {
    matches!(m, Message::Overlay(overlay::Event::Key { letter, ctrl: false }) if *letter == c)
}

/// Letters are the action shortcuts until a text field is clicked: a
/// result card (Ask AI, once ready) doesn't take the keys by itself.
#[test]
fn letters_stay_shortcuts_with_a_result_card_open() {
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    let opened = app.open_panel(actions::Panel::Ask, crop.clone(), None);
    drop(opened); // its work (the setup) is done below by hand
    let (id, _) = app.o.panel.as_ref().map(|(id, p)| (*id, p.kind())).unwrap();
    let setup = actions::ask::setup(&crop).map(Arc::new);
    assert!(setup.is_ok(), "the no-key provider needs no setup");
    let ready = app.update(Message::Panel(id, panel::Msg::Ready(setup)));
    let keys = key_after(&app, ready, None, 't');
    assert!(
        keys.iter().any(|m| is_key(m, 't')),
        "T is Translate, not typing"
    );
    // the bottom bar's field doesn't take them either
    let app = open_app(800, 600, 1.0);
    let keys = key_after(&app, Task::none(), None, 'a');
    assert!(keys.iter().any(|m| is_key(m, 'a')));
    // once clicked, it has them: a typed question, not a shortcut
    let field = bar_field(&app);
    let keys = key_after(&app, Task::none(), Some(field), 'a');
    assert!(!keys.iter().any(|m| is_key(m, 'a')));
    assert!(
        keys.iter()
            .any(|m| matches!(m, Message::AskInput(t) if t == "a"))
    );
}

// --- #101: one focus rule. A focused text field takes the typed letters; the
// single-letter shortcuts run only when none has focus. ---

use widgets::shot::{self, Clip, Focus};

/// The shortcut letters that reached the overlay.
fn shortcuts(messages: &[Message]) -> String {
    (messages.iter())
        .filter_map(|m| match m {
            Message::Overlay(overlay::Event::Key {
                letter,
                ctrl: false,
            }) => Some(*letter),
            _ => None,
        })
        .collect()
}

/// An open overlay with a selection and a ready Ask AI panel (browser provider).
fn app_with_ask_panel() -> (App, u32) {
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    let _ = app.open_panel(actions::Panel::Ask, crop.clone(), None);
    let (id, _) = app.o.panel.as_ref().map(|(id, p)| (*id, p.kind())).unwrap();
    let setup = actions::ask::setup(&crop).map(Arc::new);
    let _ = app.update(Message::Panel(id, panel::Msg::Ready(setup)));
    (app, id)
}

/// An open overlay with a selection and a Text panel that has read `text`.
fn app_with_text_panel(text: &str) -> App {
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    let _ = app.open_panel(actions::Panel::Text, crop, None);
    let id = app.o.panel.as_ref().map(|(id, _)| *id).unwrap();
    let _ = app.update(Message::Panel(id, panel::Msg::Read(Ok(text.into()))));
    app
}

fn feed(
    app: &App,
    focus: Focus,
    events: &[iced::Event],
) -> (Vec<Message>, Vec<iced::event::Status>) {
    shot::feed(
        app.view(app.overlay),
        app.o.screen,
        focus,
        events,
        &mut Clip::default(),
    )
}

#[test]
fn typing_in_the_bars_field_runs_no_shortcut() {
    let app = open_app(800, 600, 1.0);
    let (messages, _) = feed(&app, Focus::Id(ASK_INPUT), &shot::typing("real estate"));
    assert_eq!(shortcuts(&messages), "", "{messages:?}");
    let typed = (messages.iter()).filter(|m| matches!(m, Message::AskInput(_)));
    assert_eq!(typed.count(), "real estate".len());
    assert!(!messages.iter().any(|m| matches!(
        m,
        Message::Overlay(overlay::Event::Space | overlay::Event::Enter) | Message::AskSubmit
    )));
    // The same letters with no field focused are the shortcuts.
    let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("aterl"));
    assert_eq!(shortcuts(&messages), "aterl");
}

#[test]
fn typing_in_the_ask_panels_input_runs_no_shortcut() {
    let (app, _) = app_with_ask_panel();
    let (messages, _) = feed(&app, Focus::Id(panel::INPUT), &shot::typing("real estate"));
    assert_eq!(shortcuts(&messages), "", "{messages:?}");
    let typed = (messages.iter()).filter(|m| matches!(m, Message::Panel(_, panel::Msg::Input(_))));
    assert_eq!(typed.count(), "real estate".len());
    let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("aterl"));
    assert_eq!(
        shortcuts(&messages),
        "aterl",
        "the panel doesn't take keys by itself"
    );
}

#[test]
fn typing_in_the_text_panels_editor_runs_no_shortcut() {
    let app = app_with_text_panel("hello");
    let (messages, _) = feed(&app, Focus::First, &shot::typing("real estate"));
    assert_eq!(shortcuts(&messages), "", "{messages:?}");
    let edits = (messages.iter())
        .filter(|m| matches!(m, Message::Panel(_, panel::Msg::Edit(_))))
        .count();
    assert!(edits >= "real estate".len(), "{messages:?}");
    let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("aterl"));
    assert_eq!(shortcuts(&messages), "aterl");
}

#[test]
fn a_translation_has_no_field_to_take_the_letters() {
    let mut app = open_app(800, 600, 1.0);
    let frame = app.o.frame.clone().unwrap();
    let (session, _) =
        translate::Session::start(frame, [0, 0, 800, 600], 1.0, app.cache.clone(), false);
    app.o.translate = Some((7, session));
    assert!(!app.bottom_bar_shown(), "no bar, so no ask field");
    // Tab has no bar to move through; the card's E / A stay the card's.
    let _ = app.on_overlay(overlay::Event::Tab { back: false });
    assert_eq!(app.o.bar_focus, None);
    let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("ea"));
    assert_eq!(shortcuts(&messages), "ea");
}

#[test]
fn esc_leaves_a_focused_field_and_the_second_closes() {
    let mut app = open_app(800, 600, 1.0);
    let esc = shot::named(iced::keyboard::key::Named::Escape, false);
    // bar field
    let (messages, status) = feed(&app, Focus::Id(ASK_INPUT), &esc);
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Field(keys::Keyed::Left)))
    );
    assert_eq!(
        status[1],
        iced::event::Status::Captured,
        "the overlay is told a widget took it"
    );
    // the subscription turns that into EscapeTaken: nothing closes
    let _ = app.update(Message::EscapeTaken);
    assert!(app.o.frame.is_some() && app.hiding == Hiding::No);
    // with the field let go, letters are shortcuts again
    let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("t"));
    assert_eq!(shortcuts(&messages), "t");
    // the second Esc reaches nobody: it closes as today
    let (_, status) = feed(&app, Focus::Nothing, &esc);
    assert_eq!(status[1], iced::event::Status::Ignored);
    let _ = app.update(Message::Escape);
    assert!(app.o.frame.is_none());
    // the Ask panel's input and the Text panel's editor (which lets Esc through)
    let (app, _) = app_with_ask_panel();
    let (messages, status) = feed(&app, Focus::Id(panel::INPUT), &esc);
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Panel(_, panel::Msg::Key(keys::Keyed::Left))))
    );
    assert_eq!(status[1], iced::event::Status::Captured);
    let app = app_with_text_panel("hello");
    let (messages, status) = feed(&app, Focus::First, &esc);
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Panel(_, panel::Msg::Key(keys::Keyed::Left))))
    );
    assert_eq!(
        status[1],
        iced::event::Status::Captured,
        "Esc must not also close the overlay"
    );
}

#[test]
fn ctrl_c_in_a_field_copies_its_selection_else_the_capture() {
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(100.0, 80.0),
        Point::new(400.0, 280.0),
        false,
    );
    app.o.ask = "hello".into();
    let run = |app: &App, events: Vec<iced::Event>| {
        let mut clip = Clip::default();
        let (messages, _) = shot::feed(
            app.view(app.overlay),
            app.o.screen,
            Focus::Id(ASK_INPUT),
            &events,
            &mut clip,
        );
        (messages, clip.0)
    };
    let copy = |m: &Message| {
        matches!(m, Message::Field(keys::Keyed::Copy))
            || matches!(
                m,
                Message::Overlay(overlay::Event::Key {
                    letter: 'c',
                    ctrl: true
                })
            )
    };
    // text selected (Ctrl+A): the field's own copy, the capture is not copied
    let select_all = [shot::chord('a'), shot::chord('c')].concat();
    let (messages, clip) = run(&app, select_all);
    assert_eq!(clip, ["hello"]);
    assert!(!messages.iter().any(copy), "{messages:?}");
    // nothing selected: the overlay's copy of the capture
    let (messages, clip) = run(&app, shot::chord('c'));
    assert!(clip.is_empty());
    let copy_message = messages
        .into_iter()
        .find(copy)
        .expect("the capture is copied");
    let _ = copy::copied::image();
    let task = app.update(copy_message);
    run_task(&mut app, task);
    assert!(copy::copied::image().is_some(), "the capture was copied");
    // Ctrl+S saves even with the field focused (and text selected)
    // (the copy closed that overlay: a fresh one)
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(100.0, 80.0),
        Point::new(400.0, 280.0),
        false,
    );
    app.o.ask = "hello".into();
    let (messages, _) = run(&app, shot::chord('s'));
    assert!(messages.iter().any(|m| matches!(
        m,
        Message::Overlay(overlay::Event::Key {
            letter: 's',
            ctrl: true
        })
    )));
}

#[test]
fn tab_walks_the_bar_in_the_designs_order_and_enter_or_space_press() {
    let mut app = open_app(800, 600, 1.0);
    let tab = |app: &mut App, back| {
        let _ = app.on_overlay(overlay::Event::Tab { back });
        app.o.bar_focus
    };
    use bar::Stop::{Badge, Button, Field, Logo};
    let forward: Vec<_> = (0..8).map(|_| tab(&mut app, false)).collect();
    let order = [
        Logo,
        Field,
        Badge,
        Button(BarButton::Translate), // Search in browser and Copy text are dimmed
        Button(BarButton::Live),
        Button(BarButton::Song),
        Button(BarButton::Edit),
        Logo, // and round again
    ];
    assert_eq!(forward, order.map(Some));
    assert_eq!(tab(&mut app, true), Some(Button(BarButton::Edit)));
    // Tab events come from the key itself, Shift+Tab goes back
    let (messages, _) = feed(
        &app,
        Focus::Nothing,
        &shot::named(iced::keyboard::key::Named::Tab, true),
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Overlay(overlay::Event::Tab { back: true })))
    );
    // a selection brings the two pills into the order, after the badge
    app.select(Rectangle::new(
        Point::new(100.0, 80.0),
        Size::new(300.0, 200.0),
    ));
    app.o.bar_focus = Some(Badge);
    assert_eq!(tab(&mut app, false), Some(Button(BarButton::Search)));
    assert_eq!(tab(&mut app, false), Some(Button(BarButton::CopyText)));
    // Enter and Space press the focused control
    let mut app = open_app(800, 600, 1.0);
    app.o.bar_focus = Some(Button(BarButton::Translate));
    let _ = app.on_overlay(overlay::Event::Space);
    assert_eq!(app.o.pill, BarButton::Translate);
    let mut app = open_app(800, 600, 1.0);
    app.o.bar_focus = Some(Logo);
    let _ = app.on_overlay(overlay::Event::Enter);
    assert!(app.o.panel.is_some(), "the logo opens the AI chat");
    // Space with nothing focused does nothing
    let mut app = open_app(800, 600, 1.0);
    let _ = app.on_overlay(overlay::Event::Space);
    assert!(app.o.panel.is_none() && app.o.bar_focus.is_none());
    // a click puts the focus back on the overlay
    app.o.bar_focus = Some(Badge);
    let _ = app.on_overlay(overlay::Event::Press {
        at: Point::new(5.0, 5.0),
        shift: false,
    });
    assert_eq!(app.o.bar_focus, None);
}

fn behaviour(style: SelectStyle, after: After, search_with: SearchWith) -> SelectionConfig {
    SelectionConfig {
        style,
        after,
        search_with,
        text_reader: TextReader::Auto,
    }
}

#[test]
fn auto_search_runs_what_the_setting_says_and_only_for_a_search() {
    let ids = |b| auto_targets(b).iter().map(|a| a.id).collect::<Vec<_>>();
    let snap = SelectStyle::Snap;
    assert!(ids(behaviour(snap, After::Wait, SearchWith::Both)).is_empty());
    assert_eq!(
        ids(behaviour(snap, After::Search, SearchWith::Lens)),
        ["lens"]
    );
    assert_eq!(ids(behaviour(snap, After::Search, SearchWith::Ai)), ["ai"]);
    assert_eq!(
        ids(behaviour(snap, After::Search, SearchWith::Both)),
        ["lens", "ai"]
    );
    // the defaults: snap, then wait for the user's choice (N-03)
    assert_eq!(
        config::Config::default().selection,
        behaviour(snap, After::Wait, SearchWith::Lens)
    );
    // an App: a drawn selection searches; a tapped word, Snip or nothing doesn't
    let mut app = open_app(400, 300, 1.0);
    app.o.behaviour = behaviour(snap, After::Search, SearchWith::Ai);
    assert!(app.auto_actions().is_empty(), "no selection yet");
    app.select(Rectangle::new(
        Point::new(10.0, 10.0),
        Size::new(80.0, 60.0),
    ));
    assert_eq!(app.auto_actions().len(), 1);
    app.o.selection.as_mut().unwrap().kind = overlay::Kind::Word;
    assert!(app.auto_actions().is_empty(), "a tapped word waits");
    app.o.selection.as_mut().unwrap().kind = overlay::Kind::Snap;
    app.o.purpose = Purpose::Snip(Box::new(Snip::new(Vec::new(), app.place(), false, None)));
    assert!(app.auto_actions().is_empty(), "Snip has no search");
    // an App opened before the setting existed behaves as before
    assert!(auto_targets(App::idle(window::Id::unique()).o.behaviour).is_empty());
}

#[test]
fn a_circle_snaps_to_a_box_that_morphs_and_resizes_by_its_brackets() {
    let mut app = open_app(800, 600, 1.0);
    app.o.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
    circle(&mut app, Point::new(300.0, 250.0), 80.0, false);
    let sel = app.o.selection.unwrap();
    assert_eq!(sel.kind, overlay::Kind::Snap);
    assert!(close_to(
        sel.area,
        Rectangle::new(Point::new(220.0, 170.0), Size::new(160.0, 160.0))
    ));
    assert_eq!(app.o.stroke.len(), 2, "the box's corners");
    let (from, _) = app.o.morph.as_ref().expect("it morphs from the drawn loop");
    assert!(from.len() > 20);
    // a straight drag is already a box: no morph
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(300.0, 200.0),
        false,
    );
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Snap);
    assert!(app.o.morph.is_none());
    // the bottom-right bracket's arm resizes it; it stays a snapped box
    let arm = Point::new(303.0, 185.0);
    drag(&mut app, arm, arm + Vector::new(50.0, 40.0), false);
    let grown = app.o.selection.unwrap();
    assert_eq!(grown.kind, overlay::Kind::Snap);
    assert!(close_to(
        grown.area,
        Rectangle::new(Point::new(100.0, 100.0), Size::new(250.0, 140.0))
    ));
    // "As drawn": the loop as before
    app.o.behaviour.style = SelectStyle::Shape;
    circle(&mut app, Point::new(500.0, 300.0), 60.0, false);
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Loop);
    assert!(app.o.morph.is_none());
}

/// A snapped box on `text_app`'s page, the screen's text read.
fn boxed_text(area: Rectangle) -> App {
    let mut app = read_app();
    app.o.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
    let (sel, stroke) = overlay::snap(area);
    (app.o.selection, app.o.stroke) = (Some(sel), stroke);
    app
}

fn rect4(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
    Rectangle::new(Point::new(x, y), Size::new(w, h))
}

#[test]
fn a_snapped_box_resizes_by_its_edges_and_corners() {
    let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
    // the top edge, up 10: only that side moves (grabbed 1 px off it)
    drag(
        &mut app,
        Point::new(300.0, 21.0),
        Point::new(300.0, 11.0),
        false,
    );
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Snap);
    assert_eq!(app.area(), rect4(20.0, 10.0, 520.0, 180.0));
    // the right edge, in 100; the bottom-left bracket, out
    drag(
        &mut app,
        Point::new(541.0, 100.0),
        Point::new(441.0, 100.0),
        false,
    );
    assert_eq!(app.area(), rect4(20.0, 10.0, 420.0, 180.0));
    drag(
        &mut app,
        Point::new(17.0, 193.0),
        Point::new(7.0, 203.0),
        false,
    );
    assert_eq!(app.area(), rect4(10.0, 10.0, 430.0, 190.0));
    // the actions take the adjusted box, and it is remembered for the next time
    assert_eq!(app.regions.last().map(|r| r.1), Some(app.area()));
    // never off the screen (960x420), never under the minimum
    drag(
        &mut app,
        Point::new(441.0, 100.0),
        Point::new(5000.0, 100.0),
        false,
    );
    assert_eq!(app.area().x + app.area().width, 960.0);
    drag(
        &mut app,
        Point::new(961.0, 100.0),
        Point::new(-900.0, 100.0),
        false,
    );
    assert_eq!(app.area().width, overlay::MIN_SIDE);
}

#[test]
fn a_snapped_box_moves_from_inside_but_text_inside_still_selects() {
    let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
    // the empty margin left of the text: move, stopped at the screen's edge
    drag(
        &mut app,
        Point::new(40.0, 100.0),
        Point::new(60.0, 110.0),
        false,
    );
    assert_eq!(app.area(), rect4(40.0, 30.0, 520.0, 170.0));
    let boxed = rect4(20.0, 20.0, 520.0, 170.0);
    app.o.selection = Some(overlay::snap(boxed).0);
    drag(
        &mut app,
        Point::new(40.0, 100.0),
        Point::new(-900.0, 900.0),
        false,
    );
    assert_eq!(app.area(), rect4(0.0, 250.0, 520.0, 170.0));
    // on a word: text is selected, the box stays where it was
    app.o.selection = Some(overlay::snap(rect4(20.0, 20.0, 520.0, 170.0)).0);
    stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
    assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
}

#[test]
fn alt_drag_moves_the_box_even_over_text() {
    let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
    event(&mut app, overlay::Event::Alt(true));
    let on_text = caret(1, 7.0);
    drag(&mut app, on_text, on_text + Vector::new(20.0, 10.0), false);
    assert_eq!(app.area(), rect4(40.0, 30.0, 520.0, 170.0));
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Snap);
    // a bracket still resizes with Alt held
    drag(
        &mut app,
        Point::new(37.0, 33.0),
        Point::new(27.0, 23.0),
        false,
    );
    assert_eq!(app.area(), rect4(30.0, 20.0, 530.0, 180.0));
    // Alt let go: text selects again
    event(&mut app, overlay::Event::Alt(false));
    stroke(&mut app, &[caret(1, 7.0), caret(1, 21.0)]);
    assert_eq!(picked_text(&app).as_deref(), Some("while you drag"));
}

fn arrow(app: &mut App, dx: i8, dy: i8, ctrl: bool, shift: bool) {
    event(
        app,
        overlay::Event::Arrow {
            dx,
            dy,
            ctrl,
            shift,
        },
    );
}

#[test]
fn arrows_nudge_and_resize_the_box_within_the_screen() {
    let mut app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
    arrow(&mut app, 1, 0, false, false);
    arrow(&mut app, 0, 1, false, true);
    assert_eq!(app.area(), rect4(21.0, 30.0, 520.0, 170.0));
    arrow(&mut app, 1, 1, true, false);
    arrow(&mut app, 1, 0, true, true);
    assert_eq!(app.area(), rect4(21.0, 30.0, 531.0, 171.0));
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Snap);
    // the actions take the adjusted box
    assert_eq!(app.regions.last().map(|r| r.1), Some(app.area()));
    // stops at the screen (960x420) and at the minimum size
    for _ in 0..30 {
        arrow(&mut app, -1, -1, false, true);
    }
    assert_eq!(app.area(), rect4(0.0, 0.0, 531.0, 171.0));
    for _ in 0..100 {
        arrow(&mut app, 1, 1, true, true);
    }
    assert_eq!((app.area().width, app.area().height), (960.0, 420.0));
    for _ in 0..100 {
        arrow(&mut app, -1, -1, true, true);
    }
    let min = overlay::MIN_SIDE;
    assert_eq!(app.area().size(), Size::new(min, min));
    for _ in 0..200 {
        arrow(&mut app, 1, 1, false, true);
    }
    let a = app.area();
    assert_eq!((a.x + a.width, a.y + a.height), (960.0, 420.0));
}

#[test]
fn arrows_do_nothing_while_a_text_field_has_the_focus() {
    use iced::keyboard::key::{Named, NativeCode, Physical};
    use iced::keyboard::{Key, Location, Modifiers};
    let app = boxed_text(rect4(20.0, 20.0, 520.0, 170.0));
    let key = Key::Named(Named::ArrowRight);
    let press = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: Physical::Unidentified(NativeCode::Unidentified),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let arrows = |m: &[Message]| {
        (m.iter())
            .filter(|m| matches!(m, Message::Overlay(overlay::Event::Arrow { .. })))
            .count()
    };
    let (messages, _) = feed(&app, Focus::Id(ASK_INPUT), std::slice::from_ref(&press));
    assert_eq!(arrows(&messages), 0, "{messages:?}");
    let (messages, _) = feed(&app, Focus::Nothing, &[press]);
    assert_eq!(arrows(&messages), 1, "{messages:?}");
}

/// Draw, release, snap, then act: during the press only the stroke is there,
/// after the release the actions wait for the morph to end.
#[test]
fn the_selections_actions_wait_for_the_snap_to_finish() {
    let mut app = open_app(1280, 800, 1.0);
    app.o.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
    let pill = |app: &App| {
        let got = widgets::shot::click(app.view(app.overlay), app.o.screen, bar_pill(app, 0));
        got.iter().any(|m| matches!(m, Message::Pill(_)))
    };
    let ring = ring(300.0, 250.0, 80.0);
    event(
        &mut app,
        overlay::Event::Press {
            at: ring[0],
            shift: false,
        },
    );
    for &p in &ring[1..] {
        event(&mut app, overlay::Event::Move(p));
        // the press: the stroke and nothing else (no box, no snap, no actions)
        assert!(app.o.selection.is_none() && app.o.morph.is_none() && !app.settling());
        assert!(app.o.drawing && !pill(&app));
    }
    event(&mut app, overlay::Event::Release);
    assert!(app.o.selection.is_some() && app.settling(), "morphing");
    assert!(!pill(&app), "no actions mid-morph");
    // the morph ends: the actions are there
    let (from, _) = app.o.morph.take().unwrap();
    app.o.morph = Some((from, Instant::now() - overlay::MORPH * 2));
    assert!(!app.settling() && pill(&app));
}

#[test]
fn a_circle_snaps_and_searches_only_on_release() {
    let mut app = open_app(800, 600, 1.0);
    app.o.behaviour = behaviour(SelectStyle::Snap, After::Search, SearchWith::Lens);
    let ring = ring(300.0, 250.0, 80.0);
    let press = overlay::Event::Press {
        at: ring[0],
        shift: false,
    };
    event(&mut app, press);
    for &p in &ring[1..] {
        event(&mut app, overlay::Event::Move(p));
        assert!(
            app.o.selection.is_none() && app.o.morph.is_none(),
            "drawing"
        );
        assert!(app.auto_actions().is_empty(), "nothing searches yet");
    }
    event(&mut app, overlay::Event::Release);
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Snap);
    assert!(app.o.morph.is_some(), "the stroke morphs into the box");
    assert!(app.auto_actions().is_empty(), "not while it snaps");
    let (from, _) = app.o.morph.take().unwrap();
    app.o.morph = Some((from, Instant::now() - overlay::MORPH * 2));
    assert!(!app.auto_actions().is_empty(), "then the search runs");
}

/// A snapped box over `text_app`'s lines 1-3, in select-text mode with its words read.
fn select_text_app() -> App {
    let mut app = text_app();
    app.o.behaviour = behaviour(SelectStyle::Snap, After::Wait, SearchWith::Lens);
    let (sel, stroke) = overlay::snap(Rectangle::new(
        Point::new(40.0, 66.0),
        Size::new(480.0, 112.0),
    ));
    (app.o.selection, app.o.stroke) = (Some(sel), stroke);
    app.o.text_mode = Some(TextMode {
        lines: None,
        boxes: Vec::new(),
        picked: None,
        picking: false,
    });
    let lines = word::lines(text_blocks(), [0, 0], 1.0);
    let inside: Vec<_> = (lines.into_iter())
        .filter(|l| sel.area.contains(l.bounds.center()))
        .collect();
    app.on_text_lines(sel.area, Ok(inside));
    app
}

#[test]
fn select_text_highlights_every_word_and_a_drag_picks_some() {
    let mut app = select_text_app();
    let texts = widgets::shot::texts(app.view(app.overlay));
    assert!(texts.contains(&"Done".to_string()), "{texts:?}");
    let mode = app.o.text_mode.as_ref().unwrap();
    assert_eq!(mode.boxes.len(), 17, "every word of lines 1-3");
    // a stale read (the box moved on) is dropped
    app.on_text_lines(Rectangle::default(), Ok(Vec::new()));
    assert_eq!(app.o.text_mode.as_ref().unwrap().boxes.len(), 17);
    // press before "while", drag to after "words": two lines picked, the box stays
    drag(&mut app, caret(1, 7.0), caret(2, 16.0), false);
    let mode = app.o.text_mode.as_ref().unwrap();
    let picked = mode.picked.as_ref().unwrap();
    assert_eq!(picked.text(), "while you drag a finger\nacross the words");
    assert!(!mode.picking);
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Snap);
    // the bottom bar's text actions take the picked words; no pill, no second bar
    let texts = widgets::shot::texts(app.view(app.overlay));
    assert!(
        !texts.iter().any(|t| t == "Copy" || t == "Edit"),
        "{texts:?}"
    );
    let click = |k| widgets::shot::click(app.view(app.overlay), app.o.screen, bar_pill(&app, k));
    assert!(
        click(1)
            .iter()
            .any(|m| matches!(m, Message::Pill(BarButton::CopyText)))
    );
    // Ctrl+C copies the same picked characters, not the whole box's text
    let mut keyed = select_text_app();
    drag(&mut keyed, caret(1, 9.4), caret(2, 16.0), false);
    let _ = copy::copied::text();
    let ctrl_c = overlay::Event::Key {
        letter: 'c',
        ctrl: true,
    };
    let task = keyed.update(Message::Overlay(ctrl_c));
    run_task(&mut keyed, task);
    assert_eq!(keyed.windows.toasts(), [("Copied".to_string(), true)]);
    assert_eq!(
        copy::copied::text().as_deref(),
        Some("ile you drag a finger\nacross the words")
    );
    // Done leaves the mode; a press outside the box starts over
    let _ = app.update(Message::SelectText);
    assert!(app.o.text_mode.is_none());
    let _ = app.update(Message::SelectText); // no OCR models here: the Text panel asks
    drag(
        &mut app,
        Point::new(700.0, 300.0),
        Point::new(900.0, 400.0),
        false,
    );
    assert!(app.o.text_mode.is_none());
}

#[test]
fn chips_go_under_the_box_else_above_never_on_it() {
    let screen = Rectangle::with_size(Size::new(1920.0, 1080.0));
    let size = Size::new(280.0, 44.0);
    let area = Rectangle::new(Point::new(400.0, 300.0), Size::new(500.0, 200.0));
    let at = chips_at(area, size, screen);
    assert_eq!(at, Point::new(510.0, 512.0), "centred, 12 px under the box");
    // no room below: above
    let low = Rectangle::new(Point::new(400.0, 900.0), Size::new(500.0, 150.0));
    let at = chips_at(low, size, screen);
    assert_eq!(at.y, 900.0 - CHIP_SPACE - 44.0);
    for a in [area, low] {
        let chips = Rectangle::new(chips_at(a, size, screen), size);
        assert!(chips.intersection(&a).is_none(), "{a:?} -> {chips:?}");
    }
    // beside a card: kept in the free part
    let room = Rectangle::new(Point::ORIGIN, Size::new(1112.0, 1080.0));
    let edge = Rectangle::new(Point::new(950.0, 300.0), Size::new(150.0, 100.0));
    let at = chips_at(edge, size, room);
    assert!(at.x + size.width <= room.width - bar::GAP);
    // the whole screen selected: on it, at the bottom, still on screen
    let all = chips_at(screen, size, screen);
    assert_eq!(all.y, 1080.0 - bar::GAP - 44.0);
}

#[test]
fn nothing_floats_over_the_result_card() {
    let mut app = open_app(1920, 1080, 1.0);
    // a selection right up to where the card goes, bar shown beside it
    drag(
        &mut app,
        Point::new(700.0, 300.0),
        Point::new(940.0, 500.0),
        false,
    );
    let _ = app.update(Message::Run(&text::ACTION));
    let card = app.card_at();
    let room = app.room();
    assert!(room.intersection(&card).is_none(), "{room:?} vs {card:?}");
    assert!(room.width > 0.0);
    // and with no card, the whole screen
    app.o.panel = None;
    assert_eq!(app.room(), Rectangle::with_size(app.o.screen));
}

#[test]
fn a_quick_action_on_a_drawn_area_runs_alone_not_the_auto_search_too() {
    let mut app = open_app(1920, 1080, 1.0);
    app.o.behaviour = SelectionConfig::default(); // auto search: Lens
    let ask = Quick::Action(&actions::ask::ACTION);
    app.o.purpose = Purpose::Quick(ask, config::Target::Circle);
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(400.0, 300.0),
        false,
    );
    let panel = app.o.panel.as_ref().map(|(_, p)| p.kind());
    assert_eq!(panel, Some(actions::Panel::Ask), "Ask AI, not Lens over it");
}

#[test]
fn lens_in_the_browser_closes_the_overlay() {
    let mut app = open_app(1920, 1080, 1.0);
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(400.0, 300.0),
        false,
    );
    let _ = app.update(Message::Run(&lens::ACTION));
    let id = app.o.panel.as_ref().expect("the Lens panel").0;
    let opened = panel::Msg::LensOpened(Ok(lens::Held::for_test()));
    let _ = app.update(Message::Panel(id, opened));
    // Closed on every OS: nothing of ours stays on screen after the hand-off.
    assert!(app.o.frame.is_none());
    assert!(app.o.panel.is_none());
}

#[test]
fn the_result_card_opens_off_the_selection_and_moves_by_its_grip() {
    let mut app = open_app(1920, 1080, 1.0);
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(400.0, 300.0),
        false,
    );
    let _ = app.update(Message::Run(&text::ACTION)); // no models: the consent card
    assert!(app.o.panel.is_some());
    let at = app.card_at();
    assert_eq!(at.x + at.width, 1920.0 - card::GAP, "right");
    // the grip: the card follows the pointer while held
    let id = app.o.panel.as_ref().unwrap().0;
    let grab = panel::Msg::Card(card::Event::Grab(card::Drag::Move));
    let _ = app.update(Message::Panel(id, grab));
    let _ = app.update(Message::CardMoved(Point::new(1700.0, 200.0)));
    let _ = app.update(Message::CardMoved(Point::new(1100.0, 200.0)));
    assert_eq!(
        app.card_at().position(),
        at.position() + Vector::new(-600.0, 0.0)
    );
    // let go: it stays there (nothing is saved), no more moves
    let _ = app.update(Message::CardReleased);
    let _ = app.update(Message::CardMoved(Point::new(0.0, 0.0)));
    assert_eq!(
        app.card_at().position(),
        at.position() + Vector::new(-600.0, 0.0)
    );
    // the default place again; a selection under it: this one goes left
    let _ = app.update(Message::Panel(id, panel::Msg::Card(card::Event::Reset)));
    assert_eq!(app.card_at(), at);
    drag(
        &mut app,
        Point::new(1500.0, 100.0),
        Point::new(1800.0, 300.0),
        false,
    );
    assert_eq!(app.card_at().x, card::GAP);
}

/// `OPENCIRCLE_LOOK_PNG=<dir> cargo test ui::tests::look_shots -- --ignored`:
/// a snapped selection (mid-morph and settled, with its chips), select-text
/// mode with the popup, and the bar after More, over a frame with the edge glow.
#[test]
#[ignore = "writes PNGs to look at"]
fn look_shots() {
    let dir = std::path::PathBuf::from(
        std::env::var("OPENCIRCLE_LOOK_PNG").expect("OPENCIRCLE_LOOK_PNG=<dir>"),
    );
    let shot = |app: &App, name: &str| {
        let rgba = widgets::shot::render(app.view(app.overlay), 960, 420);
        actions::write_png(&dir.join(name), 960, 420, &rgba).unwrap();
    };
    let glowing_at = |app: &mut App, w: u32, h: u32| {
        if let Some(image::Handle::Rgba { pixels, .. }) = &app.o.frame {
            let mut rgba = pixels.to_vec();
            border::paint(&mut rgba, w, h, 1.0, &config::look(), border::Mode::Overlay);
            app.o.frame = Some(image::Handle::from_rgba(w, h, rgba));
        }
    };
    let glowing = |app: &mut App| {
        if let Some(image::Handle::Rgba { pixels, .. }) = &app.o.frame {
            let mut rgba = pixels.to_vec();
            let look = config::look();
            border::paint(&mut rgba, 960, 420, 1.0, &look, border::Mode::Overlay);
            app.o.frame = Some(image::Handle::from_rgba(960, 420, rgba));
        }
    };
    let mut app = text_app();
    glowing(&mut app);
    app.o.behaviour = behaviour(SelectStyle::Snap, After::Search, SearchWith::Lens);
    let ring = ring(280.0, 122.0, 110.0)
        .into_iter()
        .map(|p| Point::new(p.x * 1.9 - 250.0, p.y * 0.9 + 12.0))
        .collect::<Vec<_>>();
    let (sel, stroke) = overlay::snap(overlay::select(&ring, 1.0).unwrap().area);
    (app.o.selection, app.o.stroke) = (Some(sel), stroke);
    app.o.morph = Some((ring, Instant::now() - overlay::MORPH / 3));
    shot(&app, "look-1-snap-morphing.png");
    app.o.morph = None;
    shot(&app, "look-2-snapped-chips.png");
    let _ = app.update(Message::Run(&text::ACTION)); // no models here: the consent card
    shot(&app, "look-6-result-card.png");
    // Ask AI beside a selection at 1440x900, the bar kept out of the card,
    // the resize grip showing (hovered)
    let mut wide = open_app(1440, 900, 1.0);
    glowing_at(&mut wide, 1440, 900);
    drag(
        &mut wide,
        Point::new(120.0, 140.0),
        Point::new(640.0, 520.0),
        false,
    );
    let _ = wide.update(Message::Run(&actions::ask::ACTION));
    wide.o.card_hover = true;
    let rgba = widgets::shot::render(wide.view(wide.overlay), 1440, 900);
    actions::write_png(&dir.join("look-7-ask-card-1440.png"), 1440, 900, &rgba).unwrap();
    let mut app = select_text_app();
    glowing(&mut app);
    shot(&app, "look-4-select-text.png");
    drag(&mut app, at_word(1, 7.0), at_word(2, 11.0), false);
    shot(&app, "look-5-select-text-popup.png");
}

/// `OPENCIRCLE_LOOK_PNG=<dir> cargo test ui::tests::widget_theme_shots -- --ignored`:
/// the bar and the Ask AI card in every widget theme, glass on and off, over a busy frame.
#[test]
#[ignore = "writes PNGs to look at"]
fn widget_theme_shots() {
    use crate::config::WidgetTheme::{Default, Frost, Graphite};
    let dir = std::path::PathBuf::from(
        std::env::var("OPENCIRCLE_LOOK_PNG").expect("OPENCIRCLE_LOOK_PNG=<dir>"),
    );
    let (w, h) = (1100u32, 620u32);
    let busy: Vec<u8> = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| {
                let stripe = ((x + y) / 24) % 2 == 0;
                let wave = ((x as f32 / 37.0).sin() * 40.0 + (y as f32 / 29.0).cos() * 40.0) as i32;
                let text = (y / 11) % 3 == 0 && (x / 7) % 5 != 0 && y > 60;
                let base = if stripe { [230, 90, 70] } else { [40, 90, 200] };
                let c = base.map(|v| (v + wave).clamp(0, 255) as u8);
                if text {
                    [250, 250, 245, 255]
                } else {
                    [c[0], c[1], c[2], 255]
                }
            })
        })
        .collect();
    for (name, theme, glass) in [
        ("default", Default, true),
        ("frost-glass", Frost, true),
        ("frost-solid", Frost, false),
        ("graphite-glass", Graphite, true),
        ("graphite-solid", Graphite, false),
    ] {
        config::set_current(config::Config {
            widget_theme: theme,
            glass_effect: glass,
            ..config::Config::default()
        });
        let mut app = open_app(w, h, 1.0);
        app.o.frame = Some(image::Handle::from_rgba(w, h, busy.clone()));
        glass::set_frame(app.o.frame.as_ref(), 1.0);
        drag(
            &mut app,
            Point::new(90.0, 160.0),
            Point::new(420.0, 400.0),
            false,
        );
        let _ = app.update(Message::Run(&actions::ask::ACTION));
        let rgba = widgets::shot::render(app.view(app.overlay), w, h);
        actions::write_png(&dir.join(format!("theme-{name}.png")), w, h, &rgba).unwrap();
    }
    config::set_current(config::Config::default());
}

/// Add capture: the editor steps aside, an area dragged on the frozen frame
/// joins the board (Esc first: back to the editor, nothing added).
#[test]
fn add_capture_puts_a_picked_area_on_the_board() {
    const BLUE: [u8; 4] = [20, 60, 230, 255];
    // Left half white, right half blue.
    let rgba = (0..1200 * 800)
        .flat_map(|i| if i % 1200 < 600 { [255; 4] } else { BLUE })
        .collect();
    let frame = Crop {
        width: 1200,
        height: 800,
        rgba,
    };
    let (mut app, _) = selected_app(&frame);
    app.select(Rectangle::new(
        Point::new(10.0, 10.0),
        Size::new(300.0, 200.0),
    ));
    let _ = app.open_editor();
    let mut editor = |msg| {
        let _ = app.update(Message::Editor(msg));
    };
    editor(editor::Message::AddCapture);
    editor(editor::Message::Back);
    editor(editor::Message::AddCapture);
    editor(editor::Message::Press(Point::new(700.0, 100.0)));
    editor(editor::Message::Move(Point::new(1000.0, 400.0)));
    editor(editor::Message::Release);
    let edit = app.o.edit.as_ref().expect("still editing");
    assert_eq!(edit.editor.doc.tiles().len(), 2);
    let board = edit.image();
    assert!(
        board.rgba.chunks(4).any(|p| p == BLUE),
        "the blue area is on it"
    );
    let r = edit.canvas_rect();
    assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.width <= 1200.0 && r.y + r.height <= 800.0);
}

/// The old hand reset in `close` forgot these: a bar button Tab had focused
/// stayed focused (and took Enter) in the next opening; Alt stayed held.
#[test]
fn close_forgets_the_bar_focus_and_alt() {
    let mut app = open_app(800, 600, 1.0);
    let _ = app.update(Message::Field(keys::Keyed::Focus));
    event(&mut app, overlay::Event::Alt(true));
    assert!(app.o.bar_focus.is_some() && app.o.alt);
    let _ = app.close();
    assert_eq!(app.o.bar_focus, None, "no focus left for the next opening");
    assert!(!app.o.alt);
}

/// After an opening that used a selection, a word, the screen's text, a
/// panel, a translation and song search, close leaves a fresh `Opening`.
#[test]
fn close_leaves_a_fresh_opening() {
    let mut app = read_app(); // the screen's text read
    app.taps += 1;
    read_back(&mut app, at_word(1, 7.0), page(), false);
    assert!(app.o.word.is_some() && app.o.selection.is_some());
    let crop = app.crop().unwrap();
    let _ = app.open_panel(actions::Panel::Text, crop, None);
    let frame = app.o.frame.clone().unwrap();
    let (session, _) =
        translate::Session::start(frame, [0, 0, 40, 20], 1.0, app.cache.clone(), false);
    app.o.translate = Some((1, session));
    let (song, _) = song::Session::start();
    app.o.song = Some((1, song));
    app.o.ask = "what is this".into();
    app.o.bar_focus = Some(bar::Stop::Logo);
    app.o.clicks = Some((Instant::now(), Point::ORIGIN, 2));
    assert!(app.o.panel.is_some());
    let _ = app.close();
    assert!(app.o.is_none(), "nothing of the opening is kept");
    let (o, fresh) = (&app.o, Opening::default());
    assert!(o.frame.is_none() && o.tiles.is_none() && o.selection.is_none());
    assert!(o.stroke.is_empty() && o.others.is_empty() && o.word.is_none());
    assert_eq!(o.screen_text, fresh.screen_text);
    assert!(o.panel.is_none() && o.translate.is_none() && o.song.is_none());
    assert!(o.helper.is_none() && o.lens_pages.is_empty() && o.edit.is_none());
    assert!(o.ask.is_empty() && o.reader == fresh.reader && o.held.is_none());
    assert!(matches!(o.purpose, Purpose::Search));
    assert_eq!(o.bar_focus, None, "the old reset forgot it");
    assert!(o.clicks.is_none(), "the old reset forgot it");
}

/// Close drops the opening whole (the frozen frame's pixels have no other
/// owner left), and answers that arrive after it (a panel's, a translation's,
/// the screen's text, a tapped word, a frame) change nothing: no opening comes
/// back from them.
#[test]
fn late_answers_after_close_are_dropped_and_the_frame_is_freed() {
    let (w, h) = (64, 48);
    let frame = image::Handle::from_rgba(w, h, vec![200; (w * h * 4) as usize]);
    let image::Handle::Rgba { pixels, .. } = &frame else {
        unreachable!()
    };
    let pixels = pixels.clone();
    let mut app = App::idle(window::Id::unique());
    let shot = Shot {
        image: frame,
        scale: 1.0,
        origin: Point::ORIGIN,
        size: Size::new(w as f32, h as f32),
        snip: None,
    };
    let _ = app.open(Instant::now(), Ok(shot));
    assert!(app.o.is_some() && app.o.tiles.is_some());
    app.select(Rectangle::new(Point::new(4.0, 4.0), Size::new(40.0, 30.0)));
    let crop = app.crop().unwrap();
    let _ = app.open_panel(actions::Panel::Text, crop, None);
    assert!(app.o.panel.is_some());
    let _ = app.start_translate(); // (the panel goes)
    assert!(app.o.translate.is_some());
    assert!(!pixels.is_unique(), "the opening holds the frame");
    let (opening, panel, translation, tap) = (app.opening, app.panels, app.translates, app.taps);
    let _ = app.close();
    assert!(app.o.is_none());
    assert!(pixels.is_unique(), "nothing holds the frame after close");
    let band = Rectangle::new(Point::ORIGIN, Size::new(w as f32, h as f32));
    let read = WordRead {
        id: tap,
        at: Point::new(10.0, 10.0),
        origin: [0, 0],
        region: band,
        extend: false,
    };
    let late = [
        Message::Panel(panel, panel::Msg::Read(Ok("late".into()))),
        Message::Translate(translation, translate::Msg::Close),
        Message::ScreenText(opening, band, Ok(Vec::new())),
        Message::Tapped(read, Ok(Vec::new())),
        Message::Frame(Instant::now()),
        Message::Overlay(overlay::Event::Press {
            at: Point::new(5.0, 5.0),
            shift: false,
        }),
        Message::AskInput("late".into()),
    ];
    for message in late {
        let _ = app.update(message);
        assert!(app.o.is_none(), "a late answer brought an opening back");
    }
    assert!(pixels.is_unique());
    assert!(app.is_idle() || app.hiding != Hiding::No);
}
