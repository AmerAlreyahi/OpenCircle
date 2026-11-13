use crate::shared::app_language::{S, Said};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::{self};
use iced::widget::{container, image, space};
use iced::window::{self};
use iced::{Element, Fill, Point, Rectangle, Size, Task, Vector};

use crate::Trigger;
use crate::actions::{self, Crop, copy, qr, search_in_browser, text};
use crate::features::settings::labs::{self, BarButton};
use crate::ocr::{self, TextBlock};
use crate::shared::config::{self, QuickAction, SelectStyle, SelectionConfig, Target};
use crate::shared::platform::capture::CaptureError;
use crate::snip::annotate::{self};

use crate::shared::kit::panel_frame as card;
use crate::ui::app::message::{BarKeys, Hiding, Message, Purpose, Shot, WordRead};
use crate::ui::app::opening::Opening;
use crate::ui::app::quick::Quick;
use crate::ui::app::{keys, word};
use crate::ui::screens::bar::geometry as bar;
use crate::ui::screens::overlay::edge as border;
use crate::ui::snip::Snip;
use crate::ui::{ai_chat, overlay, snip, song, translate, windows};

use super::view::chips_at;
use super::*;
use crate::shared::kit::field;
use crate::ui::screens::bar::view::ask_placeholder;

use crate::actions::crop::STACK_GAP;
use crate::features::settings::labs::Labs;
use crate::shared::config::{BarAnchor, TextReader};

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
        windows: Vec::new(),
    })
}

/// A waiting update never installs while the user works, idle or not (owner,
/// 2026-11-10): it shows its pill above the tray; Later puts the pill away and
/// the update keeps waiting (tray dot, tray row, Settings). Before: it installed
/// at the first idle minute, the app vanishing under the user.
#[test]
fn an_update_waits_for_a_moment_the_user_picks() {
    use crate::shared::update::{Event, How, Install, Staged};
    use crate::ui::screens::update_pill::{Kind, Msg as PillMsg};
    let staged = Staged {
        version: "9.9.9".into(),
        file: "missing".into(),
        sha256: [0; 32],
        dir: std::env::temp_dir().join("oc-update-test-never-made"),
        install: Install::Exe(std::env::temp_dir().join("oc-update-test-no-exe.exe")),
        notes: String::new(),
        url: String::new(),
    };
    let mut app = App::idle(window::Id::unique());
    assert!(app.is_idle());
    let _ = app.update(Message::Trigger(Trigger::Update(Event::Ready(
        staged.clone(),
    ))));
    let w = app.update.as_ref().expect("waits");
    assert_eq!((w.version.as_str(), w.how), ("9.9.9", How::Restart));
    assert!(
        matches!(app.windows.shown_pill(), Some(Kind::Ready { .. })),
        "the pill above the tray"
    );
    let _ = app.update(Message::UpdateTick);
    assert!(app.update.is_some(), "idle, yet nothing installed");
    let _ = app.update(Message::Windows(windows::Msg::UpdatePill(PillMsg::Later)));
    assert!(app.windows.shown_pill().is_none(), "Later: the pill goes");
    let _ = app.update(Message::UpdateTick);
    assert!(app.windows.shown_pill().is_none(), "and stays away");
    assert!(app.update.is_some(), "the update keeps waiting");
}

/// Our floating windows (here a pin) are never in what we capture uninvited: every
/// capture but Snip's (Circle to Search, translate) takes them out of its grab at
/// its entry (`App::capture`; Linux: no OS call keeps a window out of a grab).
/// Snip's grab has them in, to be picked (owner, 2026-11-10): they leave once it
/// is over, before its overlay shows. Either way they come back after the overlay
/// closes. Windows keeps them out of every capture but Snip's instead
/// (`windows::floating`, `window_rect::widgets_in_grab`).
#[test]
fn our_floating_windows_are_out_of_every_grab_but_snips() {
    let linux = !cfg!(windows);
    let pin = Crop {
        width: 4,
        height: 4,
        rgba: vec![9; 4 * 4 * 4],
    };
    let mut app = App::idle(window::Id::unique());
    let _ = app.windows.pin(pin, None, 1.0);
    for snip in [None, Some(snip::Kind::Region)] {
        let _ = app.capture(Instant::now(), snip);
        let n = app.capturing.unwrap();
        let out = linux && snip.is_none();
        assert_eq!(
            app.windows.out_of_frame(),
            out,
            "out for the grab: {snip:?}"
        );
        let shot = a_shot().map(|shot| Shot {
            snip: snip.map(|k| (k, Vec::new(), None, Vec::new())),
            ..shot
        });
        let _ = app.update(Message::Captured(n, Instant::now(), shot));
        let _ = app.update(Message::Grabbed(n));
        assert_eq!(app.windows.out_of_frame(), linux, "not over the overlay");
        let _ = app.close();
        app.hiding = Hiding::No;
        assert!(!app.windows.out_of_frame(), "back once it closed");
    }
    // a grab that opens nothing (failed, cancelled): back when it's over
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    let n = app.capturing.unwrap();
    let _ = app.update(Message::Escape);
    let _ = app.update(Message::Grabbed(n));
    assert!(!app.windows.out_of_frame());
}

/// The overlay a capture closed is never in the next one's grab (Linux: the
/// desktop fades the closed window out, so Esc then the shortcut at once froze
/// it, its "Select text" chip and all): the grab waits until it's gone.
#[test]
fn a_grab_right_after_the_overlay_closed_waits_until_it_is_gone() {
    let linux = !cfg!(windows);
    let mut app = App {
        can_hide: false,
        overlay_open: false,
        ..App::idle(window::Id::unique())
    };
    assert_eq!(app.overlay_gone, None, "nothing closed yet: no wait");
    let _ = open_and_escape(&mut app);
    let gone = app.overlay_gone;
    assert_eq!(gone.is_some(), linux);
    let now = Instant::now();
    if let Some(gone) = gone {
        assert!(super::capture::grab_wait(false, Some(gone), now) > Duration::ZERO);
        let later = gone + Duration::from_secs(1);
        assert_eq!(
            super::capture::grab_wait(false, Some(gone), later),
            Duration::ZERO
        );
    }
    assert_eq!(super::capture::grab_wait(false, None, now), Duration::ZERO);
    assert!(
        super::capture::grab_wait(true, None, now) > Duration::ZERO,
        "ours just left"
    );
}

/// A copy the setup installed never installs by itself (its setup asks Windows for
/// admin rights): the pill offers it with a shield; a failed or declined Restart
/// now keeps it for another try (the tray row, Settings).
#[test]
fn an_installed_copys_update_waits_for_restart_now() {
    use crate::shared::update::{Event, How, Install, Staged};
    let staged = Staged {
        version: "9.9.9".into(),
        file: std::env::temp_dir().join("oc-update-test-no-setup.exe"), // missing: never runs
        sha256: [0; 32],
        dir: std::env::temp_dir().join("oc-update-test-never-made"),
        install: Install::Setup(r"C:\Program Files\OpenCircle\opencircle.exe".into()),
        notes: String::new(),
        url: String::new(),
    };
    let mut app = App::idle(window::Id::unique());
    let _ = app.update(Message::Trigger(Trigger::Update(Event::Ready(
        staged.clone(),
    ))));
    let _ = app.update(Message::UpdateTick);
    assert_eq!(
        app.update.as_ref().map(|w| w.how),
        Some(How::Setup),
        "idle, yet not on its own"
    );
    let _ = app.update(Message::Trigger(Trigger::RestartToUpdate));
    assert!(app.update.is_some(), "it did not run: kept for another try");
}

#[test]
fn a_shortcut_while_open_brings_the_overlay_back_and_says_so() {
    let mut app = App::idle(window::Id::unique());
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    let n = app.capturing.expect("pending");
    let said = |line: &str| {
        crate::shared::log::tail(200)
            .iter()
            .any(|l| l.contains(line))
    };
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

/// A snip's overlay shown early, unseen, while its screen is grabbed
/// (`App::pre_show`): a capture that fails, times out or is cancelled hides it
/// again (the close steps); one that opens doesn't show it a second time, it
/// only waits for its first frame to fade in.
#[test]
fn a_snip_shown_while_grabbing_hides_again_unless_it_opens() {
    let ends: [fn(u32) -> Message; 3] = [
        |n| Message::Captured(n, Instant::now(), Err(CaptureError::Failed("no".into()))),
        Message::CaptureTimedOut,
        |_| Message::Escape,
    ];
    for end in ends {
        let mut app = App::idle(window::Id::unique());
        app.capturing = Some(7);
        app.pre_shown = true;
        let _ = app.update(end(7));
        assert!(!app.pre_shown && app.capturing.is_none());
        assert_eq!(app.hiding, Hiding::Empty, "hidden by the close steps");
        assert!(app.o.frame.is_none());
    }
    let mut app = App::idle(window::Id::unique());
    app.capturing = Some(7);
    app.pre_shown = true;
    let shot = Shot {
        snip: Some((snip::Kind::Region, Vec::new(), None, Vec::new())),
        windows: Vec::new(),
        ..a_shot().unwrap()
    };
    let _ = app.update(Message::Captured(7, Instant::now(), Ok(shot)));
    assert!(app.o.frame.is_some() && !app.pre_shown);
    assert!(
        app.o.reveal.is_some(),
        "it fades in once its frame is drawn"
    );
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
    let (session, _) = translate::Session::start(
        frame.clone(),
        [0, 0, 40, 20],
        1.0,
        app.cache.clone(),
        false,
        0,
    );
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
    crate::shared::kit::testing::shot::texts(app.view(app.overlay))
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
    crate::shared::kit::testing::shot::save("overlay-qr-chip", app.view(app.overlay), size);

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
fn the_crop_search_gets_is_the_selection_at_any_scale_and_monitor() {
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
            windows: Vec::new(),
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
            app.o.ai_chat.is_none(),
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
            search_in_browser::search_png(&crop).is_ok(),
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
    assert!(!overlay_texts(&app).contains(&protected().to_string()));
    let _ = app.update(Message::Scanned(area, qr::scan(&app.crop().unwrap())));
    let texts = overlay_texts(&app);
    assert!(texts.contains(&protected().to_string()), "{texts:?}");
    let texts = overlay_texts(&app);
    assert!(
        texts.contains(&search_in_browser::ACTION.icon.to_string()),
        "{texts:?}"
    );
    crate::shared::kit::testing::shot::save(
        "overlay-protected",
        app.view(app.overlay),
        area.size(),
    );
    // An ordinary capture shows no such pill.
    let grey = Crop {
        rgba: [90, 90, 90, 255].repeat(600 * 360),
        ..black
    };
    let (mut app, area) = selected_app(&grey);
    let _ = app.update(Message::Scanned(area, qr::scan(&app.crop().unwrap())));
    assert!(!overlay_texts(&app).contains(&protected().to_string()));
}

/// An open overlay on a `w` x `h` frame at `scale`; pixel (x, y) is
/// [x % 256, y % 256, x / 256, 255].
pub(crate) fn open_app(w: u32, h: u32, scale: f32) -> App {
    let mut app = App::idle(window::Id::unique());
    reopen(&mut app, w, h, scale);
    app
}

/// `app`'s overlay opened again on a fresh `w` x `h` frame (as `open_app`).
pub(crate) fn reopen(app: &mut App, w: u32, h: u32, scale: f32) {
    app.o.start(Opening::default());
    app.o.labs = Labs::all_on();
    let rgba: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, (x >> 8) as u8, 255]))
        .collect();
    let frame = image::Handle::from_rgba(w, h, rgba);
    app.o.frame = Some(frame);
    app.o.scale = scale;
    app.o.screen = Size::new(w as f32 / scale, h as f32 / scale);
    app.o.view = overlay::View::new(w, h, app.o.screen);
    app.o.bar_keys = BarKeys::of(&config::Config::default());
    app.o.appear = Some((Instant::now(), 1.0)); // opened
    app.find_chat(Point::ORIGIN);
}

fn event(app: &mut App, e: overlay::Event) {
    let _ = app.on_overlay(e);
}

/// Runs `task` to its end (its threads' work done) and hands its messages
/// to the app (their own tasks are dropped).
pub(crate) fn run_task(app: &mut App, task: Task<Message>) {
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
pub(crate) fn drag(app: &mut App, a: Point, b: Point, shift: bool) {
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
    snipping_on(800, 600, area)
}

/// A snip over a `w` x `h` screen (scale 1) with one window (`area`) the pointer is on.
pub(crate) fn snipping_on(w: u32, h: u32, area: Rectangle) -> App {
    let mut app = open_app(w, h, 1.0);
    let window = snip::Pick::window(area, "App", 1);
    app.o.purpose = Purpose::Snip(Box::new(Snip::new(vec![window], app.place(), true, None)));
    event(&mut app, overlay::Event::Hover(area.center()));
    app
}

/// A snip drag is always the box between the press and the pointer: dragging
/// back shrinks it, crossing the press flips it, and the size chip follows.
#[test]
fn a_snip_drag_shrinks_back_and_crosses_its_start() {
    let window = Rectangle::new(Point::new(20.0, 20.0), Size::new(40.0, 40.0));
    let mut app = snipping_on(1280, 720, window);
    let box_ = |x, y, w, h| Rectangle::new(Point::new(x, y), Size::new(w, h));
    let at = Point::new;
    let start = at(700.0, 500.0);
    let press = overlay::Event::Press {
        at: start,
        shift: false,
    };
    event(&mut app, press);
    for p in [at(400.0, 300.0), at(200.0, 100.0)] {
        event(&mut app, overlay::Event::Move(p));
    }
    let out = Some(box_(200.0, 100.0, 500.0, 400.0));
    assert_eq!(app.snip_drag(), out, "out");
    event(&mut app, overlay::Event::Move(at(600.0, 450.0)));
    let back = Some(box_(600.0, 450.0, 100.0, 50.0));
    assert_eq!(app.snip_drag(), back, "back: it shrinks");
    event(&mut app, overlay::Event::Move(at(820.0, 580.0)));
    let past = Some(box_(700.0, 500.0, 120.0, 80.0));
    assert_eq!(app.snip_drag(), past, "past the start");
    event(&mut app, overlay::Event::Release);
    assert_eq!(app.o.selection.map(|s| s.area), past, "picked");
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

/// N-73: after the pick the area stays live: a handle resizes it on the frozen
/// frame, the overlay's selection follows, the toolbar docks under it, and
/// Copy takes the area as it is now. Enter while picking = the whole screen.
#[test]
fn a_snips_area_stays_live_after_the_pick_and_copy_takes_it_as_it_is() {
    use crate::snip::annotate::Message as E;
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    app.o.labs = crate::features::settings::labs::Labs::all_on();
    app.select(area);
    let crop = app.crop().unwrap();
    let _ = app.snip_routed(crop, true, false);
    let edit = app.o.edit.as_ref().expect("the editor is open");
    assert!(edit.annotator.movable && edit.annotator.tool == crate::snip::annotate::Tool::Select);
    assert_eq!(edit.canvas_rect(), area, "over the picked area");
    // The bottom-right handle out by (100, 50).
    let _ = app.on_snip_toolbar(E::Press(Point::new(400.0, 280.0)));
    let _ = app.on_snip_toolbar(E::Move(Point::new(500.0, 330.0)));
    let grown = Rectangle::new(area.position(), Size::new(400.0, 250.0));
    assert_eq!(
        app.o.selection.map(|s| s.area),
        Some(grown),
        "followed live"
    );
    let _ = app.on_snip_toolbar(E::Release);
    // The toolbar docks under the area, on the screen.
    let at = app.bar_target();
    assert!(at.y >= grown.y + grown.height, "{at:?}");
    let _ = copy::copied::image();
    let task = app.on_snip_toolbar(E::Copy);
    run_task(&mut app, task);
    assert_eq!(
        copied_image(),
        Some((400, 250)),
        "the live area, as it is now"
    );

    // Enter while picking: the whole screen.
    let mut app = snipping(area);
    let _ = copy::copied::image();
    let task = app.on_overlay(overlay::Event::Enter);
    run_task(&mut app, task);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(copy::copied::image(), None, "a pick copies nothing");
    let task = app.on_snip_toolbar(E::Copy);
    run_task(&mut app, task);
    assert_eq!(copied_image(), Some((800, 600)));
}

/// 0.1.3-2: Snip's toolbar by the keyboard: Tab walks its controls (Shift+Tab
/// from the end: Save then Copy), Enter presses the one Tab is on, Enter with
/// none copies; a press drops the focus. While the area is moved the toolbar
/// stays put (no frame per move), and glides to its new dock once let go.
#[test]
fn a_snips_toolbar_walks_by_tab_and_stays_put_while_the_area_moves() {
    use crate::snip::annotate::Message as E;
    use crate::ui::screens::snip_toolbar::toolbar;
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    app.o.labs = crate::features::settings::labs::Labs::all_on();
    app.select(area);
    let crop = app.crop().unwrap();
    let _ = app.snip_routed(crop, true, false);
    fn edit(app: &App) -> &annotate::Annotator {
        &app.o.edit.as_ref().unwrap().annotator
    }
    fn key(app: &App, named: keyboard::key::Named) -> Option<E> {
        let none = keyboard::Modifiers::empty();
        edit(app).key(&keyboard::Key::Named(named), None, none, None)
    }
    let stops = toolbar::stops(edit(&app));
    assert_eq!(stops.last(), Some(&E::Save), "no ×: Esc closes");
    assert!(
        !stops.contains(&E::Slot(annotate::Tool::Crop)),
        "no Crop in Snip"
    );
    let _ = app.on_snip_toolbar(E::Focus(true));
    let _ = app.on_snip_toolbar(E::Focus(true));
    let focus = edit(&app).focus.unwrap();
    assert_eq!(stops[focus], E::Copy);
    let _ = app.on_snip_toolbar(E::Focus(false));
    assert_eq!(stops[edit(&app).focus.unwrap()], E::Save, "and forward");
    // the keys: Tab, Enter on the focus, Enter alone = Copy
    assert_eq!(key(&app, keyboard::key::Named::Tab), Some(E::Focus(false)));
    assert_eq!(key(&app, keyboard::key::Named::Enter), Some(E::Activate));
    let _ = app.on_snip_toolbar(E::Press(Point::new(250.0, 180.0)));
    let _ = app.on_snip_toolbar(E::Release);
    assert_eq!(edit(&app).focus, None, "a press drops it");
    assert_eq!(key(&app, keyboard::key::Named::Enter), Some(E::Copy));

    // moved: the toolbar stays where it was until let go
    let start = Instant::now();
    let frame = |app: &mut App, ms| {
        let _ = app.update(Message::Frame(start + Duration::from_millis(ms)));
    };
    for ms in [0, 400, 800] {
        frame(&mut app, ms);
    }
    let docked = app.bar_at();
    let _ = app.on_snip_toolbar(E::Press(Point::new(250.0, 180.0)));
    let _ = app.on_snip_toolbar(E::Move(Point::new(350.0, 230.0)));
    frame(&mut app, 816);
    assert_eq!(app.bar_at(), docked, "still while held");
    let _ = app.on_snip_toolbar(E::Release);
    frame(&mut app, 832);
    frame(&mut app, 848);
    let at = app.bar_at();
    assert!(at != docked && at != app.bar_target(), "on its way: {at:?}");
    frame(&mut app, 2000);
    assert_eq!(
        app.bar_at(),
        app.bar_target(),
        "docked under the moved area"
    );
}

/// N-74: "After a capture: Both" copies the snip AND opens the editor (it
/// only opened the editor: nothing reached the clipboard or Win+V).
#[test]
fn a_snip_routed_to_both_is_copied_and_edited() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    app.o.labs = crate::features::settings::labs::Labs::all_on();
    app.select(area);
    let crop = app.crop().unwrap();
    let _ = copy::copied::image();
    let _ = app.snip_routed(crop, true, true);
    assert_eq!(copied_image(), Some((300, 200)), "copied");
    assert!(app.o.edit.is_some(), "and the editor is open");
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
    let place = (app.o.origin, app.o.screen, app.o.scale, None);
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

/// #23: Wayland can't cut a window to a pill (it would be a square box): the
/// overlay stays up and draws the Copied pill itself, rounded, beside the
/// selection, then closes; where windows are cut (Windows, X11) the pill is
/// its own window as before.
#[test]
fn on_wayland_the_copied_pill_is_drawn_in_the_overlay() {
    let key = overlay::Event::Key {
        letter: 'c',
        ctrl: true,
    };
    let sel = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    for can_hide in [false, true] {
        let mut app = open_app(800, 600, 1.0);
        app.can_hide = can_hide;
        app.select(sel);
        let task = app.on_overlay(key);
        run_task(&mut app, task);
        if can_hide {
            assert!(app.o.frame.is_none() && app.o.result.is_none());
            assert_eq!(app.windows.toasts(), [("Copied".to_string(), true)]);
            continue;
        }
        assert!(app.windows.toasts().is_empty(), "no window of its own");
        let (text, ok, at) = app.o.result.clone().expect("the pill over the frame");
        assert!(ok && text == "Copied" && app.o.frame.is_some());
        // below the selection, centred on it
        let size = windows::toast_size("Copied");
        let want = Point::new(sel.center_x() - size.width / 2.0, 280.0 + TOAST_MARGIN);
        assert!(at.distance(want) <= 1.0, "{at:?} != {want:?}");
        assert!(overlay_texts(&app).contains(&"Copied".to_string()));
        // read: the overlay goes with it
        let _ = app.update(Message::ToastDone);
        assert!(app.o.frame.is_none() && app.o.result.is_none());
    }
    // Snip's Copy: its toolbar is still drawn, so the pill goes clear of it
    // too (and of the area, unless the area is the whole screen)
    let whole = Rectangle::new(Point::new(2.0, 2.0), Size::new(796.0, 596.0));
    for area in [sel, whole] {
        let mut app = snipping(area);
        app.can_hide = false;
        app.select(area);
        let _ = app.open_snip_toolbar();
        let toolbar = Rectangle::new(app.bar_at(), app.dock_size());
        let task = app.on_snip_toolbar(annotate::Message::Copy);
        run_task(&mut app, task);
        let (_, _, at) = app.o.result.clone().expect("the pill over the frame");
        let pill = Rectangle::new(at, windows::toast_size("Copied"));
        assert!(!pill.intersects(&toolbar), "{pill:?} over {toolbar:?}");
        assert!(
            area == whole || !pill.intersects(&area),
            "{pill:?} {area:?}"
        );
    }
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

/// The owner's VM (2026-10-31): a full-height window on the right half of a
/// 1716 x 1271 screen (40 px taskbar) picked, Long capture pressed: neither
/// the picture nor the hint showed. The toolbar docks inside the area's top
/// edge there and reaches into the picture's column: the picture ended
/// "above" it (nothing left) and the hint went above it (off the screen).
/// From the thread's first frame on (before any scroll) both show, beside
/// the area on its free (left) side, clear of the toolbar.
#[test]
fn long_capture_shows_its_picture_and_hint_before_any_scroll() {
    use crate::shared::platform::capture::scroll::Update;
    use crate::ui::screens::long_capture;
    for y in [0.0, 0.4, 1.0] {
        let area = Rectangle::new(Point::new(858.0, y), Size::new(858.0, 1231.0 - y));
        let mut app = snipping_on(1716, 1271, area);
        app.o.work_inset = 40.0;
        app.select(area);
        let _ = app.open_snip_toolbar();
        let _ = app.update(Message::Annotate(annotate::Message::Scroll));
        // docked inside the area's top once picked; beside it for the
        // capture (over it the overlay would be kept out of the grabs,
        // and the area showed blurred on Windows 11: owner, 2026-11-03)
        let bar = app.long_bar();
        assert!(!bar.intersects(&area), "off the area: {bar:?}");
        assert_eq!(
            (bar.x + bar.width + 12.0, bar.y),
            (area.x, area.y.max(12.0))
        );
        // the size start_scroll asks the thread for
        let room = long_capture::layout(area, app.o.screen, bar, 1.0);
        let room = room.unwrap_or_else(|| panic!("no room at y {y}"));
        // the thread's first update: the first frame, scaled into that room
        let (w, h) = (room.width as u32, (area.height / 2.0) as u32);
        let first = Update {
            height: area.height as u32,
            full: false,
            lost: false,
            preview: Some(Crop {
                width: w,
                height: h,
                rgba: vec![200; (w * h * 4) as usize],
            }),
        };
        let _ = app.update(Message::Snip(snip::Msg::Progress(first)));
        let long = app.scroll().unwrap();
        let picture = long.picture(app.o.screen, bar).expect("the picture shows");
        assert!(picture.width > 0.0 && picture.height > 0.0);
        assert_eq!(
            picture.x + picture.width + 12.0,
            area.x,
            "right beside it, on its left"
        );
        assert!(
            !picture.intersects(&bar) && !picture.intersects(&area),
            "{picture:?}"
        );
        let hint = long.hint_spot(app.o.screen, bar);
        let hint = hint.unwrap_or_else(|| panic!("the hint shows at y {y}"));
        assert!(hint.x + 1.0 < area.x && hint.y >= 0.0, "{hint:?}");
        // cancelled: back to the picked area, the toolbar docked on it again
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Snip(snip::Msg::Done(Ok(Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        }))));
        assert!(app.scroll().is_none() && app.o.bar_left.is_none());
        assert!(app.long_bar().intersects(&area), "docked inside again");
    }
}

/// A long capture of the whole screen had no picture (no side has room). Like
/// iTop (owner, 2026-11-03): a small one in the area's top-right corner, under
/// the toolbar, kept in the overlay's hole (the overlay is kept out of the grabs).
#[test]
fn a_full_screen_long_capture_shows_a_small_picture_in_the_corner() {
    use crate::shared::platform::capture::scroll::Update;
    use crate::ui::screens::long_capture;
    let area = Rectangle::new(Point::ORIGIN, Size::new(1280.0, 720.0));
    let mut app = snipping_on(1280, 720, area);
    app.select(area);
    let _ = app.open_snip_toolbar();
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    let bar = app.long_bar();
    assert!(bar.intersects(&area), "the toolbar stays over the area");
    let room = long_capture::room(area, app.o.screen, bar, 1.0).expect("the corner");
    let (w, h) = (room.width as u32, 60);
    let first = Update {
        height: 720,
        full: false,
        lost: false,
        preview: Some(Crop {
            width: w,
            height: h,
            rgba: vec![200; (w * h * 4) as usize],
        }),
    };
    let _ = app.update(Message::Snip(snip::Msg::Progress(first)));
    let long = app.scroll().unwrap();
    let picture = long.picture(app.o.screen, bar).expect("the picture shows");
    assert!(area.contains(picture.position()), "{picture:?}");
    assert_eq!(picture.x + picture.width, area.width - 12.0, "top-right");
    assert!(
        picture.width <= 180.0 && !picture.intersects(&bar),
        "{picture:?}"
    );
    assert!(picture.y < area.height / 2.0, "at the top: {picture:?}");
    assert_eq!(long.cut, Some(picture.expand(1.0)), "kept in the hole");
}

/// The toolbar's 6-dot grip drags Snip's toolbar (picked, and during a long
/// capture): its own press used to stop there (the editor ignored it, the
/// toolbar's grab under it never heard it). Dropped, it stays for this area.
#[test]
fn the_snip_toolbars_grip_drags_it_and_it_stays_where_dropped() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    for long in [false, true] {
        let mut app = snipping(area);
        app.select(area);
        let _ = app.open_snip_toolbar();
        if long {
            let _ = app.update(Message::Annotate(annotate::Message::Scroll));
            assert!(app.scroll().is_some());
        }
        let docked = app.bar_at();
        let _ = app.update(Message::Annotate(annotate::Message::Grab));
        assert!(app.o.bar_drag.is_some(), "the grip holds the toolbar");
        let from = docked + Vector::new(10.0, 10.0);
        let _ = app.update(Message::BarMoved(from));
        let _ = app.update(Message::BarMoved(from + Vector::new(120.0, 100.0)));
        let _ = app.update(Message::BarReleased);
        let dropped = docked + Vector::new(120.0, 100.0);
        assert_eq!(
            app.bar_target(),
            dropped,
            "stays where dropped (long: {long})"
        );
        assert_eq!(app.scroll().is_some(), long, "the capture goes on");
    }
    // a new pick docks again
    let mut app = snipping(area);
    app.o.bar_left = Some(Point::new(5.0, 5.0));
    app.select(area);
    let _ = app.open_snip_toolbar();
    assert_ne!(app.bar_target(), Point::new(5.0, 5.0));
}

/// The three drags (owner OK, 2026-11-02): the grip moves ONLY the toolbar
/// (lifted while held; Esc mid-drag puts it back); the size chip drags the
/// picture out and moves nothing (nothing took it: all stays; a drop closes
/// the snip); a drag inside the area with Select moves ONLY the area (the
/// toolbar re-docks to it).
#[test]
fn the_three_drags_each_move_their_own_thing() {
    use crate::snip::annotate::{Message as E, dragout};
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    app.select(area);
    let _ = app.open_snip_toolbar();
    let canvas = |app: &App| app.o.edit.as_ref().unwrap().canvas_rect();
    let held = |app: &App| app.o.edit.as_ref().unwrap().annotator.held;
    let (docked, picked) = (app.bar_at(), canvas(&app));

    // a) the grip: the toolbar follows, the area stays
    let _ = app.update(Message::Annotate(E::Grab));
    assert!(held(&app), "lifted");
    let _ = app.update(Message::BarMoved(docked));
    let _ = app.update(Message::BarMoved(docked + Vector::new(60.0, 40.0)));
    assert_eq!(app.bar_target(), docked + Vector::new(60.0, 40.0));
    assert_eq!(canvas(&app), picked, "the area did not move");
    // Esc mid-drag: back where it was, the snip still open
    let _ = app.update(Message::Annotate(E::Back));
    assert!(app.o.bar_drag.is_none() && !held(&app));
    assert_eq!(app.bar_target(), docked, "glides back");
    assert!(app.editing(), "Esc only cancelled the drag");

    // b) the size chip: nothing moves; nothing took it = all stays
    dragout::TEST_DROP.set(Some(false));
    let _ = app.on_snip_toolbar(E::DragOut);
    assert_eq!((app.bar_target(), canvas(&app)), (docked, picked));
    assert!(app.editing());

    // c) inside the area with Select: the area moves, the toolbar re-docks
    let _ = app.on_snip_toolbar(E::Tool(annotate::Tool::Select));
    let z = app.o.edit.as_ref().unwrap().annotator.zoom;
    let inside = |p: Point| Point::new(p.x / z, p.y / z);
    let _ = app.on_snip_toolbar(E::Press(inside(Point::new(250.0, 180.0))));
    let _ = app.on_snip_toolbar(E::Move(inside(Point::new(290.0, 200.0))));
    let _ = app.on_snip_toolbar(E::Release);
    let moved = canvas(&app);
    assert_eq!(
        moved.position(),
        picked.position() + Vector::new(40.0, 20.0)
    );
    assert_ne!(app.bar_target(), docked, "docked to the moved area");

    // b) again: an app took it, the snip closes
    dragout::TEST_DROP.set(Some(true));
    let task = app.on_snip_toolbar(E::DragOut);
    run_task(&mut app, task);
    assert!(app.hiding != Hiding::No, "dropped: closed");
}

/// Copy is the button or Ctrl+C, Save the button or Ctrl+S (owner,
/// 2026-11-03): during a long capture the keys finish it exactly as the
/// toolbar's buttons do (its canvas, which takes the editor's keys, is off).
#[test]
fn long_capture_finishes_on_ctrl_c_and_ctrl_s_like_its_buttons() {
    use crate::ui::screens::long_capture::Finish;
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let key = |letter| overlay::Event::Key { letter, ctrl: true };
    for (letter, button, how) in [
        ('c', annotate::Message::Copy, Finish::Copy),
        ('s', annotate::Message::Save, Finish::Save),
    ] {
        let finish = |by_key: bool| {
            let mut app = snipping(area);
            app.select(area);
            let _ = app.open_snip_toolbar();
            let _ = app.update(Message::Annotate(annotate::Message::Scroll));
            let _ = match by_key {
                true => app.on_overlay(key(letter)),
                false => app.update(Message::Annotate(button.clone())),
            };
            app.scroll().and_then(|l| l.then)
        };
        assert_eq!(finish(false), Some(how), "the button");
        assert_eq!(finish(true), Some(how), "Ctrl+{letter}");
    }
}

#[test]
fn the_snip_toolbars_scroll_button_starts_a_scrolling_capture_of_the_area() {
    use crate::ui::screens::long_capture::Finish;
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let stitched = || {
        Message::Snip(snip::Msg::Done(Ok(Crop {
            width: 300,
            height: 900,
            rgba: vec![255; 300 * 900 * 4],
        })))
    };
    let mut app = snipping(area);
    app.select(area);
    let _ = app.open_snip_toolbar();
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    assert!(app.scroll().is_some(), "scrolling and stitching has begun");
    assert!(
        app.o.edit.as_ref().is_some_and(|e| e.annotator.capturing),
        "Snip's toolbar stays, in its capturing state"
    );
    assert!(
        !app.snip_pill(),
        "the overlay stays: the area is cut out of it"
    );
    // the rest of the overlay is still: no new pick, no hover outline
    let _ = app.on_overlay(overlay::Event::Press {
        at: Point::new(600.0, 500.0),
        shift: false,
    });
    assert_eq!(app.o.selection.map(|s| s.area), Some(area));
    // the toolbar's tools do nothing meanwhile
    let _ = app.update(Message::Annotate(annotate::Message::Undo));
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    assert!(app.scroll().is_some_and(|l| l.then.is_none()));
    // Esc is Cancel: once the thread hands over, back to the picked area
    let _ = app.update(Message::Escape);
    assert!(app.scroll().is_some_and(|l| l.cancelled()));
    let _ = app.update(stitched());
    assert!(app.scroll().is_none());
    assert!(
        app.o
            .edit
            .as_ref()
            .is_some_and(|e| e.annotator.movable && !e.annotator.capturing),
        "picked again"
    );

    // Save finishes it: the stitched image saved, the overlay closes
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    let _ = app.update(Message::Annotate(annotate::Message::Save));
    assert_eq!(app.scroll().and_then(|l| l.then), Some(Finish::Save));
    let _ = app.update(stitched());
    assert!(app.o.is_none(), "finished");
    // Enter is Copy
    let mut app = snipping(area);
    app.select(area);
    let _ = app.open_snip_toolbar();
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    let _ = app.on_overlay(overlay::Event::Enter);
    assert_eq!(app.scroll().and_then(|l| l.then), Some(Finish::Copy));
    let _ = app.update(stitched());
    assert!(app.o.is_none(), "copied and closed");
    // Share: the stitched image picked (shown whole), the sheet from there
    let mut app = snipping(area);
    app.select(area);
    let _ = app.open_snip_toolbar();
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    let _ = app.update(Message::Annotate(annotate::Message::Share));
    let _ = app.update(stitched());
    let edit = app
        .o
        .edit
        .as_ref()
        .expect("the editor on the stitched image");
    assert!(edit.annotator.movable && edit.annotator.own_image && !edit.annotator.capturing);
    assert_eq!((edit.image().width, edit.image().height), (300, 900));
    // no auto-scroll here (Wayland) or no snip (Edit in place): the button is dimmed
    let mut app = open_app(800, 600, 1.0);
    app.select(area);
    let _ = app.open_snip_toolbar();
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    assert!(app.o.edit.is_some() && app.scroll().is_none());
}

/// After a pick (owner, 2026-11-08): a pick lands in the picked state (the live
/// area, Snip's toolbar) and copies NOTHING by default; the Copy button writes
/// the clipboard (one Win+V entry, not two).
#[test]
fn a_pick_copies_nothing_by_default_and_the_copy_button_does() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    let _ = copy::copied::image();
    let task = app.snip_next(Some(snip::Next::Edit(area)));
    let edit = app.o.edit.as_ref().expect("picked");
    assert!(
        edit.annotator.movable && !edit.annotator.own_image,
        "the live area"
    );
    run_task(&mut app, task);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        copy::copied::image(),
        None,
        "the pick left the clipboard alone"
    );
    let task = app.on_snip_toolbar(crate::snip::annotate::Message::Copy);
    run_task(&mut app, task);
    assert_eq!(copied_image(), Some((300, 200)), "Copy wrote it");
}

/// A recent capture (the Recent strip) opens picked on its own image: the
/// whole image is the area, Snip's toolbar docks to it; taller than the
/// screen, it is shown whole (scaled down), kept at full size.
#[test]
fn a_recent_capture_opens_picked_with_snips_toolbar() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    let _ = copy::copied::image();
    let crop = Crop {
        width: 400,
        height: 1500,
        rgba: vec![200; 400 * 1500 * 4],
    };
    let task = app.on_snip(snip::Msg::Loaded(Ok(crop)));
    let edit = app.o.edit.as_ref().expect("the editor");
    assert!(edit.annotator.movable && edit.annotator.own_image);
    assert!(
        !edit.annotator.bare(),
        "its own pixels drawn, not the frozen frame's"
    );
    let stops = crate::ui::screens::snip_toolbar::toolbar::stops(&edit.annotator);
    assert!(!stops.contains(&annotate::Message::Back), "Snip's toolbar");
    assert!(
        !stops.contains(&annotate::Message::Scroll),
        "no page under it"
    );
    let shown = edit.canvas_rect();
    assert!(
        shown.y >= 0.0 && shown.y + shown.height < 600.0,
        "{shown:?}"
    );
    assert_eq!(app.o.selection.map(|s| s.area), Some(shown));
    assert!(app.snip_dock().is_some(), "its toolbar docks to it");
    run_task(&mut app, task);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(copy::copied::image(), None, "a pick copies nothing");
}

#[test]
fn after_a_drag_out_the_snip_toolbar_is_still_there_or_the_overlay_closes_whole() {
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    // nothing took the drop: the overlay is back, toolbar and all
    let mut app = snipping(area);
    app.select(area);
    let _ = app.open_snip_toolbar();
    annotate::dragout::TEST_DROP.set(Some(false));
    let _ = app.update(Message::Annotate(annotate::Message::DragOut));
    assert!(app.editing() && app.o.frame.is_some());
    // an app took it: closed like Copy, nothing of the editor or the frame left
    annotate::dragout::TEST_DROP.set(Some(true));
    let _ = app.update(Message::Annotate(annotate::Message::DragOut));
    assert!(app.o.edit.is_none() && app.o.frame.is_none() && !app.editing());
    assert_eq!(app.hiding, Hiding::Empty, "hides by the usual steps");
}

#[test]
fn handles_resize_and_moving_drags_at_any_zoom_and_scale() {
    // scale 2 (a 400x300 overlay on 800x600 pixels), zoomed in 1.5625x (at once)
    let mut app = open_app(800, 600, 2.0);
    motion::set_reduced(true);
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

/// A lost touch ends the press: the circle is not completed (a lift would select).
#[test]
fn a_cancelled_touch_completes_no_circle() {
    let mut app = open_app(800, 600, 1.0);
    let ring = ring(300.0, 200.0, 40.0);
    event(
        &mut app,
        overlay::Event::Press {
            at: ring[0],
            shift: false,
        },
    );
    ring[1..]
        .iter()
        .for_each(|&p| event(&mut app, overlay::Event::Move(p)));
    event(&mut app, overlay::Event::Cancel);
    assert!(app.o.selection.is_none() && !app.o.drawing && app.o.stroke.is_empty());
    // the lift of a finished circle does select
    circle(&mut app, Point::new(300.0, 200.0), 40.0, false);
    assert!(app.o.selection.is_some());
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
    // the bundled fonts first, whatever ran before: "monospace" then means the
    // same font in every run (it used to depend on test order)
    crate::shared::kit::text::load();
    let lines = TEXT.map(|t| -> Element<'_, Message> {
        let line = text(t).size(22).font(iced::Font::MONOSPACE);
        line.line_height(iced::Pixels(32.0)).into()
    });
    let page = container(column(lines)).padding([40, 60]);
    let rgba = crate::shared::kit::testing::shot::render(Element::from(page), 960, 420);
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
        let rgba = crate::shared::kit::testing::shot::render(app.view(app.overlay), 960, 420);
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
    let texts = crate::shared::kit::testing::shot::texts(app.view(app.overlay));
    assert!(texts.contains(&"what is this?".to_string()), "{texts:?}");
    // The design: an empty field opens Ask AI (its presets), asking nothing.
    let _ = app.update(Message::AskInput("   ".into()));
    let _ = app.update(Message::AskSubmit);
    let (_, panel) = app.o.ai_chat.as_mut().expect("Ask AI opens");
    assert!(panel.take_input().is_empty(), "no question");
    app.o.ai_chat = None;
    let _ = app.update(Message::AskInput("what is this?".into()));
    let _ = app.update(Message::AskSubmit);
    assert!(app.o.ai_chat.is_some(), "Ask AI opens");
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

/// #169: while the screen's text is read, Copy text says how far ("N of M
/// lines", from the lines the helper found); lines found but not read yet
/// wait (the working cursor); once all is read the pill is quiet again; a
/// screen with no text says so.
#[test]
fn reading_the_screen_shows_how_far_it_is() {
    let mut app = reader_app();
    assert_eq!(app.reading_text(), None);
    app.o.reading = Some(Rectangle::new(Point::ORIGIN, Size::new(960.0, 96.0)));
    assert_eq!(
        app.reading_text(),
        Some(None),
        "none found yet: \"Reading text…\""
    );
    let found: Vec<Rectangle> = (text_blocks().iter())
        .map(|b| {
            let [x, y, w, h] = b.bounds;
            Rectangle::new(Point::new(x, y), Size::new(w, h))
        })
        .collect();
    let _ = app.update(Message::ScreenFound(app.opening, found.clone()));
    assert_eq!(app.reading_text(), Some(Some((0, found.len()))));
    assert_eq!(app.text_waiting(), found);
    let _ = answer(&mut app, page());
    assert_eq!(app.reading_text(), None, "all read: quiet");
    assert!(app.text_waiting().is_empty() && !app.no_text());
    let mut empty = reader_app();
    let _ = empty.update(Message::ScreenText(empty.opening, page(), Ok(Vec::new())));
    assert!(empty.no_text(), "no text found on this screen");
}

/// #169 (sheet b7): text read as noise with no reader for its script on
/// disk: the reader for the user's language is offered, once per opening,
/// in the existing download card; not before 3 such lines.
#[test]
fn a_screen_read_as_noise_offers_the_readers_language() {
    let mut cfg = crate::shared::config::current();
    cfg.translate.target = "ar".into();
    crate::shared::config::set_current(cfg);
    let mut app = reader_app();
    let _ = app.update(Message::ScreenNoise(app.opening, 2));
    assert!(matches!(app.o.reader, Reader::Idle), "2 lines: not yet");
    let _ = app.update(Message::ScreenNoise(app.opening, 2));
    let rec = app.o.script_reader.expect("the Arabic reader is offered");
    assert_eq!(rec.id, "arabic");
    assert!(matches!(app.o.reader, Reader::Missing(bytes) if bytes > 0));
    let _ = app.update(Message::ReaderDismiss);
    let _ = app.update(Message::ScreenNoise(app.opening, 5));
    assert!(matches!(app.o.reader, Reader::Idle), "once per opening");
    crate::shared::config::set_current(crate::shared::config::Config::default());
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
    let click =
        |p| crate::shared::kit::testing::shot::click(app.view(app.overlay), app.o.screen, p);
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
    assert!(app.o.ai_chat.is_some(), "Ask AI opens");
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Word);
}

#[test]
fn the_bottom_bar_is_there_from_the_first_frame() {
    let mut app = open_app(800, 600, 1.0);
    let _ = app.update(Message::AskInput("what is on my screen?".into()));
    let has_field = |app: &App| overlay_texts(app).contains(&app.o.ask);
    assert!(has_field(&app), "before any selection");
    let clicked = crate::shared::kit::testing::shot::click(
        app.view(app.overlay),
        app.o.screen,
        bar_pill(&app, 2),
    );
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
    assert!(app.o.ai_chat.is_some());
    // the bar (and its field) stays: Translate and Copy text stay reachable
    assert!(app.bottom_bar_shown());
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
    let pill = |app: &App, k| {
        crate::shared::kit::testing::shot::click(
            app.view(app.overlay),
            app.o.screen,
            bar_pill(app, k),
        )
    };
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
    let live = crate::shared::kit::testing::shot::click(
        app.view(app.overlay),
        app.o.screen,
        bar_button(&app, 0),
    );
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

/// What a click on the reader's download card (over the bottom bar, on the
/// row of its buttons, across the bar's width) sends.
fn note_clicks(app: &App) -> Vec<Message> {
    let (_, size) = app.bar_size();
    let at = app.bar_at();
    // the buttons' middle: the card's 20 px padding and half a button up
    let y = at.y - bar::SPACING - 20.0 - 14.0;
    (0..(size.width as usize / 8))
        .flat_map(|i| {
            let p = Point::new(at.x + 8.0 * i as f32, y);
            crate::shared::kit::testing::shot::click(app.view(app.overlay), app.o.screen, p)
        })
        .collect()
}

/// A fresh install has no text reader (OCR models): a drag on text can only
/// draw. The download card over the bar says so, with the size, and
/// downloads only on its button; then its progress, a failure to try again;
/// Not now hides it for this opening.
#[test]
fn without_the_text_reader_the_bar_offers_its_download() {
    // no models, whatever another test leaves in the shared data folder
    text::MODELS_HERE.set(Some(false));
    let mut app = open_app(1280, 800, 1.0);
    app.note_reader(); // as at open (the tests have no models)
    let Reader::Missing(bytes) = app.o.reader else {
        panic!("{:?}", app.o.reader);
    };
    assert!(bytes > 1_000_000, "{bytes}");
    let texts = overlay_texts(&app);
    let mb = bytes.div_ceil(1_000_000);
    let say = format!("Text recognition · {mb} MB · from GitHub");
    assert!(texts.contains(&say), "{texts:?}");
    assert!(texts.contains(&"Download".to_string()), "{texts:?}");
    crate::shared::kit::testing::shot::save("reader-missing", app.view(app.overlay), app.o.screen);
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
    assert!(texts.contains(&"42%".to_string()), "{texts:?}");
    assert!(
        !texts.contains(&"Cancel".to_string()),
        "it runs on: {texts:?}"
    );
    crate::shared::kit::testing::shot::save(
        "reader-downloading",
        app.view(app.overlay),
        app.o.screen,
    );
    // another opening's download says nothing here
    let stale = Message::Reader(app.opening.wrapping_add(1), Err(S::md_err_other));
    let _ = app.update(stale);
    assert_eq!(app.o.reader, Reader::Downloading(0.42));
    let _ = app.update(step(Err(S::md_err_offline)));
    let texts = overlay_texts(&app);
    assert!(
        texts.contains(&crate::shared::app_language::tr(S::md_err_offline).to_string()),
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
    // Not now: no card for this opening
    assert!(
        note_clicks(&app)
            .iter()
            .any(|m| matches!(m, Message::ReaderDismiss))
    );
    let _ = app.update(Message::ReaderDismiss);
    assert_eq!(app.o.reader, Reader::Idle);
    assert!(app.reader_note().is_none());
    text::MODELS_HERE.set(None);
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
        windows: Vec::new(),
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
        windows: Vec::new(),
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
    let _ = crate::shared::kit::tooltip::tests_support::shown();
    let _ = overlay_texts(app);
    let mut names: Vec<String> = crate::shared::kit::tooltip::tests_support::shown()
        .iter()
        .map(|t| t.split(" · ").next().unwrap_or_default().to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn hidden_features_have_no_bar_button_and_turning_them_on_brings_them_back() {
    let mut app = open_app(1280, 800, 1.0);
    let (_, full) = app.bar_size();
    app.o.labs = Labs::default();
    let (_, small) = app.bar_size();
    assert!(
        small.width < full.width,
        "two buttons fewer: {small:?} < {full:?}"
    );
    let names = bar_names(&app);
    // the design's bar, no Live translate, no Song search; Snip is out of Labs
    let snip = names.iter().filter(|n| n.starts_with("Snip")).count();
    assert_eq!(snip, 1, "{names:?}");
    let rest: Vec<_> = names.iter().filter(|n| !n.starts_with("Snip")).collect();
    assert_eq!(
        rest,
        ["Ask AI", "Open OpenCircle AI chat", "Screen included"],
        "{names:?}"
    );
    let texts = overlay_texts(&app);
    for pill in ["Search in browser", "Copy text", "Translate"] {
        assert!(texts.iter().any(|t| t == pill), "{pill}: {texts:?}");
    }
    // Pins are off.
    assert!(!app.o.labs.has(labs::Feature::Pins));
    // Song search on: its button is back (one more).
    app.o.labs.set(labs::Feature::Song, true);
    assert!(bar_names(&app).iter().any(|n| n == "Song search"));
    // (the first icon also brings the divider before the icons)
    assert!(app.bar_size().1.width >= small.width + bar::BUBBLE);
    // Live translate on: its button is back.
    app.o.labs.set(labs::Feature::Live, true);
    assert!(bar_names(&app).iter().any(|n| n == "Live translate (Beta)"));
    // Snip is out of Labs (N-41): E opens the editor with every switch off.
    app.o.labs = Labs::default();
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    let _ = app.update(Message::Run(&actions::snip::ACTION));
    assert!(app.editing());
}

/// Snip is one flow (owner, 2026-10-31): main mode's Snip (the bar's button,
/// E, `run edit`) turns the frozen screen into Snip's pick step (edge glow on),
/// or, with an area selected, its picked state: the live area with Snip's
/// toolbar (no Back), the selection's corner brackets and the edge glow gone.
#[test]
fn main_modes_snip_runs_the_one_snip_flow() {
    let snip_toolbar = |app: &App| {
        let edit = app.o.edit.as_ref().expect("the editor");
        assert!(
            edit.annotator.movable,
            "Snip's live area, not the in-place editor"
        );
        let stops = crate::ui::screens::snip_toolbar::toolbar::stops(&edit.annotator);
        assert!(
            !stops.contains(&annotate::Message::Back),
            "no Back: Snip's toolbar"
        );
    };
    // the bar's Snip (and E) with nothing selected: the pick step
    let mut app = open_app(1280, 800, 1.0);
    let edit = app.bar_message(BarButton::Edit).unwrap();
    let _ = app.update(edit);
    assert!(matches!(app.o.purpose, Purpose::Snip(_)), "Snip's flow");
    assert!(app.o.edit.is_none() && app.o.selection.is_none(), "picking");
    assert!(
        app.edge_glow(None) > 0.0,
        "the edge glow: the screen is frozen"
    );
    // a drag there picks an area: the same picked state as Print Screen's
    let area = Rectangle::new(Point::new(200.0, 150.0), Size::new(300.0, 200.0));
    let _ = app.snip_next(Some(snip::Next::Edit(area)));
    snip_toolbar(&app);
    assert_eq!(app.edge_glow(None), 0.0, "picked: no edge glow");
    // with an area selected (the bar's Snip, E, `run edit`): picked at once
    for message in [
        |app: &App| app.bar_message(BarButton::Edit).unwrap(),
        |_: &App| Message::Run(&actions::snip::ACTION),
    ] {
        let mut app = open_app(1280, 800, 1.0);
        drag(
            &mut app,
            area.position(),
            area.position() + Vector::new(300.0, 200.0),
            false,
        );
        let selected = app.o.selection.unwrap().area;
        let m = message(&app);
        let _ = app.update(m);
        assert!(matches!(app.o.purpose, Purpose::Snip(_)), "Snip's flow");
        snip_toolbar(&app);
        assert_eq!(app.o.selection.map(|s| s.area), Some(selected), "its area");
        assert_eq!(app.edge_glow(None), 0.0, "picked: no edge glow");
    }
}

#[test]
fn a_still_pin_needs_no_labs_switch() {
    let mut app = open_app(1280, 800, 1.0);
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    let _ = app.update(Message::Run(&actions::snip::ACTION));
    app.o.labs.set(labs::Feature::Pins, false);
    let _ = app.update(Message::Annotate(annotate::Message::Pin));
    assert!(app.hiding != Hiding::No, "pinned: the overlay closes");
}

#[test]
fn a_hidden_features_trigger_does_nothing() {
    // The shared config is the defaults here: Song is off.
    let mut app = open_app(1280, 800, 1.0);
    app.o.frame = None;
    let qa = |action: &str| QuickAction {
        action: action.into(),
        shortcut: String::new(),
        target: Target::Circle,
        live: false,
    };
    let _ = app.update(Message::Trigger(Trigger::Quick(
        Instant::now(),
        qa("song_search"),
    )));
    assert!(
        app.capturing.is_none(),
        "song search did not start a capture"
    );
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
                p == "Ask AI…"
                    || crate::shared::kit::text::width(
                        p,
                        field::SLOTS_TEXT,
                        crate::shared::kit::text::ui_font()
                    ) <= room,
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
    assert!(
        app.o.ai_chat.is_none(),
        "no Lens, no Ask AI before a choice"
    );
    let _ = crate::shared::kit::tooltip::tests_support::shown();
    let texts = overlay_texts(&app);
    let tips = crate::shared::kit::tooltip::tests_support::shown();
    let labels = |t: &String| t.split(" · ").next().unwrap_or_default().to_string();
    let mut names: Vec<String> = tips.iter().map(labels).collect();
    names.sort();
    names.dedup();
    assert_eq!(
        names,
        [
            "Ask AI",
            "Live translate (Beta)",
            "Open OpenCircle AI chat",
            "Screen included",
            "Snip image",
            "Song search",
        ],
        "{tips:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| ask_placeholders().contains(&t.as_str())),
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
    assert!(app.o.ai_chat.is_some());
    // a file that still says "Run at once" (selection.after = "search": no longer read):
    // nothing runs by itself either (owner, 2026-10-26: no auto search)
    let mut app = open_app(1280, 800, 1.0);
    app.o.behaviour = behaviour(SelectStyle::Snap);
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    let _ = app.update(Message::Settled);
    assert!(app.o.selection.is_some());
    assert!(app.o.ai_chat.is_none(), "no Ask AI by itself");
    // the key still works: A asks
    app.o.ai_chat = None;
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'a',
            ctrl: false,
        },
    );
    assert!(app.o.ai_chat.is_some());
}

/// Frames until the bar is where it goes (it glides there).
pub(crate) fn settle_bar(app: &mut App) {
    let start = Instant::now();
    for i in 0..100 {
        let _ = app.update(Message::Frame(start + Duration::from_millis(16 * i)));
    }
    assert!(!app.bar_gliding());
}

/// The bar's bounds, and whether its Edit slot (the last) is Edit, once settled.
fn bar_box(app: &mut App) -> (Rectangle, bool) {
    settle_bar(app);
    let edit = crate::shared::kit::testing::shot::click(
        app.view(app.overlay),
        app.o.screen,
        bar_button(app, 2),
    );
    let is_edit = |m: &Message| match m {
        Message::EditScreen => true,
        Message::Run(a) => a.id == actions::snip::ACTION.id,
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
        let got = crate::shared::kit::testing::shot::click(app.view(app.overlay), app.o.screen, at);
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
    app.o.ai_chat = None; // the auto-search's card
    let (selected, edits) = bar_box(&mut app);
    assert_eq!(selected, before);
    assert!(edits);
    // Ask AI's card opens (the bar fits beside it) and closes: the same again
    let _ = app.update(Message::AskButton);
    assert!(app.o.ai_chat.is_some(), "Ask AI opens");
    assert!(
        app.bottom_bar_shown(),
        "Translate and Copy text stay reachable"
    );
    let bar = Rectangle::new(app.bar_target(), app.dock_size());
    assert!(
        !bar.intersects(&app.chat_card_at()),
        "beside the chat, not under it"
    );
    // a click on the frozen screen outside the card closes it
    event(
        &mut app,
        overlay::Event::Press {
            at: Point::new(5.0, 5.0),
            shift: false,
        },
    );
    event(&mut app, overlay::Event::Release);
    assert!(app.o.ai_chat.is_none());
    assert!(app.bottom_bar_shown());
    let (after, edits) = bar_box(&mut app);
    assert_eq!(after, before);
    assert!(edits);
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
    let grabbed =
        crate::shared::kit::testing::shot::click(app.view(app.overlay), app.o.screen, handle);
    assert!(grabbed.iter().any(|m| matches!(m, Message::BarGrab)));
    // the field and the buttons don't drag it
    let field = crate::shared::kit::testing::shot::click(
        app.view(app.overlay),
        app.o.screen,
        bar_field(&app),
    );
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
        let _ = crate::shared::kit::tooltip::tests_support::shown(); // drop older records
        let click =
            |at| crate::shared::kit::testing::shot::click(app.view(app.overlay), app.o.screen, at);
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
        let tips = crate::shared::kit::tooltip::tests_support::shown();
        for want in [
            "Live translate (Beta) · Ctrl+Shift+L",
            "Song search",
            "Snip the screen · E",
        ] {
            assert!(tips.iter().any(|t| t == want), "{want}: {tips:?}");
        }
        crate::shared::kit::tooltip::tests_support::assert_tips_add_something("bottom bar");
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
    assert!(app.o.ai_chat.is_some());
    // the same keys with nothing selected: A asks, R translates, all the
    // screen; E snips: Snip's pick step over the frozen screen
    let mut app = open_app(800, 600, 1.0);
    event(
        &mut app,
        overlay::Event::Key {
            letter: 'e',
            ctrl: false,
        },
    );
    assert!(matches!(app.o.purpose, Purpose::Snip(_)), "Snip's flow");
    assert!(app.o.edit.is_none() && app.o.selection.is_none(), "picking");
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
    assert!(app.o.ai_chat.is_some());
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
    let shot =
        |app: &App| crate::shared::kit::testing::shot::render(app.view(app.overlay), 960, 420);
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
        crate::shared::kit::testing::shot::save(
            &name,
            app.view(app.overlay),
            Size::new(960.0, 420.0),
        );
    }
    let mut app = open_app(3440, 1440, 1.0);
    app.o.appear = None;
    app.o.appear = Some((Instant::now(), 0.4));
    let t = Instant::now();
    for _ in 0..5 {
        crate::shared::kit::testing::shot::render(app.view(app.overlay), 3440, 1440);
    }
    println!("3440x1440 opening frame: {:?}", t.elapsed() / 5);
    let t = Instant::now();
    for _ in 0..5 {
        crate::shared::kit::testing::shot::render(
            Element::<'_, Message>::from(space()),
            3440,
            1440,
        );
    }
    println!("3440x1440 empty frame (the harness): {:?}", t.elapsed() / 5);
    let frame = app.o.frame.clone().unwrap();
    let t = Instant::now();
    for _ in 0..5 {
        let only = image(frame.clone())
            .width(Fill)
            .height(Fill)
            .filter_method(image::FilterMethod::Nearest);
        crate::shared::kit::testing::shot::render(Element::<'_, Message>::from(only), 3440, 1440);
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
    let opened = app.open_chat(crop.clone(), None);
    drop(opened); // its work (the setup) is done below by hand
    let id = app.o.ai_chat.as_ref().map(|(id, _)| *id).unwrap();
    let setup = crate::features::ask_ai::ask::setup(&crop).map(Arc::new);
    assert!(setup.is_ok(), "the no-key provider needs no setup");
    let ready = app.update(Message::AiChat(id, ai_chat::Msg::Ready(setup)));
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

use crate::shared::kit::testing::shot::{self, Clip, Focus};

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

/// No key (Google AI Mode): Ask AI only hands off to the browser and closes the
/// overlay; no chat card, no pinned chat (the owner, 2026-10-28).
#[test]
fn in_browser_mode_asking_opens_no_chat() {
    crate::features::ask_ai::app::ai_chat::HAND_OFF.set(true);
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    assert_eq!(config::current().ai.provider, config::Provider::Browser);
    let _ = app.open_chat(crop, Some("What is this?".into()));
    // the same from the bar: its Ask AI action and the AI field's Enter
    let _ = app.update(Message::Run(&crate::features::ask_ai::ask::ACTION));
    let _ = app.update(Message::AskInput("What is this?".into()));
    let _ = app.update(Message::AskSubmit);
    crate::features::ask_ai::app::ai_chat::HAND_OFF.set(false);
    assert!(app.o.ai_chat.is_none() && app.chat.is_none(), "no chat");
}

/// Search in browser (the pill, its key) never shows a card of ours, not for a
/// frame: the browser opens, a small pill at most (owner, 2026-11-02).
#[test]
fn search_in_browser_shows_no_card() {
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let _ = app.on_pill(BarButton::Search);
    assert!(app.o.ai_chat.is_none() && app.chat.is_none(), "the pill");
    let _ = app.update(Message::Run(&search_in_browser::ACTION));
    assert!(app.o.ai_chat.is_none() && app.chat.is_none(), "the action");
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
    let _ = app.open_chat(crop.clone(), None);
    let id = app.o.ai_chat.as_ref().map(|(id, _)| *id).unwrap();
    let setup = crate::features::ask_ai::ask::setup(&crop).map(Arc::new);
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Ready(setup)));
    (app, id)
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

/// `feed`, to the AI chat's own window.
fn feed_chat(
    app: &App,
    focus: Focus,
    events: &[iced::Event],
) -> (Vec<Message>, Vec<iced::event::Status>) {
    let w = app.chat_win.as_ref().expect("the chat's window");
    shot::feed(app.view(w.id), w.size, focus, events, &mut Clip::default())
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
    let (messages, _) = feed_chat(
        &app,
        Focus::Id(ai_chat::INPUT),
        &shot::typing("real estate"),
    );
    assert_eq!(shortcuts(&messages), "", "{messages:?}");
    let typed =
        (messages.iter()).filter(|m| matches!(m, Message::AiChat(_, ai_chat::Msg::Edit(_))));
    assert_eq!(typed.count(), "real estate".len());
    let (messages, _) = feed(&app, Focus::Nothing, &shot::typing("aterl"));
    assert_eq!(
        shortcuts(&messages),
        "aterl",
        "the panel doesn't take keys by itself (they stay the overlay's)"
    );
}

#[test]
fn a_translation_has_no_field_to_take_the_letters() {
    let mut app = open_app(800, 600, 1.0);
    let frame = app.o.frame.clone().unwrap();
    let (session, _) =
        translate::Session::start(frame, [0, 0, 800, 600], 1.0, app.cache.clone(), false, 0);
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
    // the Ask panel's input
    let (app, _) = app_with_ask_panel();
    let (messages, status) = feed_chat(&app, Focus::Id(ai_chat::INPUT), &esc);
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::AiChat(_, ai_chat::Msg::Key(keys::Keyed::Left))))
    );
    assert_eq!(status[1], iced::event::Status::Captured);
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
    use bar::Stop::{Badge, Button, Field, Include, Logo};
    let forward: Vec<_> = (0..9).map(|_| tab(&mut app, false)).collect();
    let order = [
        Logo,
        Field,
        Include,
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
    assert!(app.o.ai_chat.is_some(), "the logo opens the AI chat");
    // Space with nothing focused does nothing
    let mut app = open_app(800, 600, 1.0);
    let _ = app.on_overlay(overlay::Event::Space);
    assert!(app.o.ai_chat.is_none() && app.o.bar_focus.is_none());
    // a click puts the focus back on the overlay
    app.o.bar_focus = Some(Badge);
    let _ = app.on_overlay(overlay::Event::Press {
        at: Point::new(5.0, 5.0),
        shift: false,
    });
    assert_eq!(app.o.bar_focus, None);
}

fn behaviour(style: SelectStyle) -> SelectionConfig {
    SelectionConfig {
        style,
        text_reader: TextReader::Auto,
    }
}

#[test]
fn a_circle_snaps_to_a_box_that_morphs_and_resizes_by_its_brackets() {
    let mut app = open_app(800, 600, 1.0);
    app.o.behaviour = behaviour(SelectStyle::Snap);
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
    app.o.behaviour = behaviour(SelectStyle::Snap);
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
    app.o.behaviour = behaviour(SelectStyle::Snap);
    let pill = |app: &App| {
        let got = crate::shared::kit::testing::shot::click(
            app.view(app.overlay),
            app.o.screen,
            bar_pill(app, 0),
        );
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
fn a_circle_snaps_only_on_release() {
    let mut app = open_app(800, 600, 1.0);
    app.o.behaviour = behaviour(SelectStyle::Snap);
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
    }
    event(&mut app, overlay::Event::Release);
    assert_eq!(app.o.selection.unwrap().kind, overlay::Kind::Snap);
    assert!(app.o.morph.is_some(), "the stroke morphs into the box");
    assert!(app.settling(), "it snaps");
    let (from, _) = app.o.morph.take().unwrap();
    app.o.morph = Some((from, Instant::now() - overlay::MORPH * 2));
    assert!(!app.settling(), "then the box is settled");
}

/// A snapped box over `text_app`'s lines 1-3, in select-text mode with its words read.
fn select_text_app() -> App {
    let mut app = text_app();
    app.o.behaviour = behaviour(SelectStyle::Snap);
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
    let texts = crate::shared::kit::testing::shot::texts(app.view(app.overlay));
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
    let texts = crate::shared::kit::testing::shot::texts(app.view(app.overlay));
    assert!(
        !texts.iter().any(|t| t == "Copy" || t == "Edit"),
        "{texts:?}"
    );
    let click = |k| {
        crate::shared::kit::testing::shot::click(
            app.view(app.overlay),
            app.o.screen,
            bar_pill(&app, k),
        )
    };
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
    let _ = app.update(Message::SelectText); // no OCR models here: the download card asks
    drag(
        &mut app,
        Point::new(700.0, 300.0),
        Point::new(900.0, 400.0),
        false,
    );
    assert!(app.o.text_mode.is_none());
}

/// Select text pressed, nothing picked yet: Ctrl+C copies all the box's text
/// in reading order, not the picture of the box (owner, 2026-10-28). Before
/// its words are read it reads them (Copy text), still never the picture.
#[test]
fn ctrl_c_in_select_text_mode_copies_the_boxs_text_not_its_picture() {
    let ctrl_c = overlay::Event::Key {
        letter: 'c',
        ctrl: true,
    };
    let mut app = select_text_app();
    let _ = copy::copied::text();
    let _ = copy::copied::image();
    let want = app.box_text().expect("the box's lines");
    assert!(want.contains('\n'), "every line of the box: {want}");
    let task = app.update(Message::Overlay(ctrl_c));
    run_task(&mut app, task);
    assert_eq!(copy::copied::text().as_deref(), Some(want.as_str()));
    assert_eq!(copy::copied::image(), None, "not the picture");
    // not read yet: it reads the box, and copies no picture meanwhile
    let mut early = select_text_app();
    early.o.text_mode.as_mut().unwrap().lines = None;
    let task = early.update(Message::Overlay(ctrl_c));
    let reading = early.o.toast == Some(Said(S::bar_reading));
    let asking = early.o.copy_after; // no models: the download card, then it copies
    assert!(reading || asking, "reads the box (or asks for its model)");
    run_task(&mut early, task);
    assert_eq!(copy::copied::image(), None, "not the picture");
}

#[test]
fn chips_go_under_the_box_else_above_never_on_it() {
    let screen = Rectangle::with_size(Size::new(1920.0, 1080.0));
    let size = Size::new(280.0, 44.0);
    let area = Rectangle::new(Point::new(400.0, 300.0), Size::new(500.0, 200.0));
    let at = chips_at(area, size, screen, None);
    assert_eq!(at, Point::new(510.0, 512.0), "centred, 12 px under the box");
    // no room below: above
    let low = Rectangle::new(Point::new(400.0, 900.0), Size::new(500.0, 150.0));
    let at = chips_at(low, size, screen, None);
    assert_eq!(at.y, 900.0 - CHIP_SPACE - 44.0);
    for a in [area, low] {
        let chips = Rectangle::new(chips_at(a, size, screen, None), size);
        assert!(chips.intersection(&a).is_none(), "{a:?} -> {chips:?}");
    }
    // beside a card: kept in the free part
    let room = Rectangle::new(Point::ORIGIN, Size::new(1112.0, 1080.0));
    let edge = Rectangle::new(Point::new(950.0, 300.0), Size::new(150.0, 100.0));
    let at = chips_at(edge, size, room, None);
    assert!(at.x + size.width <= room.width - bar::GAP);
    // the whole screen selected: on it, at the bottom, still on screen
    let all = chips_at(screen, size, screen, None);
    assert_eq!(all.y, 1080.0 - bar::GAP - 44.0);
    // gate 0.1.2 bug 1: the bar docked at the top, a box (the address bar)
    // just above it: the chip goes past the bar, never under it
    let url = Rectangle::new(Point::new(106.0, 50.0), Size::new(266.0, 22.0));
    let dock = Rectangle::new(Point::new(20.0, 84.0), Size::new(900.0, 56.0));
    let chip = Rectangle::new(chips_at(url, size, screen, Some(dock)), size);
    assert!(chip.intersection(&dock).is_none(), "{chip:?} under the bar");
    assert!(chip.intersection(&url).is_none() && chip.y >= dock.y + dock.height);
    // a bar elsewhere changes nothing
    let far = Rectangle::new(Point::new(500.0, 1000.0), Size::new(900.0, 56.0));
    assert_eq!(
        chips_at(area, size, screen, Some(far)),
        Point::new(510.0, 512.0)
    );
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
    let _ = app.update(Message::Run(&crate::features::ask_ai::ask::ACTION));
    let card = app.chat_card_at();
    let room = app.room();
    assert!(room.intersection(&card).is_none(), "{room:?} vs {card:?}");
    assert!(room.width > 0.0);
    // and with no card, the whole screen
    app.o.ai_chat = None;
    assert_eq!(app.room(), Rectangle::with_size(app.o.screen));
}

#[test]
fn a_quick_action_on_a_drawn_area_runs_it() {
    let mut app = open_app(1920, 1080, 1.0);
    app.o.behaviour = SelectionConfig::default();
    let ask = Quick::Action(&crate::features::ask_ai::ask::ACTION);
    app.o.purpose = Purpose::Quick(ask, config::Target::Circle);
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(400.0, 300.0),
        false,
    );
    assert!(app.o.ai_chat.is_some(), "Ask AI, not Lens over it");
}

#[test]
fn search_in_browser_closes_the_overlay() {
    let mut app = open_app(1920, 1080, 1.0);
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(400.0, 300.0),
        false,
    );
    let _ = app.update(Message::Run(&search_in_browser::ACTION));
    // never a card of ours, not even while the page is being written
    assert!(app.o.ai_chat.is_none(), "Search in browser opens no panel");
    let opened = Message::SearchOpened(Ok(()));
    let _ = app.update(opened);
    // Closed on every OS: nothing of ours stays on screen after the hand-off.
    assert!(app.o.frame.is_none());
    assert!(app.o.ai_chat.is_none());
}

#[test]
fn the_result_card_opens_off_the_selection() {
    let mut app = open_app(1920, 1080, 1.0);
    drag(
        &mut app,
        Point::new(100.0, 100.0),
        Point::new(400.0, 300.0),
        false,
    );
    let _ = app.update(Message::Run(&crate::features::ask_ai::ask::ACTION)); // a card
    assert!(app.o.ai_chat.is_some());
    let at = app.chat_card_at();
    assert_eq!(at.x + at.width, 1920.0 - 16.0, "right");
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
        let rgba = crate::shared::kit::testing::shot::render(app.view(app.overlay), 960, 420);
        actions::write_png(&dir.join(name), 960, 420, &rgba).unwrap();
    };
    let glowing_at = |app: &mut App, w: u32, h: u32| {
        if let Some(image::Handle::Rgba { pixels, .. }) = &app.o.frame {
            let mut rgba = pixels.to_vec();
            border::paint(
                &mut rgba,
                w,
                h,
                1.0,
                &config::look_config(),
                border::Mode::Overlay,
            );
            app.o.frame = Some(image::Handle::from_rgba(w, h, rgba));
        }
    };
    let glowing = |app: &mut App| {
        if let Some(image::Handle::Rgba { pixels, .. }) = &app.o.frame {
            let mut rgba = pixels.to_vec();
            let look = config::look_config();
            border::paint(&mut rgba, 960, 420, 1.0, &look, border::Mode::Overlay);
            app.o.frame = Some(image::Handle::from_rgba(960, 420, rgba));
        }
    };
    let mut app = text_app();
    glowing(&mut app);
    app.o.behaviour = behaviour(SelectStyle::Snap);
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
    let _ = app.update(Message::Run(&crate::features::ask_ai::ask::ACTION)); // a card
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
    let _ = wide.update(Message::Run(&crate::features::ask_ai::ask::ACTION));
    wide.o.chat_hover = true;
    let rgba = crate::shared::kit::testing::shot::render(wide.view(wide.overlay), 1440, 900);
    actions::write_png(&dir.join("look-7-ask-card-1440.png"), 1440, 900, &rgba).unwrap();
    let mut app = select_text_app();
    glowing(&mut app);
    shot(&app, "look-4-select-text.png");
    drag(&mut app, at_word(1, 7.0), at_word(2, 11.0), false);
    shot(&app, "look-5-select-text-popup.png");
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
    let _ = app.open_chat(crop, None);
    let frame = app.o.frame.clone().unwrap();
    let (session, _) =
        translate::Session::start(frame, [0, 0, 40, 20], 1.0, app.cache.clone(), false, 0);
    app.o.translate = Some((1, session));
    let (song, _) = song::Session::start();
    app.o.song = Some((1, song));
    app.o.ask = "what is this".into();
    app.o.bar_focus = Some(bar::Stop::Logo);
    app.o.clicks = Some((Instant::now(), Point::ORIGIN, 2));
    assert!(app.o.ai_chat.is_some());
    let _ = app.close();
    assert!(app.o.is_none(), "nothing of the opening is kept");
    let (o, fresh) = (&app.o, Opening::default());
    assert!(o.frame.is_none() && o.selection.is_none());
    assert!(o.stroke.is_empty() && o.others.is_empty() && o.word.is_none());
    assert_eq!(o.screen_text, fresh.screen_text);
    assert!(o.ai_chat.is_none() && o.translate.is_none() && o.song.is_none());
    assert!(o.helper.is_none() && o.edit.is_none());
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
        windows: Vec::new(),
    };
    let _ = app.open(Instant::now(), Ok(shot));
    assert!(app.o.is_some() && app.o.frame.is_some());
    app.select(Rectangle::new(Point::new(4.0, 4.0), Size::new(40.0, 30.0)));
    let crop = app.crop().unwrap();
    let _ = app.open_chat(crop, None);
    assert!(app.o.ai_chat.is_some());
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
        Message::AiChat(panel, ai_chat::Msg::Delta("late".into())),
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

#[test]
fn the_chat_docks_where_it_is_let_go_on_whole_pixels() {
    let (mut app, id) = app_with_ask_panel(); // 800 x 600, a selection at the left
    let at = app.chat_card_at();
    let d = app.chat_dock(app.o.screen, card::Side::Right, app.o.work_inset);
    assert_eq!(
        (at.x, at.width, at.height),
        (d.x.round(), d.width.round(), d.height.round())
    );
    // held: it follows the pointer 1:1 across, by fractions of a px too,
    // drawn on whole px, picked up (scaled, accent edge); full height, it
    // moves only across
    let grab = ai_chat::Msg::Card(card::Event::Grab(card::Drag::Move));
    let _ = app.update(Message::AiChat(id, grab));
    let _ = app.update(Message::CardMoved(Point::new(700.0, 30.0)));
    let _ = app.update(Message::CardMoved(Point::new(500.3, 40.6)));
    let held = app.chat_card_at();
    assert_eq!(held.position(), Point::new((at.x - 199.7).round(), at.y));
    assert!(app.o.ai_chat.as_ref().unwrap().1.held);
    // ... but never past 6 px from the screen's edge (design 3.4)
    let _ = app.update(Message::CardMoved(Point::new(-400.0, 40.6)));
    let held = app.chat_card_at();
    assert_eq!(held.x, motion::DRAG_EDGE);
    // let go left of the middle: it glides to the left dock, and stays there
    let _ = app.update(Message::CardReleased);
    assert!(app.o.chat_glide.is_some() && app.animating());
    let began = app.o.chat_glide.unwrap().0;
    // the frame strip (OPENCIRCLE_RENDER_DIR): let go, then every 60 ms
    if std::env::var_os("OPENCIRCLE_RENDER_DIR").is_some() {
        let long_ago = began - std::time::Duration::from_secs(2);
        let _ = app.update(Message::Frame(long_ago)); // the card opened long before
        for k in 0..4u32 {
            let _ = app.update(Message::Frame(began + motion::DOCK * k / 4));
            let name = format!("chat-glide-{k}");
            shot::save(&name, app.view(app.overlay), app.o.screen);
        }
    }
    let _ = app.update(Message::Frame(began + motion::DOCK / 3));
    let mid = app.chat_card_at();
    assert!(
        mid.x > held.x && mid.x < 16.0 && mid.x == mid.x.round(),
        "{mid:?}"
    );
    let _ = app.update(Message::Frame(began + motion::DOCK));
    assert!(app.o.chat_glide.is_none());
    assert!(!app.o.ai_chat.as_ref().unwrap().1.held, "let go: scale 1");
    shot::save("chat-glide-4", app.view(app.overlay), app.o.screen);
    assert_eq!(app.chat_card_at().x, 16.0);
    assert_eq!(app.chat_side, Some(card::Side::Left));
    // the next chat in this run opens on that side
    app.o.ai_chat = None;
    let crop = app.crop().unwrap();
    let _ = app.open_chat(crop, None);
    assert_eq!(app.chat_card_at().x, 16.0);
}

/// The chat never covers the bar: for every bar anchor and either side,
/// the docked chat and the bar at its spot don't meet (owner, 2026-10-28).
#[test]
fn the_docked_chat_never_covers_the_bar() {
    for (w, h) in [(1280, 720), (1920, 1080), (2560, 1440)] {
        let mut app = open_app(w, h, 1.0);
        app.o.work_inset = 48.0;
        drag(
            &mut app,
            Point::new(50.0, 50.0),
            Point::new(150.0, 120.0),
            false,
        );
        let crop = app.crop().unwrap();
        drop(app.open_chat(crop, None));
        let id = app.o.ai_chat.as_ref().unwrap().0;
        api_ready(&mut app, id, b"first");
        chat_turn(&mut app, id, "What is this?", "A chart.");
        let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin)); // (a follow-up would close the overlay)
        chat_turn(&mut app, id, "And the trend?", "Up.");
        assert!(app.chat.is_some(), "pinned");
        app.o.selection = None;
        for side in [card::Side::Left, card::Side::Right] {
            app.chat_side = Some(side);
            let chat = app.chat_rect();
            // full height: the chat is never cut short for the bar
            assert_eq!(chat.height, ((h as f32 - 48.0) * 0.95).round(), "{w}x{h}");
            for anchor in crate::ui::screens::bar::geometry::ANCHORS {
                app.o.bar_anchor = anchor;
                let bar = Rectangle::new(app.bar_target(), app.dock_size());
                assert!(
                    !chat.intersects(&bar),
                    "{w}x{h} {anchor:?} {side:?}: {chat:?} {bar:?}"
                );
                // let go where it is drawn: the same spot (beside a wide
                // chat, a centre and a corner can be one spot)
                use crate::ui::screens::bar::geometry::{anchor_at, nearest};
                let room = app.bar_room();
                let at = bar.position() - iced::Vector::new(room.x, room.y);
                let near = nearest(at, room.size(), bar.size(), 48.0);
                let spot = |a| anchor_at(a, room.size(), bar.size(), 48.0);
                assert_eq!(spot(near), spot(anchor), "{w}x{h} {anchor:?}");
            }
        }
    }
}

/// The chat's inner edge sets its width, kept for the next chat; a
/// double-click on it goes back to the design's width.
#[test]
fn the_chats_inner_edge_sets_its_width() {
    let mut app = open_app(1920, 1080, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    drop(app.open_chat(crop, None));
    let id = app.o.ai_chat.as_ref().unwrap().0;
    let before = app.chat_rect();
    let grab = ai_chat::Msg::Card(card::Event::Grab(card::Drag::Resize));
    let _ = app.update(Message::AiChat(id, grab));
    let _ = app.update(Message::CardMoved(Point::new(1000.0, 500.0)));
    let _ = app.update(Message::CardMoved(Point::new(900.0, 520.0)));
    let _ = app.update(Message::CardReleased);
    let after = app.chat_rect();
    assert_eq!(after.width, before.width + 100.0, "wider by the drag");
    assert_eq!(after.height, before.height, "only the width");
    assert_eq!(
        after.x + after.width,
        before.x + before.width,
        "still docked"
    );
    let reset = ai_chat::Msg::Card(card::Event::Reset);
    let _ = app.update(Message::AiChat(id, reset));
    assert_eq!(app.chat_rect(), before);
}

/// The pinned chat's own window, let go mid-screen, glides to its dock frame
/// by frame (`motion::DOCK`, standard-out) and ends exactly on it.
#[test]
fn a_pinned_chat_window_glides_to_its_dock() {
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    drop(app.open_chat(crop, None));
    let id = app.o.ai_chat.as_ref().unwrap().0;
    api_ready(&mut app, id, b"first");
    chat_turn(&mut app, id, "What is this?", "A chart.");
    chat_turn(&mut app, id, "And the trend?", "Up.");
    let _ = app.update(Message::Escape);
    let window = app.chat_win.as_ref().map(|w| w.id).expect("its own window");
    // the OS dragged it to the middle-left, then it stopped
    let _ = app.update(Message::ChatMoved(window, Point::new(150.0, 100.0)));
    let moves = app.chat_win.as_ref().unwrap().moves;
    let _ = app.update(Message::ChatDocked(moves, 1.0));
    let (began, from, to) = (app.chat_win.as_ref().unwrap().glide).expect("a glide, not a jump");
    let (origin, screen, inset) = app.chat_win.as_ref().unwrap().home;
    let dock = app.chat_dock(screen, card::Side::Left, inset);
    assert_eq!(to, origin + Vector::new(dock.x, dock.y));
    let _ = app.update(Message::ChatFrame(began + motion::DOCK / 3));
    let mid = app.chat_win.as_ref().unwrap().at;
    assert!(mid.x < from.x && mid.x > to.x, "on its way: {mid:?}");
    let _ = app.update(Message::ChatFrame(began + motion::DOCK));
    let chat = app.chat_win.as_ref().unwrap();
    assert_eq!(chat.at, to, "exactly on the dock");
    assert!(chat.glide.is_none(), "the frames stop");
}

/// The pinned chat's window dragged onto another screen (another DPI) docks to
/// THAT screen's side and glides there, not back to the one it opened on (#125).
#[test]
fn a_pinned_chat_dragged_to_another_screen_docks_there() {
    use crate::shared::platform::monitor::Place;
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    drop(app.open_chat(crop, None));
    let id = app.o.ai_chat.as_ref().unwrap().0;
    api_ready(&mut app, id, b"first");
    chat_turn(&mut app, id, "What is this?", "A chart.");
    chat_turn(&mut app, id, "And the trend?", "Up.");
    let _ = app.update(Message::Escape);
    let window = app.chat_win.as_ref().map(|w| w.id).expect("window");
    let here = Place {
        origin: (0, 0),
        px: (800, 600),
        scale: 1.0,
    };
    let there = Place {
        origin: (800, 0),
        px: (1600, 1200),
        scale: 2.0,
    };
    // the OS dragged it onto the 200 % screen (logical px at that scale) and it stopped
    let _ = app.update(Message::ChatMoved(window, Point::new(500.0, 100.0)));
    let moves = app.chat_win.as_ref().unwrap().moves;
    let _ = app.chat_docked(moves, 2.0, &[here, there]);
    let chat = app.chat_win.as_ref().unwrap();
    let (origin, screen, _) = chat.home;
    assert_eq!(
        (origin, screen),
        (Point::new(400.0, 0.0), Size::new(800.0, 600.0))
    );
    let (_, _, to) = chat.glide.expect("a glide to its dock");
    assert!(
        to.x >= 400.0 && to.x + chat.size.width <= 1200.0 + 0.5,
        "docked on the second screen: {to:?}"
    );
}

/// Copy text (the key T, `opencircle run text`) on picked words copies exactly
/// those words and shows the Copied pill: no Text card ever opens.
#[test]
fn copy_text_copies_the_picked_words_and_opens_no_card() {
    let mut app = read_app();
    app.taps += 1;
    read_back(&mut app, at_word(1, 7.0), page(), false);
    let want = picked_text(&app).expect("a word is picked");
    let _ = copy::copied::text();
    let task = app.update(Message::Run(&text::ACTION));
    assert!(app.o.ai_chat.is_none(), "no Text card");
    run_task(&mut app, task);
    assert_eq!(copy::copied::text().as_deref(), Some(want.as_str()));
    assert_eq!(copy::copied::image(), None, "never the picture");
}

/// Without the OCR models Copy text shows the download card in place (the
/// bar's), downloads only on its button, then copies once it is Done.
#[test]
fn copy_text_without_models_shows_the_download_card_then_copies() {
    text::MODELS_HERE.set(Some(false));
    let mut app = open_app(1280, 800, 1.0);
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    app.o.ai_chat = None;
    let _ = app.update(Message::Run(&text::ACTION));
    assert!(app.o.ai_chat.is_none(), "no Text card");
    assert!(app.o.copy_after && matches!(app.o.reader, Reader::Missing(_)));
    let texts = overlay_texts(&app);
    assert!(texts.contains(&"Download".to_string()), "{texts:?}");
    assert!(app.o.toast.is_none(), "nothing is read before the download");
    // Done: the models are here now, so it reads and copies
    text::MODELS_HERE.set(Some(true));
    let _ = app.update(Message::ReaderDownload);
    let opening = app.opening;
    let _ = app.update(Message::Reader(opening, Ok(text::Progress::Reading)));
    assert!(!app.o.copy_after);
    assert_eq!(app.o.toast, Some(Said(S::bar_reading)));
    // Not now: nothing is copied afterwards
    text::MODELS_HERE.set(Some(false));
    let mut app = open_app(1280, 800, 1.0);
    drag(
        &mut app,
        Point::new(200.0, 150.0),
        Point::new(500.0, 350.0),
        false,
    );
    let _ = app.update(Message::Run(&text::ACTION));
    assert!(app.o.copy_after);
    let _ = app.update(Message::ReaderDismiss);
    assert!(!app.o.copy_after && app.o.reader == Reader::Idle);
    text::MODELS_HERE.set(None);
}

/// A link or email in the selected words gets a chip beside the selection;
/// plain text gets none; a click (only) asks for it to be opened.
#[test]
fn a_link_in_the_selected_words_gets_a_click_only_chip() {
    let pick = |line: &str| {
        let mut app = reader_app();
        // a blank frame: the words lie where OCR says, not on the sample page's
        // ink (its glyphs differ per machine's fonts; CI picked "see")
        app.o.frame = Some(image::Handle::from_rgba(960, 420, vec![255; 960 * 420 * 4]));
        app.taps += 1;
        let block = TextBlock {
            text: line.into(),
            bounds: [60.0, 43.0, line.len() as f32 * CHAR, 26.0],
            confidence: 1.0,
        };
        let what = WordRead {
            id: app.taps,
            at: at_word(0, 12.0),
            origin: [0, 0],
            region: page(),
            extend: false,
        };
        let _ = app.on_tapped(what, Ok(vec![block]));
        app
    };
    let app = pick("see https://shop.example/track now");
    let entity = app.chip_entity().unwrap_or_else(|| {
        panic!(
            "the link: word {:?}, selection {:?}",
            app.o.word.as_ref().map(word::Picked::text),
            app.o.selection
        )
    });
    let texts = overlay_texts(&app);
    assert!(
        texts.contains(&"Open shop.example".to_string()),
        "{texts:?}"
    );
    // nothing is opened by showing it; its click asks for the link
    let size = Size::new(LINK_WIDTH, bar::HEIGHT);
    let area = app.o.view.rect_to_screen(app.o.selection.unwrap().area);
    let at = chips_at(area, size, app.room(), app.chip_bar())
        + Vector::new(LINK_WIDTH / 2.0, bar::HEIGHT / 2.0);
    let clicks = crate::shared::kit::testing::shot::click(app.view(app.overlay), app.o.screen, at);
    assert!(
        clicks
            .iter()
            .any(|m| matches!(m, Message::OpenEntity(e) if *e == entity)),
        "{clicks:?}"
    );
    let app = pick("mail help@shop.example please");
    assert!(overlay_texts(&app).contains(&"Write help@shop.example".to_string()));
    let app = pick("just some plain words here");
    assert!(app.chip_entity().is_none());
    assert!(!overlay_texts(&app).iter().any(|t| t.starts_with("Open ")));
}

/// The result cards have no close button (the design has none): the AI chat
/// keeps its "+ New"; Esc closes, and so does a click on the frozen screen
/// outside the card.
#[test]
fn result_cards_have_no_close_button_esc_and_an_outside_click_close_them() {
    let close = crate::shared::kit::icon::Icon::Close.glyph().to_string();
    let (app, _) = app_with_ask_panel();
    let texts = texts_of(&app, chat_window(&app).unwrap());
    assert!(texts.contains(&"+ New".to_string()), "{texts:?}");
    assert!(!texts.contains(&close), "a close button: {texts:?}");
    // Esc
    let mut esc = app_with_ask_panel().0;
    let _ = esc.update(Message::Escape);
    assert!(esc.o.is_none(), "Esc closes");
    // a click outside the card (the card is docked right, the click is far left, away from the selection)
    let mut out = app_with_ask_panel().0;
    let card = out.chat_card_at();
    let at = Point::new(card.x / 2.0, out.o.screen.height - 20.0);
    event(&mut out, overlay::Event::Press { at, shift: false });
    event(&mut out, overlay::Event::Release);
    assert!(out.o.ai_chat.is_none(), "the click closes the card");
    // the Lens card too
    let mut lens = open_app(800, 600, 1.0);
    drag(
        &mut lens,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = lens.crop().unwrap();
    let _ = lens.open_chat(crop, None);
    let _ = lens.sync_chat_window();
    assert!(!texts_of(&lens, chat_window(&lens).unwrap()).contains(&close));
}

/// #149: the Copy / Save pill goes beside the selection (below it, else above
/// it), else above the bar, never mid-screen; in physical px, so it lands there
/// at 100, 175 and 250 % and on a second monitor.
#[test]
fn the_copied_pill_goes_beside_the_selection_at_any_scale() {
    use super::actions::pill_at;
    let size = windows::toast_size("Copied");
    let screen = Size::new(1280.0, 720.0);
    let sel = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let low = Rectangle::new(Point::new(600.0, 560.0), Size::new(200.0, 120.0));
    for scale in [1.0, 1.75, 2.5] {
        // the primary monitor, and one right of a 3840 px wide one
        for (px0, py0) in [(0, 0), (3840, -200)] {
            let origin = Point::new(px0 as f32 / scale, py0 as f32 / scale);
            let local = |near| {
                let p = pill_at((origin, screen, scale, near), size, 40.0);
                assert_eq!(p.scale, scale);
                let px = |v: f32| (v * scale).round() as u32;
                assert_eq!(p.px, (px(size.width), px(size.height)), "{scale}");
                let back = |v: i32, o: i32| (v - o) as f32 / scale;
                Point::new(back(p.origin.0, px0), back(p.origin.1, py0))
            };
            let near = |a: Point, b: Point| a.distance(b) <= 1.0;
            let centre = |r: Rectangle| r.center_x() - size.width / 2.0;
            // below the selection, centred on it
            let at = local(Some(sel));
            let want = Point::new(centre(sel), 280.0 + TOAST_MARGIN);
            assert!(near(at, want), "{scale} {px0}: {at:?} != {want:?}");
            // no room below (the taskbar): above it
            let at = local(Some(low));
            let want = Point::new(centre(low), 560.0 - TOAST_MARGIN - size.height);
            assert!(near(at, want), "{scale} {px0}: {at:?} != {want:?}");
            // nothing beside: bottom right, above the taskbar
            let at = local(None);
            let want = Point::new(
                1280.0 - TOAST_MARGIN - size.width,
                720.0 - 40.0 - TOAST_MARGIN - size.height,
            );
            assert!(near(at, want), "{scale} {px0}: {at:?} != {want:?}");
        }
    }
    // what it goes beside: the selection as shown, else the bar
    let mut app = open_app(1280, 720, 1.0);
    let bar = Rectangle::new(app.bar_at(), app.bar_size().1);
    assert_eq!(app.pill_place().3, Some(bar));
    app.select(sel);
    assert_eq!(app.pill_place().3, Some(sel));
    // above the bar, not over it
    let at = pill_at((Point::ORIGIN, screen, 1.0, Some(bar)), size, 0.0);
    assert!(
        (at.origin.1 as f32) + size.height <= bar.y,
        "{at:?} {bar:?}"
    );
}

/// #150: at 175 %, with the screen read by Windows OCR, Select text over an
/// email offers exactly the words a text selection there has: a double click
/// picks the whole address, at the same place, and nothing reads the box
/// again (no PP-OCR models needed).
#[test]
fn select_text_mode_picks_the_words_windows_ocr_read() {
    use iced::widget::text as label;
    text::MODELS_HERE.set(Some(false));
    crate::shared::kit::text::load();
    const LINE: &str = "Write to name.k@example-solutions.com today";
    const EMAIL: &str = "name.k@example-solutions.com";
    let s = 1.75;
    let line = label(LINE).size(22.0 * s).font(iced::Font::MONOSPACE);
    let page: iced::widget::Container<Message> =
        container(line.line_height(iced::Pixels(32.0 * s))).padding([40.0 * s, 60.0 * s]);
    let rgba = crate::shared::kit::testing::shot::render(Element::from(page), 1680, 700);
    let mut app = open_app(1680, 700, s);
    app.o.frame = Some(image::Handle::from_rgba(1680, 700, rgba));
    // Windows OCR's read of the whole screen (frame px)
    let block = TextBlock {
        text: LINE.into(),
        bounds: [60.0 * s, 43.0 * s, LINE.len() as f32 * CHAR * s, 26.0 * s],
        confidence: 1.0,
    };
    let mut lines = word::lines(vec![block], [0, 0], s);
    if let Some(image::Handle::Rgba { pixels, .. }) = &app.o.frame {
        word::measure(&mut lines, pixels, 1680, 700, s);
    }
    let whole = Rectangle::with_size(app.o.screen);
    let screen = app.o.screen;
    app.o
        .screen_text
        .add_read(ocr::Source::System, whole, lines, screen);
    app.o.text_by = Some(ocr::Source::System);
    let on_email = Point::new(60.0 + 20.0 * CHAR, 56.0);
    // normal mode: a double click picks the address
    click_at(&mut app, on_email, false);
    click_at(&mut app, on_email, false);
    assert_eq!(picked_text(&app).as_deref(), Some(EMAIL));
    let normal = app.o.word.as_ref().unwrap().rects();
    // select-text mode over it: the same address, in the same place
    app.o.word = None;
    app.o.clicks = None;
    let (sel, stroke) = overlay::snap(Rectangle::new(
        Point::new(60.0 + 8.5 * CHAR, 30.0),
        Size::new(29.0 * CHAR, 50.0),
    ));
    (app.o.selection, app.o.stroke) = (Some(sel), stroke);
    let task = app.update(Message::SelectText);
    assert_eq!(task.units(), 0, "nothing read again");
    assert_eq!(app.o.reader, Reader::Idle, "no download asked");
    let mode = app.o.text_mode.as_ref().unwrap();
    assert_eq!(mode.boxes.len(), 1, "only the address is in the box");
    for _ in 0..2 {
        let at = on_email;
        event(&mut app, overlay::Event::Press { at, shift: false });
        event(&mut app, overlay::Event::Release);
    }
    let picked = app.o.text_mode.as_ref().unwrap().picked.clone().unwrap();
    assert_eq!(picked.text(), EMAIL);
    assert_eq!(picked.rects(), normal);
    text::MODELS_HERE.set(None);
}

/// The AI field's Include screen (on by default): Tab reaches it after the
/// field, Space switches it; it is the chat's switch too (one state both
/// ways), and off a question goes without the screen.
#[test]
fn the_ai_fields_include_screen_is_the_chats_switch() {
    let mut app = open_app(1280, 720, 1.0);
    assert!(app.o.include, "on by default");
    for _ in 0..3 {
        let _ = app.tab_bar(false);
    }
    assert_eq!(app.o.bar_focus, Some(bar::Stop::Include));
    let _ = app.on_overlay(overlay::Event::Space);
    assert!(!app.o.include);
    let _ = app.on_overlay(overlay::Event::Space);
    assert!(app.o.include, "and back");
    let _ = app.on_overlay(overlay::Event::Space);
    // its look follows: accent on, dimmed off (the golden images show both)
    let _ = app.update(Message::AskInput("what is this?".into()));
    let _ = app.update(Message::AskSubmit);
    let (id, panel) = app.o.ai_chat.as_ref().expect("Ask AI opens");
    assert_eq!(panel.include(), Some(false), "the chat starts off too");
    // the chat's switch is the field's
    let id = *id;
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::IncludeScreen));
    assert!(app.o.include);
    // a new opening starts on
    let app = open_app(1280, 720, 1.0);
    assert!(app.o.include);
}

/// The wheel zoom eases (#89): a notch sets the target, the drawn view moves
/// there by the clock with the pointer's frame pixel under it all the way;
/// a notch on the way goes on from where it is (no jump); at rest it is the
/// stepped zoom's view (the same crops) and the frame clock stops. Reduced
/// motion: at once.
#[test]
fn the_wheel_zoom_eases_to_its_target_and_stops() {
    let mut app = open_app(3840, 2160, 2.0);
    settle_bar(&mut app);
    let still = app.o.view;
    let at = Point::new(600.0, 300.0);
    let under = still.to_image(at);
    event(&mut app, overlay::Event::Zoom { at, notches: 2.0 });
    assert_eq!(app.o.view, still, "nothing moves before the next frame");
    assert!(app.animating());
    let t0 = Instant::now();
    let mut last = still.zoom();
    for ms in [0, 30, 60, 90] {
        let _ = app.update(Message::Frame(t0 + Duration::from_millis(ms)));
        let v = app.o.view;
        assert!(
            v.to_image(at).distance(under) < 0.5,
            "{ms} ms: {:?}",
            v.to_image(at)
        );
        assert!(v.zoom() >= last, "{ms} ms: only ever in");
        last = v.zoom();
    }
    assert!(
        last > 1.0 && last < still.zoomed(at, 2.0).zoom() - 0.01,
        "on its way: {last}"
    );
    // a notch on the way: on from where it is (the next frame starts there)
    let shown = app.o.view;
    event(&mut app, overlay::Event::Zoom { at, notches: 1.0 });
    let _ = app.update(Message::Frame(t0 + Duration::from_millis(100)));
    assert_eq!(app.o.view, shown, "no jump");
    let _ = app.update(Message::Frame(
        t0 + Duration::from_millis(100) + motion::GLIDE,
    ));
    // at rest: what the stepped zoom shows, the clock off
    assert_eq!(app.o.view, still.zoomed(at, 3.0));
    assert_eq!(app.o.view.crop(), still.zoomed(at, 3.0).crop());
    assert!(app.o.zoom.is_none() && !app.animating());
    // reduced motion: there at once
    motion::set_reduced(true);
    event(&mut app, overlay::Event::ZoomReset);
    assert_eq!(app.o.view, still);
    assert!(app.o.zoom.is_none());
}

/// The hint at the top of the overlay is a kit pill on the bar's surface and
/// text, in every theme (it was the old palette's black pill): the overlay
/// with its hint and bar in Sapphire and Daylight.
#[test]
fn the_overlay_hint_matches_the_bar_in_every_theme() {
    use crate::shared::kit::testing::golden;
    golden::each_theme(|theme, failed| {
        let mut app = open_app(720, 260, 1.0);
        settle_bar(&mut app);
        let file = format!("overlay-hint-{}", theme.id);
        failed.extend(golden::check(
            "screens",
            &file,
            app.o.screen,
            &[1.0],
            || app.view(app.overlay),
        ));
    });
}

// --- The pinned AI chat (owner, 2026-10-28) ---

/// Ask `question` in the chat `id` and answer it (an API provider that is
/// never called: the answer is fed in, as its thread would send it).
fn chat_turn(app: &mut App, id: u32, question: &str, answer: &str) {
    let _ = app.update(Message::AiChat(
        id,
        ai_chat::Msg::Edit(iced::widget::text_editor::Action::SelectAll),
    ));
    let _ = app.update(Message::AiChat(
        id,
        crate::features::ask_ai::screens::ai_chat::tests::typed(question),
    ));
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Submit));
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Delta(answer.into())));
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Answered(Ok(()))));
}

fn api_ready(app: &mut App, id: u32, png: &[u8]) {
    let mut setup =
        crate::features::ask_ai::ask::tests::local_setup("http://127.0.0.1:9/v1".into());
    setup.png = png.to_vec();
    let _ = app.update(Message::AiChat(
        id,
        ai_chat::Msg::Ready(Ok(Arc::new(setup))),
    ));
}

/// The overlay opened, a selection made, a chat opened and asked once (not pinned).
fn chat_asked() -> (App, u32) {
    let mut app = open_app(800, 600, 1.0);
    drag(
        &mut app,
        Point::new(50.0, 50.0),
        Point::new(150.0, 120.0),
        false,
    );
    let crop = app.crop().unwrap();
    drop(app.open_chat(crop, None));
    let id = app.o.ai_chat.as_ref().unwrap().0;
    api_ready(&mut app, id, b"first");
    chat_turn(&mut app, id, "What is this?", "A chart.");
    (app, id)
}

/// The chat's own window, if it is open.
fn chat_window(app: &App) -> Option<window::Id> {
    app.chat_win.as_ref().map(|w| w.id)
}

/// What the window `id` draws, as text.
fn texts_of(app: &App, id: window::Id) -> Vec<String> {
    crate::shared::kit::testing::shot::texts(app.view(id))
}

/// The follow-up that pins the chat ends the tool by itself (no Esc): the
/// pinned chat goes on in the SAME window it had from its first frame. The
/// first question, the header's pin and a drag in progress do not close it (#170).
#[test]
fn the_follow_up_that_pins_the_chat_closes_the_overlay() {
    let (mut app, id) = chat_asked();
    assert!(app.o.is_some() && app.chat.is_none(), "one question: open");
    let window = chat_window(&app).expect("its own window from its first frame");
    chat_turn(&mut app, id, "And the trend?", "Up.");
    assert!(app.o.is_none(), "the follow-up closed the overlay, at once");
    assert_eq!(chat_window(&app), Some(window), "the same window");
    let chat = app.chat.as_ref().expect("the chat stays");
    assert_eq!(chat.panel.chat().unwrap().turns.len(), 2, "both answers");

    // the header's pin pins by hand: the overlay stays, a later question too
    let (mut app, id) = chat_asked();
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin));
    chat_turn(&mut app, id, "And the trend?", "Up.");
    assert!(app.o.is_some() && app.chat.is_some(), "pinned by hand");

    // a drag in progress is never cut short
    let (mut app, id) = chat_asked();
    app.o.drawing = true;
    chat_turn(&mut app, id, "And the trend?", "Up.");
    assert!(app.o.is_some() && app.chat.is_some(), "mid-drag: open");
}

/// The chat is never drawn on the overlay where it has its own window: the
/// overlay leaves it out and the chat's window draws it, quick or pinned; the
/// overlay still lays its bar out beside it. A chat closed (a click outside,
/// Esc) takes its window with it.
#[test]
fn the_chat_is_drawn_by_its_own_window_only() {
    let (mut app, id) = chat_asked();
    let window = chat_window(&app).unwrap();
    assert!(
        windows::is_floating(window),
        "a floating window (cut, on top)"
    );
    let asked = |texts: Vec<String>| texts.iter().any(|t| t == "What is this?");
    assert!(asked(texts_of(&app, window)), "its window draws it");
    assert!(!asked(texts_of(&app, app.overlay)), "the overlay does not");
    let chat = app.chat_rect();
    // its window sits where the overlay docks it (physical px)
    let placed = app.chat_win.as_ref().unwrap().placed.unwrap();
    assert_eq!(placed.origin, (chat.x as i32, chat.y as i32));
    assert_eq!(placed.px, (chat.width as u32, chat.height as u32));
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin));
    assert!(
        !asked(texts_of(&app, app.overlay)),
        "pinned: not drawn either"
    );
    assert!(asked(texts_of(&app, window)));
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin)); // unpin = close
    assert!(
        app.chat.is_none() && chat_window(&app).is_none(),
        "gone with it"
    );

    // a quick chat closed by a click outside: its window goes too
    let (mut app, _) = chat_asked();
    let press = overlay::Event::Press {
        at: Point::new(600.0, 500.0),
        shift: false,
    };
    let _ = app.update(Message::Overlay(press));
    let _ = app.update(Message::Overlay(overlay::Event::Release));
    assert!(app.o.ai_chat.is_none() && chat_window(&app).is_none());
    // and by Esc
    let (mut app, _) = chat_asked();
    let _ = app.update(Message::Escape);
    assert!(app.o.is_none() && chat_window(&app).is_none());
}

/// A quick question closes on a click outside; a follow-up pins the chat: it
/// survives clicks, the overlay closing (the same window, never re-created)
/// and the next capture: the chat stays put above the new overlay, and the
/// bar's question goes into the SAME chat with its crop, closing the overlay
/// (C4c). One chat only; its pin closes it.
#[test]
fn a_pinned_chat_outlives_the_overlay_and_takes_the_next_capture() {
    let (mut app, id) = chat_asked();
    let window = chat_window(&app).unwrap();
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin)); // (a follow-up would close the overlay)
    chat_turn(&mut app, id, "And the trend?", "Up.");
    assert!(
        app.o.ai_chat.is_none() && app.chat.is_some(),
        "the follow-up pinned it"
    );
    // clicks outside no longer close it
    drag(
        &mut app,
        Point::new(300.0, 300.0),
        Point::new(400.0, 380.0),
        false,
    );
    assert!(app.chat.is_some());
    let placed = app.chat_win.as_ref().unwrap().placed;
    // the overlay closes: nothing about the chat changes
    let _ = app.update(Message::Escape);
    assert!(app.o.is_none(), "Esc closed the overlay at once");
    let chat = app.chat_win.as_ref().expect("its window");
    assert_eq!(chat.id, window, "the same window");
    assert!(!chat.hidden && chat.placed == placed, "where it was, shown");
    assert!(texts_of(&app, window).iter().any(|t| t == "And the trend?"));
    // its composer works while the overlay is closed (messages reach it)
    let _ = app.update(Message::AiChat(
        id,
        ai_chat::Msg::Edit(iced::widget::text_editor::Action::SelectAll),
    ));

    // the next capture: the overlay opens round it; it stays put (not moved,
    // not drawn on the overlay, out of the frame)
    reopen(&mut app, 800, 600, 1.0);
    let _ = app.update(Message::Frame(Instant::now()));
    let chat = app.chat_win.as_ref().unwrap();
    assert_eq!((chat.id, chat.placed), (window, placed), "stays put");
    assert_eq!(app.o.chat_fixed, Some(app.chat_rect()));
    assert_eq!(app.o.chat_hole, Some(app.chat_rect()), "out of the frame");
    drag(
        &mut app,
        Point::new(400.0, 300.0),
        Point::new(432.0, 316.0),
        false,
    );
    let new_crop = app.crop().unwrap();
    let size = (new_crop.width, new_crop.height);
    app.o.ask = "Is this the same?".into();
    drop(app.update(Message::AskSubmit));
    assert!(app.o.is_none(), "the question closed the overlay (C4c)");
    assert_eq!(chat_window(&app), Some(window), "the same window");
    api_ready(&mut app, id, b"second");
    let turns = &app.chat.as_ref().unwrap().panel.chat().unwrap().turns;
    assert_eq!(turns.len(), 3, "the same chat");
    assert_eq!(turns[2].shot, Some(size), "with the new crop");
    assert_eq!(turns[2].image.as_deref(), Some(&b"second"[..]));
    let texts = texts_of(&app, window);
    assert!(texts.iter().any(|t| t == "Is this the same?"), "{texts:?}");
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Answered(Ok(()))));
    // the pin closes it, and its window
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin));
    assert!(app.chat.is_none() && chat_window(&app).is_none());
}

/// A pinned chat found on the overlay, picked up by its grip, is docked by the
/// overlay from the very spot it was (no jump), then follows the drag.
#[test]
fn a_pinned_chat_picked_up_on_the_overlay_starts_where_it_is() {
    let (mut app, id) = chat_asked();
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin));
    let _ = app.update(Message::Escape);
    // the OS dragged its window somewhere else, then the next capture
    let window = chat_window(&app).unwrap();
    let w = app.chat_win.as_mut().unwrap();
    w.at.x = 300.0;
    reopen(&mut app, 800, 600, 1.0);
    let found = app.chat_rect();
    assert_eq!(found.x, 300.0);
    let grab = ai_chat::Msg::Card(card::Event::Grab(card::Drag::Move));
    let _ = app.update(Message::AiChat(id, grab));
    assert!(app.o.chat_fixed.is_none(), "the overlay docks it now");
    assert_eq!(app.chat_rect(), found, "from where it was");
    let _ = app.update(Message::CardMovedOn(window, Point::new(10.0, 10.0)));
    let _ = app.update(Message::CardMovedOn(window, Point::new(-40.0, 10.0)));
    assert_eq!(app.chat_rect().x, found.x - 50.0, "it follows the pointer");
    let placed = app.chat_win.as_ref().unwrap().placed.unwrap();
    assert_eq!(placed.origin.0, (found.x - 50.0) as i32, "its window too");
}

/// Snip over a chat: its window hides (Snip's frozen screen holds its picture,
/// a pick like any window; nothing of ours covers the outline) and shows again,
/// the same window, when Snip is over.
#[test]
fn snip_hides_the_chats_window_until_it_is_over() {
    let (mut app, id) = chat_asked();
    let _ = app.update(Message::AiChat(id, ai_chat::Msg::Pin));
    let window = chat_window(&app).unwrap();
    let pick = snip::Pick::window(app.chat_rect(), "OpenCircle chat", 1);
    app.o.purpose = Purpose::Snip(Box::new(Snip::new(vec![pick], app.place(), true, None)));
    let _ = app.update(Message::Frame(Instant::now()));
    assert!(app.chat_win.as_ref().unwrap().hidden, "hidden in Snip");
    let texts = texts_of(&app, app.overlay);
    assert!(
        !texts.iter().any(|t| t == "What is this?"),
        "not drawn on Snip"
    );
    let _ = app.update(Message::Escape);
    let chat = app.chat_win.as_ref().unwrap();
    assert!(chat.id == window && !chat.hidden, "back, the same window");
}

/// The pinned chat's window is kept out of captures: where it was, the frame
/// may be black. That part is not judged; a black app elsewhere still is.
#[test]
fn the_pinned_chats_black_spot_is_no_protected_app() {
    let (w, h) = (600u32, 360u32);
    let chat = Rectangle::new(Point::new(400.0, 0.0), Size::new(200.0, 360.0));
    let mut rgba = [90, 90, 90, 255].repeat((w * h) as usize);
    for y in 0..h {
        for x in 400..w {
            rgba[((y * w + x) * 4) as usize..][..3].fill(0);
        }
    }
    let frame = Crop {
        width: w,
        height: h,
        rgba,
    };
    let (mut app, _) = selected_app(&frame);
    app.o.chat_hole = Some(chat);
    // a selection over the chat's black spot and a sliver of grey (98.5 %
    // black): no pill
    let area = Rectangle::new(Point::new(397.0, 50.0), Size::new(203.0, 200.0));
    app.o.selection = Some(overlay::Selection {
        area,
        kind: overlay::Kind::Drag,
    });
    assert!(
        qr::scan(&app.crop().unwrap()).protected,
        "judged whole, it looks protected"
    );
    app.o.selection = Some(overlay::Selection {
        area,
        kind: overlay::Kind::Drag,
    });
    let scan = qr::scan_around(&app.crop().unwrap(), app.chat_hole(), &[[0, 0, w, h]]);
    assert!(!scan.protected, "only our own chat was there");
    // wholly under it: judged as usual (black)
    let area = Rectangle::new(Point::new(450.0, 50.0), Size::new(100.0, 100.0));
    app.o.selection = Some(overlay::Selection {
        area,
        kind: overlay::Kind::Drag,
    });
    assert!(qr::scan_around(&app.crop().unwrap(), app.chat_hole(), &[[0, 0, w, h]]).protected);
    // a real black window elsewhere still says so
    let black = Crop {
        width: w,
        height: h,
        rgba: [0, 0, 0, 255].repeat((w * h) as usize),
    };
    let (mut app, area) = selected_app(&black);
    app.o.chat_hole = Some(chat);
    let _ = app.update(Message::Scanned(
        area,
        qr::scan_around(&app.crop().unwrap(), app.chat_hole(), &[[0, 0, w, h]]),
    ));
    assert!(overlay_texts(&app).contains(&protected().to_string()));
}

/// What a real key press (Ctrl held) produces on the open Snip editor's view.
fn ctrl_key_after(app: &App, c: char, physical: iced::keyboard::key::Code) -> Vec<Message> {
    use iced::advanced::clipboard;
    use iced::keyboard::{Key, Location, Modifiers, key};
    use iced_runtime::user_interface::{Cache, UserInterface};
    let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
    let mut ui = UserInterface::build(
        app.view(app.overlay),
        app.o.screen,
        Cache::default(),
        &mut renderer,
    );
    let letter = Key::Character(c.to_string().into());
    let press = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: letter.clone(),
        modified_key: letter,
        physical_key: key::Physical::Code(physical),
        location: Location::Standard,
        modifiers: Modifiers::CTRL,
        text: None,
        repeat: false,
    });
    let mut messages = Vec::new();
    ui.update(
        &[press],
        iced::mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    messages
}

#[test]
fn ctrl_z_reaches_the_snip_toolbar() {
    use crate::snip::annotate::Message as E;
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = snipping(area);
    app.select(area);
    let crop = app.crop().unwrap();
    let _ = app.snip_routed(crop, true, false);
    let keys = ctrl_key_after(&app, 'z', keyboard::key::Code::KeyZ);
    assert!(
        keys.iter().any(|m| matches!(m, Message::Annotate(E::Undo))),
        "Ctrl+Z -> Undo, got {keys:?}"
    );
}

/// Wayland's long capture (owner, 2026-11-10): the overlay can't be cut, so
/// it goes at once and the area is streamed; the pop-up shows beside the
/// area from the start (never over it), updates as the page grows, and the
/// tray shows it is capturing.
fn casting(area: Rectangle) -> App {
    casting_on(800, 600, area)
}

fn casting_on(w: u32, h: u32, area: Rectangle) -> App {
    let mut app = snipping_on(w, h, area);
    app.can_hide = false;
    app.select(area);
    let _ = app.open_snip_toolbar();
    let _ = app.update(Message::Annotate(annotate::Message::Scroll));
    app
}

fn cast_frames() -> Vec<crate::ui::long_capture_popup::Frame> {
    use crate::ui::long_capture_popup::{Msg, tests::sent};
    (sent(false).into_iter())
        .filter_map(|m| match m {
            Msg::Frame(f) => Some(f),
            Msg::End(_) => None,
        })
        .collect()
}

fn stitched_page(height: u32) -> Crop {
    Crop {
        width: 300,
        height,
        rgba: vec![255; 300 * height as usize * 4],
    }
}

#[test]
fn on_wayland_a_long_capture_shows_its_pop_up_beside_the_area_and_the_tray_state() {
    use crate::shared::platform::capture::scroll::Update;
    use crate::shared::platform::tray::SHOWN;
    SHOWN.with_borrow_mut(Vec::clear);
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = casting(area);
    assert!(app.o.is_none(), "the overlay went at once");
    assert!(app.cast.as_ref().is_some_and(|c| c.popup.is_some()));
    let first = cast_frames();
    let f = first.last().expect("the pop-up shows from the start");
    assert!(!f.window.intersects(&area), "never over the area: {f:?}");
    assert_eq!(
        (f.window.x, f.window.y),
        (412.0, 80.0),
        "right beside it, top-aligned"
    );
    assert!(!f.left && f.picture.is_none());
    assert_eq!(
        SHOWN.with_borrow(|s| s.last().cloned()),
        Some(Some("● 0 px".into()))
    );
    // the page grows: the picture and the count follow, still beside the area
    let preview = Crop {
        width: 120,
        height: 300,
        rgba: vec![0; 120 * 300 * 4],
    };
    let _ = app.update(Message::Snip(snip::Msg::Progress(Update {
        height: 1500,
        full: false,
        lost: false,
        preview: Some(preview),
    })));
    let f = cast_frames().pop().expect("a new frame");
    let (picture, size) = f.picture.as_ref().expect("the picture");
    assert_eq!(
        (picture.width, picture.height, size.width),
        (120, 300, 120.0)
    );
    assert_eq!(f.count, (1500, false, false));
    assert!(f.window.height > 300.0 && !f.window.intersects(&area));
    assert_eq!(
        SHOWN.with_borrow(|s| s.last().cloned()),
        Some(Some("● 0 px".into())),
        "the tray's label changes at most twice a second (it may be in the stream)"
    );
    std::thread::sleep(Duration::from_millis(510));
    let _ = app.update(Message::Snip(snip::Msg::Progress(Update {
        height: 1600,
        full: false,
        lost: false,
        preview: None,
    })));
    assert_eq!(
        SHOWN.with_borrow(|s| s.last().cloned()),
        Some(Some("● 1,600 px".into()))
    );
}

/// An area that fills the screen (a click in Snip takes the screen on
/// Wayland; owner 2026-11-10: "no actual effect", there was no pop-up): the
/// pop-up still shows, inside the area's top-right corner, only once the
/// first frame is read (it is then in the stream: the stitcher leaves its
/// corner out), with the whole-screen tip until the page grows.
#[test]
fn on_wayland_a_whole_screen_long_capture_shows_its_pop_up_in_the_corner() {
    use crate::shared::platform::capture::scroll::Update;
    let area = Rectangle::new(Point::new(0.0, 0.0), Size::new(1280.0, 720.0));
    let mut app = casting_on(1280, 720, area);
    assert!(
        app.cast.as_ref().is_some_and(|c| c.popup.is_some()),
        "a pop-up"
    );
    assert!(
        cast_frames().is_empty(),
        "not before the first frame is read"
    );
    let progress = |height| {
        Message::Snip(snip::Msg::Progress(Update {
            height,
            full: false,
            lost: false,
            preview: Some(Crop {
                width: 100,
                height: 60,
                rgba: vec![0; 100 * 60 * 4],
            }),
        }))
    };
    let _ = app.update(progress(720));
    let f = cast_frames().pop().expect("shown with the first frame");
    assert!(f.left && f.tip, "{f:?}");
    assert_eq!(
        f.window.x + f.window.width,
        1280.0 - 12.0,
        "against the corner"
    );
    assert!(
        f.inside && f.window.y + f.window.height < 200.0,
        "only the top rows"
    );
    let _ = app.update(progress(1500));
    let f = cast_frames().pop().expect("a new frame");
    assert!(!f.tip, "the tip goes once the page grows");
}

/// Copy from the pop-up: the stitched page is copied at once (no Snip), the
/// pop-up says so and goes, the tray is itself again.
#[test]
fn on_wayland_copy_from_the_pop_up_copies_the_page() {
    use crate::shared::platform::capture::scroll::Finish;
    use crate::shared::platform::tray::SHOWN;
    use crate::ui::long_capture_popup::{Msg, tests::sent};
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = casting(area);
    let _ = app.update(Message::Trigger(Trigger::LongCapture(Finish::Copy)));
    assert!(
        app.cast.as_ref().is_some_and(|c| c.stopped()),
        "the stream stops"
    );
    let _ = app.update(Message::Snip(snip::Msg::Done(Ok(stitched_page(900)))));
    assert!(
        app.cast.is_none() && app.capturing.is_none(),
        "no Snip opens"
    );
    let end = sent(true).into_iter().find_map(|m| match m {
        Msg::End(said) => Some(said),
        Msg::Frame(_) => None,
    });
    assert!(end.is_some(), "the pop-up says how it went");
    assert_eq!(SHOWN.with_borrow(|s| s.last().cloned()), Some(None));
}

/// Done (the pop-up, the tray's Finish) or any shortcut: Snip opens on a
/// fresh frame with the stitched page picked; Cancel drops the page.
#[test]
fn on_wayland_done_opens_snip_with_the_page_and_cancel_drops_it() {
    use crate::shared::platform::capture::scroll::Finish;
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = casting(area);
    let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
    assert!(app.cast.as_ref().is_some_and(|c| c.stopped()));
    assert!(app.capturing.is_none(), "the image comes first");
    // a later Cancel doesn't undo the first end asked for
    let _ = app.update(Message::Trigger(Trigger::LongCapture(Finish::Cancel)));
    let _ = app.update(Message::Snip(snip::Msg::Done(Ok(stitched_page(900)))));
    assert!(app.capturing.is_some(), "Snip opens on a fresh frame");
    assert!(
        (app.picked_next.as_ref()).is_some_and(|(step, c)| step.is_none() && c.height == 900),
        "with the stitched page picked"
    );
    let mut app = casting(area);
    let _ = app.update(Message::Trigger(Trigger::LongCapture(Finish::Cancel)));
    let _ = app.update(Message::Snip(snip::Msg::Done(Ok(stitched_page(900)))));
    assert!(app.cast.is_none() && app.capturing.is_none() && app.picked_next.is_none());
}

/// Without XWayland there is no pop-up: the tray's capturing state is the
/// sign and its menu finishes it.
#[test]
fn on_wayland_without_xwayland_the_tray_alone_shows_the_capture() {
    use crate::shared::platform::capture::scroll::Finish;
    use crate::shared::platform::tray::SHOWN;
    use crate::ui::long_capture_popup::tests::NO_X11;
    NO_X11.set(true);
    let area = Rectangle::new(Point::new(100.0, 80.0), Size::new(300.0, 200.0));
    let mut app = casting(area);
    NO_X11.set(false);
    assert!(app.cast.as_ref().is_some_and(|c| c.popup.is_none()));
    assert_eq!(
        SHOWN.with_borrow(|s| s.last().cloned()),
        Some(Some("● 0 px".into()))
    );
    let _ = app.update(Message::Trigger(Trigger::LongCapture(Finish::Done)));
    let _ = app.update(Message::Snip(snip::Msg::Done(Ok(stitched_page(700)))));
    assert!(app.picked_next.is_some(), "Snip opens with the page");
    assert_eq!(SHOWN.with_borrow(|s| s.last().cloned()), Some(None));
}

/// Snip the translated page (owner, 2026-11-10): over a translation, every
/// Snip entry (Alt+A / tray, the bar's Snip key) takes the page as translated,
/// full size, with no windows outlined; without one, nothing changes.
#[test]
fn snip_over_a_translation_takes_the_translated_page() {
    let snip = QuickAction {
        action: "snip".into(),
        shortcut: String::new(),
        target: Target::Circle,
        live: false,
    };
    let translated = |app: &mut App| {
        let frame = app.o.frame.clone().unwrap();
        let (mut session, _) =
            translate::Session::start(frame, [0, 0, 40, 20], 1.0, app.cache.clone(), false, 0);
        let page = [9, 9, 9, 255].repeat(40 * 20); // opaque (the editor premultiplies)
        session.show(image::Handle::from_rgba(40, 20, page));
        app.o.translate = Some((1, session));
        app.o.windows = vec![snip::Pick {
            rect: Rectangle::new(Point::new(2.0, 2.0), Size::new(10.0, 10.0)),
            label: "Window: app".into(),
            window: 5,
        }];
    };
    let key = actions::snip::ACTION.key;
    for entry in ["shortcut", "key", "pill button"] {
        let mut app = open_app(40, 20, 1.0);
        translated(&mut app);
        match entry {
            "shortcut" => {
                let _ = app.update(Message::Trigger(Trigger::Quick(
                    Instant::now(),
                    snip.clone(),
                )));
            }
            "pill button" => {
                let _ = app.on_translate(1, translate::Msg::Snip);
            }
            _ => event(
                &mut app,
                overlay::Event::Key {
                    letter: key.letter,
                    ctrl: key.ctrl,
                },
            ),
        }
        let Purpose::Snip(s) = &app.o.purpose else {
            panic!("{entry}: Snip opened");
        };
        assert!(
            s.outlined().is_none() && app.o.translate.is_none(),
            "{entry}"
        );
        event(&mut app, overlay::Event::Enter); // the screen
        let crop = app.crop().expect("picked");
        assert_eq!((crop.width, crop.height), (40, 20), "{entry}: full size");
        let page = [9, 9, 9, 255].repeat(40 * 20);
        assert!(crop.rgba == page, "{entry}: the translated page");
    }
    // no translation: the shortcut only brings the overlay to the front
    let mut app = open_app(40, 20, 1.0);
    let _ = app.update(Message::Trigger(Trigger::Quick(Instant::now(), snip)));
    assert!(matches!(app.o.purpose, Purpose::Search));
}

/// The second reader (`text::needs_no_second_reader`).
mod second_reader {
    use super::super::text::needs_no_second_reader;
    use crate::ocr::system::read_well;

    #[test]
    fn an_english_page_sends_no_line_to_the_second_reader() {
        let page = [
            "Lighthouse",
            "Main page",
            "A lighthouse is a tower or other structure",
            "Article Talk Read Edit History",
        ];
        assert!(needs_no_second_reader(&read_well(&page, Some("en-US"))));
    }

    #[test]
    fn arabic_noise_or_an_empty_read_still_reach_the_second_reader() {
        let mixed = [
            "Main page",
            "awgall n",
            "oscluall",
            "A tower or other structure",
        ];
        assert!(!needs_no_second_reader(&read_well(&mixed, Some("en-US"))));
        assert!(!needs_no_second_reader(&[]));
    }
}
