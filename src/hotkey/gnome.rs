//! GNOME without the GlobalShortcuts portal (before GNOME 48, e.g. Ubuntu 22.04): the
//! desktop gives apps no way to own a key, but it runs any command on a key from its own
//! custom shortcuts (`org.gnome.settings-daemon.plugins.media-keys`). OpenCircle keeps
//! ONE entry per planned shortcut there, running the frozen CLI (`opencircle capture` /
//! `opencircle run <action> --target <t>`); the CLI socket does the rest.
//!
//! Only entries under `OURS` are ever written or removed; the user's own are read (to
//! name a real conflict) and never touched.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use gtk::gio::{self, prelude::*};

use super::{Binding, What, desktop_spelling, target_name};
use crate::config::parse_shortcut;

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const ENTRY_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const LIST: &str = "custom-keybindings";
const BASE: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
/// Our entries' path prefix: what makes an entry ours.
const OURS: &str = "opencircle-";

/// Our entries are in GNOME's custom shortcuts (set by `adopt`).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The user's own custom shortcuts as (key, name): a key of ours on one of those is a
/// real conflict, named. Refreshed by every `sync`.
static FOREIGN: Mutex<Vec<(HotKey, String)>> = Mutex::new(Vec::new());

fn foreign() -> MutexGuard<'static, Vec<(HotKey, String)>> {
    FOREIGN.lock().unwrap_or_else(|e| e.into_inner())
}

/// One custom shortcut as GNOME stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    path: String,
    name: String,
    command: String,
    /// GTK accelerator ("<Control><Shift>space"); empty = off.
    binding: String,
}

/// The GNOME custom-shortcut store (the real one is dconf; tests use a map).
trait Store {
    fn list(&self) -> Vec<String>;
    fn set_list(&self, paths: &[String]);
    fn get(&self, path: &str) -> Option<Entry>;
    fn set(&self, entry: &Entry);
    fn reset(&self, path: &str);
}

struct Dconf;

impl Dconf {
    fn at(path: &str) -> gio::Settings {
        gio::Settings::with_path(ENTRY_SCHEMA, path)
    }
}

impl Store for Dconf {
    fn list(&self) -> Vec<String> {
        gio::Settings::new(SCHEMA)
            .strv(LIST)
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    fn set_list(&self, paths: &[String]) {
        let _ = gio::Settings::new(SCHEMA).set_strv(LIST, paths);
    }
    fn get(&self, path: &str) -> Option<Entry> {
        let s = Self::at(path);
        Some(Entry {
            path: path.into(),
            name: s.string("name").into(),
            command: s.string("command").into(),
            binding: s.string("binding").into(),
        })
    }
    fn set(&self, e: &Entry) {
        let s = Self::at(&e.path);
        let _ = s.set_string("name", &e.name);
        let _ = s.set_string("command", &e.command);
        let _ = s.set_string("binding", &e.binding);
    }
    fn reset(&self, path: &str) {
        let s = Self::at(path);
        for key in ["name", "command", "binding"] {
            s.reset(key);
        }
    }
}

fn schemas_present() -> bool {
    gio::SettingsSchemaSource::default().is_some_and(|src| {
        src.lookup(SCHEMA, true).is_some() && src.lookup(ENTRY_SCHEMA, true).is_some()
    })
}

/// Is `desktop` (`XDG_CURRENT_DESKTOP`) GNOME?
pub fn is_gnome(desktop: &str) -> bool {
    desktop.split(':').any(|d| d.eq_ignore_ascii_case("gnome"))
}

/// The path choice when the portal is missing: GNOME's custom shortcuts, on GNOME
/// with its settings schemas. (The portal, when there, is always first; X11 grabs.)
fn applies(desktop: &str, schemas: bool) -> bool {
    is_gnome(desktop) && schemas
}

/// Are our entries what holds the shortcuts now?
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Takes the shortcuts over when this desktop is GNOME without the portal. `false` =
/// not applicable (or GNOME refused), the caller reports the manual way.
pub fn adopt(bindings: &[Binding]) -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if cfg!(test) || !applies(&desktop, schemas_present()) {
        return false;
    }
    ACTIVE.store(true, Ordering::Relaxed);
    sync(bindings, super::suspended());
    crate::log!(
        "[opencircle] global shortcuts: GNOME has no shortcut portal here; OpenCircle keeps its own \
         entries in GNOME's custom shortcuts (Settings > Keyboard > Custom Shortcuts)"
    );
    true
}

/// Brings our GNOME entries in line with `bindings` (`held_off`: all keys cleared while a
/// shortcut field records, so the recorder gets every key).
pub fn sync(bindings: &[Binding], held_off: bool) {
    let exe = crate::sys::launcher()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "opencircle".into());
    let want = entries(bindings, &exe, held_off);
    *foreign() = foreign_keys(&Dconf);
    apply(&Dconf, &want);
    gio::Settings::sync();
}

/// Removes every entry of ours (reset / uninstall). The user's stay.
pub fn forget() {
    if schemas_present() {
        apply(&Dconf, &[]);
        gio::Settings::sync();
    }
    ACTIVE.store(false, Ordering::Relaxed);
}

/// The name of the user's own shortcut that already uses `shortcut`, if any.
pub fn used_by(shortcut: &str) -> Option<String> {
    let hotkey = parse_shortcut(shortcut).ok()?;
    foreign()
        .iter()
        .find(|(k, _)| *k == hotkey)
        .map(|(_, n)| n.clone())
}

/// The entry per binding. A `live` quick action has no CLI spelling (Labs, hidden): skipped.
fn entries(bindings: &[Binding], exe: &str, held_off: bool) -> Vec<Entry> {
    bindings
        .iter()
        .filter_map(|b| {
            let args = match &b.what {
                What::Capture => "capture".to_string(),
                What::Quick(q) if q.live => return None,
                What::Quick(q) => format!("run {} --target {}", q.action, target_name(q.target)),
            };
            let slug: String =
                b.id.chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                    .collect();
            Some(Entry {
                path: format!("{BASE}{OURS}{slug}/"),
                name: format!("OpenCircle: {}", b.description),
                command: format!("{} {args}", shell_quote(exe)),
                binding: if held_off {
                    String::new()
                } else {
                    accelerator(b.hotkey)
                },
            })
        })
        .collect()
}

/// `exe` as one argument of GNOME's command line (it parses it like a shell).
fn shell_quote(exe: &str) -> String {
    if exe
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
    {
        exe.into()
    } else {
        format!("'{}'", exe.replace('\'', r"'\''"))
    }
}

/// Is `path` one of ours?
fn is_ours(path: &str) -> bool {
    path.strip_prefix(BASE)
        .is_some_and(|rest| rest.starts_with(OURS))
}

/// Makes the store hold exactly `want` among our entries: stale ones go, changed ones are
/// rewritten (unchanged ones are not written at all), the user's entries and their order
/// stay as they are.
fn apply<S: Store>(store: &S, want: &[Entry]) {
    let have = store.list();
    for old in have.iter().filter(|p| is_ours(p)) {
        if !want.iter().any(|e| e.path == *old) {
            store.reset(old);
        }
    }
    for e in want {
        if store.get(&e.path).as_ref() != Some(e) {
            store.set(e);
        }
    }
    let mut list: Vec<String> = have.iter().filter(|p| !is_ours(p)).cloned().collect();
    list.extend(want.iter().map(|e| e.path.clone()));
    if list != have {
        store.set_list(&list);
    }
}

/// The user's own custom shortcuts that have a key.
fn foreign_keys<S: Store>(store: &S) -> Vec<(HotKey, String)> {
    store
        .list()
        .iter()
        .filter(|p| !is_ours(p))
        .filter_map(|p| store.get(p))
        .filter_map(|e| {
            let key = parse_shortcut(&desktop_spelling(&e.binding)).ok()?;
            Some((key, e.name))
        })
        .collect()
}

/// "Ctrl+Shift+Space" as a GTK accelerator, "<Control><Shift>space".
fn accelerator(hotkey: HotKey) -> String {
    let mods = [
        (Modifiers::CONTROL, "<Control>"),
        (Modifiers::ALT, "<Alt>"),
        (Modifiers::SHIFT, "<Shift>"),
        (Modifiers::META, "<Super>"),
    ];
    let mut out: String = mods
        .iter()
        .filter(|(m, _)| hotkey.mods.contains(*m))
        .map(|(_, s)| *s)
        .collect();
    let code = format!("{:?}", hotkey.key);
    let key = match hotkey.key {
        Code::Space => "space".into(),
        Code::Enter => "Return".into(),
        Code::Escape => "Escape".into(),
        Code::PrintScreen => "Print".into(),
        Code::ScrollLock => "Scroll_Lock".into(),
        Code::PageUp => "Page_Up".into(),
        Code::PageDown => "Page_Down".into(),
        Code::ArrowUp => "Up".into(),
        Code::ArrowDown => "Down".into(),
        Code::ArrowLeft => "Left".into(),
        Code::ArrowRight => "Right".into(),
        Code::Minus => "minus".into(),
        Code::Equal => "equal".into(),
        _ => match (code.strip_prefix("Key"), code.strip_prefix("Digit")) {
            (Some(l), _) if l.len() == 1 => l.to_ascii_lowercase(),
            (_, Some(d)) if d.len() == 1 => d.to_string(),
            _ => code,
        },
    };
    out.push_str(&key);
    out
}

/// Keys GNOME (or IBus under it) keeps for itself, with the reason. Refused when a
/// shortcut is recorded: it would fire there instead of here, or do both.
pub fn reserved_on(desktop: &str, shortcut: &str) -> Option<String> {
    if !is_gnome(desktop) {
        return None;
    }
    let hotkey = parse_shortcut(shortcut).ok()?;
    let mods = hotkey.mods;
    if hotkey.key == Code::KeyU && mods == Modifiers::CONTROL | Modifiers::SHIFT {
        return Some("Used by GNOME for Unicode input".into());
    }
    if hotkey.key == Code::PrintScreen {
        return Some("Used by GNOME for screenshots".into());
    }
    if mods.contains(Modifiers::META) {
        return Some("Super shortcuts are reserved by GNOME".into());
    }
    None
}

/// The desktop swallowed a key while only modifiers reached us: with Ctrl+Shift held on
/// GNOME that is its Unicode input (Ctrl+Shift+U, IBus).
pub fn eaten_reason(
    desktop: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    logo: bool,
) -> Option<String> {
    (is_gnome(desktop) && ctrl && shift && !alt && !logo)
        .then(|| "Used by GNOME for Unicode input (Ctrl+Shift+U)".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Fake {
        list: RefCell<Vec<String>>,
        entries: RefCell<BTreeMap<String, Entry>>,
        writes: RefCell<u32>,
    }

    impl Store for Fake {
        fn list(&self) -> Vec<String> {
            self.list.borrow().clone()
        }
        fn set_list(&self, paths: &[String]) {
            *self.writes.borrow_mut() += 1;
            *self.list.borrow_mut() = paths.to_vec();
        }
        fn get(&self, path: &str) -> Option<Entry> {
            self.entries.borrow().get(path).cloned()
        }
        fn set(&self, e: &Entry) {
            *self.writes.borrow_mut() += 1;
            self.entries.borrow_mut().insert(e.path.clone(), e.clone());
        }
        fn reset(&self, path: &str) {
            *self.writes.borrow_mut() += 1;
            self.entries.borrow_mut().remove(path);
        }
    }

    fn users(store: &Fake) -> Entry {
        let e = Entry {
            path: format!("{BASE}custom0/"),
            name: "Open terminal".into(),
            command: "gnome-terminal".into(),
            binding: "<Control><Alt>t".into(),
        };
        store.set(&e);
        store.set_list(std::slice::from_ref(&e.path));
        e
    }

    fn planned(shortcut: &str) -> Vec<Binding> {
        let cfg = crate::config::Config {
            shortcut: shortcut.into(),
            ..Default::default()
        };
        super::super::plan(&cfg).0
    }

    #[test]
    fn the_path_choice_is_gnome_with_its_schemas_only() {
        assert!(applies("ubuntu:GNOME", true));
        assert!(applies("GNOME", true));
        assert!(!applies("ubuntu:GNOME", false), "no GNOME schemas");
        assert!(!applies("KDE", true));
        assert!(!applies("", true));
    }

    #[test]
    fn accelerators_and_commands_for_every_default() {
        assert_eq!(
            accelerator(parse_shortcut("Ctrl+Shift+Space").unwrap()),
            "<Control><Shift>space"
        );
        assert_eq!(accelerator(parse_shortcut("Alt+A").unwrap()), "<Alt>a");
        assert_eq!(
            accelerator(parse_shortcut("Super+Shift+F5").unwrap()),
            "<Shift><Super>F5"
        );
        assert_eq!(accelerator(parse_shortcut("Ctrl+1").unwrap()), "<Control>1");
        assert_eq!(accelerator(parse_shortcut("PrintScreen").unwrap()), "Print");
        let quoted = entries(&planned("Ctrl+Shift+Space"), "/opt/open circle/oc", false);
        let cmds: Vec<&str> = quoted.iter().map(|e| e.command.as_str()).collect();
        assert!(cmds.contains(&"'/opt/open circle/oc' capture"), "{cmds:?}");
        assert!(cmds.contains(&"'/opt/open circle/oc' run translate --target full_screen"));
        // every default shortcut is on a key, and runs a valid CLI command
        let all = entries(&planned("Ctrl+Shift+Space"), "oc", false);
        assert!(
            all.iter()
                .all(|e| is_ours(&e.path) && e.path.ends_with('/'))
        );
        for e in &all {
            assert!(!e.binding.is_empty(), "{e:?}");
            let args: Vec<String> = e.command.split(' ').skip(1).map(String::from).collect();
            assert!(
                super::super::Command::from_args(&args).is_ok(),
                "{}",
                e.command
            );
        }
    }

    #[test]
    fn syncing_adds_ours_and_never_touches_the_users() {
        let store = Fake::default();
        let mine = users(&store);
        let want = entries(&planned("Ctrl+Shift+Space"), "oc", false);
        apply(&store, &want);
        assert_eq!(store.get(&mine.path), Some(mine.clone()));
        assert_eq!(
            store.list()[0],
            mine.path,
            "the user's entries keep their place"
        );
        assert_eq!(store.list().len(), 1 + want.len());
        // the same list again writes nothing
        let before = *store.writes.borrow();
        apply(&store, &want);
        assert_eq!(*store.writes.borrow(), before);
    }

    #[test]
    fn changing_a_key_updates_the_same_entry_and_forgetting_removes_only_ours() {
        let store = Fake::default();
        let mine = users(&store);
        apply(&store, &entries(&planned("Ctrl+Shift+Space"), "oc", false));
        let n = store.list().len();
        apply(&store, &entries(&planned("Ctrl+Alt+Space"), "oc", false));
        assert_eq!(store.list().len(), n, "same entry, new key");
        let capture = store.get(&format!("{BASE}{OURS}capture/")).unwrap();
        assert_eq!(capture.binding, "<Control><Alt>space");
        // held off while recording: bindings cleared, entries kept
        apply(&store, &entries(&planned("Ctrl+Alt+Space"), "oc", true));
        assert_eq!(store.get(&capture.path).unwrap().binding, "");
        apply(&store, &[]);
        assert_eq!(store.list(), std::slice::from_ref(&mine.path));
        assert_eq!(store.get(&mine.path), Some(mine));
        assert_eq!(store.entries.borrow().len(), 1);
    }

    #[test]
    fn a_user_shortcut_on_our_key_is_named_and_ours_never_count() {
        let store = Fake::default();
        users(&store);
        apply(&store, &entries(&planned("Ctrl+Shift+Space"), "oc", false));
        let found = foreign_keys(&store);
        assert_eq!(
            found.len(),
            1,
            "our entries are not 'another app': {found:?}"
        );
        assert_eq!(found[0].1, "Open terminal");
        assert_eq!(found[0].0, parse_shortcut("Ctrl+Alt+T").unwrap());
    }

    #[test]
    fn a_swallowed_ctrl_shift_key_on_gnome_is_named() {
        let on = |c, a, s, l| eaten_reason("ubuntu:GNOME", c, a, s, l);
        assert_eq!(
            on(true, false, true, false).as_deref(),
            Some("Used by GNOME for Unicode input (Ctrl+Shift+U)")
        );
        assert_eq!(on(true, false, false, false), None);
        assert_eq!(on(true, true, true, false), None);
        assert_eq!(eaten_reason("KDE", true, false, true, false), None);
    }

    #[test]
    fn gnome_reserved_keys_are_refused_with_their_reason() {
        let on = |k| reserved_on("ubuntu:GNOME", k);
        assert_eq!(
            on("Ctrl+Shift+U").as_deref(),
            Some("Used by GNOME for Unicode input")
        );
        assert_eq!(
            on("PrintScreen").as_deref(),
            Some("Used by GNOME for screenshots")
        );
        assert_eq!(
            on("Alt+PrintScreen").as_deref(),
            Some("Used by GNOME for screenshots")
        );
        assert!(on("Super+T").is_some());
        // our defaults are free
        for k in [
            "Ctrl+Shift+Space",
            "Ctrl+Shift+T",
            "Ctrl+Shift+L",
            "Ctrl+Shift+E",
            "Ctrl+Shift+X",
            "Alt+A",
        ] {
            assert_eq!(on(k), None, "{k}");
        }
        assert_eq!(reserved_on("KDE", "Ctrl+Shift+U"), None);
    }
}
