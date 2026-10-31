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
            .chain(shown)
            .chain(Task::done(id))
    });
    (id, open)
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
        .chain(window::set_mode(id, window::Mode::Windowed))
        .chain(fade(id, true, how, false))
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

/// A screen-sized window (the capture overlay, the live layer): no frame, not
/// cut to a shape; each sets its own transparency and level.
/// Never a snip target (`no_pick`).
pub fn screen(settings: window::Settings) -> (window::Id, Task<window::Id>) {
    let (id, open) = open(settings);
    (id, open.then(|id| no_pick(id).chain(Task::done(id))))
}

/// Snip never outlines or picks the window `id` (`window_rect::no_pick`): the
/// overlay and the screen-sized ones (`screen`), toasts. Every other window of
/// ours is a target like any app's (Settings, the AI chat, pins).
pub fn no_pick<T: Send + 'static>(id: window::Id) -> Task<T> {
    window::run(id, |w| {
        if let Ok(handle) = w.window_handle()
            && let Some(hwnd) = super::shape::fadeable(handle.as_raw())
        {
            crate::capture::window_rect::no_pick(hwnd);
        }
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
    let (id, opened) = window::open(app_window(settings));
    let caption_off = move |id| {
        window::run(id, move |w| {
            if let Ok(handle) = w.window_handle() {
                super::shape::no_caption(handle.as_raw());
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

/// Windows: keeps the window out of every screen capture (`on`), or back in. The
/// scrolling capture's pill sits over the area it grabs: in the frames its
/// changing text would keep the page from ever settling, and end up in the
/// stitched image. Needs Windows 10 2004+; elsewhere nothing.
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
