//! Wayland global shortcut through org.freedesktop.portal.GlobalShortcuts (KDE Plasma 6,
//! GNOME 48+, Hyprland). Where it's missing or the user declines, the user binds
//! `opencircle capture` in the desktop's own shortcut settings (the CLI socket path).
use std::time::{Duration, Instant};

use gtk::gio::{Cancellable, DBusCallFlags, DBusSignalFlags};
use gtk::glib::{self, ToVariant, Variant, VariantDict, variant::ObjectPath};

use crate::capture::portal::{DESKTOP, PATH, handle_path, request, session_bus};
use crate::{Trigger, Triggers};

const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const SHORTCUT: &str = "capture";

/// Binds the capture shortcut on a background thread that then serves its presses.
/// Never fails: on any error it prints, once, how to bind the shortcut by hand.
pub fn bind(tx: Triggers) {
    std::thread::spawn(move || {
        if let Err(e) = serve(tx) {
            eprintln!("{}", manual_hint(&e));
        }
    });
}

fn serve(tx: Triggers) -> Result<(), String> {
    let conn = session_bus()?;
    // Host (non-Flatpak) apps have no app id unless they register one first; some
    // desktops refuse GlobalShortcuts without it. Older portals lack Registry: fine.
    let _ = conn.call_sync(
        Some(DESKTOP),
        PATH,
        "org.freedesktop.host.portal.Registry",
        "Register",
        Some(&("com.opencircle.app", VariantDict::new(None)).to_variant()),
        None,
        DBusCallFlags::NONE,
        -1,
        None::<&Cancellable>,
    );

    let sender = conn
        .unique_name()
        .ok_or("not connected to the session bus")?;
    let session = handle_path("session", &sender, "opencircle");
    request(
        &conn,
        IFACE,
        "CreateSession",
        Duration::from_secs(30),
        |token| {
            let options = VariantDict::new(None);
            options.insert("handle_token", token);
            options.insert("session_handle_token", "opencircle");
            (options,).to_variant()
        },
    )?;

    // The desktop may show a dialog to confirm or change the keys: wait for the user.
    let bound = request(
        &conn,
        IFACE,
        "BindShortcuts",
        Duration::from_secs(600),
        |token| {
            let shortcut = VariantDict::new(None);
            shortcut.insert("description", "Circle to search");
            shortcut.insert("preferred_trigger", "CTRL+SHIFT+space");
            let options = VariantDict::new(None);
            options.insert("handle_token", token);
            let path = ObjectPath::try_from(session.as_str()).expect("valid session path");
            (path, vec![(SHORTCUT, shortcut)], "", options).to_variant()
        },
    )?;
    match bound
        .lookup_value("shortcuts", None)
        .and_then(|list| trigger_of(&list))
    {
        Some(keys) => eprintln!("[opencircle] global shortcut {keys} (desktop portal)"),
        None => eprintln!(
            "[opencircle] shortcut registered with the desktop but no key assigned; set one in its shortcut settings"
        ),
    }

    let ctx = glib::MainContext::new();
    ctx.with_thread_default(|| {
        // Lives as long as the process: the session ends with our bus connection.
        let _sub = conn.signal_subscribe(
            Some(DESKTOP),
            Some(IFACE),
            Some("Activated"),
            Some(PATH),
            None,
            DBusSignalFlags::NONE,
            move |_, _, _, _, _, params| {
                if is_ours(params, &session) {
                    let _ = tx.unbounded_send(Trigger::Capture(Instant::now()));
                }
            },
        );
        glib::MainLoop::new(Some(&ctx), false).run();
    })
    .map_err(|e| e.to_string())
}

/// Activated(o session_handle, s shortcut_id, t timestamp, a{sv} options).
fn is_ours(params: &Variant, session: &str) -> bool {
    params.n_children() >= 2
        && params.child_value(0).str() == Some(session)
        && params.child_value(1).str() == Some(SHORTCUT)
}

/// The human-readable keys the desktop gave our shortcut, from `a(sa{sv})`.
fn trigger_of(list: &Variant) -> Option<String> {
    list.iter()
        .filter_map(|item| item.get::<(String, VariantDict)>())
        .find(|(id, _)| id == SHORTCUT)
        .and_then(|(_, info)| info.lookup::<String>("trigger_description").ok().flatten())
        .filter(|keys| !keys.is_empty())
}

fn manual_hint(why: &str) -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "opencircle".into());
    format!(
        "global shortcut unavailable on this Wayland desktop ({why}).\n\
         Bind a key to the command `{exe} capture` yourself:\n\
         \x20 GNOME: Settings > Keyboard > View and Customize Shortcuts > Custom Shortcuts > Add Shortcut\n\
         \x20 KDE: System Settings > Keyboard > Shortcuts > Add New > Command or Script"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_our_shortcut_in_our_session() {
        let session = "/org/freedesktop/portal/desktop/session/1_9/opencircle";
        let activated = |path: &str, id: &str| {
            let path = ObjectPath::try_from(path).unwrap();
            (path, id, 7u64, VariantDict::new(None)).to_variant()
        };
        assert!(is_ours(&activated(session, "capture"), session));
        assert!(!is_ours(&activated(session, "other"), session));
        assert!(!is_ours(
            &activated("/org/freedesktop/portal/desktop/session/1_9/x", "capture"),
            session
        ));
    }

    #[test]
    fn reads_the_assigned_trigger() {
        let info = VariantDict::new(None);
        info.insert("trigger_description", "Ctrl+Shift+Space");
        let list = vec![("other", VariantDict::new(None)), (SHORTCUT, info)].to_variant();
        assert_eq!(trigger_of(&list).as_deref(), Some("Ctrl+Shift+Space"));
        assert_eq!(
            trigger_of(&Vec::<(&str, VariantDict)>::new().to_variant()),
            None
        );
    }

    #[test]
    fn hint_names_the_capture_command() {
        let hint = manual_hint("no portal");
        assert!(hint.contains("no portal") && hint.contains(" capture`"));
    }
}
