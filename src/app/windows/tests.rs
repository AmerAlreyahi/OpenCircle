use super::*;
use crate::shared::kit::motion;
use crate::shared::kit::testing::shot;
use crate::shared::secrets::{AiKey, Secret, SecretsError};

struct NoKeys;
impl crate::shared::secrets::KeyStore for NoKeys {
    fn is_set(&self, _: AiKey) -> Result<bool, SecretsError> {
        Ok(false)
    }
    fn read(&self, _: AiKey) -> Result<Option<Secret>, SecretsError> {
        Ok(None)
    }
    fn set(&self, _: AiKey, _: &Secret) -> Result<(), SecretsError> {
        Ok(())
    }
    fn delete(&self, _: AiKey) -> Result<(), SecretsError> {
        Ok(())
    }
}

/// No tray: a window opens (Settings; the onboarding while it is open, never both).
#[test]
fn with_no_tray_a_window_opens() {
    let mut windows = Windows::default();
    let _ = windows.open_main();
    assert!(windows.settings.is_some() && windows.onboarding.is_none());
    let mut first_run = Windows::default();
    let _ = first_run.open(Kind::Onboarding);
    let _ = first_run.open_main();
    assert!(first_run.onboarding.is_some() && first_run.settings.is_none());
}

fn settings_state() -> settings::State {
    settings::State::new(Config::default(), vec![], None, Box::new(NoKeys))
}

/// While a shortcut field records, the app's own shortcuts are held off
/// (Ctrl+Shift+X opened the overlay instead of being recorded); back on
/// when it stops: Esc, a key set, or the window closed mid-recording.
#[test]
fn global_shortcuts_are_held_off_while_a_shortcut_field_records() {
    use settings::{Chord, Slot};
    let key = |key: &str, ctrl: bool| {
        let chord = Chord {
            ctrl,
            alt: false,
            shift: ctrl,
            logo: false,
            key: key.into(),
        };
        Msg::Settings(settings::Message::Key(chord))
    };
    let record = Msg::Settings(settings::Message::Record(Slot::Capture));
    let mut windows = Windows::default();
    let id = window::Id::unique();
    windows.settings = Some((id, settings_state()));
    assert!(!crate::shared::platform::hotkey::suspended());
    let _ = windows.update(record.clone());
    assert!(
        crate::shared::platform::hotkey::suspended(),
        "recording: held off"
    );
    let _ = windows.update(key("Escape", false));
    assert!(
        !crate::shared::platform::hotkey::suspended(),
        "Esc: back on"
    );
    let _ = windows.update(record.clone());
    let _ = windows.update(key("KeyK", true));
    assert!(
        !crate::shared::platform::hotkey::suspended(),
        "a key set: back on"
    );
    let _ = windows.update(record);
    assert!(crate::shared::platform::hotkey::suspended());
    let _ = windows.update(Msg::Closed(id));
    assert!(
        !crate::shared::platform::hotkey::suspended(),
        "the window closed: back on"
    );
}

#[test]
fn a_saved_config_takes_effect_even_when_the_settings_window_is_gone() {
    let dir = std::env::temp_dir().join(format!("oc-win-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut state = settings::State::new(
        Config::default(),
        vec![],
        Some(dir.join("config.toml")),
        Box::new(NoKeys),
    );
    settings::update(
        &mut state,
        settings::Message::Theme(crate::shared::config::Theme::Dark),
    );
    let settings::Outcome::Write(w) = settings::update(&mut state, settings::Message::Save) else {
        panic!("no write")
    };
    let done = w.run();
    drop(state); // the window closed while the write ran
    let mut windows = Windows::default();
    let _ = windows.update(Msg::Settings(settings::Message::Written(done)));
    assert_eq!(config::current().theme, crate::shared::config::Theme::Dark);
    config::set_current(Config::default());
    theme::apply(crate::shared::config::Theme::System);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Closing Settings mid-preview drops its state: the clock (its
/// subscription) stops and the preview's memory is freed.
#[test]
fn closing_settings_stops_a_playing_preview() {
    let mut windows = Windows::default();
    let id = window::Id::unique();
    windows.settings = Some((id, settings_state()));
    let section = settings::Message::Section(settings::Section::Translate);
    let _ = windows.update(Msg::Settings(section));
    assert!(
        windows
            .settings
            .as_ref()
            .is_some_and(|(_, s)| s.animating())
    );
    let _ = windows.update(Msg::Closed(id));
    assert!(windows.settings.is_none());
}

#[test]
fn one_settings_window_at_a_time() {
    let mut windows = Windows::default();
    assert_eq!(windows.id(Kind::Settings), None);
    let id = window::Id::unique();
    windows.settings = Some((id, settings_state()));
    // A second request focuses the open window: same id, state kept.
    let _ = windows.open(Kind::Settings);
    assert_eq!(windows.id(Kind::Settings), Some(id));
    assert_eq!(windows.title(id), "OpenCircle Settings");
    assert!(windows.view(id).is_some());
    assert!(windows.view(window::Id::unique()).is_none());
    assert_eq!(windows.id(Kind::Onboarding), None);
}

#[test]
fn the_permission_prompt_closes_once_allowed() {
    let mut windows = Windows::default();
    let _ = windows.open(Kind::Permission);
    let id = windows.id(Kind::Permission).unwrap();
    assert_eq!(windows.title(id), "OpenCircle needs screenshot permission");
    assert!(windows.view(id).is_some());
    let _ = windows.update(Msg::Permission(onboarding::Message::Allow));
    let refused = Err(crate::shared::platform::capture::CaptureError::Refused(
        "refused by the desktop".into(),
    ));
    let _ = windows.update(Msg::Permission(onboarding::Message::Allowed(refused)));
    assert_eq!(windows.id(Kind::Permission), Some(id), "stays to try again");
    let _ = windows.update(Msg::Permission(onboarding::Message::Allow));
    let _ = windows.update(Msg::Permission(onboarding::Message::Allowed(Ok(()))));
    assert_eq!(windows.id(Kind::Permission), None);
}

#[test]
fn closing_frees_the_state() {
    let mut windows = Windows::default();
    let (id, other) = (window::Id::unique(), window::Id::unique());
    windows.settings = Some((id, settings_state()));
    let _ = windows.update(Msg::Closed(other)); // the overlay or a stranger
    assert_eq!(windows.id(Kind::Settings), Some(id));
    let _ = windows.update(Msg::Closed(id));
    assert_eq!(windows.id(Kind::Settings), None);
    assert!(windows.view(id).is_none());
}

/// A `w`x`h` picture with a diagonal gradient and a frame, so a render shows it.
fn picture(w: u32, h: u32) -> Crop {
    let rgba = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let edge = x < 6 || y < 6 || x >= w - 6 || y >= h - 6;
            if edge {
                [255, 200, 40, 255]
            } else {
                [(x * 255 / w) as u8, (y * 255 / h) as u8, 160, 255]
            }
        })
        .collect();
    Crop {
        width: w,
        height: h,
        rgba,
    }
}

/// Our floating windows with a strip, a pin, a pill and the update card open.
fn floating_windows() -> (Windows, window::Id, window::Id) {
    let mut windows = Windows::default();
    let _ = windows.pin(picture(200, 100), None, 1.0);
    let monitor = platform::monitor::Place {
        origin: (0, 0),
        px: (1920, 1080),
        scale: 1.0,
    };
    let strip = window::Id::unique();
    windows.steps = Some((Some(strip), Strip::new(monitor, 0.0, false)));
    let _ = windows.toast("Copied", true, None, 1000);
    let card = update_card::Card::preview("9.9.9", "notes", "https://example.invalid");
    let _ = windows.update_card(card);
    let pill = update_pill::Pill::new(update_pill::Kind::Updating);
    let _ = windows.update_pill(Some(pill));
    let pin = windows.pins[0].id;
    (windows, pin, strip)
}

/// Wayland can't hide a window: for a grab our floating windows close (their
/// state kept: an OS "closed" for them drops nothing) and open again after it,
/// the pill for good. Only for that capture's end, or any (`None`). The
/// update's card and pill stay: they show in captures (owner, 2026-11-10).
#[test]
fn where_windows_cant_hide_a_grab_closes_our_floating_ones_and_opens_them_again() {
    let (mut windows, pin, strip) = floating_windows();
    assert!(windows.leave_frame_as(7, false).is_some());
    assert!(windows.pins.is_empty() && windows.toasts.is_empty());
    assert!(windows.steps_window().is_none());
    assert!(windows.update_card.is_some() && windows.update_pill.is_some());
    let _ = windows.update(Msg::Closed(pin));
    let _ = windows.update(Msg::Closed(strip));
    assert!(windows.steps.is_some(), "the strip's steps stay");
    assert!(windows.leave_frame_as(8, false).is_none(), "out already");
    let _ = windows.back_in_frame(Some(7));
    assert!(windows.out.is_some(), "capture 8's now");
    let _ = windows.back_in_frame(Some(8));
    assert!(windows.out.is_none());
    let back = pin_ids(&windows);
    assert_eq!(back.len(), 1);
    assert!(
        back[0] != pin && windows.view(back[0]).is_some(),
        "a new window"
    );
    assert!(windows.steps_window().is_some_and(|id| id != strip));
    assert!(windows.update_card.is_some());
    assert!(windows.toasts.is_empty(), "a pill doesn't come back");
    assert!(windows.leave_frame_as(9, false).is_some(), "again");
    let _ = windows.back_in_frame(None);
    assert_eq!(pin_ids(&windows).len(), 1);
}

/// Where windows hide (X11) they hide for the grab, and show again as they
/// are; with none open the grab doesn't wait.
#[test]
fn where_windows_hide_a_grab_hides_our_floating_ones() {
    let (mut windows, pin, strip) = floating_windows();
    assert!(windows.leave_frame_as(1, true).is_some());
    let hidden = &windows.out.as_ref().unwrap().hidden;
    for id in [pin, strip] {
        assert!(hidden.contains(&id));
    }
    assert_eq!(pin_ids(&windows), [pin], "kept as it is");
    assert!(windows.toasts.is_empty(), "the pill went");
    let _ = windows.back_in_frame(Some(1));
    assert!(windows.out.is_none());
    assert_eq!(windows.steps_window(), Some(strip));
    assert!(Windows::default().leave_frame_as(2, true).is_none());
    assert!(Windows::default().leave_frame_as(2, false).is_none());
}

/// The update pill sits above the tray: the main screen's bottom-right
/// corner, 16 px in and 12 px over the taskbar, scaled with the screen.
/// Before: the ready toast opened in the screen's middle.
#[test]
fn the_update_pill_docks_above_the_tray() {
    let monitor = platform::monitor::Place {
        origin: (0, 0),
        px: (1920, 1080),
        scale: 1.0,
    };
    let p = pill_place(monitor, 40.0, Size::new(400.0, 64.0));
    assert_eq!(p.origin, (1920 - 16 - 400, 1080 - 40 - 12 - 64));
    let hidpi = platform::monitor::Place {
        scale: 2.0,
        px: (3840, 2160),
        ..monitor
    };
    let p = pill_place(hidpi, 40.0, Size::new(400.0, 64.0));
    assert_eq!(
        (p.origin, p.px),
        ((3840 - 32 - 800, 2160 - 104 - 128), (800, 128))
    );
    // Windows asks the pill's buttons of the App; closing it forgets it
    let mut windows = Windows::default();
    let _ = windows.update_pill(Some(update_pill::Pill::new(update_pill::Kind::Updating)));
    assert_eq!(windows.shown_pill(), Some(&update_pill::Kind::Updating));
    let id = windows.update_pill.as_ref().unwrap().0;
    let _ = windows.update(Msg::Closed(id));
    assert!(windows.shown_pill().is_none());
}

fn pin_ids(windows: &Windows) -> Vec<window::Id> {
    windows.pins.iter().map(|p| p.id).collect()
}

#[test]
fn several_pins_coexist_and_each_frees_its_image_on_close() {
    let mut windows = Windows::default();
    for n in 0..3 {
        let at = (100 * n as i32, 50);
        let _ = windows.pin(picture(200 + n * 10, 100), Some(at), 1.0);
    }
    let ids = pin_ids(&windows);
    assert_eq!(ids.len(), 3);
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    for id in &ids {
        assert!(windows.view(*id).is_some());
        assert_eq!(windows.title(*id), "OpenCircle pin");
    }
    assert_eq!(windows.pins[1].size(), Size::new(210.0, 100.0));
    // The pixels are held by the pin only.
    assert!(
        windows.pins[0]
            .crop()
            .is_some_and(|c| c.rgba.len() == 200 * 100 * 4)
    );

    // Esc / double-click / the close button: that pin goes, the others
    // stay. It fades out first: shown until its window has closed.
    let _ = windows.update(Msg::Pin(ids[1], PinMsg::Close));
    assert!(windows.view(ids[1]).is_some(), "fading out");
    let _ = windows.update(Msg::Closed(ids[1]));
    assert_eq!(pin_ids(&windows), [ids[0], ids[2]]);
    assert!(
        windows.view(ids[1]).is_none(),
        "its view and pixels are gone"
    );
    // The OS closing a window (Alt+F4) frees it too.
    let _ = windows.update(Msg::Closed(ids[0]));
    assert_eq!(pin_ids(&windows), [ids[2]]);
    // A key aimed at another window (or a closed pin) does nothing.
    let _ = windows.update(Msg::Pin(window::Id::unique(), PinMsg::Close));
    let _ = windows.update(Msg::Pin(ids[1], PinMsg::Close));
    assert_eq!(pin_ids(&windows), [ids[2]]);
    let _ = windows.update(Msg::Pin(ids[2], PinMsg::Close));
    let _ = windows.update(Msg::Closed(ids[2]));
    assert!(windows.pins.is_empty());
}

#[test]
fn a_pin_keeps_its_on_screen_size_and_big_ones_shrink_to_fit() {
    let mut windows = Windows::default();
    // 150 % scaling: 600x300 physical = 400x200 logical.
    let _ = windows.pin(picture(600, 300), None, 1.5);
    assert_eq!(windows.pins[0].size(), Size::new(400.0, 200.0));
    // A tall scrolling capture starts reduced to fit.
    let _ = windows.pin(picture(780, 6000), None, 1.0);
    let tall = windows.pins[1].size();
    assert!(
        tall.height <= PIN_MAX.height + 0.01 && tall.width < 780.0,
        "{tall:?}"
    );
    assert!(
        (tall.width / tall.height - 780.0 / 6000.0).abs() < 1e-4,
        "same shape"
    );
}

#[test]
fn scrolling_zooms_within_limits() {
    let lines = |y| mouse::ScrollDelta::Lines { x: 0.0, y };
    assert!(zoom_after(1.0, lines(1.0)) > 1.0);
    assert!(zoom_after(1.0, lines(-1.0)) < 1.0);
    assert!((zoom_after(zoom_after(1.0, lines(1.0)), lines(-1.0)) - 1.0).abs() < 1e-5);
    let pixels = mouse::ScrollDelta::Pixels {
        x: 0.0,
        y: PIXELS_PER_NOTCH,
    };
    assert!((zoom_after(1.0, pixels) - zoom_after(1.0, lines(1.0))).abs() < 1e-6);
    assert_eq!(zoom_after(7.9, lines(50.0)), 8.0, "upper limit");
    assert_eq!(zoom_after(0.11, lines(-50.0)), 0.1, "lower limit");

    let mut windows = Windows::default();
    let _ = windows.pin(picture(200, 100), None, 1.0);
    let id = windows.pins[0].id;
    let _ = windows.update(Msg::Pin(id, PinMsg::Zoom(lines(3.0))));
    let size = windows.pins[0].size();
    assert!(
        (size.width / 200.0 - 1.1_f32.powi(3)).abs() < 1e-4,
        "{size:?}"
    );
    assert!((size.width / size.height - 2.0).abs() < 1e-4, "shape kept");
}

#[test]
fn hover_shows_the_bar_only_when_it_fits() {
    let mut windows = Windows::default();
    let _ = windows.pin(picture(300, 200), None, 1.0);
    let _ = windows.pin(picture(60, 40), None, 1.0);
    let (big, small) = (windows.pins[0].id, windows.pins[1].id);
    let draw = |windows: &Windows, id, w, h| shot::render(windows.view(id).unwrap(), w, h);
    let (calm_big, calm_small) = (draw(&windows, big, 300, 200), draw(&windows, small, 60, 40));
    for id in [big, small] {
        let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
    }
    assert!(draw(&windows, big, 300, 200) != calm_big, "the bar shows");
    assert!(
        draw(&windows, small, 60, 40) == calm_small,
        "no room for it"
    );
}

#[test]
fn renders_a_pinned_image() {
    let mut windows = Windows::default();
    let _ = windows.pin(picture(420, 260), None, 1.0);
    let id = windows.pins[0].id;
    for (name, hover) in [("pin", false), ("pin-hover", true)] {
        let _ = windows.update(Msg::Pin(id, PinMsg::Hover(hover)));
        let px = shot::save(name, windows.view(id).unwrap(), Size::new(420.0, 260.0));
        // The picture fills the window: no window-colour background shows.
        let bg = crate::shared::kit::themes::c().bg;
        let bg = [bg.r, bg.g, bg.b].map(|c| (c * 255.0).round() as u8);
        let background = px.chunks(4).filter(|p| p[..3] == bg).count();
        assert!(background < 100, "{name}: {background} background pixels");
    }
    // Hover adds the bar: the two renders differ.
    let calm = shot::render(windows.view(id).unwrap(), 420, 260);
    let _ = windows.update(Msg::Pin(id, PinMsg::Hover(false)));
    let still = shot::render(windows.view(id).unwrap(), 420, 260);
    assert!(calm != still);
}

#[test]
fn the_pin_bar_adds_the_shortcuts_to_its_icons() {
    use crate::shared::kit::tooltip::tests_support::{assert_tips_add_something, shown};
    let mut windows = Windows::default();
    let _ = windows.pin(picture(300, 200), None, 1.0);
    let id = windows.pins[0].id;
    let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
    let _ = windows.view(id).unwrap();
    // Drag-out (Windows only) is the editor's Drag chip: its word says it, no tooltip.
    assert_eq!(shown(), ["Copy · Ctrl+C", "Save · Ctrl+S", "Close · Esc"]);
    assert_tips_add_something("pin bar");
}

/// A pin made live by hand (no window capture in tests): its source never runs.
fn live(windows: &mut Windows, w: u32, h: u32, scale: f32) -> window::Id {
    let _ = windows.open_pin(picture(w, h), None, scale);
    let pick = live::Pick {
        x: 0,
        y: 0,
        w,
        h,
        dpi: 96,
    };
    let (source, _never_run) = live::source(0, pick, |_| true);
    let pin = windows.pins.last_mut().unwrap();
    pin.live = Some(Live {
        _source: source,
        scale,
        opacity: 0,
        through: false,
    });
    pin.id
}

#[test]
fn a_live_pin_shows_each_frame_and_keeps_the_crops_shape() {
    let mut windows = Windows::default();
    let id = live(&mut windows, 300, 200, 1.5);
    assert_eq!(
        windows.pins[0].size(),
        Size::new(300.0, 200.0) * (1.0 / 1.5)
    );
    // a new frame of the same size: only the picture changes
    let mut frame = picture(300, 200);
    frame.rgba[0] = 7;
    let _ = windows.update(Msg::Pin(id, PinMsg::Live(live::Event::Frame(frame))));
    assert_eq!(windows.pins[0].crop().unwrap().rgba[0], 7);
    assert_eq!(windows.pins[0].base, Size::new(300.0, 200.0) * (1.0 / 1.5));
    // the source window got narrower (or changed DPI): the pin follows the crop
    let _ = windows.update(Msg::Pin(
        id,
        PinMsg::Live(live::Event::Frame(picture(150, 200))),
    ));
    assert_eq!(
        windows.pins[0].size(),
        Size::new(150.0, 200.0) * (1.0 / 1.5)
    );
    // a still pin ignores frames (a late one after the source failed)
    let _ = windows.update(Msg::Pin(id, PinMsg::Live(live::Event::Failed("x".into()))));
    assert!(windows.pins[0].live.is_none(), "a still pin from here");
    assert_eq!(
        windows.toasts(),
        [(
            "Can't follow this window: the pin stays still".into(),
            false
        )]
    );
    let _ = windows.update(Msg::Pin(
        id,
        PinMsg::Live(live::Event::Frame(picture(10, 10))),
    ));
    assert_eq!(windows.pins[0].crop().unwrap().width, 150);
}

#[test]
fn a_live_pin_closes_with_its_window_and_says_so() {
    let mut windows = Windows::default();
    let id = live(&mut windows, 300, 200, 1.0);
    let _ = windows.update(Msg::Pin(id, PinMsg::Live(live::Event::Gone)));
    assert_eq!(
        windows.toasts(),
        [("The pinned window closed".into(), false)]
    );
    // the pin goes once its window has (it fades out first): the capture stops with it
    let _ = windows.update(Msg::Closed(id));
    assert!(windows.pins.is_empty());
}

#[test]
fn a_live_pin_steps_its_opacity_and_lets_clicks_through_until_the_tray_says() {
    let mut windows = Windows::default();
    let id = live(&mut windows, 400, 200, 1.0);
    let still = windows.pin(picture(400, 200), None, 1.0);
    drop(still);
    let step = |w: &Windows| w.pins[0].live.as_ref().unwrap().opacity;
    let seen: Vec<f32> = (0..5)
        .map(|_| {
            let _ = windows.update(Msg::Pin(id, PinMsg::Opacity));
            OPACITY[step(&windows)].0
        })
        .collect();
    assert_eq!(seen, [0.8, 0.6, 0.4, 1.0, 0.8], "round the steps");
    // a still pin has neither control
    let other = windows.pins[1].id;
    let _ = windows.update(Msg::Pin(other, PinMsg::ClickThrough));
    assert!(!windows.pins_through());
    // click-through: the tray menu offers the way back, above Settings
    let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
    let _ = windows.update(Msg::Pin(id, PinMsg::ClickThrough));
    assert!(windows.pins_through() && !windows.pins[0].hover);
    let cfg = Config::default();
    let menu = tray_menu::Menu::new(&cfg.labs, &cfg, false);
    let rows = menu.clone().with_click_pins().entries();
    let at = rows
        .iter()
        .position(|e| *e == platform::tray::Entry::ClickPins);
    assert_eq!(at, Some(rows.len() - 3), "{rows:?}");
    assert!(!menu.entries().contains(&platform::tray::Entry::ClickPins));
    let _ = windows.clickable_pins();
    assert!(!windows.pins_through());
}

#[test]
fn live_pins_are_limited() {
    let mut windows = Windows::default();
    for _ in 0..MAX_LIVE {
        live(&mut windows, 100, 100, 1.0);
    }
    let _ = windows.live_pin(1, (0, 0, 10, 10), picture(10, 10), None, 1.0);
    assert_eq!(windows.pins.len(), MAX_LIVE, "no fifth");
    assert_eq!(
        windows.toasts(),
        [("Up to 4 live pins: close one first".into(), false)]
    );
}

#[test]
fn the_live_pin_bar_and_its_glyphs() {
    use crate::shared::kit::tooltip::tests_support::{assert_tips_add_something, shown};
    let script = include_str!("../../../assets/lucide/subset.sh");
    for glyph in [OPACITY_GLYPH, THROUGH] {
        let code = format!("U+{:04X}", glyph as u32);
        assert!(script.contains(&code), "add {code} to subset.sh");
    }
    let mut windows = Windows::default();
    let id = live(&mut windows, 420, 260, 1.0);
    let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
    let _ = shown();
    let _ = windows.view(id).unwrap();
    assert_eq!(
        shown(),
        [
            "Opacity: 100 %",
            "Let clicks pass through (back: tray menu)",
            "Copy · Ctrl+C",
            "Save · Ctrl+S",
            "Close · Esc"
        ]
    );
    assert_tips_add_something("live pin bar");
    // too narrow for its two more buttons: no bar (as a small still pin)
    let id = live(&mut windows, 260, 200, 1.0);
    let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
    let bare = shot::render(windows.view(id).unwrap(), 260, 200);
    let _ = windows.update(Msg::Pin(id, PinMsg::Hover(false)));
    assert_eq!(bare, shot::render(windows.view(id).unwrap(), 260, 200));
}

/// The live pin's hover bar, Sapphire and Daylight.
#[test]
fn the_live_pin_matches_its_golden_images() {
    use crate::shared::kit::testing::golden;
    let mut windows = Windows::default();
    let id = live(&mut windows, 420, 260, 1.0);
    let _ = windows.update(Msg::Pin(id, PinMsg::Hover(true)));
    golden::each_theme(|theme, failed| {
        let name = format!("live-pin-hover-{}", theme.id);
        failed.extend(golden::check(
            "screens",
            &name,
            Size::new(420.0, 260.0),
            &[1.0],
            || windows.view(id).unwrap(),
        ));
    });
}

#[test]
fn normal_windows_draw_their_own_header() {
    let mut windows = Windows::default();
    let (id, other) = (window::Id::unique(), window::Id::unique());
    windows.settings = Some((id, settings_state()));
    let texts = shot::texts(windows.view(id).unwrap());
    assert!(
        texts.iter().any(|t| t == "OpenCircle Settings"),
        "{texts:?}"
    );
    assert!(windows.mode(id).resizable && !windows.mode(id).maximised);
    // Maximised once the OS says so; edges are gone, restore shows.
    windows.chrome.insert(id, Chrome::default());
    let _ = windows.update(Msg::Maximised(id, true));
    assert!(windows.mode(id).maximised);
    // Onboarding has the design's fixed size, its own header; nor can the
    // permission prompt be maximised.
    windows.onboarding = Some((other, onboarding::State::open()));
    assert!(!windows.mode(other).resizable);
    let texts = shot::texts(windows.view(other).unwrap());
    assert!(
        texts.iter().any(|t| t == "Welcome to OpenCircle"),
        "{texts:?}"
    );
    let prompt = window::Id::unique();
    windows.permission = Some((prompt, onboarding::Access::Unasked));
    assert!(!windows.mode(prompt).resizable);
}

#[test]
fn renders_the_header_in_both_themes() {
    let mut windows = Windows::default();
    let (id, other) = (window::Id::unique(), window::Id::unique());
    windows.settings = Some((id, settings_state()));
    windows.onboarding = Some((other, onboarding::State::open()));
    for (name, choice) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        theme::apply(choice);
        for (what, id, size) in [
            ("settings", id, settings::WINDOW_SIZE),
            ("onboarding", other, onboarding::WINDOW_SIZE),
        ] {
            let px = shot::save(
                &format!("{what}-header-{name}"),
                windows.view(id).unwrap(),
                size,
            );
            // The header is the window's own colour all the way across (no gray strip).
            let w = size.width as usize;
            let bg = crate::shared::kit::themes::c().bg;
            let bg = [bg.r, bg.g, bg.b].map(|c| (c * 255.0).round() as u8);
            let row = &px[(10 * w) * 4..(11 * w) * 4];
            let plain = row.chunks(4).filter(|p| p[..3] == bg).count();
            assert!(plain > w * 6 / 10, "{what} {name}: {plain} of {w}");
        }
    }
    theme::apply(Theme::Dark);
}

#[test]
fn two_quick_presses_on_the_header_are_a_double_click() {
    let t = Instant::now();
    assert!(!is_double_click(None, t));
    assert!(is_double_click(Some(t), t + Duration::from_millis(200)));
    assert!(!is_double_click(Some(t), t + Duration::from_millis(900)));
}

#[test]
fn floating_windows_are_known_until_they_close() {
    let (id, _open) = floating(window::Settings::default(), 20.0, POPUP);
    assert!(is_floating(id));
    assert!(!is_floating(window::Id::unique()));
    let mut windows = Windows::default();
    let _ = windows.update(Msg::Closed(id));
    assert!(!is_floating(id));
}

#[test]
fn a_taken_shortcut_is_told_once() {
    let keys = |k: &[&str]| k.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    assert_eq!(
        taken_notice(&keys(&["Alt+A"]), &[]).as_deref(),
        Some("Alt+A is taken by another app: pick another key in Settings")
    );
    // the next start, the same key: nothing
    assert_eq!(taken_notice(&keys(&["Alt+A"]), &keys(&["Alt+A"])), None);
    assert_eq!(taken_notice(&[], &keys(&["Alt+A"])), None, "none taken");
    // another key taken since: both named
    let both = taken_notice(&keys(&["Alt+A", "Ctrl+Shift+Space"]), &keys(&["Alt+A"]));
    assert!(
        both.unwrap()
            .starts_with("Alt+A, Ctrl+Shift+Space are taken")
    );
}

#[test]
fn the_tray_notice_names_the_configured_shortcut() {
    assert_eq!(
        tray_notice("Ctrl+Shift+Space"),
        "OpenCircle is running in the tray. Press Ctrl+Shift+Space."
    );
    assert!(tray_notice("Alt+PrintScreen").ends_with("Alt+PrtSc."));
}

/// Popups come and go by the design's motion (`motion`): the toast pill
/// fades with an 8 px rise in 200 ms each way (#52), the others like
/// every widget (210 ms open eased out, 150 ms close eased in). Where the
/// OS composites a window's opacity it starts clear and fades in; the
/// toast's fade by the clock: clear and 8 px low, then there.
#[test]
fn popups_come_and_go_by_the_design_motion() {
    let motion_of = |id| {
        let floating = platform::windows::FLOATING.lock().unwrap();
        floating.iter().find(|(f, ..)| *f == id).map(|(.., m)| *m)
    };
    let mut windows = Windows::default();
    let _ = windows.toast("Copied", true, None, TOAST_MS);
    let toast = windows.toasts.last().unwrap().0;
    assert_eq!(motion_of(toast), Some(TOAST));
    assert_eq!((TOAST.pace, TOAST.rise), (motion::TOAST_PACE, 8.0));
    let _ = windows.pin(picture(20, 10), None, 1.0);
    assert_eq!(motion_of(windows.pins[0].id), Some(POPUP));
    assert_eq!((POPUP.pace, POPUP.rise), (motion::WIDGET, 0.0));
    let t0 = Instant::now();
    let shown = motion::Shown::opening(TOAST.pace, t0);
    let at = |ms| shown.at(t0 + Duration::from_millis(ms));
    let low = |a: f32| (1.0 - a) * TOAST.rise;
    assert_eq!((at(0), low(at(0))), (0.0, 8.0));
    assert!(at(100) > 0.5 && at(100) < 1.0);
    assert_eq!((at(200), low(at(200))), (1.0, 0.0));
    let gone = motion::Shown::closing(TOAST.pace, t0);
    assert_eq!(gone.at(t0 + motion::TOAST), 0.0);
    let _ = windows.update(Msg::Closed(toast));
    assert!(motion_of(toast).is_none());
}

#[test]
fn os_theme_message_reaches_the_palette() {
    let mut windows = Windows::default();
    theme::apply(Theme::System);
    let _ = windows.update(Msg::OsTheme(iced::theme::Mode::Light));
    let light = crate::shared::kit::themes::c().text;
    let _ = windows.update(Msg::OsTheme(iced::theme::Mode::Dark));
    assert_ne!(crate::shared::kit::themes::c().text, light);
}

mod studio_window {
    use super::*;
    use crate::features::snip::annotate::{Message, Tool};
    use crate::features::snip::screens::studio as st;

    fn crop() -> Crop {
        Crop {
            width: 40,
            height: 30,
            rgba: vec![255; 40 * 30 * 4],
        }
    }

    fn open() -> Windows {
        let mut w = Windows::default();
        let _ = w.open_studio(Some(st::Picture::Crop(crop())));
        w
    }

    fn draw(w: &mut Windows) {
        let _ = w.on_studio(st::Msg::Editor(Message::Tool(Tool::Pen)));
        for m in [
            Message::Press(iced::Point::new(2.0, 2.0)),
            Message::Move(iced::Point::new(30.0, 20.0)),
            Message::Release,
        ] {
            let _ = w.on_studio(st::Msg::Editor(m));
        }
    }

    fn size(w: &Windows) -> (u32, u32) {
        let s = w.studio.as_ref().unwrap().1.probe().0.unwrap();
        (s.width as u32, s.height as u32)
    }

    #[test]
    fn opening_twice_keeps_one_studio_and_loads_the_new_picture() {
        let mut w = open();
        let id = w.studio_window().unwrap();
        let _ = w.open_studio(Some(st::Picture::Crop(Crop {
            width: 80,
            height: 20,
            rgba: vec![0; 80 * 20 * 4],
        })));
        assert_eq!(w.studio_window(), Some(id), "the same window");
        assert_eq!(size(&w), (80, 20));
    }

    #[test]
    fn save_then_continue_opens_what_waited() {
        let mut w = open();
        draw(&mut w);
        assert!(w.studio_dirty());
        let _ = w.open_studio(Some(st::Picture::Crop(Crop {
            width: 80,
            height: 20,
            rgba: vec![0; 80 * 20 * 4],
        })));
        assert_eq!(size(&w), (40, 30), "asks first: the old one stays");
        let _ = w.studio_saved(Ok("Saved".into()), true);
        assert_eq!(size(&w), (80, 20));
        assert!(!w.studio_dirty());
        // a failed Save goes nowhere
        let mut w = open();
        draw(&mut w);
        let _ = w.close_studio();
        let _ = w.studio_saved(Err("disk full".into()), true);
        assert!(w.studio_dirty() && w.studio.as_ref().unwrap().1.asking());
    }

    #[test]
    fn a_close_request_asks_when_unsaved() {
        let mut w = open();
        let id = w.studio_window().unwrap();
        let _ = w.update(Msg::CloseRequest(id));
        assert!(!w.studio.as_ref().unwrap().1.asking(), "clean: no question");
        draw(&mut w);
        let _ = w.update(Msg::CloseRequest(id));
        assert!(w.studio.as_ref().unwrap().1.asking());
        assert!(w.busy());
    }

    #[test]
    fn quit_asks_while_dirty_and_goes_when_clean() {
        let mut w = open();
        assert!(w.quit_studio().is_none(), "clean: the app may quit");
        draw(&mut w);
        assert!(w.quit_studio().is_some());
        assert!(w.studio.as_ref().unwrap().1.asking());
        let _ = w.on_studio(st::Msg::Cancel);
        assert!(!w.studio.as_ref().unwrap().1.asking() && w.studio_dirty());
        let _ = w.quit_studio();
        let _ = w.on_studio(st::Msg::DontSave);
        assert!(w.quit_studio().is_none(), "answered: nothing left to ask");
    }

    #[test]
    fn an_unreadable_file_says_so_and_changes_nothing() {
        let mut w = open();
        let _ = w.studio_read(Err("bad".into()));
        assert_eq!(size(&w), (40, 30));
        assert!(!w.toasts.is_empty(), "the error pill");
    }

    #[test]
    fn ctrl_is_cleared_when_the_studio_loses_focus() {
        let mut w = open();
        let id = w.studio_window().unwrap();
        let _ = w.update(Msg::StudioEvent(window::Id::unique(), st::Msg::Ctrl(true)));
        assert!(
            w.studio.as_ref().unwrap().1.probe().1,
            "any window's Ctrl counts"
        );
        let _ = w.update(Msg::Unfocused(id));
        assert!(!w.studio.as_ref().unwrap().1.probe().1);
    }
}

/// F12-2: a failure card window: folded at first, Show details opens it (one
/// window replaces the other), the fix closes it and runs its trigger, Esc closes it.
#[test]
fn f12_the_failure_card_folds_its_details_and_its_fix_runs_then_closes_it() {
    use super::failure::Failure;
    let failure = Failure {
        headline: "A step could not be added".into(),
        hint: "Capture it again; your other steps are safe.".into(),
        detail: "no pixels".into(),
        fix: Some(("Capture again", crate::Trigger::Quit)),
    };
    let mut windows = Windows::default();
    let _ = windows.failure(failure.clone());
    let (_, open) = windows.failure_open().unwrap();
    assert!(!open, "folded at first");
    assert!(
        failure.size(true).height > failure.size(false).height,
        "the open card is taller"
    );
    let _ = windows.update(Msg::FailureDetails);
    assert!(windows.failure_open().unwrap().1, "details open");
    let _ = windows.update(Msg::FailureDetails);
    assert!(!windows.failure_open().unwrap().1, "and folded again");
    // the fix: the card goes
    let _ = windows.update(Msg::FailureFix);
    assert!(windows.failure_open().is_none());
    // a new failure replaces an open one; Esc closes it
    let _ = windows.failure(failure.clone());
    let _ = windows.failure(failure);
    let _ = windows.update(Msg::FailureEsc);
    assert!(windows.failure_open().is_none());
}

/// F12-3: a step that could not be made stays as a red placeholder in the
/// strip (its window grows by one place) and says so on the one error card;
/// the next step added, or a click on the placeholder, takes it away.
#[test]
fn f12_a_failed_step_stays_as_a_placeholder_with_the_error_card() {
    use crate::features::snip::screens::steps::Msg as S;
    let (mut windows, _, _) = floating_windows();
    let step = crate::features::snip::screens::steps::Step::new(&picture(200, 100)).unwrap();
    assert!(
        windows
            .steps
            .as_mut()
            .unwrap()
            .1
            .push(std::sync::Arc::new(step))
    );
    let before = windows.steps.as_ref().unwrap().1.size();
    let _ = windows.update(Msg::Steps(S::Failed("no pixels".into())));
    let strip = &windows.steps.as_ref().unwrap().1;
    assert!(strip.failed, "the placeholder stays");
    assert!(strip.size().height > before.height, "one more place");
    let (card, open) = windows.failure_open().expect("the error card");
    assert_eq!(card.headline, "A step could not be added");
    assert_eq!(card.detail, "no pixels");
    assert!(!open);
    // a click on the placeholder takes it away
    let _ = windows.update(Msg::Steps(S::FailedSeen));
    assert!(!windows.steps.as_ref().unwrap().1.failed);
}

/// F5-6: an action toast ("Snip closed . Undo") has a timer line (a clock that runs
/// while it shows); a plain toast has none; the clock goes with the window.
#[test]
fn f5_an_action_toast_has_a_timer_line_that_goes_with_it() {
    let mut windows = Windows::default();
    let _ = windows.toast("Copied", true, None, 1000);
    assert!(windows.toast_clock.is_empty());
    let _ = windows.toast_with("Snip closed", true, None, 8000, Some("Undo"));
    assert_eq!(windows.toast_clock.len(), 1);
    let id = windows.toast_clock[0].0;
    let _ = windows.update(Msg::Closed(id));
    assert!(windows.toast_clock.is_empty());
}

/// F6-5: the Try it step holds the global shortcuts off (so a press ticks its row and
/// opens nothing); leaving the step, going back and closing the window give them back.
#[test]
fn f6_the_try_it_step_holds_the_global_shortcuts_off_and_gives_them_back() {
    use crate::features::onboarding::{Message as O, State};
    use crate::shared::platform::hotkey::suspended;
    let mut windows = Windows::default();
    let id = window::Id::unique();
    let state = State::new(
        crate::shared::config::Config::default(),
        None,
        Box::new(crate::shared::secrets::MemoryStore::default()),
    );
    windows.onboarding = Some((id, state));
    let go = |windows: &mut Windows, m: O| {
        let _ = windows.update(Msg::Onboarding(m));
    };
    go(&mut windows, O::Tick);
    assert!(!suspended(), "the Language step: shortcuts work");
    go(&mut windows, O::Next);
    go(&mut windows, O::Next);
    assert!(suspended(), "Try it: held off");
    go(&mut windows, O::Next);
    assert!(!suspended(), "leaving the step gives them back");
    go(&mut windows, O::Back);
    assert!(suspended(), "back on Try it");
    let _ = windows.update(Msg::Closed(id));
    assert!(
        windows.onboarding.is_none() || !suspended(),
        "closing the window"
    );
    assert!(!suspended(), "closed: back on");
}

/// W108 (owner round 3): the Studio's steps are never lost by the strip's x or by the
/// strip going away; once the Studio holds the steps the strip's window is gone.
mod studio_steps {
    use super::*;
    use crate::features::snip::screens::steps::{Msg as S, Step};
    use std::sync::Arc;

    fn with_steps(n: usize) -> (Windows, window::Id) {
        let (mut w, _, strip) = floating_windows();
        for _ in 0..n {
            let step = Arc::new(Step::new(&picture(60, 40)).unwrap());
            assert!(w.steps.as_mut().unwrap().1.push(step));
        }
        (w, strip)
    }

    fn held(w: &Windows) -> usize {
        w.steps.as_ref().map_or(0, |(_, s)| s.steps().len())
    }

    #[test]
    fn opening_the_studio_closes_the_strip_and_keeps_its_steps() {
        let (mut w, strip) = with_steps(2);
        let _ = w.open_studio(None);
        let _ = w.update(Msg::Closed(strip)); // the window we asked to close is gone
        assert_eq!(w.steps_window(), None, "no strip window beside the Studio");
        assert_eq!(held(&w), 2, "the Studio's sidebar still has both steps");
    }

    #[test]
    fn the_strips_x_never_wipes_an_open_studios_steps() {
        let (mut w, _) = with_steps(2);
        let _ = w.open_studio(None);
        let _ = w.update(Msg::Steps(S::Clear));
        assert_eq!(held(&w), 2);
        assert!(!w.steps.as_ref().unwrap().1.cleared);
    }

    #[test]
    fn a_later_step_joins_the_open_studio_and_no_strip_comes_back() {
        let (mut w, strip) = with_steps(1);
        let _ = w.open_studio(None);
        let _ = w.update(Msg::Closed(strip));
        let step = Arc::new(Step::new(&picture(60, 40)).unwrap());
        let _ = w.update(Msg::Steps(S::Ready(step, None)));
        assert_eq!(held(&w), 2);
        assert_eq!(w.steps_window(), None);
    }

    #[test]
    fn closing_the_studio_that_held_the_steps_drops_them() {
        let (mut w, strip) = with_steps(2);
        let _ = w.open_studio(None);
        let studio = w.studio_window().unwrap();
        let _ = w.update(Msg::Closed(strip));
        assert_eq!(held(&w), 2, "kept while the Studio is open");
        let _ = w.update(Msg::Closed(studio));
        assert!(w.steps.is_none());
        w.pins.clear(); // (the helper's pin is not what this is about)
        assert!(!w.busy(), "nothing of ours is left open");
        assert_eq!(w.steps(), None, "Snip no longer offers Add step N");
    }

    #[test]
    fn the_cleared_pill_open_while_the_studio_opens_ends_with_nothing_kept() {
        let (mut w, _) = with_steps(2);
        let _ = w.update(Msg::Steps(S::Clear)); // the pill: Steps cleared . Undo
        let pill = w.steps_window().unwrap(); // (where windows can't resize, a new one)
        let _ = w.open_studio(None);
        let studio = w.studio_window().unwrap();
        let _ = w.update(Msg::Closed(pill)); // the pill's time is over
        let _ = w.update(Msg::Closed(studio));
        assert!(w.steps.is_none());
        w.pins.clear();
        assert!(!w.busy());
    }

    /// The strip's "+" makes Windows start the Snip (the tray's one: a quick action "snip").
    #[test]
    fn the_strips_plus_starts_a_snip() {
        let (mut w, _) = with_steps(1);
        let task = w.update(Msg::Steps(S::AddStep));
        let sent: Vec<Msg> = iced_runtime::task::into_stream(task)
            .map(iced::futures::executor::block_on_stream)
            .into_iter()
            .flatten()
            .filter_map(|a| match a {
                iced_runtime::Action::Output(m) => Some(m),
                _ => None,
            })
            .collect();
        let snip =
            |m: &Msg| matches!(m, Msg::Run(crate::Trigger::Quick(_, q)) if q.action == "snip");
        assert!(
            sent.iter().any(snip),
            "{} messages, none starts a Snip",
            sent.len()
        );
    }

    #[test]
    fn closing_the_strip_without_a_studio_still_drops_the_steps() {
        let (mut w, strip) = with_steps(2);
        let _ = w.update(Msg::Closed(strip));
        assert!(w.steps.is_none());
    }
}

#[test]
fn w108_the_strip_opens_where_the_user_left_it_on_that_screen() {
    let mut w = Windows::default();
    w.strip_spots.insert((0, 0), Point::new(640.0, 200.0));
    let place = platform::monitor::Place {
        origin: (0, 0),
        px: (1920, 1080),
        scale: 1.0,
    };
    let _ = w.add_step(picture(60, 40), None, place, 0.0);
    assert_eq!(w.steps.as_ref().unwrap().1.place().origin, (640, 200));
}

/// W108 (owner round 3): after an export a small card "Exported · name" with Open and
/// Show in folder; it is the kit toast with two actions and goes by itself.
mod exported_card {
    use super::*;
    use crate::shared::platform::sys::open::launched;

    fn card() -> (Windows, std::path::PathBuf) {
        let mut w = Windows::default();
        let path = std::env::temp_dir().join("oc-w108-steps-1.md");
        let _ = w.update(Msg::StudioExported(Ok(path.clone())));
        (w, path)
    }

    #[test]
    fn an_export_shows_the_card_with_its_name() {
        let (w, _) = card();
        let want = vec![("Exported · oc-w108-steps-1.md".to_string(), true)];
        assert_eq!(w.toasts(), want);
        assert!(w.exported.is_some());
    }

    #[test]
    fn open_runs_the_default_app_and_the_card_goes() {
        let (mut w, path) = card();
        let _ = w.update(Msg::ExportedOpen);
        assert!(launched().contains(&path.to_string_lossy().into_owned()));
        assert!(w.exported.is_none());
    }

    #[test]
    fn show_in_folder_opens_the_folder_that_holds_it() {
        let (mut w, path) = card();
        let _ = w.update(Msg::ExportedShow);
        let dir = path.parent().unwrap().to_string_lossy().into_owned();
        assert!(launched().contains(&dir));
    }

    #[test]
    fn a_failed_export_is_the_plain_error_pill() {
        let mut w = Windows::default();
        let _ = w.update(Msg::StudioExported(Err("Not saved".into())));
        assert_eq!(w.toasts(), vec![("Not saved".to_string(), false)]);
        assert!(w.exported.is_none());
    }

    #[test]
    fn a_new_export_replaces_the_card() {
        let (mut w, _) = card();
        let first = w.exported.as_ref().unwrap().0;
        let _ = w.update(Msg::StudioExported(Ok("export-b.png".into())));
        assert_ne!(w.exported.as_ref().unwrap().0, first);
    }
}

/// W108: the strip opens where it was left in the last run (`snip.steps_spot`), on that
/// screen only; one left past a smaller screen's edge is kept inside it.
#[test]
fn w108_the_strip_opens_at_its_saved_spot_on_that_screen_and_stays_on_screen() {
    let screen = |origin: (i32, i32)| platform::monitor::Place {
        origin,
        px: (1920, 1080),
        scale: 1.0,
    };
    let mut cfg = crate::shared::config::Config::default();
    cfg.snip.steps_spot = Some([0, 0, 640, 200]);
    let origin_of = |cfg: &crate::shared::config::Config, monitor| {
        crate::shared::config::with_test_config(cfg.clone(), || {
            let mut w = Windows::default();
            let _ = w.add_step(picture(60, 40), None, monitor, 0.0);
            w.steps.as_ref().unwrap().1.place().origin
        })
    };
    assert_eq!(origin_of(&cfg, screen((0, 0))), (640, 200));
    // another screen setup: docked at its edge as before
    assert_eq!(origin_of(&cfg, screen((1920, 0))).0, 1920 + 1920 - 12 - 104);
    // a spot past the edge (a smaller screen now): kept on it
    cfg.snip.steps_spot = Some([0, 0, 9000, 9000]);
    let p = origin_of(&cfg, screen((0, 0)));
    assert!(p.0 <= 1920 - 104 && p.1 <= 1080 - 162, "{p:?}");
}
