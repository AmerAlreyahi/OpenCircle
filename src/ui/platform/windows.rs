//! THE window helper: every window of ours is opened here (`open`), floating
//! popups (`floating`), screen-sized ones (`screen`), the setup's (`setup_window`).
//! The OS side of windows only: opacity, fades, the cut shape, capture exclusion.
//! Which windows exist and what they show is `ui::windows`.
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use iced::Size;
use iced::Task;
use iced::window::{self, Level, Position};

use crate::ui::kit::motion; // the one kit part platform reads: the fade's clock and pace

/// Our logo as the window icon (taskbar, Alt+Tab): given once at start by the UI, which owns the
/// picture (platform imports none of the UI).
static ICON: OnceLock<window::Icon> = OnceLock::new();

/// Sets the icon every window gets (the first call wins).
pub fn set_icon(rgba: Vec<u8>, width: u32, height: u32) {
    if let Ok(icon) = window::icon::from_rgba(rgba, width, height) {
        let _ = ICON.set(icon);
    }
}

/// The setup's window (`ui::screens::setup`): a fixed-size normal window (taskbar entry,
/// our own header), centred, cut to the design's rounded corners (`radius`, as
/// a floating window: nothing shows outside them, and it fades in and out by
/// `POPUP`). After a `window::resize`, `reshape` cuts it again.
pub fn setup_window(size: Size, radius: f32) -> (window::Id, Task<window::Id>) {
    let fades = super::shape::can_fade();
    let (id, open) = open(window::Settings {
        size,
        resizable: false,
        position: Position::Centered,
        exit_on_close_request: false,
        visible: !fades, // shown once cut (`appear`)
        ..window::Settings::default()
    });
    FLOATING.lock().unwrap().push((id, radius, POPUP));
    let open = open.then(move |id| {
        let shown = match fades {
            true => appear(id, POPUP),
            false => Task::none(),
        };
        reshape::<()>(id)
            .discard()
            .chain(shown)
            .chain(Task::done(id))
    });
    (id, open)
}

/// A small popup (toast, pin, live controller, any future one): THE way to open one.
/// Undecorated, always on top, no taskbar entry (Windows), transparent with a
/// fully clear background (`clear_theme`), and cut to its content's rounded
/// rectangle of at most `radius` logical px (`shape`): where the window has no
/// per-pixel alpha (Windows, X11 without a compositor) nothing outside the shape
/// exists, so no box shows behind a pill. Nothing in a floating window may paint
/// outside its content (no shadow: a solid surface, e.g. `kit::toast`).
/// After a `window::resize` nothing is needed: the resize event reshapes it.
/// It comes and goes by `how` (`Motion`): faded in once its shape is cut,
/// faded out by `close`.
pub fn floating(
    settings: window::Settings,
    radius: f32,
    how: Motion,
) -> (window::Id, Task<window::Id>) {
    floating_as(settings, radius, how, false)
}

/// `floating`, but it shows in screen captures, the user's screenshots and
/// recordings (the update pills and What's new: owner, 2026-11-10, the one
/// exception to "own floating windows out of captures"), so a Snip target in
/// every capture.
pub fn floating_in_captures(
    settings: window::Settings,
    radius: f32,
    how: Motion,
) -> (window::Id, Task<window::Id>) {
    floating_as(settings, radius, how, true)
}

fn floating_as(
    settings: window::Settings,
    radius: f32,
    how: Motion,
    seen: bool,
) -> (window::Id, Task<window::Id>) {
    let fades = settings.visible && super::shape::can_fade();
    let settings = window::Settings {
        decorations: false,
        transparent: true,
        level: Level::AlwaysOnTop,
        exit_on_close_request: false,
        // shown once clear and cut (`appear`)
        visible: settings.visible && !fades,
        ..settings
    };
    // ponytail: Linux cannot hide a window from the taskbar through iced.
    #[cfg(windows)]
    let settings = window::Settings {
        platform_specific: window::settings::PlatformSpecific {
            skip_taskbar: true,
            ..Default::default()
        },
        ..settings
    };
    let (id, open) = open(settings);
    FLOATING.lock().unwrap().push((id, radius, how));
    let open = open.then(move |id| {
        let shown = match fades {
            true => appear(id, how),
            false => Task::none(),
        };
        reshape::<()>(id)
            .discard()
            .chain(keep_out(id, seen))
            .chain(shown)
            .chain(Task::done(id))
    });
    (id, open)
}

/// Test hook: our floating windows show in screen captures (VM screenshots of
/// them over the desktop; they then also show in the next capture's frozen frame).
const CAPTURABLE: &str = "OPENCIRCLE_TEST_CAPTURABLE";

/// A floating window of ours (Steps strip, pins, pills, the update card, the
/// chat's, the tray menu) is never in what we capture or translate uninvited:
/// Windows keeps it out of every capture (`hide_from_capture`, before it shows),
/// and so out of the targets (`window_rect::list`: a pick would hold what is under
/// it). Linux has no such call: the capture takes it out of the frame
/// (`Windows::leave_frame`). Snip's grab alone has them in, to pick one
/// (`window_rect::widgets_in_grab`; owner, 2026-11-10).
/// `seen`: one that shows in captures (`floating_in_captures`).
fn keep_out<T: Send + 'static>(id: window::Id, seen: bool) -> Task<T> {
    let out = !seen && crate::sys::test_hook(CAPTURABLE).is_none();
    hide_from_capture(id, out)
}

/// How a floating window comes and goes (`motion`): its pace, and how far it
/// rises as it shows (sinks as it goes), logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub pace: motion::Pace,
    pub rise: f32,
}

/// Popups (pin, update card, live controller): the design's widget open/close.
pub const POPUP: Motion = Motion {
    pace: motion::WIDGET,
    rise: 0.0,
};
/// The toast pill: fade and an 8 px rise (#52).
pub const TOAST: Motion = Motion {
    pace: motion::TOAST_PACE,
    rise: motion::TOAST_RISE,
};

/// Shows the hidden window `id` from fully clear and fades it in by `how`
/// (`fade`; the task ends at once, the fade runs on).
pub fn appear<T: Send + 'static>(id: window::Id, how: Motion) -> Task<T> {
    opacity(id, 0.0)
        .chain(show(id, window::Mode::Windowed))
        .chain(fade(id, true, how, false))
}

/// Shows our always-on-top window `id` in `mode` (a floating one Windowed, the
/// overlay Fullscreen): THE way one is shown. X11: "always on top" again once
/// mapped. A window manager drops a state asked while the window was unmapped
/// (EWMH: `_NET_WM_STATE` messages are for mapped windows; winit asks at
/// creation), so a window opened hidden came back as a normal one (#172, KDE
/// X11): the chat under any window the user activated, the overlay under the
/// Plasma panel (KWin keeps docks over a full-screen window that isn't "above").
/// Windows keeps the level itself; Wayland has none.
pub fn show<T: Send + 'static>(id: window::Id, mode: window::Mode) -> Task<T> {
    let shown = window::set_mode(id, mode);
    #[cfg(target_os = "linux")]
    let shown = shown.chain(window::set_level(id, Level::AlwaysOnTop));
    shown
}

/// Closes the window `id`: a floating one fades out first (by its `Motion`),
/// where windows can fade; any other closes at once.
pub fn close<T: Send + 'static>(id: window::Id) -> Task<T> {
    let how = (FLOATING.lock().unwrap().iter())
        .find(|(f, ..)| *f == id)
        .map(|(_, _, how)| *how);
    match how {
        Some(how) if super::shape::can_fade() => {
            fade(id, false, how, true).chain(window::close(id))
        }
        _ => window::close(id),
    }
}

/// The window `id` at `alpha` (0..1) of its opacity, as a whole (`super::shape::opacity`).
pub fn opacity<T: Send + 'static>(id: window::Id, alpha: f32) -> Task<T> {
    window::run(id, move |w| {
        if let Ok(handle) = w.window_handle()
            && let Err(e) = super::shape::opacity(handle.as_raw(), alpha)
        {
            crate::log!("[opencircle] window opacity: {e}");
        }
    })
    .discard()
}

/// Fades the window `id` in (`open`) or out by `how` on a thread of its own
/// (`super::shape::fade`: the OS composites each step, nothing is redrawn); the
/// task ends at once, or with the fade when `wait`. Nothing where windows
/// can't fade.
pub fn fade<T: Send + 'static>(id: window::Id, open: bool, how: Motion, wait: bool) -> Task<T> {
    if !super::shape::can_fade() {
        return Task::none();
    }
    let started = window::run(id, move |w| {
        let (done, finished) = iced::futures::channel::oneshot::channel::<()>();
        let window = w
            .window_handle()
            .ok()
            .and_then(|h| super::shape::fadeable(h.as_raw()));
        if let Some(window) = window {
            motion::refresh();
            let now = Instant::now();
            let shown = match open {
                true => motion::Shown::opening(how.pace, now),
                false => motion::Shown::closing(how.pace, now),
            };
            let run = super::shape::fade(window, shown, how.rise);
            std::thread::spawn(move || {
                run();
                let _ = done.send(());
            });
        }
        finished
    });
    match wait {
        true => started.then(Task::future).discard(),
        false => started.discard(),
    }
}

/// Wayland's long capture pop-up (`ui::long_capture_popup`, a helper process
/// on XWayland): an X11 override-redirect window, so it sits exactly where
/// asked and stays over every other window (a Wayland window can't be placed
/// or kept on top on GNOME). Takes no keyboard. `popup_shape` cuts it.
pub fn x11_popup(settings: window::Settings) -> (window::Id, Task<window::Id>) {
    #[cfg(target_os = "linux")]
    let settings = window::Settings {
        platform_specific: window::settings::PlatformSpecific {
            override_redirect: true,
            ..settings.platform_specific
        },
        ..settings
    };
    open(window::Settings {
        decorations: false,
        resizable: false,
        level: Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..settings
    })
}

/// Cuts the window `id` to `parts` (logical px of it, each with its corner
/// radius) at `scale` (`super::shape::parts`).
pub fn popup_shape<T: Send + 'static>(
    id: window::Id,
    parts: Vec<(iced::Rectangle, f32)>,
    scale: f32,
) -> Task<T> {
    let px = move |(r, radius): (iced::Rectangle, f32)| {
        let at = |v: f32| (v * scale).round() as i32;
        let (x, y) = (at(r.x), at(r.y));
        let (w, h) = (at(r.x + r.width) - x, at(r.y + r.height) - y);
        (
            (x, y, w.max(0) as u32, h.max(0) as u32),
            (radius * scale).round() as u32,
        )
    };
    window::run(id, move |w| {
        let parts: Vec<_> = parts.into_iter().map(px).collect();
        if let Ok(handle) = w.window_handle()
            && let Err(e) = super::shape::parts(handle.as_raw(), &parts)
        {
            crate::log!("[opencircle] window shape: {e}");
        }
    })
    .discard()
}

/// A screen-sized window (the capture overlay, the live layer): no frame, not
/// cut to a shape; each sets its own transparency and level.
/// Never a snip target (`no_pick`).
pub fn screen(settings: window::Settings) -> (window::Id, Task<window::Id>) {
    let (id, open) = open(settings);
    (id, open.then(|id| no_pick(id).chain(Task::done(id))))
}

/// Snip never outlines or picks the window `id` (`window_rect::no_pick`): the
/// overlay and the screen-sized ones (`screen`). Every other window of ours is a
/// target like any app's (Settings, onboarding; the floating ones while they are
/// in the grab, `keep_out`).
pub fn no_pick<T: Send + 'static>(id: window::Id) -> Task<T> {
    use iced::window::raw_window_handle::RawWindowHandle;
    window::run(id, |w| {
        let handle = match w.window_handle().map(|h| h.as_raw()) {
            #[cfg(windows)]
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            Ok(RawWindowHandle::Xlib(h)) => h.window as isize,
            Ok(RawWindowHandle::Xcb(h)) => h.window.get() as isize,
            _ => return,
        };
        crate::capture::window_rect::no_pick(handle);
    })
    .discard()
}

/// What the three above share: `app_window`'s settings, and on Windows no classic
/// title bar painted over the window at (de)activation (`super::shape::no_caption`).
/// A window asked off the taskbar (`skip_taskbar`: the overlay, popups) is a tool
/// window for good (`super::shape::no_taskbar`: winit's own skip comes undone).
pub(in crate::ui) fn open(settings: window::Settings) -> (window::Id, Task<window::Id>) {
    #[cfg(windows)]
    let no_taskbar = settings.platform_specific.skip_taskbar;
    #[cfg(not(windows))]
    let no_taskbar = false;
    let transparent = settings.transparent;
    let (id, opened) = window::open(app_window(settings));
    let caption_off = move |id| {
        window::run(id, move |w| {
            if let Ok(handle) = w.window_handle() {
                super::shape::no_caption(handle.as_raw());
                super::shape::no_backdrop(handle.as_raw(), transparent);
                if no_taskbar {
                    super::shape::no_taskbar(handle.as_raw());
                }
                if let Some(raw) = super::shape::fadeable(handle.as_raw()) {
                    HANDLES.lock().unwrap().push((id, raw));
                }
            }
        })
        .discard()
        .chain(Task::done(id))
    };
    (id, opened.then(caption_off))
}

/// Every window `open` opened, with its handle as `super::shape::fadeable` gives it
/// (where windows can fade), for the calls that cannot wait on a task.
static HANDLES: Mutex<Vec<(window::Id, isize)>> = Mutex::new(Vec::new());

/// The window `id` clear NOW, on the UI thread, any fade of it stopped
/// (`super::shape::clear_now`): every frame it draws from here on stays unseen until
/// its opacity is set again (`show` does). A `Task` would run after the next
/// frame may already be on screen. Nothing where windows can't fade.
pub fn clear_now(id: window::Id) {
    #[cfg(test)]
    CLEARED.with(|c| c.borrow_mut().push(id));
    let raw = (HANDLES.lock().unwrap().iter()).find_map(|(w, raw)| (*w == id).then_some(*raw));
    if let Some(raw) = raw {
        super::shape::clear_now(raw);
    }
}

#[cfg(test)]
thread_local! {
    /// The windows `clear_now` was called on (this test's thread).
    pub static CLEARED: std::cell::RefCell<Vec<window::Id>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Floating windows and their corner radius (see `floating`).
pub(in crate::ui) static FLOATING: Mutex<Vec<(window::Id, f32, Motion)>> = Mutex::new(Vec::new());

pub fn is_floating(id: window::Id) -> bool {
    FLOATING.lock().unwrap().iter().any(|(f, ..)| *f == id)
}

fn forget_floating(id: window::Id) {
    FLOATING.lock().unwrap().retain(|(f, ..)| *f != id);
}

/// A closed window: no longer floating, its handle dropped.
pub fn forget(id: window::Id) {
    forget_floating(id);
    HANDLES.lock().unwrap().retain(|(w, _)| *w != id);
}

/// The theme of floating windows: a fully clear background.
pub fn clear_theme() -> iced::Theme {
    static THEME: std::sync::LazyLock<iced::Theme> = std::sync::LazyLock::new(|| {
        let mut palette = iced::Theme::Dark.palette();
        palette.background = iced::Color::TRANSPARENT;
        iced::Theme::custom("clear", palette)
    });
    THEME.clone()
}

/// Cuts an existing window (the capture overlay shrunk to a pill) to its content's
/// rounded rectangle of at most `radius` px, now and after every resize, until
/// `uncut`. Where a window is opened as a popup, `floating` does this itself.
pub fn cut<T: Send + 'static>(id: window::Id, radius: f32) -> Task<T> {
    forget_floating(id);
    FLOATING.lock().unwrap().push((id, radius, POPUP));
    reshape(id)
}

/// Windows: keeps the window out of every screen capture (`on`), or back in:
/// every floating window (`keep_out`), and the overlay during a long capture
/// (its pill sits over the area it grabs: in the frames its changing text would
/// keep the page from ever settling, and end up in the stitched image). Needs
/// Windows 10 2004+; elsewhere nothing.
pub fn hide_from_capture<T: Send + 'static>(id: window::Id, on: bool) -> Task<T> {
    window::run(id, move |w| {
        #[cfg(windows)]
        if let Ok(handle) = w.window_handle()
            && let iced::window::raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw()
        {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
            };
            let affinity = if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
            // SAFETY: `h` is this window's live handle (an iced window of ours).
            if let Err(e) =
                unsafe { SetWindowDisplayAffinity(HWND(h.hwnd.get() as *mut _), affinity) }
            {
                crate::log!("[opencircle] window affinity: {e}");
            }
        }
        #[cfg(not(windows))]
        let _ = (w, on);
    })
    .discard()
}

/// The window `id` always stacked above the window `owner` (the AI chat over
/// the overlay, #170), whichever of them comes to the front, while `owner`
/// still gets every click outside it (`super::shape::keep_above`).
pub fn keep_above<T: Send + 'static>(id: window::Id, owner: window::Id) -> Task<T> {
    window::run(owner, |w| {
        w.window_handle()
            .ok()
            .and_then(|h| super::shape::raw_id(h.as_raw()))
    })
    .then(move |owner| {
        window::run(id, move |w| {
            if let (Some(owner), Ok(handle)) = (owner, w.window_handle())
                && let Err(e) = super::shape::keep_above(handle.as_raw(), owner)
            {
                crate::log!("[opencircle] window stacking: {e}");
            }
        })
    })
    .discard()
}

/// The full-screen overlay with `hole` (logical px, `scale` physical px per
/// logical) cut out of it but for `keep` (its bars there): clicks and the
/// wheel in the hole reach the app under it (Snip's long capture). `uncut`
/// makes it whole again.
pub fn hole<T: Send + 'static>(
    id: window::Id,
    hole: iced::Rectangle,
    keep: Vec<iced::Rectangle>,
    scale: f32,
) -> Task<T> {
    forget_floating(id);
    // outward to whole px, as `actions::crop_bounds`: what is grabbed is in the hole
    let px = move |r: iced::Rectangle| {
        let (x0, y0) = ((r.x * scale).floor() as i32, (r.y * scale).floor() as i32);
        let (x1, y1) = (
            ((r.x + r.width) * scale).ceil() as i32,
            ((r.y + r.height) * scale).ceil() as i32,
        );
        (x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32)
    };
    window::run(id, move |w| {
        let keep: Vec<_> = keep.into_iter().map(px).collect();
        if let Ok(handle) = w.window_handle()
            && let Err(e) = super::shape::hole(handle.as_raw(), px(hole), &keep)
        {
            crate::log!("[opencircle] window shape: {e}");
        }
    })
    .discard()
}

/// No system backdrop behind the transparent window `id` again
/// (`super::shape::no_backdrop`; `open` did it once): a theming mod re-applies its
/// blur to every window when its settings change, and long capture grabs
/// through the overlay's hole.
pub fn no_backdrop<T: Send + 'static>(id: window::Id) -> Task<T> {
    window::run(id, |w| {
        if let Ok(handle) = w.window_handle() {
            super::shape::no_backdrop(handle.as_raw(), true);
        }
    })
    .discard()
}

/// Whole again (and not floating): the overlay is going full-screen.
pub fn uncut<T: Send + 'static>(id: window::Id) -> Task<T> {
    forget_floating(id);
    window::run(id, |w| {
        if let Ok(handle) = w.window_handle()
            && let Err(e) = super::shape::clear(handle.as_raw())
        {
            crate::log!("[opencircle] window shape: {e}");
        }
    })
    .discard()
}

/// Cuts the floating window `id` to its content's shape again (it was resized).
/// Nothing for any other window.
pub fn reshape<T: Send + 'static>(id: window::Id) -> Task<T> {
    let radius = FLOATING
        .lock()
        .unwrap()
        .iter()
        .find(|(f, ..)| *f == id)
        .map(|(_, r, _)| *r);
    let Some(radius) = radius else {
        return Task::none();
    };
    window::run(id, move |w| {
        if let Ok(handle) = w.window_handle()
            && let Err(e) = super::shape::apply(handle.as_raw(), radius)
        {
            crate::log!("[opencircle] window shape: {e}");
        }
    })
    .discard()
}

/// `settings` with no OS title bar (every window of ours draws its own header or
/// none, whatever the caller asked for), our logo as the window icon (taskbar,
/// Alt+Tab) and, on Linux,
/// our app id: the desktop ties the window to `APP_ID.desktop`, and GNOME asks for
/// screenshot permission only for the app it sees in front.
pub(in crate::ui) fn app_window(settings: window::Settings) -> window::Settings {
    let settings = window::Settings {
        decorations: false,
        icon: ICON.get().cloned(),
        ..settings
    };
    #[cfg(target_os = "linux")]
    return window::Settings {
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::capture::portal::APP_ID.into(),
            ..settings.platform_specific
        },
        ..settings
    };
    #[cfg(not(target_os = "linux"))]
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_window_ever_gets_the_os_title_bar() {
        let asked = window::Settings {
            decorations: true,
            ..window::Settings::default()
        };
        assert!(!app_window(asked).decorations);
    }

    /// Every window is opened by `normal` (ui/windows.rs), `floating`, `screen` or `setup_window` here, all
    /// through `open`, so none can show a box, a gray strip or the OS title bar by forgetting
    /// a setting.
    #[test]
    fn no_window_is_opened_outside_the_helpers() {
        let call = concat!("window::", "open(");
        let mut stray = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    // The tests of this very file mention the call.
                    let code = text.split("#[cfg(test)]").next().unwrap();
                    let n = code.matches(call).count();
                    if path.ends_with("ui/platform/windows.rs") {
                        assert_eq!(n, 1, "normal, floating and screen all go through open");
                    } else if n > 0 {
                        stray.push(path.display().to_string());
                    }
                }
            }
        }
        assert!(
            stray.is_empty(),
            "open windows through windows::floating / screen: {stray:?}"
        );
    }
}

/// The keyboard for `id` (a capture overlay shown from a shortcut), so Esc,
/// Enter and the arrows work without a click. `gain_focus` (winit: an Alt tap,
/// then SetForegroundWindow) is refused while the foreground is a hidden
/// window, e.g. the one Explorer keeps for the activation watermark (Windows
/// VM, 2026-11-03: SetForegroundWindow again, SwitchToThisWindow and
/// AllowSetForegroundWindow from our own process all failed). Then only
/// joining the foreground thread's input for the call works; it is skipped
/// for a hung window (attaching would hang us with it).
///
/// The Start menu or the Search pane (also the Action Center on Windows 10)
/// in front: Windows keeps its band above every app window and refuses the
/// foreground while it is open, so it is closed first (`shell_launcher`).
///
/// Linux: the shell's overview (GNOME's Activities) is drawn over every window;
/// it is left first, else the overlay stays behind it (#173, `sys::shell`).
pub fn foreground<T: Send + 'static>(id: window::Id) -> Task<T> {
    #[cfg(target_os = "linux")]
    crate::sys::shell::leave_overview();
    // Start / Search is closed before gain_focus: its Alt tap would reach
    // Start first and, with the shortcut's Alt still held (Alt+A), leave it in
    // its access-key mode, where Esc no longer closes it (VM).
    #[cfg(windows)]
    let close_launcher = window::run(id, |_| win::close_launcher()).discard();
    #[cfg(not(windows))]
    let close_launcher = Task::none();
    close_launcher
        .chain(window::gain_focus(id))
        .chain(window::run(id, |w| {
            #[cfg(windows)]
            if let Ok(handle) = w.window_handle()
                && let iced::window::raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw()
            {
                win::take_foreground(h.hwnd.get());
            }
            #[cfg(not(windows))]
            let _ = w;
        }))
        .discard()
}

/// The window opened next takes the activation token its trigger brought (the
/// desktop's permission to come to the front: Wayland xdg-activation, X11
/// startup notification), once; elsewhere there is none.
pub fn activate_next() {
    let token = crate::hotkey::activation::take();
    #[cfg(target_os = "linux")]
    if let Some(token) = token {
        crate::log!("[opencircle] the window opens with the desktop's activation token");
        iced_winit::activate_next_window(token);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = token;
}

/// Whether `foreground` must join the foreground thread's input: another
/// window than `ours` is in front (none: nothing to join) and it answers (a
/// hung one would hang us).
#[cfg_attr(not(windows), allow(dead_code))]
fn must_join(front: isize, ours: isize, hung: bool) -> bool {
    front != 0 && front != ours && !hung
}

/// The processes that host the Start menu, the Search pane and (Windows 10)
/// the Action Center / tray flyouts, by exe name: Windows 10 1903+ and 11 use
/// the first three, older Windows 10 ShellExperienceHost for Start too.
#[cfg_attr(not(windows), allow(dead_code))]
fn shell_launcher(exe: &str) -> bool {
    [
        "StartMenuExperienceHost.exe",
        "SearchApp.exe",
        "SearchHost.exe",
        "ShellExperienceHost.exe",
    ]
    .iter()
    .any(|n| n.eq_ignore_ascii_case(exe))
}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, SetFocus, VIRTUAL_KEY, VK_CONTROL,
        VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
        VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsHungAppWindow,
        SetForegroundWindow,
    };

    /// Whether `front` belongs to the Start / Search host.
    fn launcher(front: HWND) -> bool {
        let mut pid = 0u32;
        // SAFETY: a plain query on a window handle the system gave us.
        unsafe { GetWindowThreadProcessId(front, Some(&mut pid)) };
        pid != 0 && super::shell_launcher(&crate::capture::window_rect::exe_name(pid))
    }

    /// Closes the Start menu / Search pane in front with an Esc, the way the
    /// user would. Windows has no documented call that dismisses it
    /// (IAppVisibility only reports it). The shortcut's modifiers may still be
    /// held: they are released first, else the Esc becomes Ctrl+Shift+Esc
    /// (Task Manager), Alt+Esc (next window) or Ctrl+Esc (Start again).
    fn dismiss_launcher() {
        let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        };
        // The generic key is checked (a synthetic Alt without a scan code
        // sets only VK_MENU, VM); both sides go up.
        let ext = KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY;
        let mut keys: Vec<INPUT> = [
            (VK_CONTROL, VK_LCONTROL, VK_RCONTROL),
            (VK_SHIFT, VK_LSHIFT, VK_RSHIFT),
            (VK_MENU, VK_LMENU, VK_RMENU),
        ]
        .into_iter()
        // SAFETY: a plain key-state query.
        .filter(|(vk, _, _)| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0)
        .flat_map(|(_, left, right)| {
            let right_flags = if right == VK_RSHIFT {
                KEYEVENTF_KEYUP
            } else {
                ext
            };
            [key(left, KEYEVENTF_KEYUP), key(right, right_flags)]
        })
        .collect();
        keys.extend([
            key(VK_ESCAPE, KEYBD_EVENT_FLAGS(0)),
            key(VK_ESCAPE, KEYEVENTF_KEYUP),
        ]);
        // SAFETY: INPUTs of ours, their size given.
        unsafe { SendInput(&keys, size_of::<INPUT>() as i32) };
    }

    /// Closes the Start menu / Search pane when it is in front.
    pub fn close_launcher() {
        // SAFETY: plain queries on the foreground window.
        let mut front = unsafe { GetForegroundWindow() };
        if !launcher(front) {
            return;
        }
        dismiss_launcher();
        // ponytail: waits on the UI thread, at most 0.5 s and only with Start /
        // Search open; Start hands the foreground back to the app below it
        // within a few frames (to no window for a moment)
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            front = unsafe { GetForegroundWindow() };
            if !front.is_invalid() && !launcher(front) {
                break;
            }
        }
        crate::log!(
            "[opencircle] closed the Start menu / Search in front of the overlay: {}",
            !launcher(front)
        );
    }

    pub fn take_foreground(hwnd: isize) {
        let ours = HWND(hwnd as *mut _);
        // SAFETY: plain Win32 calls on our live window and the foreground one;
        // the input is detached again before returning.
        unsafe {
            let front = GetForegroundWindow();
            let hung = IsHungAppWindow(front).as_bool();
            if !super::must_join(front.0 as isize, hwnd, hung) {
                return;
            }
            let (them, me) = (GetWindowThreadProcessId(front, None), GetCurrentThreadId());
            let joined = them != me && AttachThreadInput(them, me, true).as_bool();
            let _ = SetForegroundWindow(ours);
            let _ = BringWindowToTop(ours);
            let _ = SetFocus(Some(ours));
            if joined {
                let _ = AttachThreadInput(them, me, false);
            }
            let taken = GetForegroundWindow() == ours;
            crate::log!("[opencircle] overlay took the keyboard from a refusing window: {taken}");
        }
    }
}

#[cfg(test)]
mod foreground_tests {
    use super::{must_join, shell_launcher};

    #[test]
    fn the_start_menu_and_search_hosts_are_closed_first_and_nothing_else() {
        for exe in [
            "StartMenuExperienceHost.exe",
            "SearchApp.exe",
            "SearchHost.exe",
            "shellexperiencehost.EXE",
        ] {
            assert!(shell_launcher(exe), "{exe}");
        }
        for exe in ["explorer.exe", "firefox.exe", "opencircle.exe", ""] {
            assert!(!shell_launcher(exe), "{exe}");
        }
    }

    #[test]
    fn the_overlay_joins_the_foreground_input_only_when_another_live_window_holds_it() {
        assert!(
            must_join(0x10116, 0x2000, false),
            "a refusing window holds it"
        );
        assert!(!must_join(0x2000, 0x2000, false), "ours already");
        assert!(
            !must_join(0, 0x2000, false),
            "no foreground: nothing to join"
        );
        assert!(
            !must_join(0x10116, 0x2000, true),
            "a hung window: never attach"
        );
    }
}
