//! Tray icon with "Circle to search", "Snip", "Live translate on/off", "Settings…" and "Quit". A missing tray is not
//! an error: the app keeps working through its shortcut and `opencircle capture`. The capturing items wait until the
//! menu is off the screen, so it is never captured.
use std::time::{Duration, Instant};

use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::labs::{self, Feature, Labs};
use crate::{Trigger, Triggers};

/// The tray menu's entries, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Circle,
    Snip,
    Live,
    Settings,
    Quit,
}

/// The entries shown with these Labs switches: Snip only when it is on.
fn entries(labs: &Labs) -> Vec<Entry> {
    [
        Entry::Circle,
        Entry::Snip,
        Entry::Live,
        Entry::Settings,
        Entry::Quit,
    ]
    .into_iter()
    .filter(|e| *e != Entry::Snip || labs.has(Feature::Snip))
    .collect()
}

/// The menu and its items, on the thread that owns the icon (they are not `Send`).
struct Built {
    menu: Menu,
    items: [(Entry, MenuItem); 5],
}

thread_local! {
    static BUILT: std::cell::RefCell<Option<Built>> = const { std::cell::RefCell::new(None) };
}

/// Shows exactly `entries(labs)` in the menu.
fn fill(built: &Built, labs: &Labs) {
    for (_, item) in &built.items {
        let _ = built.menu.remove(item);
    }
    let shown = entries(labs);
    for (entry, item) in &built.items {
        if shown.contains(entry) {
            let _ = built.menu.append(item as &dyn IsMenuItem);
        }
    }
}

/// Labs changed: rebuild the menu now (on the tray's own thread: the GTK one on Linux).
pub fn refresh() {
    let labs = crate::config::current().labs;
    let apply = move || BUILT.with_borrow(|b| b.iter().for_each(|b| fill(b, &labs)));
    #[cfg(target_os = "linux")]
    gtk::glib::idle_add_once(apply);
    #[cfg(not(target_os = "linux"))]
    apply();
}

/// The tray icon's one identity. Windows (a fixed GUID, bound to the exe path) replaces a ghost left by a killed
/// run instead of stacking a new icon beside it. Never change it.
const TRAY_GUID: u128 = 0x6f70_656e_6369_7263_6c65_5472_6179_0001;

/// Windows only: removes whatever icon holds `TRAY_GUID` (a ghost of a killed run, or our own on a crash path).
/// Cheap and safe to call when there is none.
#[cfg(windows)]
pub fn remove_icon() {
    use windows::Win32::UI::Shell::{NIF_GUID, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW};
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        uFlags: NIF_GUID,
        guidItem: windows::core::GUID::from_u128(TRAY_GUID),
        ..Default::default()
    };
    // SAFETY: plain shell call with a fully initialised struct.
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
}

/// Cleans the icon up when the process dies the hard way: a panic, or Ctrl+C / console close in a terminal.
#[cfg(windows)]
fn remove_on_crash() {
    use windows::Win32::System::Console::SetConsoleCtrlHandler;
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        remove_icon();
        previous(info);
    }));
    unsafe extern "system" fn on_ctrl(_: u32) -> windows::core::BOOL {
        remove_icon();
        false.into() // keep default handling (the process ends)
    }
    // SAFETY: registers a handler that only calls the shell.
    let _ = unsafe { SetConsoleCtrlHandler(Some(on_ctrl), true) };
}

/// Starts the tray. Keep the returned icon alive (Linux keeps it on its GTK thread).
pub fn start(tx: Triggers) -> Option<TrayIcon> {
    #[cfg(target_os = "linux")]
    {
        // tray-icon on Linux needs a GTK main loop on the thread that owns the icon.
        std::thread::spawn(move || {
            if let Some(why) = linux_tray_missing() {
                eprintln!(
                    "[opencircle] tray unavailable: {why}; use the shortcut or `opencircle capture`"
                );
                return;
            }
            let _tray = build(tx);
            gtk::main();
        });
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        #[cfg(windows)]
        {
            remove_icon(); // a ghost with our GUID would make the add below fail
            remove_on_crash();
        }
        build(tx)
    }
}

/// Why a Linux tray cannot start, or `None` when it can. `libappindicator-sys`
/// loads its library lazily and panics when it is missing, so look for it here
/// first (the same names it tries).
#[cfg(target_os = "linux")]
fn linux_tray_missing() -> Option<&'static str> {
    const LIBS: [&std::ffi::CStr; 4] = [
        c"libayatana-appindicator3.so.1",
        c"libappindicator3.so.1",
        c"libayatana-appindicator3.so",
        c"libappindicator3.so",
    ];
    // SAFETY: dlopen by name; the handle is kept open (never closed).
    let found = LIBS
        .iter()
        .any(|lib| !unsafe { libc::dlopen(lib.as_ptr(), libc::RTLD_LAZY) }.is_null());
    if !found {
        return Some(
            "libayatana-appindicator3 is not installed (apt: libayatana-appindicator3-1, \
             dnf: libayatana-appindicator-gtk3)",
        );
    }
    gtk::init()
        .is_err()
        .then_some("GTK could not start (no display?)")
}

fn build(tx: Triggers) -> Option<TrayIcon> {
    let item = |label| MenuItem::new(label, true, None);
    let items = [
        (Entry::Circle, item("Circle to search")),
        (Entry::Snip, item("Snip")),
        (Entry::Live, item("Live translate on/off (Beta)")),
        (Entry::Settings, item("Settings…")),
        (Entry::Quit, item("Quit")),
    ];
    let id = |entry| {
        items
            .iter()
            .find(|(e, _)| *e == entry)
            .map(|(_, i)| i.id().clone())
    };
    let (circle, snip, live, settings, quit) = (
        id(Entry::Circle)?,
        id(Entry::Snip)?,
        id(Entry::Live)?,
        id(Entry::Settings)?,
        id(Entry::Quit)?,
    );
    let built = Built {
        menu: Menu::new(),
        items,
    };
    fill(&built, &crate::config::current().labs);
    let menu = built.menu.clone();
    BUILT.set(Some(built));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let trigger = if event.id == circle {
            Trigger::Capture(Instant::now())
        } else if event.id == snip && labs::has(Feature::Snip) {
            let run = crate::hotkey::Command::Run {
                action: "snip".into(),
                target: None,
            };
            crate::hotkey::trigger(&run, Instant::now())
        } else if event.id == live {
            let toggle = crate::config::QuickAction {
                action: "translate".into(),
                shortcut: String::new(),
                target: crate::config::Target::FullScreen,
                live: true,
            };
            Trigger::Quick(Instant::now(), toggle)
        } else if event.id == settings {
            Trigger::OpenSettings
        } else if event.id == quit {
            Trigger::Quit
        } else {
            return;
        };
        // Circle, Snip and Live capture the screen: not while our menu (or
        // its fade-out) is still on it, or it is captured and translated.
        let captures = !matches!(trigger, Trigger::OpenSettings | Trigger::Quit);
        let tx = tx.clone();
        let send = move || {
            let _ = tx.unbounded_send(trigger);
        };
        if captures {
            std::thread::spawn(|| after_menu(menu_shown, MENU_WAIT, send));
        } else {
            send();
        }
    }));
    let icon = icon();
    add_with_fallback(|guid| {
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("OpenCircle")
            .with_icon(icon.clone());
        if let Some(guid) = guid {
            builder = builder.with_guid(guid);
        }
        let tray = builder.build().map_err(|e| e.to_string())?;
        // tray-icon ignores a refused add (Windows: GUID bound to another exe path), so ask the shell
        // whether the icon exists: with a GUID it must have a position.
        #[cfg(windows)]
        if guid.is_some() && tray.rect().is_none() {
            return Err("the shell did not take the icon".to_string());
        }
        Ok::<_, String>(tray)
    })
    .map_err(|e| eprintln!("[opencircle] tray unavailable: {e}"))
    .ok()
}

/// The longest wait for the menu to go (then the capture goes ahead).
const MENU_WAIT: Duration = Duration::from_secs(1);
/// After the menu is gone: the screen redraws what it covered.
const SETTLE: Duration = Duration::from_millis(150);

/// Runs `then` once `shown` says the menu is off the screen (polled, at most
/// `wait`) and the screen had `SETTLE` to redraw.
fn after_menu(shown: impl Fn() -> bool, wait: Duration, then: impl FnOnce()) {
    let start = Instant::now();
    while shown() && start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(SETTLE);
    then();
}

/// A popup menu (ours, from the tray) or its fade-out is on the screen.
#[cfg(windows)]
fn menu_shown() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible};
    use windows::core::w;
    // SAFETY: lookups of top-level windows by class; nothing is changed.
    [w!("#32768"), w!("SysFader")]
        .into_iter()
        .any(|class| unsafe {
            FindWindowW(class, None).is_ok_and(|h| IsWindowVisible(h).as_bool())
        })
}

/// GTK's menu lives in the shell (no handle here): the settle delay only.
#[cfg(not(windows))]
fn menu_shown() -> bool {
    false
}

/// Adds the icon under the fixed identity first; if the shell refuses it (Windows binds a GUID to the exe path,
/// so a moved or second copy of the exe is refused), adds it without one: the icon shows, and only a force-kill
/// can leave a ghost. `add` gets the GUID to use.
fn add_with_fallback<T, E: std::fmt::Display>(
    mut add: impl FnMut(Option<u128>) -> Result<T, E>,
) -> Result<T, E> {
    match add(Some(TRAY_GUID)) {
        Ok(icon) => Ok(icon),
        Err(e) => {
            eprintln!("[opencircle] tray: fixed identity refused ({e}); using a plain icon");
            add(None)
        }
    }
}

/// The 32 px app icon, embedded in the binary.
// ponytail: one size; pick 16/24/32 by tray DPI if it looks soft on scaled displays.
fn icon() -> Icon {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../../assets/icons/32x32.png"
    )));
    let mut reader = decoder.read_info().expect("embedded icon is a valid PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("small icon")];
    let info = reader.next_frame(&mut rgba).expect("embedded icon decodes");
    rgba.truncate(info.buffer_size());
    Icon::from_rgba(rgba, info.width, info.height).expect("embedded icon is RGBA")
}

#[cfg(test)]
mod tests {
    use super::{TRAY_GUID, add_with_fallback};

    /// Started from the tray menu, a capture waits until the menu (and its
    /// fade-out) is gone and the screen settled. Before: the menu was
    /// captured and its translation stayed over the page.
    #[test]
    fn a_capture_from_the_menu_waits_for_it_to_close() {
        use super::{SETTLE, after_menu};
        use std::cell::Cell;
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let polls = Cell::new(0);
        let shown = || {
            polls.set(polls.get() + 1);
            start.elapsed() < Duration::from_millis(100) // closes after 100 ms
        };
        let sent = Cell::new(None);
        after_menu(shown, Duration::from_secs(1), || {
            sent.set(Some(start.elapsed()))
        });
        let at = sent.get().expect("sent");
        assert!(at >= Duration::from_millis(100) + SETTLE, "{at:?}");
        assert!(polls.get() > 1);
        // a menu that never reports closed does not hold the capture forever
        let start = Instant::now();
        after_menu(|| true, Duration::from_millis(50), || {});
        assert!(start.elapsed() < Duration::from_millis(50) + SETTLE + Duration::from_millis(100));
    }

    #[test]
    fn a_hidden_feature_has_no_tray_item() {
        use super::{Entry, entries};
        use crate::labs::{Feature, Labs};
        let off = entries(&Labs::default());
        assert_eq!(
            off,
            [Entry::Circle, Entry::Live, Entry::Settings, Entry::Quit]
        );
        let mut on = Labs::default();
        on.set(Feature::Song, true);
        on.set(Feature::Editor, true);
        assert_eq!(entries(&on), off, "only Snip has a tray item");
        on.set(Feature::Snip, true);
        assert_eq!(entries(&on).len(), 5);
        assert_eq!(entries(&on)[1], Entry::Snip);
    }

    #[test]
    fn the_fixed_identity_is_tried_first_and_used_when_accepted() {
        let mut tried = Vec::new();
        let r: Result<(), String> = add_with_fallback(|g| {
            tried.push(g);
            Ok(())
        });
        assert!(r.is_ok());
        assert_eq!(tried, [Some(TRAY_GUID)]);
    }

    #[test]
    fn a_refused_identity_falls_back_to_a_plain_icon() {
        let mut tried = Vec::new();
        let r: Result<&str, String> = add_with_fallback(|g| {
            tried.push(g);
            if g.is_some() {
                Err("refused".to_string())
            } else {
                Ok("plain")
            }
        });
        assert_eq!(r, Ok("plain"));
        assert_eq!(tried, [Some(TRAY_GUID), None]);
    }
}
