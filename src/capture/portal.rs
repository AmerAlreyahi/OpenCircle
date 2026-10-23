//! Native Wayland capture through the desktop portal (org.freedesktop.portal.Screenshot).
//! D-Bus goes through GIO, already linked for the GTK tray: no new dependency.
//!
//! Limits: the portal hands back the whole desktop as one image, and Wayland tells no
//! client where the cursor is, so the frame is the full desktop at origin 0,0, scale 1.
// ponytail: exact on one monitor; multi-monitor or HiDPI needs the wl_output layout
// plus the output the overlay lands on, to crop and scale. Add it with real hardware.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use gtk::gio::{self, Cancellable, DBusCallFlags, DBusConnection, DBusSignalFlags};
use gtk::glib::{self, ToVariant, Variant, VariantDict};

use super::Frame;

pub const DESKTOP: &str = "org.freedesktop.portal.Desktop";
pub const PATH: &str = "/org/freedesktop/portal/desktop";

/// The full desktop, without asking the user to pick (`interactive` = false). The
/// desktop may still ask once for permission (GNOME does), hence the long wait.
pub fn capture() -> Result<Frame, String> {
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
    .map_err(err)?;
    let uri: String = results
        .lookup("uri")
        .ok()
        .flatten()
        .ok_or_else(|| err("no image in the answer".into()))?;
    let (path, _) = glib::filename_from_uri(&uri).map_err(|e| err(format!("{uri}: {e}")))?;
    let frame = super::from_png(path.as_os_str());
    // AGENTS.md: no capture left on disk, whatever the decode did.
    if let Err(e) = std::fs::remove_file(&path) {
        eprintln!(
            "could not delete the portal screenshot {}: {e}",
            path.display()
        );
    }
    frame
}

pub fn session_bus() -> Result<DBusConnection, String> {
    gio::bus_get_sync(gio::BusType::Session, None::<&Cancellable>)
        .map_err(|e| format!("no D-Bus session bus ({e})"))
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
) -> Result<VariantDict, String> {
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
fn response(code: u32) -> Result<(), String> {
    match code {
        0 => Ok(()),
        1 => Err("cancelled by the user".into()),
        _ => Err("refused by the desktop (permission denied?)".into()),
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
        assert!(response(1).unwrap_err().contains("cancelled"));
        assert!(response(2).unwrap_err().contains("refused"));
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
}
