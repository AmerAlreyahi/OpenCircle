//! Triggers from outside the UI: the global shortcuts (capture + one per quick
//! action, all from the config) and the CLI socket (`capture`, `run <action>`).
//!
//! `plan` turns the config into a list of `Binding`s; that list is the one source of
//! truth for registering with the OS, for routing a key press to its `Trigger`, and
//! for the stable ids the Wayland portal needs.
#[cfg(target_os = "linux")]
mod portal;
mod socket;

pub use socket::{Command, forward, listen};

use std::cell::RefCell;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::config::{Config, QuickAction, Target, parse_shortcut};
use crate::{Trigger, Triggers};

/// Ids `opencircle run` takes that are not in the action registry (`ui/quick.rs`
/// runs ai_explain and copy_text; snip opens Snip mode; translate comes later).
const PLANNED_ACTIONS: &[&str] = &["translate", "ai_explain", "copy_text", "lens", "snip"];

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
    }
}

fn parse_target(name: &str) -> Option<Target> {
    [Target::FullScreen, Target::Circle, Target::LastRegion]
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
            Err(e) => eprintln!(
                "[opencircle] shortcut {} ({}) is unavailable: {e}. Another app may use it; \
                 pick a different one in Settings.",
                b.shortcut, b.description
            ),
        }
    }
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
                "quick-copy_text-circle"
            ]
        );
        assert_eq!(b[0].shortcut, "Ctrl+Shift+Space");
        assert_eq!(b[2].description, "Translate (full screen, live)");
        assert_eq!(b[3].description, "Ai explain (circle)");
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
        for t in [Target::FullScreen, Target::Circle, Target::LastRegion] {
            assert_eq!(parse_target(target_name(t)), Some(t));
        }
        assert_eq!(parse_target("Circle"), None);
        assert_eq!(parse_target(""), None);
    }

    #[test]
    fn registry_actions_and_planned_ones_are_known() {
        assert!(known_action("copy") && known_action("save"));
        assert!(known_action("translate") && known_action("lens"));
        assert!(!known_action("") && !known_action("rm") && !known_action("Copy"));
    }
}
