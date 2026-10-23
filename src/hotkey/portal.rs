//! Wayland global shortcuts through org.freedesktop.portal.GlobalShortcuts (KDE Plasma 6,
//! GNOME 48+, Hyprland): one session, one portal shortcut per planned binding. Where
//! it's missing or the user declines, the user binds `opencircle capture` /
//! `opencircle run <action>` in the desktop's own shortcut settings (the CLI socket path).
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gtk::gio::{Cancellable, DBusCallFlags, DBusConnection, DBusSignalFlags};
use gtk::glib::{self, ToVariant, Variant, VariantDict, variant::ObjectPath};

use super::{Binding, bindings, route_id};
use crate::Triggers;
use crate::capture::portal::{DESKTOP, PATH, handle_path, request, session_bus};

const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

/// The portal thread's main context, to hand it work from `rebind`.
static WAKE: Mutex<Option<glib::MainContext>> = Mutex::new(None);

/// What the portal thread keeps between `bind` and `rebind`; only that thread touches it.
struct Portal {
    conn: DBusConnection,
    /// Session whose `Activated` signals count (shared with the signal callback).
    session: Rc<RefCell<String>>,
    /// Sessions opened after the first: makes each session's token unique.
    generation: u32,
}

thread_local! {
    static PORTAL: RefCell<Option<Portal>> = const { RefCell::new(None) };
}

/// Binds every planned shortcut on a background thread that then serves their presses.
/// Never fails: on any error it prints, once, how to bind the shortcuts by hand.
pub fn bind(tx: Triggers) {
    std::thread::spawn(move || {
        if let Err(e) = serve(tx) {
            eprintln!("{}", manual_hint(&e));
        }
    });
}

/// After a config change: opens a fresh session with the new shortcut list. (The portal
/// can't unbind or change a bound shortcut; a new session is the one way that works on
/// every desktop.) Before the portal is up, the new list is simply picked up when it binds.
// ponytail: not tested against a real portal (none on the dev machine); try KDE and GNOME.
pub fn rebind() {
    let ctx = WAKE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(ctx) = ctx {
        ctx.invoke(|| {
            PORTAL.with_borrow_mut(|portal| {
                let Some(p) = portal else { return };
                p.generation += 1;
                let old = p.session.borrow().clone();
                close(&p.conn, &old);
                match open_session(&p.conn, p.generation) {
                    Ok(path) => *p.session.borrow_mut() = path,
                    Err(e) => eprintln!("[opencircle] could not rebind shortcuts: {e}"),
                }
            });
        });
    }
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

    let session = Rc::new(RefCell::new(open_session(&conn, 0)?));

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
            {
                let session = session.clone();
                move |_, _, _, _, _, params| {
                    let id = activated_id(params, &session.borrow());
                    let trigger = id.and_then(|id| route_id(&bindings(), &id, Instant::now()));
                    if let Some(trigger) = trigger {
                        let _ = tx.unbounded_send(trigger);
                    }
                }
            },
        );
        PORTAL.set(Some(Portal {
            conn: conn.clone(),
            session,
            generation: 0,
        }));
        *WAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx.clone());
        glib::MainLoop::new(Some(&ctx), false).run();
    })
    .map_err(|e| e.to_string())
}

/// CreateSession + BindShortcuts for the current binding list; returns the session path.
fn open_session(conn: &DBusConnection, generation: u32) -> Result<String, String> {
    let token = match generation {
        0 => "opencircle".to_string(),
        n => format!("opencircle{n}"),
    };
    let sender = conn
        .unique_name()
        .ok_or("not connected to the session bus")?;
    let session = handle_path("session", &sender, &token);
    request(
        conn,
        IFACE,
        "CreateSession",
        Duration::from_secs(30),
        |handle| {
            let options = VariantDict::new(None);
            options.insert("handle_token", handle);
            options.insert("session_handle_token", token.as_str());
            (options,).to_variant()
        },
    )?;

    let planned = bindings().clone();
    // The desktop may show a dialog to confirm or change the keys: wait for the user.
    let bound = request(
        conn,
        IFACE,
        "BindShortcuts",
        Duration::from_secs(600),
        |handle| {
            let shortcuts: Vec<(String, VariantDict)> = planned
                .iter()
                .map(|b| (b.id.clone(), shortcut_options(b)))
                .collect();
            let options = VariantDict::new(None);
            options.insert("handle_token", handle);
            let path = ObjectPath::try_from(session.as_str()).expect("valid session path");
            (path, shortcuts, "", options).to_variant()
        },
    )?;
    let list = bound.lookup_value("shortcuts", None);
    for b in &planned {
        match list.as_ref().and_then(|list| trigger_of(list, &b.id)) {
            Some(keys) => eprintln!(
                "[opencircle] global shortcut {keys}: {} (desktop portal)",
                b.description
            ),
            None => eprintln!(
                "[opencircle] \"{}\" has no key assigned by the desktop; set one in its \
                 shortcut settings",
                b.description
            ),
        }
    }
    Ok(session)
}

fn shortcut_options(b: &Binding) -> VariantDict {
    let options = VariantDict::new(None);
    options.insert("description", b.description.as_str());
    options.insert("preferred_trigger", portal_trigger(&b.shortcut).as_str());
    options
}

fn close(conn: &DBusConnection, session: &str) {
    let _ = conn.call_sync(
        Some(DESKTOP),
        session,
        "org.freedesktop.portal.Session",
        "Close",
        None,
        None,
        DBusCallFlags::NONE,
        -1,
        None::<&Cancellable>,
    );
}

/// Our "Ctrl+Shift+Space" in the portal's spelling, "CTRL+SHIFT+space" (modifiers in
/// capitals, the key as an XKB keysym name). Only a hint: the user has the last word.
fn portal_trigger(shortcut: &str) -> String {
    shortcut
        .split('+')
        .map(|part| {
            let part = part.trim();
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => "CTRL".into(),
                "shift" => "SHIFT".into(),
                "alt" | "option" => "ALT".into(),
                "super" | "win" | "windows" | "meta" | "cmd" | "command" => "LOGO".into(),
                "space" => "space".into(),
                "enter" | "return" => "Return".into(),
                "esc" | "escape" => "Escape".into(),
                "printscreen" => "Print".into(),
                "scrolllock" => "Scroll_Lock".into(),
                k if k.len() == 1 => k.to_string(),
                k if k.starts_with('f') && k[1..].bytes().all(|b| b.is_ascii_digit()) => {
                    k.to_ascii_uppercase()
                }
                _ => part.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The shortcut id of an Activated(o session_handle, s shortcut_id, t timestamp,
/// a{sv} options) signal for our `session`.
fn activated_id(params: &Variant, session: &str) -> Option<String> {
    (params.is_container()
        && params.n_children() >= 2
        && params.child_value(0).str() == Some(session))
    .then(|| params.child_value(1).str().map(str::to_string))
    .flatten()
}

/// The human-readable keys the desktop gave shortcut `id`, from `a(sa{sv})`.
fn trigger_of(list: &Variant, id: &str) -> Option<String> {
    list.iter()
        .filter_map(|item| item.get::<(String, VariantDict)>())
        .find(|(found, _)| found == id)
        .and_then(|(_, info)| info.lookup::<String>("trigger_description").ok().flatten())
        .filter(|keys| !keys.is_empty())
}

fn manual_hint(why: &str) -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "opencircle".into());
    format!(
        "global shortcuts unavailable on this Wayland desktop ({why}).\n\
         Bind keys to the commands `{exe} capture` and `{exe} run <action>` (for example \
         `{exe} run translate`) yourself:\n\
         \x20 GNOME: Settings > Keyboard > View and Customize Shortcuts > Custom Shortcuts > Add Shortcut\n\
         \x20 KDE: System Settings > Keyboard > Shortcuts > Add New > Command or Script"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_our_session_and_returns_the_shortcut_id() {
        let session = "/org/freedesktop/portal/desktop/session/1_9/opencircle";
        let activated = |path: &str, id: &str| {
            let path = ObjectPath::try_from(path).unwrap();
            (path, id, 7u64, VariantDict::new(None)).to_variant()
        };
        assert_eq!(
            activated_id(&activated(session, "capture"), session).as_deref(),
            Some("capture")
        );
        assert_eq!(
            activated_id(&activated(session, "quick-lens-circle"), session).as_deref(),
            Some("quick-lens-circle")
        );
        let other = "/org/freedesktop/portal/desktop/session/1_9/x";
        assert_eq!(activated_id(&activated(other, "capture"), session), None);
        assert_eq!(activated_id(&"junk".to_variant(), session), None);
    }

    #[test]
    fn reads_the_assigned_trigger_per_shortcut() {
        let keys = |text: &str| {
            let info = VariantDict::new(None);
            info.insert("trigger_description", text);
            info
        };
        let list = vec![
            ("capture", keys("Ctrl+Shift+Space")),
            ("quick-lens-circle", keys("")),
            ("other", VariantDict::new(None)),
        ]
        .to_variant();
        assert_eq!(
            trigger_of(&list, "capture").as_deref(),
            Some("Ctrl+Shift+Space")
        );
        assert_eq!(trigger_of(&list, "quick-lens-circle"), None, "empty");
        assert_eq!(trigger_of(&list, "other"), None);
        assert_eq!(trigger_of(&list, "missing"), None);
    }

    #[test]
    fn converts_shortcuts_to_the_portal_spelling() {
        for (ours, theirs) in [
            ("Ctrl+Shift+Space", "CTRL+SHIFT+space"),
            ("control+alt+T", "CTRL+ALT+t"),
            ("Super+Shift+F5", "LOGO+SHIFT+F5"),
            ("ctrl+f12", "CTRL+F12"),
            ("Alt+Enter", "ALT+Return"),
            ("PrintScreen", "Print"),
            ("Ctrl+1", "CTRL+1"),
        ] {
            assert_eq!(portal_trigger(ours), theirs, "{ours}");
        }
    }

    #[test]
    fn hint_names_both_commands() {
        let hint = manual_hint("no portal");
        assert!(hint.contains("no portal") && hint.contains(" capture`"));
        assert!(hint.contains(" run <action>"));
    }
}
