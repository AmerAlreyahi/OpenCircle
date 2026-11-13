//! KDE Plasma without the GlobalShortcuts portal (Plasma 5 on X11 and Wayland, Plasma 6
//! on X11): KDE's own shortcut service, KGlobalAccel, spoken to over D-Bus the way KDE's
//! own apps do (no Qt or KDE library linked). Every planned shortcut is one action of
//! the "OpenCircle" component: it shows in System Settings' Shortcuts page, where the
//! user may rebind it, KDE keeps it across restarts, refuses a key another action holds
//! and says which, and presses come back as `globalShortcutPressed` signals.
//!
//! Who wins: KDE keeps the key it stored (a rebinding there is honoured, Settings shows
//! it); a key changed in OpenCircle's Settings is written through to KDE.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use gtk::gio::{self, Cancellable, DBusCallFlags, DBusConnection, DBusSignalFlags};
use gtk::glib::{self, ToVariant, Variant};

use super::{Binding, bindings, desktop_spelling, set_desktop_keys};
use crate::Triggers;
use crate::config::{Config, parse_shortcut};

const BUS: &str = "org.kde.kglobalaccel";
const PATH: &str = "/kglobalaccel";
const IFACE: &str = "org.kde.KGlobalAccel";
const COMPONENT_IFACE: &str = "org.kde.kglobalaccel.Component";
/// Our component: the app id, so KDE can match it to our desktop file.
const COMPONENT: &str = crate::shared::platform::capture::portal::APP_ID;
const FRIENDLY: &str = "OpenCircle";

// `setShortcutKeys` flags (KGlobalAccel's SetShortcutFlag).
const IS_DEFAULT: u32 = 1;
const SET_PRESENT: u32 = 2;
const NO_AUTOLOADING: u32 = 4;

/// KGlobalAccel holds our shortcuts now (set by `adopt`).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// `blockGlobalShortcuts` is on (a shortcut field records).
static BLOCKED: AtomicBool = AtomicBool::new(false);

/// Per binding id, the shortcut OpenCircle last wrote: a different one now is a change
/// made in OpenCircle, written through; the same one leaves KDE's key as it is.
static WRITTEN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
/// Per binding id, the key KDE has for it (`None` = no key).
static KEYS: Mutex<Vec<(String, Option<HotKey>)>> = Mutex::new(Vec::new());
/// Our keys KDE refused, with the action that holds each.
static HELD: Mutex<Vec<(HotKey, String)>> = Mutex::new(Vec::new());

fn lock<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Is `desktop` (`XDG_CURRENT_DESKTOP`) KDE?
pub fn is_kde(desktop: &str) -> bool {
    desktop.split(':').any(|d| d.eq_ignore_ascii_case("kde"))
}

/// Are our shortcuts KGlobalAccel's now?
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

fn bus() -> Option<DBusConnection> {
    gio::bus_get_sync(gio::BusType::Session, None::<&Cancellable>).ok()
}

fn call(
    conn: &DBusConnection,
    method: &str,
    args: Variant,
    flags: DBusCallFlags,
) -> Result<Variant, String> {
    conn.call_sync(
        Some(BUS),
        PATH,
        IFACE,
        method,
        Some(&args),
        None,
        flags,
        3000,
        None::<&Cancellable>,
    )
    .map_err(|e| e.to_string())
}

/// Takes the shortcuts over when this desktop is KDE and KGlobalAccel answers, then
/// serves their presses on a thread of its own. `false` = not applicable.
pub fn adopt(tx: Triggers) -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if cfg!(test) || !is_kde(&desktop) {
        return false;
    }
    let Some(conn) = bus() else { return false };
    let empty = ().to_variant();
    if let Err(e) = call(&conn, "allMainComponents", empty, DBusCallFlags::NONE) {
        crate::log!("[opencircle] KDE's shortcut service (KGlobalAccel) is not answering: {e}");
        return false;
    }
    ACTIVE.store(true, Ordering::Relaxed);
    let planned = bindings().clone();
    // What KDE already stores wins at start: a rebinding made there while we were away.
    // An action KDE has that is no longer planned (cleared, Labs off) goes at the write.
    let mut seed: Vec<(String, String)> = (planned.iter())
        .map(|b| (b.id.clone(), b.shortcut.clone()))
        .collect();
    let stale = ours(&conn, DBusCallFlags::NONE).into_iter();
    seed.extend(
        stale
            .filter(|n| !planned.iter().any(|b| b.id == *n))
            .map(|n| (n, String::new())),
    );
    *lock(&WRITTEN) = seed;
    std::thread::spawn({
        let conn = conn.clone();
        move || listen(&conn, tx)
    });
    write(&conn, &planned);
    crate::log!(
        "[opencircle] global shortcuts: KDE's shortcut service holds them (System Settings > \
         Shortcuts > OpenCircle)"
    );
    true
}

/// Brings KDE in line with `planned` (`held_off`: every global shortcut blocked while a
/// shortcut field records, so the recorder gets every key, KDE's own too).
pub fn sync(planned: &[Binding], held_off: bool) {
    let Some(conn) = bus() else { return };
    if BLOCKED.swap(held_off, Ordering::Relaxed) != held_off {
        let _ = call(
            &conn,
            "blockGlobalShortcuts",
            (held_off,).to_variant(),
            DBusCallFlags::NONE,
        );
    }
    write(&conn, planned);
}

/// The one writer: registers every planned action, writes a key changed in OpenCircle
/// through, removes the actions that are no longer planned, and keeps what KDE answered.
fn write(conn: &DBusConnection, planned: &[Binding]) {
    let defaults = super::plan(&Config {
        labs: crate::labs::Labs::all_on(),
        ..Config::default()
    })
    .0;
    let mut written = lock(&WRITTEN);
    for (gone, _) in written
        .iter()
        .filter(|(id, _)| !planned.iter().any(|b| b.id == *id))
    {
        let _ = call(
            conn,
            "unregister",
            (COMPONENT, gone.as_str()).to_variant(),
            DBusCallFlags::NONE,
        );
    }
    let mut keys = Vec::new();
    let mut held = Vec::new();
    for b in planned {
        let Some(qt) = qt_key(b.hotkey) else {
            crate::log!(
                "[opencircle] KDE: {} has no KDE key code; left unbound",
                b.shortcut
            );
            continue;
        };
        let id = action_id(b);
        let default: Vec<i32> = (defaults.iter())
            .find(|d| d.id == b.id)
            .and_then(|d| qt_key(d.hotkey))
            .into_iter()
            .collect();
        let changed = written.iter().find(|(i, _)| *i == b.id).map(|(_, s)| s) != Some(&b.shortcut);
        let _ = call(
            conn,
            "doRegister",
            (id.clone(),).to_variant(),
            DBusCallFlags::NONE,
        );
        let _ = call(
            conn,
            "setShortcutKeys",
            set_keys(&id, &default, IS_DEFAULT),
            DBusCallFlags::NONE,
        );
        let flags = SET_PRESENT | if changed { NO_AUTOLOADING } else { 0 };
        let got = call(
            conn,
            "setShortcutKeys",
            set_keys(&id, &[qt], flags),
            DBusCallFlags::NONE,
        )
        .ok()
        .and_then(|r| first_key(&r.child_value(0)));
        match got {
            Some(k) if k == b.hotkey => {
                crate::log!(
                    "[opencircle] global shortcut {}: {} (KDE)",
                    b.shortcut,
                    b.description
                )
            }
            Some(k) => crate::log!(
                "[opencircle] \"{}\" stays on {}, set in KDE's System Settings (not {})",
                b.description,
                spelling(k),
                b.shortcut
            ),
            None => {
                let by = holder(conn, qt);
                crate::log!(
                    "[opencircle] shortcut {} ({}) is unavailable: {}",
                    b.shortcut,
                    b.description,
                    by.as_deref()
                        .map_or("KDE gave it no key".into(), |n| format!("used by {n}"))
                );
                held.extend(by.map(|n| (b.hotkey, n)));
            }
        }
        keys.push((b.id.clone(), got));
    }
    *written = (planned.iter())
        .map(|b| (b.id.clone(), b.shortcut.clone()))
        .collect();
    *lock(&HELD) = held;
    *lock(&KEYS) = keys;
    publish(planned);
}

/// Hands Settings what KDE has, per binding (`desktop_key`).
fn publish(planned: &[Binding]) {
    let keys = lock(&KEYS);
    set_desktop_keys(
        planned
            .iter()
            .filter_map(|b| {
                let (_, key) = keys.iter().find(|(id, _)| *id == b.id)?;
                Some((b.portal_id(), key.map(spelling)))
            })
            .collect(),
    );
}

/// Presses and rebindings, for as long as the process runs.
fn listen(conn: &DBusConnection, tx: Triggers) {
    let ctx = glib::MainContext::new();
    let _ = ctx.with_thread_default(|| {
        let _pressed = conn.signal_subscribe(
            Some(BUS),
            Some(COMPONENT_IFACE),
            Some("globalShortcutPressed"),
            None,
            None,
            DBusSignalFlags::NONE,
            move |_, _, _, _, _, params| {
                // a shortcut field is recording: the press is its, not ours
                let id = pressed(params).filter(|_| !super::suspended());
                let trigger = id.and_then(|id| {
                    (bindings().iter())
                        .find(|b| b.id == id)
                        .map(|b| b.trigger(Instant::now()))
                });
                if let Some(trigger) = trigger {
                    let _ = tx.unbounded_send(trigger);
                }
            },
        );
        // The user changed one of ours in System Settings.
        let _changed = conn.signal_subscribe(
            Some(BUS),
            Some(IFACE),
            Some("yourShortcutsChanged"),
            Some(PATH),
            None,
            DBusSignalFlags::NONE,
            |_, _, _, _, _, params| {
                let Some((id, key)) = changed(params) else {
                    return;
                };
                crate::log!(
                    "[opencircle] KDE: {id} is now on {}",
                    key.map_or("no key".into(), spelling)
                );
                let mut keys = lock(&KEYS);
                keys.retain(|(i, _)| *i != id);
                keys.push((id, key));
                drop(keys);
                publish(&bindings());
            },
        );
        glib::MainLoop::new(Some(&ctx), false).run();
    });
}

/// Lets go of our keys while OpenCircle isn't running (quit): KDE keeps them, unused,
/// and the keys reach other apps again until the next start.
pub fn release() {
    if !active() {
        return;
    }
    let Some(conn) = bus() else { return };
    for b in bindings().iter() {
        let _ = call(
            &conn,
            "setInactive",
            (action_id(b),).to_variant(),
            DBusCallFlags::NONE,
        );
    }
    if BLOCKED.swap(false, Ordering::Relaxed) {
        let _ = call(
            &conn,
            "blockGlobalShortcuts",
            (false,).to_variant(),
            DBusCallFlags::NONE,
        );
    }
}

/// Removes every action of ours from KDE (reset / uninstall), also when another session
/// type registered them. Never starts KDE's service where it isn't running.
pub fn forget() {
    ACTIVE.store(false, Ordering::Relaxed);
    let Some(conn) = bus() else { return };
    for id in ours(&conn, DBusCallFlags::NO_AUTO_START) {
        let _ = call(
            &conn,
            "unregister",
            (COMPONENT, id.as_str()).to_variant(),
            DBusCallFlags::NO_AUTO_START,
        );
    }
    lock(&WRITTEN).clear();
}

/// The names of every action KDE keeps for our component.
fn ours(conn: &DBusConnection, flags: DBusCallFlags) -> Vec<String> {
    let id = ([COMPONENT.to_string()].to_vec(),).to_variant();
    call(conn, "allActionsForComponent", id, flags)
        .map(|all| action_names(&all.child_value(0)))
        .unwrap_or_default()
}

/// The KDE action that holds `shortcut` instead of us (from the last write).
pub fn used_by(shortcut: &str) -> Option<String> {
    let hotkey = parse_shortcut(shortcut).ok()?;
    let held = lock(&HELD);
    held.iter()
        .find(|(k, _)| *k == hotkey)
        .map(|(_, n)| n.clone())
}

/// When a key is recorded: the KDE action (not ours) that already has it.
pub fn holder_of(shortcut: &str) -> Option<String> {
    if !active() {
        return None;
    }
    let qt = qt_key(parse_shortcut(shortcut).ok()?)?;
    holder(&bus()?, qt)
}

fn holder(conn: &DBusConnection, qt: i32) -> Option<String> {
    let found = call(
        conn,
        "getGlobalShortcutsByKey",
        (qt,).to_variant(),
        DBusCallFlags::NONE,
    );
    holder_in(&found.ok()?.child_value(0))
}

/// ["com.opencircle.app", id, "OpenCircle", description]: KGlobalAccel's action id.
fn action_id(b: &Binding) -> Vec<String> {
    [COMPONENT, &b.id, FRIENDLY, &b.description]
        .map(String::from)
        .to_vec()
}

/// `setShortcutKeys(as actionId, a(ai) keys, u flags)` with one one-chord sequence per key.
/// A sequence is ALWAYS four chords (0 = none): KDE 5 reads four, and a shorter one
/// aborts the reader (KWin 5.24 on Wayland crashed on one).
fn set_keys(id: &[String], keys: &[i32], flags: u32) -> Variant {
    let seqs: Vec<(Vec<i32>,)> = keys.iter().map(|k| (vec![*k, 0, 0, 0],)).collect();
    (id.to_vec(), seqs, flags).to_variant()
}

/// The first chord of the first sequence of an `a(ai)` answer, when it is a key we know.
fn first_key(seqs: &Variant) -> Option<HotKey> {
    let seqs = seqs.get::<Vec<(Vec<i32>,)>>()?;
    let (chords,) = seqs.into_iter().next()?;
    from_qt(*chords.iter().find(|k| **k != 0)?)
}

/// The action id of `globalShortcutPressed(s component, s action, x timestamp)` when it
/// is one of ours.
fn pressed(params: &Variant) -> Option<String> {
    let (component, action, _) = params.get::<(String, String, i64)>()?;
    (component == COMPONENT).then_some(action)
}

/// One of ours rebound: `yourShortcutsChanged(as actionId, a(ai) keys)` -> (id, key).
fn changed(params: &Variant) -> Option<(String, Option<HotKey>)> {
    if params.n_children() != 2 {
        return None;
    }
    let id = params.child_value(0).get::<Vec<String>>()?;
    match id.as_slice() {
        [component, action, ..] if component == COMPONENT => {
            Some((action.clone(), first_key(&params.child_value(1))))
        }
        _ => None,
    }
}

/// "KWin: Switch to Desktop 1" from `getGlobalShortcutsByKey`'s `a(ssssssaiai)`
/// (action, action name, component, component name, context, context name, keys,
/// default keys; as Plasma 5.24 answers), the first entry that isn't ours.
fn holder_in(list: &Variant) -> Option<String> {
    type Info = (
        String,
        String,
        String,
        String,
        String,
        String,
        Vec<i32>,
        Vec<i32>,
    );
    list.iter()
        .filter_map(|v| v.get::<Info>())
        .find(|i| i.2 != COMPONENT)
        .map(|i| format!("{}: {}", i.3, i.1))
}

/// The action names (second field) of `allActionsForComponent`'s `aas`.
fn action_names(list: &Variant) -> Vec<String> {
    (list
        .get::<Vec<Vec<String>>>()
        .unwrap_or_default()
        .into_iter())
    .filter_map(|id| id.into_iter().nth(1))
    .collect()
}

const SHIFT: i32 = 0x0200_0000;
const CTRL: i32 = 0x0400_0000;
const ALT: i32 = 0x0800_0000;
const META: i32 = 0x1000_0000;
const KEYPAD: i32 = 0x2000_0000;
const MODS: [(Modifiers, i32); 4] = [
    (Modifiers::SHIFT, SHIFT),
    (Modifiers::CONTROL, CTRL),
    (Modifiers::ALT, ALT),
    (Modifiers::META, META),
];

/// Qt's key codes for the keys that aren't a letter, digit or F-key.
const QT_KEYS: [(Code, i32); 29] = [
    (Code::Space, 0x20),
    (Code::Quote, 0x27),
    (Code::Comma, 0x2c),
    (Code::Minus, 0x2d),
    (Code::Period, 0x2e),
    (Code::Slash, 0x2f),
    (Code::Semicolon, 0x3b),
    (Code::Equal, 0x3d),
    (Code::BracketLeft, 0x5b),
    (Code::Backslash, 0x5c),
    (Code::BracketRight, 0x5d),
    (Code::Backquote, 0x60),
    (Code::Escape, 0x0100_0000),
    (Code::Tab, 0x0100_0001),
    (Code::Backspace, 0x0100_0003),
    (Code::Enter, 0x0100_0004),
    (Code::Insert, 0x0100_0006),
    (Code::Delete, 0x0100_0007),
    (Code::Pause, 0x0100_0008),
    (Code::PrintScreen, 0x0100_0009),
    (Code::Home, 0x0100_0010),
    (Code::End, 0x0100_0011),
    (Code::ArrowLeft, 0x0100_0012),
    (Code::ArrowUp, 0x0100_0013),
    (Code::ArrowRight, 0x0100_0014),
    (Code::ArrowDown, 0x0100_0015),
    (Code::PageUp, 0x0100_0016),
    (Code::PageDown, 0x0100_0017),
    (Code::ScrollLock, 0x0100_0026),
];
const QT_F1: i32 = 0x0100_0030;

/// "Ctrl+Shift+Space" as KDE's key code (Qt's key | modifiers).
fn qt_key(hotkey: HotKey) -> Option<i32> {
    let name = format!("{:?}", hotkey.key);
    let key = if let Some((_, qt)) = QT_KEYS.iter().find(|(c, _)| *c == hotkey.key) {
        *qt
    } else if let Some(l) = name.strip_prefix("Key").filter(|l| l.len() == 1) {
        l.as_bytes()[0] as i32
    } else if let Some(d) = name.strip_prefix("Digit").filter(|d| d.len() == 1) {
        d.as_bytes()[0] as i32
    } else {
        let n: i32 = name.strip_prefix('F')?.parse().ok()?;
        (1..=24).contains(&n).then_some(QT_F1 + n - 1)?
    };
    let mods = MODS.iter().filter(|(m, _)| hotkey.mods.contains(*m));
    Some(mods.fold(key, |k, (_, bit)| k | bit))
}

/// KDE's key code as ours; `None` for a key we don't know (or none, 0).
fn from_qt(qt: i32) -> Option<HotKey> {
    let key = qt & !(SHIFT | CTRL | ALT | META | KEYPAD);
    let name = match key {
        0x41..=0x5a => format!("Key{}", key as u8 as char),
        0x30..=0x39 => format!("Digit{}", key as u8 as char),
        k if (QT_F1..QT_F1 + 24).contains(&k) => format!("F{}", k - QT_F1 + 1),
        k => format!("{:?}", QT_KEYS.iter().find(|(_, q)| *q == k)?.0),
    };
    let code: Code = name.parse().ok()?;
    let mods = (MODS.iter())
        .filter(|(_, bit)| qt & bit != 0)
        .fold(Modifiers::empty(), |m, (flag, _)| m | *flag);
    Some(HotKey::new(Some(mods), code))
}

/// A key in our spelling, "Ctrl+Shift+Space".
fn spelling(hotkey: HotKey) -> String {
    desktop_spelling(&super::gnome::accelerator(hotkey))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned() -> Vec<Binding> {
        super::super::plan(&Config::default()).0
    }

    #[test]
    fn only_kde_takes_this_path() {
        assert!(is_kde("KDE"));
        assert!(is_kde("ubuntu:KDE"));
        assert!(!is_kde("ubuntu:GNOME"));
        assert!(!is_kde(""));
    }

    #[test]
    fn every_default_key_round_trips_through_kdes_key_codes() {
        let p = planned();
        assert!(p.len() >= 5, "{p:?}");
        for b in &p {
            let qt = qt_key(b.hotkey).unwrap_or_else(|| panic!("{}", b.shortcut));
            assert_eq!(from_qt(qt), Some(b.hotkey), "{}", b.shortcut);
            assert_eq!(parse_shortcut(&spelling(b.hotkey)).ok(), Some(b.hotkey));
        }
        let k = |s| qt_key(parse_shortcut(s).unwrap()).unwrap();
        // Qt::CTRL | Qt::SHIFT | Qt::Key_Space, Qt::ALT | Qt::Key_A, ...
        assert_eq!(k("Ctrl+Shift+Space"), 0x0600_0020);
        assert_eq!(k("Alt+A"), 0x0800_0041);
        assert_eq!(k("Super+F5"), 0x1100_0034);
        assert_eq!(k("PrintScreen"), 0x0100_0009);
        assert_eq!(k("Ctrl+1"), 0x0400_0031);
        assert_eq!(spelling(from_qt(0x0600_0020).unwrap()), "Ctrl+Shift+Space");
        assert_eq!(from_qt(0), None, "no key");
        assert_eq!(from_qt(0x0100_1234), None, "a key we don't know");
    }

    #[test]
    fn the_messages_have_kglobalaccels_signatures() {
        let b = &planned()[0];
        let id = action_id(b);
        assert_eq!(
            id,
            [
                "com.opencircle.app",
                "capture",
                "OpenCircle",
                "Circle to search"
            ]
        );
        assert_eq!((id.clone(),).to_variant().type_().as_str(), "(as)");
        let v = set_keys(&id, &[0x0600_0020], SET_PRESENT | NO_AUTOLOADING);
        assert_eq!(v.type_().as_str(), "(asa(ai)u)");
        assert_eq!(v.child_value(2).get::<u32>(), Some(6));
        let seqs = v.child_value(1).get::<Vec<(Vec<i32>,)>>().unwrap();
        assert_eq!(
            seqs,
            [(vec![0x0600_0020, 0, 0, 0],)],
            "four chords, as KDE reads them"
        );
        assert_eq!(first_key(&v.child_value(1)), Some(b.hotkey));
        assert_eq!(
            set_keys(&id, &[], IS_DEFAULT).child_value(1).n_children(),
            0
        );
        assert_eq!((COMPONENT, "capture").to_variant().type_().as_str(), "(ss)");
        assert_eq!((true,).to_variant().type_().as_str(), "(b)");
    }

    #[test]
    fn reads_presses_and_rebindings_of_ours_only() {
        let press = |c: &str, a: &str| (c, a, 1234i64).to_variant();
        assert_eq!(
            pressed(&press(COMPONENT, "capture")).as_deref(),
            Some("capture")
        );
        assert_eq!(pressed(&press("kwin", "capture")), None);
        assert_eq!(pressed(&"junk".to_variant()), None);

        let id = |c: &str| {
            vec![
                c.to_string(),
                "quick-snip-circle".into(),
                "x".into(),
                "y".into(),
            ]
        };
        let to = |keys: Vec<(Vec<i32>,)>, c: &str| (id(c), keys).to_variant();
        assert_eq!(
            changed(&to(vec![(vec![0x1000_0041],)], COMPONENT)),
            Some(("quick-snip-circle".into(), parse_shortcut("Super+A").ok()))
        );
        assert_eq!(
            changed(&to(vec![], COMPONENT)),
            Some(("quick-snip-circle".into(), None)),
            "cleared in KDE"
        );
        assert_eq!(changed(&to(vec![(vec![0x41],)], "kwin")), None);
    }

    #[test]
    fn names_the_kde_action_that_holds_a_key_and_lists_ours() {
        let info = |c: &'static str, cn: &'static str, an: &'static str| {
            // the shape of a real answer: ("act", "Act", "w13test", "W13", "default",
            // "Default Context", [key], [])
            (
                "act",
                an,
                c,
                cn,
                "default",
                "Default Context",
                vec![1i32],
                Vec::<i32>::new(),
            )
        };
        let list = vec![
            info(COMPONENT, "OpenCircle", "Circle to search"),
            info("kwin", "KWin", "Switch to Desktop 1"),
        ]
        .to_variant();
        assert_eq!(list.type_().as_str(), "a(ssssssaiai)");
        assert_eq!(
            holder_in(&list).as_deref(),
            Some("KWin: Switch to Desktop 1")
        );
        let only_ours = vec![info(COMPONENT, "OpenCircle", "x")].to_variant();
        assert_eq!(holder_in(&only_ours), None, "our own key is no conflict");

        let all = vec![
            vec![
                COMPONENT.to_string(),
                "capture".into(),
                "OpenCircle".into(),
                "C".into(),
            ],
            vec![COMPONENT.to_string(), "quick-snip-circle".into()],
        ]
        .to_variant();
        assert_eq!(action_names(&all), ["capture", "quick-snip-circle"]);
    }
}
