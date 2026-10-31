//! Triggers from outside the UI: the global shortcuts (capture + one per quick
//! action, all from the config) and the CLI socket (`capture`, `run <action>`).
//!
//! `plan` turns the config into a list of `Binding`s; that list is the one source of
//! truth for registering with the OS, for routing a key press to its `Trigger`, and
//! for the stable ids the Wayland portal needs.
#[cfg(target_os = "linux")]
mod portal;
mod quick_id;
mod socket;

pub use quick_id::QuickId;
pub use socket::{Command, forward, forward_or_start, is_older, listen, running, running_version};

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::config::{Config, QuickAction, Target, parse_shortcut};
use crate::{Trigger, Triggers};

/// Is `id` an action `opencircle run` may start?
fn known_action(id: &str) -> bool {
    QuickId::from_name(id).is_known()
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
    /// Stable across runs and reloads (the portal id adds the key: `portal_id`).
    id: String,
    /// Shown by the desktop in its shortcut settings.
    description: String,
    /// As written in the config; always valid (`parse_shortcut` accepted it).
    shortcut: String,
    hotkey: HotKey,
    what: What,
}

impl Binding {
    /// The id the Wayland portal knows this shortcut by. A key that is not our
    /// default for it rides in the id, so changing the key in Settings is a new
    /// shortcut for the desktop, which then takes the key we ask for; under the bare
    /// `id` GNOME keeps the key it stored for it and ignores ours. A default key keeps
    /// the bare id (the ids of earlier versions, so an update asks nothing again, and
    /// a key the user set in the desktop's own settings is kept).
    fn portal_id(&self) -> String {
        // Labs on: the ids of the shortcuts that can be hidden stay the same.
        let default = plan(&Config {
            labs: crate::labs::Labs::all_on(),
            ..Config::default()
        })
        .0;
        if default
            .iter()
            .any(|d| d.id == self.id && d.shortcut == self.shortcut)
        {
            return self.id.clone();
        }
        let key: String = self
            .shortcut
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        format!("{}.{key}", self.id)
    }

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
        // A hidden (Labs) feature has no shortcut; its entry stays in the config.
        if qa.shortcut.is_empty() || !crate::labs::quick_on(&cfg.labs, qa) {
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
    let mut name = qa.action.as_str().replace('_', " ");
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
    /// The OS refused it: another app holds the key (`regrab`).
    Taken,
}

/// The shortcuts the OS refused at the last grab (another app holds them).
static TAKEN: Mutex<Vec<HotKey>> = Mutex::new(Vec::new());

fn taken() -> MutexGuard<'static, Vec<HotKey>> {
    TAKEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The desktop's own say on `shortcut` (a key of the running config) when it is
/// not simply that key. `None` = active as configured, or nothing known (not
/// Wayland, the desktop hasn't answered yet, or not a running shortcut).
pub fn desktop_key(shortcut: &str) -> Option<DesktopKey> {
    let refused = parse_shortcut(shortcut).is_ok_and(|h| taken().contains(&h));
    if refused {
        return Some(DesktopKey::Taken);
    }
    desktop_key_in(&bindings(), &desktop_keys(), shortcut)
}

/// A registry value `snipping_has_print_screen` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reg {
    /// HKCU\Control Panel\Keyboard `PrintScreenKeyForSnippingEnabled` (DWORD).
    Snipping,
    /// HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion `CurrentBuildNumber`.
    Build,
}

/// Windows opens its own screen capture with Print Screen ("Use the Print
/// screen key to open screen capture"): OpenCircle never gets the key then.
/// Only read: the user turns it off in Windows' settings (Settings > Snip
/// opens the page), never us.
/// Tests never read the machine's setting (on in CI's Windows): they set
/// `print_screen_taken` themselves.
pub fn windows_snips_print_screen() -> bool {
    !cfg!(test) && snipping_has_print_screen(&read_reg)
}

/// The switch as `read` answers: its value when set; unset, it is on from
/// Windows 11 (build 22000) and off before.
fn snipping_has_print_screen(read: &dyn Fn(Reg) -> Option<u32>) -> bool {
    match read(Reg::Snipping) {
        Some(on) => on != 0,
        None => read(Reg::Build).is_some_and(|build| build >= 22_000),
    }
}

#[cfg(windows)]
fn read_reg(value: Reg) -> Option<u32> {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::w;
    match value {
        Reg::Snipping => {
            let mut v = 0u32;
            let mut bytes = std::mem::size_of::<u32>() as u32;
            // SAFETY: plain registry query into a u32 of `bytes` bytes.
            let e = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    w!(r"Control Panel\Keyboard"),
                    w!("PrintScreenKeyForSnippingEnabled"),
                    RRF_RT_REG_DWORD,
                    None,
                    Some((&raw mut v).cast()),
                    Some(&mut bytes),
                )
            };
            (e.0 == 0).then_some(v)
        }
        Reg::Build => {
            let mut buf = [0u16; 32];
            let mut bytes = std::mem::size_of_val(&buf) as u32;
            // SAFETY: plain registry query into a buffer of `bytes` bytes.
            let e = unsafe {
                RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
                    w!("CurrentBuildNumber"),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut bytes),
                )
            };
            if e.0 != 0 {
                return None;
            }
            let len = (bytes as usize / 2).min(buf.len());
            let text = String::from_utf16_lossy(&buf[..len]);
            text.trim_end_matches('\0').parse().ok()
        }
    }
}

#[cfg(not(windows))]
fn read_reg(_: Reg) -> Option<u32> {
    None
}

fn desktop_key_in(
    bindings: &[Binding],
    desktop: &[(String, Option<String>)],
    shortcut: &str,
) -> Option<DesktopKey> {
    let hotkey = parse_shortcut(shortcut).ok()?;
    let binding = bindings.iter().find(|b| b.hotkey == hotkey)?;
    let (_, keys) = desktop.iter().find(|(id, _)| *id == binding.portal_id())?;
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
    bindings
        .iter()
        .find(|b| b.portal_id() == id)
        .map(|b| b.trigger(at))
}

/// The trigger for a command from the socket or the command line. A `run` without a
/// target takes the one configured for that action (first entry), else the default.
pub fn trigger(command: &Command, at: Instant) -> Trigger {
    match command {
        Command::Capture => Trigger::Capture(at),
        Command::OpenSettings => Trigger::OpenSettings,
        Command::Quit => Trigger::Quit,
        Command::Version => unreachable!("answered by the socket, never a trigger"),
        Command::Run { action, target } => {
            let configured = bindings().iter().find_map(|b| match &b.what {
                What::Quick(qa) if qa.action == action.as_str() => Some(qa.clone()),
                _ => None,
            });
            let mut qa = configured.unwrap_or_else(|| QuickAction {
                action: QuickId::from_name(action),
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
        if event.state == HotKeyState::Pressed && !suspended() {
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
        Err(e) => crate::log!(
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
        Some(_) if suspended() => {} // `suspend(false)` grabs the new ones
        Some(grab) => regrab(grab, &bindings()),
        None => crate::log!("[opencircle] shortcuts were not started here; reload skipped"),
    });
}

/// Set while a shortcut field records (`suspend`).
static SUSPENDED: AtomicBool = AtomicBool::new(false);

/// Holds every global shortcut off while `on`, back on after: a shortcut field
/// that is recording must get the keys, never start an action with them.
/// Windows / X11 let go of the grab (the press then reaches the focused
/// window); Wayland's portal can't, so its presses are dropped. Only a change
/// does anything. Call from the thread that called `start`.
pub fn suspend(on: bool) {
    if SUSPENDED.swap(on, Ordering::SeqCst) == on {
        return;
    }
    crate::log!(
        "[opencircle] global shortcuts {}",
        if on { "held off" } else { "back on" }
    );
    GRAB.with_borrow_mut(|slot| match slot {
        Some(grab) if on => {
            let _ = grab.manager.unregister_all(&grab.held);
            grab.held.clear();
        }
        Some(grab) => regrab(grab, &bindings()),
        None => {} // Wayland, or no grab: presses are dropped while suspended
    });
}

/// Whether the global shortcuts are held off (`suspend`).
pub fn suspended() -> bool {
    SUSPENDED.load(Ordering::SeqCst)
}

/// Replaces what the grab holds with `planned`. A shortcut another app already owns is
/// reported once, here; the rest still work.
fn regrab(grab: &mut Grab, planned: &[Binding]) {
    let _ = grab.manager.unregister_all(&grab.held);
    grab.held.clear();
    let mut refused = Vec::new();
    for b in planned {
        match grab.manager.register(b.hotkey) {
            Ok(()) => grab.held.push(b.hotkey),
            Err(e) => {
                crate::log!("[opencircle] {}", unavailable(b, &e.to_string()));
                refused.push(b.hotkey);
            }
        }
    }
    *taken() = refused; // Settings shows these keys as in use
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

/// GNOME's shell grabs Print Screen and Alt+Print Screen for its own screenshot tool
/// before any portal shortcut: the portal reports the key as bound and it never
/// fires. The one line that frees it (and puts it back); `None` for any other key
/// or desktop.
#[cfg(target_os = "linux")]
fn gnome_print_key(b: &Binding, desktop: &str) -> Option<String> {
    use global_hotkey::hotkey::Modifiers;
    if !desktop.split(':').any(|d| d.eq_ignore_ascii_case("gnome"))
        || b.hotkey.key != Code::PrintScreen
    {
        return None;
    }
    let key = match b.hotkey.mods {
        m if m.is_empty() => "show-screenshot-ui",
        Modifiers::ALT => "screenshot-window",
        Modifiers::SHIFT => "screenshot",
        _ => return None,
    };
    Some(format!(
        "GNOME keeps {} for its own screenshot tool. To use it here: \
         gsettings set org.gnome.shell.keybindings {key} \"[]\", then restart OpenCircle \
         (undo: gsettings reset org.gnome.shell.keybindings {key}).",
        b.shortcut
    ))
}

fn log_all(warnings: &[String]) {
    for w in warnings {
        crate::log!("[opencircle] {w}");
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

    fn all_labs() -> Config {
        Config {
            labs: crate::labs::Labs::all_on(),
            ..Config::default()
        }
    }

    #[test]
    fn a_hidden_feature_has_no_shortcut_and_turning_it_on_brings_it_back() {
        use crate::labs::Feature;
        let ids = |c: &Config| -> Vec<String> { plan(c).0.into_iter().map(|b| b.id).collect() };
        let off = ids(&Config::default());
        assert!(!off.iter().any(|i| i.contains("song")), "{off:?}");
        assert!(
            off.contains(&"capture".to_string())
                && off.contains(&"quick-translate-full_screen".to_string())
        );
        // Snip is out of Labs (N-41): its three keys are there with every switch off.
        assert_eq!(
            off.iter().filter(|i| i.starts_with("quick-snip-")).count(),
            3,
            "{off:?}"
        );
        assert!(
            plan(&Config::default())
                .0
                .iter()
                .any(|b| b.hotkey.key == Code::PrintScreen)
        );
        let with = |f: Feature| {
            let mut c = Config::default();
            c.labs.set(f, true);
            // a song shortcut is unbound by default: bind one to see it come back
            for q in &mut c.quick_actions {
                if q.action == "song_search" {
                    q.shortcut = "Ctrl+Shift+M".into();
                }
            }
            ids(&c)
        };
        assert!(!with(Feature::Live).iter().any(|i| i.contains("song")));
        assert!(
            with(Feature::Song)
                .iter()
                .any(|i| i.contains("song_search"))
        );
        // The entries stay in the config while hidden.
        assert!(
            Config::default()
                .quick_actions
                .iter()
                .any(|q| q.action == "snip")
        );
        // Ids a shortcut had before it could be hidden are unchanged (the portal keeps them).
        assert_eq!(plan(&all_labs()).0[5].portal_id(), "quick-snip-circle");
    }

    #[test]
    fn defaults_give_capture_plus_every_bound_quick_action() {
        let (b, w) = plan(&all_labs());
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
        let (b, _) = plan(&all_labs());
        let at = Instant::now();
        let id = |s: &str| parse_shortcut(s).unwrap().id();
        // Live translate is a Labs feature: off, Ctrl+Shift+L is not registered.
        let (off, _) = plan(&Config::default());
        assert!(route_hotkey(&off, id("Ctrl+Shift+L"), at).is_none());
        assert!(!off.iter().any(|b| b.id.ends_with("-live")));

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

    /// Windows' "Use the Print screen key to open screen capture", read through
    /// a fake registry: set = its value; unset = on from Windows 11 only.
    #[test]
    fn windows_snipping_owns_print_screen_as_the_registry_says() {
        let reg = |snipping: Option<u32>, build: Option<u32>| {
            move |r: Reg| match r {
                Reg::Snipping => snipping,
                Reg::Build => build,
            }
        };
        assert!(snipping_has_print_screen(&reg(Some(1), Some(19045))));
        assert!(!snipping_has_print_screen(&reg(Some(0), Some(26100))));
        assert!(snipping_has_print_screen(&reg(None, Some(22631))));
        assert!(!snipping_has_print_screen(&reg(None, Some(19045))));
        assert!(!snipping_has_print_screen(&reg(None, None)));
        #[cfg(not(windows))]
        assert!(!windows_snips_print_screen());
    }

    /// A key the OS refused shows as taken in Settings (`desktop_key`), until a
    /// later grab gets it.
    #[test]
    fn a_refused_shortcut_reads_as_taken() {
        let key = parse_shortcut("Ctrl+Alt+F9").unwrap();
        *taken() = vec![key];
        assert_eq!(desktop_key("Ctrl+Alt+F9"), Some(DesktopKey::Taken));
        assert_ne!(desktop_key("Ctrl+Alt+F10"), Some(DesktopKey::Taken));
        taken().clear();
        assert_eq!(desktop_key("Ctrl+Alt+F9"), None);
    }

    #[test]
    fn a_refused_print_screen_says_how_to_free_it() {
        let (b, _) = plan(&all_labs());
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
    fn a_changed_key_is_a_new_portal_shortcut_an_unchanged_one_keeps_its_id() {
        let id = |c: &Config, which: &str| {
            let (b, _) = plan(c);
            b.iter().find(|b| b.id == which).unwrap().portal_id()
        };
        let before = Config::default();
        let after = cfg("Ctrl+Shift+K", before.quick_actions.clone());
        // GNOME keeps the key it stored under an id and ignores ours: a new key must
        // arrive under a new id (B2), the rest must stay where the user put them.
        assert_ne!(id(&before, "capture"), id(&after, "capture"));
        assert_eq!(
            id(&before, "quick-ai_explain-circle"),
            id(&after, "quick-ai_explain-circle")
        );
        assert_eq!(
            id(&before, "capture"),
            "capture",
            "a default key: the old id"
        );
        assert_eq!(id(&after, "capture"), "capture.ctrl-shift-k");
        // The press the portal reports under the new id still reaches the action.
        let (b, _) = plan(&after);
        let at = Instant::now();
        #[cfg(target_os = "linux")]
        {
            assert!(matches!(
                route_id(&b, "capture.ctrl-shift-k", at),
                Some(Trigger::Capture(_))
            ));
            assert!(route_id(&b, "capture", at).is_none());
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (b, at);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn names_the_gnome_setting_that_holds_each_print_screen_key() {
        let (b, _) = plan(&all_labs());
        let by = |id: &str| b.iter().find(|b| b.id == id).unwrap();
        let hint = |id: &str, desktop: &str| gnome_print_key(by(id), desktop);
        let print = hint("quick-snip-circle", "ubuntu:GNOME").unwrap();
        assert!(print.contains("show-screenshot-ui") && print.contains("gsettings reset"));
        assert!(
            hint("quick-snip-window", "GNOME")
                .unwrap()
                .contains("screenshot-window")
        );
        assert_eq!(
            hint("quick-snip-circle", "KDE"),
            None,
            "only GNOME grabs it"
        );
        assert_eq!(
            hint("quick-snip-full_screen", "GNOME"),
            None,
            "Ctrl+Print is free"
        );
        assert_eq!(hint("capture", "GNOME"), None, "not a Print key");
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
