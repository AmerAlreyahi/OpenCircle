//! OS plumbing with no UI: opening links and files, detached process start,
//! the desktop session's kind, the user's language.

pub mod locale;
pub mod mem;
pub mod open;
pub mod serve_once;
pub mod session;
pub mod spawn;

use std::ffi::OsString;

/// `s` as the NUL-terminated UTF-16 the Windows APIs take.
#[cfg(any(windows, test))]
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// A test hook's variable (`OPENCIRCLE_TEST_*`: screenshots of our windows to
/// a folder, a PNG instead of the screen, a test crash). Release builds read
/// hooks only with the one opt-in `OPENCIRCLE_TEST_HOOKS=1` (the VM scripts
/// set it), so a stray variable can't make a user's copy write captures to
/// disk. `OPENCIRCLE_TEST_UPDATE_URL` is not one: the update gate needs it in
/// published builds (and an update still needs the release key).
pub fn test_hook(name: &str) -> Option<OsString> {
    hooks_on(
        cfg!(debug_assertions),
        std::env::var_os("OPENCIRCLE_TEST_HOOKS"),
    )
    .then(|| std::env::var_os(name))
    .flatten()
}

fn hooks_on(debug_build: bool, opt_in: Option<OsString>) -> bool {
    debug_build || opt_in.is_some_and(|v| v == "1")
}

#[test]
fn wide_strings_end_in_a_nul() {
    assert_eq!(wide("a\u{e9}"), [0x61, 0xe9, 0]);
    assert_eq!(wide(""), [0]);
}

#[test]
fn test_hooks_need_the_opt_in_in_release_builds() {
    assert!(!hooks_on(false, None), "release: off by default");
    assert!(!hooks_on(false, Some("0".into())));
    assert!(!hooks_on(false, Some("".into())));
    assert!(hooks_on(false, Some("1".into())), "the VM scripts' opt-in");
    assert!(hooks_on(true, None), "debug and test builds: always on");
}

/// GStreamer plugin-path variables an AppImage's launcher points only into the image
/// (`$APPDIR/usr/lib/gstreamer-1.0`, which holds no plugins): they hide the system's
/// plugins, so the ScreenCast capture finds no `pipewiresrc` and GNOME asks on every
/// capture. Such a variable is dropped; one that names any folder outside the image stays.
const GST_PATH_VARS: [&str; 2] = ["GST_PLUGIN_SYSTEM_PATH_1_0", "GST_PLUGIN_SYSTEM_PATH"];

/// Whether `value` (a `:`-separated path list) only points inside `appdir`.
fn only_inside(value: &str, appdir: &str) -> bool {
    let appdir = appdir.trim_end_matches('/');
    !appdir.is_empty()
        && value
            .split(':')
            .filter(|p| !p.is_empty())
            .all(|p| p == appdir || p.starts_with(&format!("{appdir}/")))
}

/// The program a desktop entry, shortcut or autostart line should start: the
/// AppImage itself (its mount, `current_exe`, is new on every start), else this exe.
pub fn launcher() -> std::io::Result<std::path::PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(image) => Ok(image.into()),
        None => std::env::current_exe(),
    }
}

/// Makes libX11 thread-safe: run first thing in `main`, before any thread starts.
/// The tray's GTK thread (AppIndicator) and winit both talk Xlib; without
/// `XInitThreads()` before the first Xlib call of the process they race and
/// crash (KDE X11 at every start, #172; GNOME X11 only by luck of timing).
/// libX11 is opened at run time, so a Wayland-only system without it still starts.
/// Returns whether libX11 was there.
#[cfg(target_os = "linux")]
pub fn init_x11_threads() -> bool {
    // SAFETY: dlopen/dlsym by name, the handle is kept open (never closed);
    // XInitThreads takes nothing, is called before any thread exists, and a
    // second call (winit's own) is a no-op.
    unsafe {
        let lib = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if lib.is_null() {
            return false;
        }
        let f = libc::dlsym(lib, c"XInitThreads".as_ptr());
        if !f.is_null() {
            let init: extern "C" fn() -> std::ffi::c_int = std::mem::transmute(f);
            init();
        }
        true
    }
}

#[cfg(target_os = "linux")]
#[test]
fn xlib_is_made_thread_safe_first_in_main() {
    // the crash comes back if anything starts a thread or opens X before it
    let main = include_str!("../main.rs");
    let body = &main[main.find("fn main() -> ExitCode {").unwrap()..];
    let first = body.lines().nth(1).unwrap().trim();
    assert_eq!(first, "#[cfg(target_os = \"linux\")]");
    assert!(
        body.lines()
            .nth(2)
            .unwrap()
            .contains("sys::init_x11_threads()")
    );
}

/// Run first thing in `main`, before any thread starts (env changes are not thread-safe).
pub fn fix_appimage_env() {
    let Some(appdir) = std::env::var_os("APPDIR") else {
        return;
    };
    let appdir = appdir.to_string_lossy().into_owned();
    for var in GST_PATH_VARS {
        if std::env::var(var).is_ok_and(|v| only_inside(&v, &appdir)) {
            // SAFETY: called at the start of `main`, before any other thread exists.
            unsafe { std::env::remove_var(var) };
        }
    }
}

#[cfg(test)]
mod appimage_env_tests {
    use super::only_inside;

    #[test]
    fn a_gstreamer_path_only_inside_the_appimage_is_dropped() {
        let app = "/tmp/.mount_OpenCiXYZ";
        assert!(only_inside(
            "/tmp/.mount_OpenCiXYZ/usr/lib/gstreamer-1.0:",
            app
        ));
        assert!(only_inside(
            "/tmp/.mount_OpenCiXYZ/usr/lib/gstreamer-1.0",
            &format!("{app}/")
        ));
        assert!(!only_inside(
            "/tmp/.mount_OpenCiXYZ/usr/lib/gstreamer-1.0:/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
            app
        ));
        assert!(!only_inside("/usr/lib/x86_64-linux-gnu/gstreamer-1.0", app));
        assert!(!only_inside("/tmp/.mount_OpenCiXYZother/lib", app));
        assert!(!only_inside("/anything", ""));
    }
}
