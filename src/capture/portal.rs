//! Native Wayland capture through the desktop portal (org.freedesktop.portal.Screenshot).
//! D-Bus goes through GIO, already linked for the GTK tray: no new dependency.
//!
//! Limits: the portal hands back the whole desktop as one image, and Wayland tells no
//! client where the cursor is, so the frame is the full desktop at origin 0,0, scale 1.
// ponytail: exact on one monitor; multi-monitor or HiDPI needs the wl_output layout
// plus the output the overlay lands on, to crop and scale. Add it with real hardware.
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Once;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use gtk::gio::{self, Cancellable, DBusCallFlags, DBusConnection, DBusSignalFlags};
use gtk::glib::{self, ToVariant, Variant, VariantDict};

use super::{CaptureError, Frame};

pub const DESKTOP: &str = "org.freedesktop.portal.Desktop";
pub const PATH: &str = "/org/freedesktop/portal/desktop";

/// Our app id everywhere: portal registration, desktop file name, window app id,
/// icon name, packager identifier. GNOME grants shortcuts and screenshots only to
/// an app whose desktop file is `APP_ID.desktop`.
pub const APP_ID: &str = "com.opencircle.app";

/// The full desktop, without asking the user to pick (`interactive` = false). The
/// desktop may still ask once for permission (GNOME does), hence the long wait.
pub fn capture() -> Result<Frame, CaptureError> {
    Ok(read_and_delete(&screenshot()?)?)
}

/// One screenshot request just for its permission dialog: call while one of our
/// windows has the focus (GNOME won't ask otherwise). The image is deleted unread.
pub fn allow() -> Result<(), CaptureError> {
    forget_refusal();
    let path = screenshot()?;
    Ok(std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?)
}

/// After "Deny" GNOME refuses silently for good, and its Settings has no switch for
/// a non-Flatpak app. The user just clicked Allow, asking to be asked: drop the
/// stored "no" so the desktop shows its own dialog again (the desktop still decides).
fn forget_refusal() {
    const STORE: &str = "org.freedesktop.impl.portal.PermissionStore";
    const STORE_PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
    let Ok(conn) = session_bus() else { return };
    let call = |method: &str, args: Variant| {
        conn.call_sync(
            Some(STORE),
            STORE_PATH,
            STORE,
            method,
            Some(&args),
            None,
            DBusCallFlags::NONE,
            -1,
            None::<&Cancellable>,
        )
    };
    let Ok(found) = call("Lookup", ("screenshot", "screenshot").to_variant()) else {
        return; // no table yet: never asked
    };
    let table = found
        .child_value(0)
        .get::<std::collections::HashMap<String, Vec<String>>>();
    let ours = table.as_ref().and_then(|t| t.get(APP_ID));
    if ours.is_some_and(|v| v.iter().any(|p| p == "no")) {
        let _ = call(
            "DeletePermission",
            ("screenshot", "screenshot", APP_ID).to_variant(),
        );
    }
}

/// Asks the portal for a screenshot; returns the file it wrote.
fn screenshot() -> Result<PathBuf, CaptureError> {
    let err = |e: String| format!("Wayland screenshot portal: {e}");
    let conn = session_bus().map_err(err)?;
    let results = request(
        &conn,
        "org.freedesktop.portal.Screenshot",
        "Screenshot",
        Duration::from_secs(120),
        |token| {
            let options = VariantDict::new(None);
            options.insert("handle_token", token);
            options.insert("interactive", false);
            ("", options).to_variant()
        },
    )
    .map_err(|e| match e {
        CaptureError::Refused(why) => CaptureError::Refused(err(why)),
        CaptureError::Failed(why) => CaptureError::Failed(err(why)),
    })?;
    let uri: String = results
        .lookup("uri")
        .ok()
        .flatten()
        .ok_or_else(|| err("no image in the answer".into()))?;
    let (path, _) = glib::filename_from_uri(&uri).map_err(|e| err(format!("{uri}: {e}")))?;
    Ok(path)
}

/// Decodes the portal's file, then deletes it whatever the decode did (AGENTS.md:
/// no capture left on disk).
fn read_and_delete(path: &Path) -> Result<Frame, String> {
    let frame = super::from_png(path.as_os_str());
    if let Err(e) = std::fs::remove_file(path) {
        crate::log!(
            "could not delete the portal screenshot {}: {e}",
            path.display()
        );
    }
    frame
}

/// The session bus, with our app id registered on it first (once per process: the
/// portal takes an id only before any other portal call on the connection).
pub fn session_bus() -> Result<DBusConnection, String> {
    static REGISTER: Once = Once::new();
    let conn = gio::bus_get_sync(gio::BusType::Session, None::<&Cancellable>)
        .map_err(|e| format!("no D-Bus session bus ({e})"))?;
    REGISTER.call_once(|| register(&conn));
    Ok(conn)
}

/// Host (non-Flatpak) apps have no app id unless they register one, and the portal
/// only takes an id that has a desktop file: make sure there is one, then register.
/// Older portals lack Registry; an app the desktop launched may already have the id.
fn register(conn: &DBusConnection) {
    let wrote = match ensure_desktop_file() {
        Ok(Some(path)) => {
            crate::log!(
                "[opencircle] wrote {} so the desktop knows OpenCircle as {APP_ID}; \
                 delete it to remove that",
                path.display()
            );
            true
        }
        Ok(None) => false,
        Err(e) => {
            crate::log!("[opencircle] could not write {APP_ID}.desktop: {e}");
            false
        }
    };
    // A file written just now reaches the portal's app list a moment later.
    let mut tries = if wrote { 10 } else { 1 };
    loop {
        let called = conn.call_sync(
            Some(DESKTOP),
            PATH,
            "org.freedesktop.host.portal.Registry",
            "Register",
            Some(&(APP_ID, VariantDict::new(None)).to_variant()),
            None,
            DBusCallFlags::NONE,
            -1,
            None::<&Cancellable>,
        );
        tries -= 1;
        match called {
            Ok(_) => {
                return crate::log!("[opencircle] registered {APP_ID} with the desktop portal");
            }
            Err(e) if tries == 0 => {
                return crate::log!(
                    "[opencircle] could not register {APP_ID} with the desktop portal ({e}). \
                     Global shortcuts and screenshots may be refused: install the package, \
                     or check ~/.local/share/applications/{APP_ID}.desktop"
                );
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// A run without a package (raw binary, AppImage) has no desktop file: writes a
/// minimal user-level one, only if no `APP_ID.desktop` exists yet (or the one we
/// wrote names another path). Returns the path written.
fn ensure_desktop_file() -> std::io::Result<Option<PathBuf>> {
    let (data_home, system) = desktop_dirs()?;
    // An AppImage runs from a new mount each time: name the image itself.
    let exe = match std::env::var_os("APPIMAGE") {
        Some(image) => PathBuf::from(image),
        None => std::env::current_exe()?,
    };
    write_desktop_file(&data_home, &system, &exe)
}

/// The user's data dir and the system ones, where `applications/` lives.
fn desktop_dirs() -> std::io::Result<(PathBuf, Vec<PathBuf>)> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| std::io::Error::other("no HOME"))?;
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    Ok((data_home, dirs.split(':').map(PathBuf::from).collect()))
}

/// Run from a package (the .deb installed `APP_ID.desktop` and we are under /usr): a
/// user-level `APP_ID.desktop` that starts anything else (an AppImage since deleted)
/// shadows the package's launcher, and the menu entry fails. Removes that one.
pub fn repair_user_launcher() {
    if cfg!(debug_assertions) {
        return; // a dev build never touches the machine's launchers
    }
    let Ok(((data_home, system), exe)) =
        desktop_dirs().and_then(|d| Ok((d, std::env::current_exe()?)))
    else {
        return;
    };
    match remove_shadowing_launcher(&data_home, &system, &exe) {
        Ok(Some(path)) => crate::log!(
            "[opencircle] removed {}: it started another copy and hid the installed launcher",
            path.display()
        ),
        Ok(None) => {}
        Err(e) => crate::log!("[opencircle] could not repair the user launcher: {e}"),
    }
}

fn remove_shadowing_launcher(
    data_home: &Path,
    system: &[PathBuf],
    exe: &Path,
) -> std::io::Result<Option<PathBuf>> {
    let name = format!("{APP_ID}.desktop");
    let packaged = exe.starts_with("/usr")
        && system
            .iter()
            .any(|d| d.join("applications").join(&name).exists());
    let user = data_home.join("applications").join(&name);
    let Some(have) = packaged
        .then(|| std::fs::read_to_string(&user).ok())
        .flatten()
    else {
        return Ok(None);
    };
    if exec_program(&have).as_deref() == exe.to_str() {
        return Ok(None);
    }
    std::fs::remove_file(&user)?;
    Ok(Some(user))
}

/// The program of a desktop file's `Exec=` line (quoted or plain; its arguments dropped).
fn exec_program(entry: &str) -> Option<String> {
    let exec = entry.lines().find_map(|l| l.strip_prefix("Exec="))?.trim();
    match exec.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().map(str::to_string),
        None => exec.split_whitespace().next().map(str::to_string),
    }
}

/// Marks a desktop file OpenCircle wrote itself: it may rewrite that one, never a package's.
const GENERATED: &str = "X-OpenCircle-Generated=true";

fn write_desktop_file(
    data_home: &Path,
    system: &[PathBuf],
    exe: &Path,
) -> std::io::Result<Option<PathBuf>> {
    let name = format!("{APP_ID}.desktop");
    let installed = |dir: &Path| dir.join("applications").join(&name).exists();
    if system.iter().any(|d| installed(d)) {
        return Ok(None);
    }
    let path = data_home.join("applications").join(&name);
    let wanted = desktop_entry(exe);
    match std::fs::read_to_string(&path) {
        Ok(have) if have == wanted || !have.contains(GENERATED) => return Ok(None),
        Ok(_) => {} // ours, naming another path (a moved AppImage): rewrite
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::fs::create_dir_all(data_home.join("applications"))?;
    std::fs::write(&path, wanted)?;
    // Its icon, for the desktop's app list (where GNOME manages our shortcuts).
    let icons = data_home.join(ICON_DIR);
    if !icons.join(format!("{APP_ID}.png")).exists() {
        std::fs::create_dir_all(&icons)?;
        std::fs::write(icons.join(format!("{APP_ID}.png")), ICON)?;
    }
    Ok(Some(path))
}

const ICON_DIR: &str = "icons/hicolor/128x128/apps";
const ICON: &[u8] = include_bytes!("../../assets/icons/128x128.png");

/// The minimal entry. Not hidden: GNOME lists an app's global shortcuts on its
/// page in Settings → Apps, which skips hidden entries.
fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         # Written by OpenCircle: the desktop grants shortcuts and screenshots by app id.\n\
         # Delete it (and ~/.local/share/{ICON_DIR}/{APP_ID}.png) to remove it;\n\
         # the .deb and .rpm install their own.\n\
         Type=Application\n\
         Name=OpenCircle\n\
         Comment=Circle anything on screen to search, read, translate or ask about it\n\
         Exec={}\n\
         Icon={APP_ID}\n\
         StartupWMClass={APP_ID}\n\
         Terminal=false\n\
         Categories=Utility;\n\
         {GENERATED}\n",
        exec_quote(&exe.to_string_lossy())
    )
}

/// `path` as the Exec program (desktop entry spec): quoted unless every character
/// is plain; inside the quotes `"` `` ` `` `$` `\` take a backslash, then the
/// string's own escaping doubles every backslash. `%` is always doubled.
fn exec_quote(path: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+".contains(c);
    if path.chars().all(plain) {
        return path.to_string();
    }
    let mut out = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' => {
                out.push_str(r"\\");
                out.push(c);
            }
            '\\' => out.push_str(r"\\\\"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Calls a portal method that answers through a Request object and waits (up to
/// `wait`) for that Request's Response. `args` builds the call's parameters from the
/// handle token, which must go in the call's options as `handle_token`.
pub fn request(
    conn: &DBusConnection,
    interface: &str,
    method: &str,
    wait: Duration,
    args: impl FnOnce(&str) -> Variant,
) -> Result<VariantDict, CaptureError> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let token = format!("opencircle{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let sender = conn
        .unique_name()
        .ok_or("not connected to the session bus")?;
    let path = handle_path("request", &sender, &token);

    let ctx = glib::MainContext::new();
    let answer = ctx
        .with_thread_default(|| {
            let main_loop = glib::MainLoop::new(Some(&ctx), false);
            let answer = Rc::new(RefCell::new(None));
            // Subscribe before calling: the Response may come before the call returns.
            let sub = conn.signal_subscribe(
                Some(DESKTOP),
                Some("org.freedesktop.portal.Request"),
                Some("Response"),
                Some(&path),
                None,
                DBusSignalFlags::NONE,
                {
                    let (answer, main_loop) = (answer.clone(), main_loop.clone());
                    move |_, _, _, _, _, params| {
                        *answer.borrow_mut() = Some(params.clone());
                        main_loop.quit();
                    }
                },
            );
            let called = conn.call_sync(
                Some(DESKTOP),
                PATH,
                interface,
                method,
                Some(&args(&token)),
                None,
                DBusCallFlags::NONE,
                -1,
                None::<&Cancellable>,
            );
            if called.is_ok() {
                let timer = glib::timeout_source_new(wait, None, glib::Priority::DEFAULT, {
                    let main_loop = main_loop.clone();
                    move || {
                        main_loop.quit();
                        glib::ControlFlow::Break
                    }
                });
                timer.attach(Some(&ctx));
                main_loop.run();
                timer.destroy();
            }
            conn.signal_unsubscribe(sub);
            if called.is_ok() && answer.borrow().is_none() {
                // Timed out: end the request and its dialog (no Response follows).
                let _ = conn.call_sync(
                    Some(DESKTOP),
                    &path,
                    "org.freedesktop.portal.Request",
                    "Close",
                    None,
                    None,
                    DBusCallFlags::NONE,
                    -1,
                    None::<&Cancellable>,
                );
            }
            called
                .map(|_| answer.take())
                .map_err(|e| format!("{interface} unavailable ({e})"))
        })
        .map_err(|e| e.to_string())??
        .ok_or("no answer from the desktop (timed out)")?;

    let (code, results) = answer
        .get::<(u32, VariantDict)>()
        .ok_or("malformed answer from the desktop")?;
    response(code).map(|()| results)
}

/// org.freedesktop.portal.Request::Response codes.
fn response(code: u32) -> Result<(), CaptureError> {
    match code {
        0 => Ok(()),
        1 => Err("cancelled by the user".into()),
        // The desktop said no (GNOME also says no when it would have to ask while
        // another app has the focus).
        _ => Err(CaptureError::Refused(
            "refused by the desktop (permission denied?)".into(),
        )),
    }
}

/// `/org/freedesktop/portal/desktop/<kind>/<sender>/<token>`, where the sender is
/// our unique bus name without the ':' and with '.' as '_' (portal spec).
pub fn handle_path(kind: &str, sender: &str, token: &str) -> String {
    let sender = sender.trim_start_matches(':').replace('.', "_");
    format!("{PATH}/{kind}/{sender}/{token}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_handle_paths_from_the_unique_name() {
        assert_eq!(
            handle_path("request", ":1.42", "opencircle0"),
            "/org/freedesktop/portal/desktop/request/1_42/opencircle0"
        );
        assert_eq!(
            handle_path("session", ":1.7", "oc"),
            "/org/freedesktop/portal/desktop/session/1_7/oc"
        );
    }

    #[test]
    fn maps_response_codes() {
        assert!(response(0).is_ok());
        assert!(!response(1).unwrap_err().is_refused());
        assert!(response(1).unwrap_err().to_string().contains("cancelled"));
        assert!(response(2).unwrap_err().is_refused());
    }

    #[test]
    fn decodes_percent_encoded_file_uris() {
        let (path, _) = glib::filename_from_uri(
            "file:///home/me/Pictures/Screenshot%20from%20%C3%A9t%C3%A9.png",
        )
        .unwrap();
        assert_eq!(
            path,
            std::path::Path::new("/home/me/Pictures/Screenshot from été.png")
        );
        assert!(glib::filename_from_uri("https://example.com/a.png").is_err());
    }

    #[test]
    fn reads_a_response_signal_body() {
        let results = VariantDict::new(None);
        results.insert("uri", "file:///tmp/a.png");
        let body = (0u32, results).to_variant();
        let (code, results) = body.get::<(u32, VariantDict)>().unwrap();
        assert_eq!(code, 0);
        assert_eq!(
            results.lookup::<String>("uri").unwrap().as_deref(),
            Some("file:///tmp/a.png")
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-portal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_desktop_entry_names_our_id_and_the_exe() {
        let entry = desktop_entry(Path::new("/opt/oc/opencircle"));
        let lines: Vec<&str> = entry.lines().collect();
        assert_eq!(lines[0], "[Desktop Entry]");
        for want in [
            "Type=Application",
            "Name=OpenCircle",
            "Exec=/opt/oc/opencircle",
            "Icon=com.opencircle.app",
            GENERATED,
        ] {
            assert!(lines.contains(&want), "{want} in {entry}");
        }
        // GNOME's Settings → Apps (where the shortcuts are changed) skips hidden apps.
        assert!(!entry.contains("NoDisplay"));
        // Spaces and specials are quoted per the desktop entry spec.
        assert_eq!(exec_quote("/home/a b/oc"), r#""/home/a b/oc""#);
        assert_eq!(exec_quote("/x/$y%"), r#""/x/\\$y%%""#);
        assert_eq!(exec_quote(r"/x\y"), r#""/x\\\\y""#);
    }

    #[test]
    fn writes_the_desktop_file_only_if_missing() {
        let dir = scratch("desktop");
        let (home, sys) = (dir.join("home"), dir.join("sys"));
        let file = home.join("applications/com.opencircle.app.desktop");
        let exe = Path::new("/opt/oc/opencircle");

        // Nothing anywhere: written.
        let wrote = write_desktop_file(&home, std::slice::from_ref(&sys), exe).unwrap();
        assert_eq!(wrote.as_deref(), Some(file.as_path()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), desktop_entry(exe));
        let icon = home.join(ICON_DIR).join("com.opencircle.app.png");
        assert_eq!(std::fs::read(&icon).unwrap(), ICON, "with its icon");
        // Again: already there, untouched.
        assert_eq!(
            write_desktop_file(&home, std::slice::from_ref(&sys), exe).unwrap(),
            None
        );
        // Ours, for a moved AppImage: rewritten to the new path.
        let moved = Path::new("/home/me/OpenCircle.AppImage");
        assert!(
            write_desktop_file(&home, std::slice::from_ref(&sys), moved)
                .unwrap()
                .is_some()
        );
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("Exec=/home/me/OpenCircle.AppImage")
        );
        // The user's own file (no marker) is never replaced.
        std::fs::write(&file, "[Desktop Entry]\nExec=mine\n").unwrap();
        assert_eq!(
            write_desktop_file(&home, std::slice::from_ref(&sys), exe).unwrap(),
            None
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "[Desktop Entry]\nExec=mine\n"
        );
        // A package's system-wide file: nothing written at user level.
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir_all(sys.join("applications")).unwrap();
        std::fs::write(sys.join("applications/com.opencircle.app.desktop"), "x").unwrap();
        assert_eq!(write_desktop_file(&home, &[sys], exe).unwrap(), None);
        assert!(!file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// #146: the .deb is installed, but an old AppImage's user-level launcher (same id)
    /// shadows the system one in the app menu.
    #[test]
    fn a_stale_user_launcher_is_removed_when_run_from_a_package() {
        let dir = scratch("stale");
        let (home, sys) = (dir.join("home"), dir.join("sys"));
        let user = home.join("applications/com.opencircle.app.desktop");
        let system = sys.join("applications/com.opencircle.app.desktop");
        let packaged = Path::new("/usr/bin/opencircle");
        let run = |exe: &Path| remove_shadowing_launcher(&home, std::slice::from_ref(&sys), exe);
        std::fs::create_dir_all(user.parent().unwrap()).unwrap();
        std::fs::write(
            &user,
            desktop_entry(Path::new("/home/me/My Apps/oc.AppImage")),
        )
        .unwrap();

        // No package launcher yet (a raw binary or an AppImage run): never touched.
        assert_eq!(run(packaged).unwrap(), None);
        assert!(user.exists());
        std::fs::create_dir_all(system.parent().unwrap()).unwrap();
        std::fs::write(&system, "x").unwrap();
        // Package launcher, but this is not the packaged copy: left alone.
        assert_eq!(run(Path::new("/home/me/opencircle")).unwrap(), None);
        assert!(user.exists());
        // The packaged copy runs: the launcher that starts another copy goes.
        assert_eq!(run(packaged).unwrap().as_deref(), Some(user.as_path()));
        assert!(!user.exists());
        assert_eq!(run(packaged).unwrap(), None, "nothing left to do");
        // One that starts this very copy stays.
        std::fs::write(&user, desktop_entry(packaged)).unwrap();
        assert_eq!(run(packaged).unwrap(), None);
        assert!(user.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_the_program_of_an_exec_line() {
        for (exec, want) in [
            ("Exec=/usr/bin/opencircle", "/usr/bin/opencircle"),
            ("Exec=/usr/bin/opencircle %U", "/usr/bin/opencircle"),
            ("Exec=\"/home/me/My Apps/oc\" %U", "/home/me/My Apps/oc"),
        ] {
            let entry = format!("[Desktop Entry]\nName=x\n{exec}\nIcon=y\n");
            assert_eq!(exec_program(&entry).as_deref(), Some(want));
        }
        assert_eq!(exec_program("[Desktop Entry]\nName=x\n"), None);
    }

    #[test]
    fn a_screenshot_file_is_deleted_even_when_it_does_not_decode() {
        let dir = scratch("shot");
        let bad = dir.join("Screenshot.png");
        std::fs::write(&bad, b"not a png").unwrap();
        assert!(read_and_delete(&bad).is_err());
        assert!(!bad.exists(), "deleted");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
