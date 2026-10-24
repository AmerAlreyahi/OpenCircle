//! Triggers from outside the UI: the global shortcuts (capture + one per quick
//! action, all from the config) and the CLI socket (`capture`, `run <action>`).
//!
//! `plan` turns the config into a list of `Binding`s; that list is the one source of
//! truth for registering with the OS, for routing a key press to its `Trigger`, and
//! for the stable ids the Wayland portal needs.
#[cfg(target_os = "linux")]
mod portal;
mod socket;

pub use socket::{Command, forward, forward_or_start, listen};

use std::cell::RefCell;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::config::{Config, QuickAction, Target, parse_shortcut};
use crate::{Trigger, Triggers};

/// Ids `opencircle run` takes that are not in the action registry (`ui/quick.rs`
/// runs ai_explain, copy_text and song_search; snip opens Snip mode).
const PLANNED_ACTIONS: &[&str] = &["ai_explain", "copy_text", "snip", "song_search"];

/// Is `id` an action `opencircle run` may start?
fn known_action(id: &str) -> bool {
    PLANNED_ACTIONS.contains(&id) || crate::actions::ACTIONS.iter().any(|a| a.id == id)
}

/// The config/CLI spelling of a target.
fn target_name(target: Target) -> &'static str {
    match target {
        Target::FullScreen => "full_screen",
        Target::Circle => "circle",
        Target::LastRegion => "last_region",
        Target::Window => "window",
    }
}

fn parse_target(name: &str) -> Option<Target> {
    [
        Target::FullScreen,
        Target::Circle,
        Target::LastRegion,
        Target::Window,
    ]
    .into_iter()
    .find(|t| target_name(*t) == name)
}

/// What a shortcut does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum What {
    Capture,
    Quick(QuickAction),
}

/// One shortcut to register.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    /// Stable across runs and reloads: the Wayland portal keys the user's choice on it.
    id: String,
    /// Shown by the desktop in its shortcut settings.
    description: String,
    /// As written in the config; always valid (`parse_shortcut` accepted it).
    shortcut: String,
    hotkey: HotKey,
    what: What,
}

impl Binding {
    fn trigger(&self, at: Instant) -> Trigger {
        match &self.what {
            What::Capture => Trigger::Capture(at),
            What::Quick(qa) => Trigger::Quick(at, qa.clone()),
        }
    }
}

/// The config as a registration list, plus one warning per entry that was left out
/// or replaced. An empty shortcut is "not bound" and gives no warning.
fn plan(cfg: &Config) -> (Vec<Binding>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut out: Vec<Binding> = Vec::new();

    let default = Config::default().shortcut;
    let (shortcut, hotkey) = match parse_shortcut(&cfg.shortcut) {
        Ok(hk) => (cfg.shortcut.clone(), hk),
        Err(e) => {
            warnings.push(format!(
                "shortcut \"{}\": {e}; using {default}",
                cfg.shortcut
            ));
            let hk = parse_shortcut(&default).expect("the default shortcut is valid");
            (default, hk)
        }
    };
    out.push(Binding {
        id: "capture".into(),
        description: "Circle to search".into(),
        shortcut,
        hotkey,
        what: What::Capture,
    });

    for qa in &cfg.quick_actions {
        if qa.shortcut.is_empty() {
            continue;
        }
        let hotkey = match parse_shortcut(&qa.shortcut) {
            Ok(hk) => hk,
            Err(e) => {
                warnings.push(format!(
                    "quick action \"{}\": shortcut \"{}\": {e}; left unbound",
                    qa.action, qa.shortcut
                ));
                continue;
            }
        };
        if let Some(first) = out.iter().find(|b| b.hotkey == hotkey) {
            warnings.push(format!(
                "quick action \"{}\": shortcut \"{}\" is already used by \"{}\"; left unbound",
                qa.action, qa.shortcut, first.description
            ));
            continue;
        }
        let base = format!(
            "quick-{}-{}{}",
            qa.action,
            target_name(qa.target),
            if qa.live { "-live" } else { "" }
        );
        let mut id = base.clone();
        let mut n = 2;
        while out.iter().any(|b| b.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        out.push(Binding {
            id,
            description: describe(qa),
            shortcut: qa.shortcut.clone(),
            hotkey,
            what: What::Quick(qa.clone()),
        });
    }
    (out, warnings)
}

/// "Translate (full screen, live)" from the action id and target.
fn describe(qa: &QuickAction) -> String {
    let mut name = qa.action.replace('_', " ");
    if let Some(rest) = name.strip_prefix("ai ") {
        name = format!("AI {rest}");
    }
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    let live = if qa.live { ", live" } else { "" };
    format!(
        "{name} ({}{live})",
        target_name(qa.target).replace('_', " ")
    )
}

/// The planned shortcuts, for routing. The OS handlers and the CLI read it; only
/// `start` / `reload` write it.
static BINDINGS: Mutex<Vec<Binding>> = Mutex::new(Vec::new());

fn bindings() -> MutexGuard<'static, Vec<Binding>> {
    BINDINGS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Wayland: the key the desktop actually bound per binding id (`None` = no key), from
/// the portal's last answer. Empty elsewhere and until the desktop answers.
static DESKTOP_KEYS: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());

fn desktop_keys() -> MutexGuard<'static, Vec<(String, Option<String>)>> {
    DESKTOP_KEYS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where the user changes a key the desktop keeps (logged, and shown in Settings).
pub fn managed() -> &'static str {
    managed_on(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
}

fn managed_on(desktop: &str) -> &'static str {
    if desktop.split(':').any(|d| d.eq_ignore_ascii_case("gnome")) {
        "Your desktop manages this shortcut: change it in Settings → Apps → OpenCircle → Global Shortcuts"
    } else if desktop.split(':').any(|d| d.eq_ignore_ascii_case("kde")) {
        "Your desktop manages this shortcut: change it in System Settings → Keyboard → Shortcuts"
    } else {
        "Your desktop manages this shortcut: change it in its keyboard shortcut settings"
    }
}

/// What the desktop made of a shortcut it manages (Wayland portal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopKey {
    /// The desktop uses this key instead (in our spelling when we can read it).
    Other(String),
    /// The desktop gave it no key at all.
    Unassigned,
}

/// The desktop's own say on `shortcut` (a key of the running config) when it is
/// not simply that key. `None` = active as configured, or nothing known (not
/// Wayland, the desktop hasn't answered yet, or not a running shortcut).
pub fn desktop_key(shortcut: &str) -> Option<DesktopKey> {
    desktop_key_in(&bindings(), &desktop_keys(), shortcut)
}

fn desktop_key_in(
    bindings: &[Binding],
    desktop: &[(String, Option<String>)],
    shortcut: &str,
) -> Option<DesktopKey> {
    let hotkey = parse_shortcut(shortcut).ok()?;
    let binding = bindings.iter().find(|b| b.hotkey == hotkey)?;
    let (_, keys) = desktop.iter().find(|(id, _)| *id == binding.id)?;
    let Some(keys) = keys else {
        return Some(DesktopKey::Unassigned);
    };
    let keys = desktop_spelling(keys);
    (parse_shortcut(&keys).ok() != Some(hotkey)).then_some(DesktopKey::Other(keys))
}

/// A desktop's name for a key, in our spelling where it can be read: GNOME says
/// "Press <Shift><Control>space" (GTK accelerator), KDE "Ctrl+Shift+Space".
fn desktop_spelling(said: &str) -> String {
    let Some(start) = said.find('<') else {
        // No modifiers ("Press Print") or already ours: the last word.
        let last = said.split_whitespace().last().unwrap_or(said);
        return key_name(last);
    };
    let mut rest = &said[start..];
    let (mut ctrl, mut alt, mut shift, mut sup) = (false, false, false, false);
    while let Some(end) = rest.strip_prefix('<').and_then(|r| r.find('>')) {
        match rest[1..=end].to_ascii_lowercase().as_str() {
            "control" | "ctrl" | "primary" => ctrl = true,
            "alt" | "mod1" => alt = true,
            "shift" => shift = true,
            "super" | "meta" | "hyper" | "mod4" => sup = true,
            _ => return said.trim().to_string(), // unknown: show it as the desktop says
        }
        rest = &rest[end + 2..];
    }
    let mods = [
        (ctrl, "Ctrl"),
        (alt, "Alt"),
        (shift, "Shift"),
        (sup, "Super"),
    ];
    let mut parts: Vec<String> = mods
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, m)| m.to_string())
        .collect();
    parts.push(key_name(rest.trim()));
    parts.join("+")
}

/// An XKB key name ("space", "Print", "e") as we write keys.
fn key_name(key: &str) -> String {
    match key {
        "space" => "Space".into(),
        "Print" => "PrintScreen".into(),
        "Return" => "Enter".into(),
        "Escape" => "Esc".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    }
}

/// The portal's answer: what the desktop bound, per binding id.
#[cfg(target_os = "linux")]
fn set_desktop_keys(keys: Vec<(String, Option<String>)>) {
    *desktop_keys() = keys;
}

/// The trigger for a key press the OS reported by hotkey id.
fn route_hotkey(bindings: &[Binding], id: u32, at: Instant) -> Option<Trigger> {
    bindings
        .iter()
        .find(|b| b.hotkey.id() == id)
        .map(|b| b.trigger(at))
}

/// The trigger for a portal shortcut id.
#[cfg(target_os = "linux")]
fn route_id(bindings: &[Binding], id: &str, at: Instant) -> Option<Trigger> {
    bindings.iter().find(|b| b.id == id).map(|b| b.trigger(at))
}

/// The trigger for a command from the socket or the command line. A `run` without a
/// target takes the one configured for that action (first entry), else the default.
pub fn trigger(command: &Command, at: Instant) -> Trigger {
    match command {
        Command::Capture => Trigger::Capture(at),
        Command::Run { action, target } => {
            let configured = bindings().iter().find_map(|b| match &b.what {
                What::Quick(qa) if qa.action == *action => Some(qa.clone()),
                _ => None,
            });
            let mut qa = configured.unwrap_or_else(|| QuickAction {
                action: action.clone(),
                shortcut: String::new(),
                target: Target::default(),
                live: false,
            });
            if let Some(t) = target {
                qa.target = *t;
                qa.live = false;
            }
            qa.shortcut.clear(); // not a key press
            Trigger::Quick(at, qa)
        }
    }
}

/// The key grab (Windows/X11). It must stay on the thread that made it (Windows
/// delivers key events to that thread's message loop), so it lives in a thread-local:
/// `start` and `reload` are both called from the main thread.
struct Grab {
    manager: GlobalHotKeyManager,
    held: Vec<HotKey>,
}

thread_local! {
    static GRAB: RefCell<Option<Grab>> = const { RefCell::new(None) };
}

/// Starts every shortcut of the config: the desktop portal on Wayland (no key grabs
/// there), a key grab elsewhere. Call from the main thread.
pub fn start(tx: Triggers, cfg: &Config) {
    let (planned, warnings) = plan(cfg);
    log_all(&warnings);
    *bindings() = planned;

    #[cfg(target_os = "linux")]
    if crate::capture::wayland_session() {
        portal::bind(tx);
        return;
    }

    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed {
            let trigger = route_hotkey(&bindings(), event.id, Instant::now());
            if let Some(trigger) = trigger {
                let _ = tx.unbounded_send(trigger);
            }
        }
    }));
    match GlobalHotKeyManager::new() {
        Ok(manager) => GRAB.with_borrow_mut(|slot| {
            let grab = slot.insert(Grab {
                manager,
                held: Vec::new(),
            });
            regrab(grab, &bindings());
        }),
        Err(e) => eprintln!(
            "[opencircle] global shortcuts unavailable ({e}); bind `opencircle capture` and \
             `opencircle run <action>` in your desktop's shortcut settings instead"
        ),
    }
}

/// Applies a changed config: unregisters the old shortcuts, registers the new ones.
/// Call from the thread that called `start` (the main thread).
// On Wayland the portal has no "unbind": the session is closed and a new one opened.
pub fn reload(cfg: &Config) {
    let (planned, warnings) = plan(cfg);
    log_all(&warnings);
    *bindings() = planned;

    #[cfg(target_os = "linux")]
    if crate::capture::wayland_session() {
        portal::rebind();
        return;
    }
    GRAB.with_borrow_mut(|slot| match slot {
        Some(grab) => regrab(grab, &bindings()),
        None => eprintln!("[opencircle] shortcuts were not started here; reload skipped"),
    });
}

/// Replaces what the grab holds with `planned`. A shortcut another app already owns is
/// reported once, here; the rest still work.
fn regrab(grab: &mut Grab, planned: &[Binding]) {
    let _ = grab.manager.unregister_all(&grab.held);
    grab.held.clear();
    for b in planned {
        match grab.manager.register(b.hotkey) {
            Ok(()) => grab.held.push(b.hotkey),
            Err(e) => eprintln!("[opencircle] {}", unavailable(b, &e.to_string())),
        }
    }
}

/// The one log line for a shortcut the OS refused. Print Screen also says how to
/// free it: the desktop's own screenshot tool usually owns that key.
fn unavailable(b: &Binding, why: &str) -> String {
    let mut line = format!(
        "shortcut {} ({}) is unavailable: {why}. Another app may use it; pick a different one in Settings.",
        b.shortcut, b.description
    );
    if b.hotkey.key == Code::PrintScreen {
        line.push_str(if cfg!(windows) {
            " To free Print Screen: Windows Settings > Accessibility > Keyboard > \
             \"Use the Print screen key to open screen capture\" > off, then restart OpenCircle."
        } else {
            " To free Print Screen, unbind it from your desktop's screenshot tool in its \
             keyboard shortcut settings, then restart OpenCircle."
        });
    }
    line
}

fn log_all(warnings: &[String]) {
    for w in warnings {
        eprintln!("[opencircle] {w}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qa(action: &str, shortcut: &str, target: Target, live: bool) -> QuickAction {
        QuickAction {
            action: action.into(),
            shortcut: shortcut.into(),
            target,
            live,
        }
    }

    fn cfg(shortcut: &str, quick: Vec<QuickAction>) -> Config {
        Config {
            shortcut: shortcut.into(),
            quick_actions: quick,
            ..Config::default()
        }
    }

    #[test]
    fn defaults_give_capture_plus_every_bound_quick_action() {
        let (b, w) = plan(&Config::default());
        assert!(w.is_empty(), "{w:?}");
        let ids: Vec<_> = b.iter().map(|b| b.id.as_str()).collect();
        // `lens` has no shortcut by default, so it gets no binding.
        assert_eq!(
            ids,
            [
                "capture",
                "quick-translate-full_screen",
                "quick-translate-full_screen-live",
                "quick-ai_explain-circle",
                "quick-copy_text-circle",
                "quick-snip-circle",
                "quick-snip-window",
                "quick-snip-full_screen",
            ]
        );
        assert_eq!(b[0].shortcut, "Ctrl+Shift+Space");
        assert_eq!(b[2].description, "Translate (full screen, live)");
        assert_eq!(b[3].description, "AI explain (circle)");
    }

    #[test]
    fn ids_are_stable_and_unique() {
        let config = cfg(
            "Ctrl+Shift+Space",
            vec![
                qa("lens", "Ctrl+Alt+1", Target::Circle, false),
                qa("lens", "Ctrl+Alt+2", Target::Circle, false),
                qa("lens", "Ctrl+Alt+3", Target::Circle, false),
            ],
        );
        let ids = |c: &Config| plan(c).0.into_iter().map(|b| b.id).collect::<Vec<_>>();
        let first = ids(&config);
        assert_eq!(
            first,
            [
                "capture",
                "quick-lens-circle",
                "quick-lens-circle-2",
                "quick-lens-circle-3"
            ]
        );
        assert_eq!(first, ids(&config), "same config, same ids");
        // Ids do not depend on the key chosen.
        let moved = cfg(
            "Ctrl+Alt+Q",
            vec![qa("lens", "Ctrl+Alt+9", Target::Circle, false)],
        );
        assert_eq!(ids(&moved), ["capture", "quick-lens-circle"]);
    }

    #[test]
    fn invalid_capture_shortcut_falls_back_with_a_warning() {
        for bad in ["", "Space", "Ctrl+Banana"] {
            let (b, w) = plan(&cfg(bad, vec![]));
            assert_eq!(b.len(), 1);
            assert_eq!(b[0].shortcut, "Ctrl+Shift+Space", "{bad:?}");
            assert_eq!(w.len(), 1, "{bad:?}");
            assert!(w[0].contains("using Ctrl+Shift+Space"), "{}", w[0]);
        }
    }

    #[test]
    fn invalid_and_duplicate_quick_shortcuts_are_skipped_with_a_warning() {
        let config = cfg(
            "Ctrl+Shift+Space",
            vec![
                qa("translate", "Ctrl+Shift+T", Target::FullScreen, false),
                qa("lens", "", Target::Circle, false), // unbound: silent
                qa("lens", "Ctrl+Banana", Target::Circle, false), // invalid
                qa("copy_text", "ctrl+shift+t", Target::Circle, false), // same key, other spelling
                qa("save", "Control+Shift+Space", Target::Circle, false), // the capture key
                qa("ai_explain", "Ctrl+Shift+E", Target::Circle, false),
            ],
        );
        let (b, w) = plan(&config);
        let ids: Vec<_> = b.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "capture",
                "quick-translate-full_screen",
                "quick-ai_explain-circle"
            ]
        );
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(w[0].contains("lens") && w[0].contains("Ctrl+Banana"));
        assert!(w[1].contains("copy_text") && w[1].contains("already used"));
        assert!(w[2].contains("save") && w[2].contains("Circle to search"));
    }

    #[test]
    fn a_key_press_routes_to_its_trigger() {
        let (b, _) = plan(&Config::default());
        let at = Instant::now();
        let id = |s: &str| parse_shortcut(s).unwrap().id();

        assert!(matches!(
            route_hotkey(&b, id("Ctrl+Shift+Space"), at),
            Some(Trigger::Capture(_))
        ));
        match route_hotkey(&b, id("Ctrl+Shift+L"), at) {
            Some(Trigger::Quick(_, q)) => {
                assert_eq!(
                    (q.action.as_str(), q.target, q.live),
                    ("translate", Target::FullScreen, true)
                );
            }
            other => panic!("{other:?}"),
        }
        match route_hotkey(&b, id("Ctrl+Shift+E"), at) {
            Some(Trigger::Quick(_, q)) => assert_eq!(q.action, "ai_explain"),
            other => panic!("{other:?}"),
        }
        assert!(route_hotkey(&b, id("Ctrl+Shift+F9"), at).is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_portal_id_routes_to_its_trigger() {
        let (b, _) = plan(&Config::default());
        let at = Instant::now();
        assert!(matches!(
            route_id(&b, "capture", at),
            Some(Trigger::Capture(_))
        ));
        assert!(matches!(
            route_id(&b, "quick-copy_text-circle", at),
            Some(Trigger::Quick(_, q)) if q.action == "copy_text"
        ));
        assert!(route_id(&b, "quick-lens-circle", at).is_none(), "unbound");
        assert!(route_id(&b, "", at).is_none());
    }

    #[test]
    fn run_commands_become_quick_triggers() {
        // Single test touching the global table (tests run in parallel).
        let (planned, _) = plan(&cfg(
            "Ctrl+Shift+Space",
            vec![qa("ai_explain", "Ctrl+Shift+E", Target::Circle, false)],
        ));
        *bindings() = planned;
        let at = Instant::now();
        let quick = |t: Trigger| match t {
            Trigger::Quick(_, q) => (q.action, q.target, q.live, q.shortcut),
            other => panic!("{other:?}"),
        };
        let run = |action: &str, target| Command::Run {
            action: action.into(),
            target,
        };

        // No target: the configured one; or the default when the action has no entry.
        assert_eq!(
            quick(trigger(&run("ai_explain", None), at)),
            ("ai_explain".into(), Target::Circle, false, String::new())
        );
        assert_eq!(quick(trigger(&run("lens", None), at)).1, Target::FullScreen);
        // An explicit target wins.
        assert_eq!(
            quick(trigger(&run("ai_explain", Some(Target::LastRegion)), at)).1,
            Target::LastRegion
        );
        assert!(matches!(
            trigger(&Command::Capture, at),
            Trigger::Capture(_)
        ));
    }

    #[test]
    fn target_names_round_trip() {
        for t in [
            Target::FullScreen,
            Target::Circle,
            Target::LastRegion,
            Target::Window,
        ] {
            assert_eq!(parse_target(target_name(t)), Some(t));
        }
        assert_eq!(parse_target("Circle"), None);
        assert_eq!(parse_target(""), None);
    }

    #[test]
    fn a_refused_print_screen_says_how_to_free_it() {
        let (b, _) = plan(&Config::default());
        let by = |id: &str| b.iter().find(|b| b.id == id).unwrap();
        let line = unavailable(by("quick-snip-circle"), "already registered");
        assert!(line.contains("PrintScreen") && line.contains("already registered"));
        #[cfg(windows)]
        assert!(line.contains("Use the Print screen key to open screen capture"));
        #[cfg(not(windows))]
        assert!(line.contains("screenshot tool"));
        // Alt+Print Screen needs the same hint; other keys get none.
        assert!(unavailable(by("quick-snip-window"), "x").contains("free Print Screen"));
        assert!(!unavailable(by("quick-copy_text-circle"), "x").contains("free Print Screen"));
        assert_eq!(line.lines().count(), 1, "one line");
    }

    #[test]
    fn reads_the_desktops_key_names() {
        for (said, ours) in [
            ("Press <Shift><Control>space", "Ctrl+Shift+Space"),
            ("Press <Control>Print", "Ctrl+PrintScreen"),
            ("Press Print", "PrintScreen"),
            ("Press <Super>o", "Super+O"),
            ("<Alt>F5", "Alt+F5"),
            ("Ctrl+Shift+Space", "Ctrl+Shift+Space"), // KDE: already ours
            ("Press <Fancy>x", "Press <Fancy>x"),     // unknown: as said
        ] {
            assert_eq!(desktop_spelling(said), ours, "{said}");
        }
    }

    #[test]
    fn names_where_each_desktop_changes_its_keys() {
        assert!(
            managed_on("ubuntu:GNOME").ends_with("Settings → Apps → OpenCircle → Global Shortcuts")
        );
        assert!(managed_on("KDE").contains("System Settings → Keyboard → Shortcuts"));
        assert!(managed_on("Hyprland").starts_with("Your desktop manages this shortcut"));
    }

    #[test]
    fn the_desktops_key_is_reported_only_when_it_differs() {
        let (b, _) = plan(&Config::default());
        let desktop = vec![
            (
                "capture".to_string(),
                Some("Press <Shift><Control>space".to_string()),
            ),
            (
                "quick-ai_explain-circle".into(),
                Some("Press <Super>e".into()),
            ),
            ("quick-copy_text-circle".into(), None),
        ];
        let say = |k: &str| desktop_key_in(&b, &desktop, k);
        assert_eq!(say("Ctrl+Shift+Space"), None, "same key, other spelling");
        assert_eq!(
            say("Ctrl+Shift+E"),
            Some(DesktopKey::Other("Super+E".into()))
        );
        assert_eq!(say("Ctrl+Shift+X"), Some(DesktopKey::Unassigned));
        assert_eq!(say("Ctrl+Shift+T"), None, "the desktop hasn't said");
        assert_eq!(say("Ctrl+Alt+9"), None, "not a running shortcut");
        assert_eq!(say(""), None);
        assert_eq!(desktop_key_in(&b, &[], "Ctrl+Shift+E"), None, "no portal");
    }

    #[test]
    fn registry_actions_and_planned_ones_are_known() {
        assert!(known_action("copy") && known_action("save"));
        assert!(known_action("translate") && known_action("lens"));
        assert!(known_action("song_search"));
        assert!(!known_action("") && !known_action("rm") && !known_action("Copy"));
    }
}
