//! The tray icon and its menu's entries (`entries`, the design's widgets/tray-menu.js). Windows shows our own
//! popup for them (`ui::screens::tray_menu`, opened at the icon by `Trigger::TrayMenu`); Linux the desktop's
//! own menu (AppIndicator) with the same rows. A missing tray is not an error: the app keeps working through
//! its shortcut and `opencircle capture`. The capturing entries wait until the menu is off the screen, so it
//! is never captured.
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use super::monitor::Place;
use crate::config::{Config, Target};
use crate::labs::{Feature, Labs};
use crate::{Trigger, Triggers};

/// The tray menu's entries, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Circle,
    Translate,
    Snip,
    Live,
    Song,
    /// Only while a live pin lets clicks pass through (`tray_menu::Menu::with_click_pins`;
    /// not in `ALL`): they take clicks again. `ui::windows` runs it itself.
    ClickPins,
    Settings,
    Quit,
}

impl Entry {
    const ALL: [Entry; 7] = [
        Entry::Circle,
        Entry::Translate,
        Entry::Snip,
        Entry::Live,
        Entry::Song,
        Entry::Settings,
        Entry::Quit,
    ];

    /// Its row's text (also its accessible name).
    pub fn label(self) -> &'static str {
        match self {
            Entry::Circle => "Circle to Search",
            Entry::Translate => "Translate screen",
            Entry::Snip => "Snip",
            Entry::Live => "Live translate",
            Entry::Song => "Song search",
            Entry::ClickPins => "Make pins clickable",
            Entry::Settings => "Settings…",
            Entry::Quit => "Quit",
        }
    }

    /// A separator goes above it.
    pub fn separated(self) -> bool {
        self == Entry::Settings
    }

    /// The Labs switch that shows it (`None` = always shown).
    fn feature(self) -> Option<Feature> {
        match self {
            Entry::Live => Some(Feature::Live),
            Entry::Song => Some(Feature::Song),
            _ => None,
        }
    }

    /// The user's own shortcut for it in `cfg` (the hint at the row's end), when one is bound.
    pub fn shortcut(self, cfg: &Config) -> Option<String> {
        let quick = |action: &str, live: bool, target: Option<Target>| {
            (cfg.quick_actions.iter())
                .find(|q| {
                    q.action == action
                        && q.live == live
                        && target.is_none_or(|t| q.target == t)
                        && !q.shortcut.is_empty()
                })
                .map(|q| q.shortcut.clone())
        };
        match self {
            Entry::Circle => Some(cfg.shortcut.clone()).filter(|s| !s.is_empty()),
            Entry::Translate => quick("translate", false, Some(Target::FullScreen)),
            Entry::Snip => quick("snip", false, None),
            Entry::Live => quick("translate", true, None),
            Entry::Song => quick("song_search", false, None),
            Entry::ClickPins | Entry::Settings | Entry::Quit => None,
        }
    }

    /// It captures the screen, so it waits until the menu is off it.
    pub fn captures(self) -> bool {
        !matches!(self, Entry::ClickPins | Entry::Settings | Entry::Quit)
    }

    /// What choosing it does.
    pub fn trigger(self, at: Instant) -> Trigger {
        let run = |action: &str, target| {
            let run = crate::hotkey::Command::Run {
                action: action.into(),
                target,
            };
            crate::hotkey::trigger(&run, at)
        };
        match self {
            Entry::Circle => Trigger::Capture(at),
            // the one-shot translate, never the live one bound to the same action
            Entry::Translate => run("translate", Some(Target::FullScreen)),
            Entry::Snip => run("snip", None),
            Entry::Live => Trigger::Quick(
                at,
                crate::config::QuickAction {
                    action: "translate".into(),
                    shortcut: String::new(),
                    target: Target::FullScreen,
                    live: true,
                },
            ),
            Entry::Song => run("song_search", None),
            // never sent: `ui::windows` runs it (the pins are its own); Settings at worst
            Entry::ClickPins | Entry::Settings => Trigger::OpenSettings,
            Entry::Quit => Trigger::Quit,
        }
    }
}

/// The entries shown with these Labs switches: a Labs feature's only while it is on.
pub fn entries(labs: &Labs) -> Vec<Entry> {
    (Entry::ALL.into_iter())
        .filter(|e| e.feature().is_none_or(|f| labs.has(f)))
        .collect()
}

/// After the menu is gone: the screen redraws what it covered.
pub const SETTLE: Duration = Duration::from_millis(150);

/// Where the menu (`menu` px) opens for the tray icon at `icon` on a monitor whose work area is `work` (both
/// `[left, top, right, bottom]`, physical desktop px), `gap` px off the taskbar: above a bottom taskbar, below
/// a top one, beside a left or right one, centred on the icon along it. An icon inside the work area (the
/// overflow flyout, a hidden taskbar): above it, else below. Always inside the work area.
pub fn place(icon: [i32; 4], work: [i32; 4], menu: (i32, i32), gap: i32) -> (i32, i32) {
    let [left, top, right, bottom] = icon;
    let [w_left, w_top, w_right, w_bottom] = work;
    let (w, h) = menu;
    let (cx, cy) = ((left + right - w) / 2, (top + bottom - h) / 2);
    let (x, y) = if top >= w_bottom {
        (cx, w_bottom - gap - h)
    } else if bottom <= w_top {
        (cx, w_top + gap)
    } else if right <= w_left {
        (w_left + gap, cy)
    } else if left >= w_right {
        (w_right - gap - w, cy)
    } else if top - gap - h >= w_top {
        (cx, top - gap - h)
    } else {
        (cx, bottom + gap)
    };
    let inside = |v: i32, lo: i32, hi: i32| v.min(hi).max(lo);
    (
        inside(x, w_left + gap, w_right - gap - w),
        inside(y, w_top + gap, w_bottom - gap - h),
    )
}

/// The menu's window (`Place`: physical px at the icon monitor's scale) for the icon at `icon` and a menu
/// of `size` logical px, 8 logical px off the taskbar.
pub fn menu_place(icon: [i32; 4], size: iced::Size) -> Place {
    let (work, scale) = monitor_of(icon);
    let px = (
        (size.width * scale).round() as i32,
        (size.height * scale).round() as i32,
    );
    Place {
        origin: place(icon, work, px, (8.0 * scale).round() as i32),
        px: (px.0 as u32, px.1 as u32),
        scale,
    }
}

/// The work area (`[left, top, right, bottom]`, physical px) and the scale of the monitor the icon is on.
#[cfg(windows)]
fn monitor_of(icon: [i32; 4]) -> ([i32; 4], f32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let [left, top, right, bottom] = icon;
    let rect = RECT {
        left,
        top,
        right,
        bottom,
    };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    let (mut dpi, mut unused) = (96, 96);
    // SAFETY: plain Win32 queries; `info` is sized as cbSize says.
    unsafe {
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(monitor, &mut info);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
    }
    let w = info.rcWork;
    ([w.left, w.top, w.right, w.bottom], dpi as f32 / 96.0)
}

/// Elsewhere the desktop draws the menu itself: never asked.
#[cfg(not(windows))]
fn monitor_of(icon: [i32; 4]) -> ([i32; 4], f32) {
    (icon, 1.0)
}

/// Linux: the desktop's menu and its items, on the thread that owns the icon (they are not `Send`).
#[cfg(target_os = "linux")]
struct Built {
    menu: Menu,
    items: Vec<(Entry, MenuItem)>,
    separator: PredefinedMenuItem,
}

#[cfg(target_os = "linux")]
thread_local! {
    static BUILT: std::cell::RefCell<Option<Built>> = const { std::cell::RefCell::new(None) };
}

/// Shows exactly `entries(labs)` in the menu, the separator above Settings.
#[cfg(target_os = "linux")]
fn fill(built: &Built, labs: &Labs) {
    for (_, item) in &built.items {
        let _ = built.menu.remove(item);
    }
    let _ = built.menu.remove(&built.separator);
    let shown = entries(labs);
    for (entry, item) in &built.items {
        if shown.contains(entry) {
            if entry.separated() {
                let _ = built.menu.append(&built.separator as &dyn IsMenuItem);
            }
            let _ = built.menu.append(item as &dyn IsMenuItem);
        }
    }
}

/// Labs changed: Linux rebuilds the menu now, on the tray's own (GTK) thread. Windows builds its popup at
/// each opening.
pub fn refresh() {
    #[cfg(target_os = "linux")]
    {
        let labs = crate::config::current().labs;
        let apply = move || BUILT.with_borrow(|b| b.iter().for_each(|b| fill(b, &labs)));
        gtk::glib::idle_add_once(apply);
    }
}

/// The tray icon's identity for the exe at `path`: a hash of the lower-cased path (FNV-1a, 128 bit, so it never
/// changes between Rust versions). Windows binds a GUID to the exe path that first registers it and refuses it
/// from any other path, so one constant GUID fails for every copy but one (portable, installed, updated); a GUID
/// per path is always ours, so a new start of the same exe may delete the ghost a killed run left under it.
fn guid_for(path: &std::path::Path) -> u128 {
    path.to_string_lossy()
        .to_lowercase()
        .bytes()
        .fold(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d, |h, b| {
            (h ^ u128::from(b)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b)
        })
}

/// The GUID the icon is registered under (not set for the plain fallback).
#[cfg(windows)]
static HELD_GUID: std::sync::OnceLock<u128> = std::sync::OnceLock::new();

/// Windows only: removes our GUID icon on a crash path (a panic, Ctrl+C), and only when we hold one.
#[cfg(windows)]
pub fn remove_icon() {
    if let Some(guid) = HELD_GUID.get() {
        delete_icon(*guid);
    }
}

/// Windows only: deletes the icon under `guid`, also one whose process is dead (a ghost). Only the exe path the
/// GUID is bound to may do it, which is us (`guid_for`).
#[cfg(windows)]
fn delete_icon(guid: u128) {
    use windows::Win32::UI::Shell::{NIF_GUID, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW};
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        uFlags: NIF_GUID,
        guidItem: windows::core::GUID::from_u128(guid),
        ..Default::default()
    };
    // SAFETY: plain shell call with a fully initialised struct.
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
}

/// Windows only: the shell holds an icon under `guid`. `Shell_NotifyIconGetRect` answers S_OK on the taskbar and
/// S_FALSE in the closed overflow flyout (VM, Windows 10); both are success here. tray-icon's `rect()` takes S_OK
/// alone, so an accepted icon in the flyout looked refused and a second, plain icon was added beside it.
#[cfg(windows)]
fn has_icon(guid: u128) -> bool {
    use windows::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
    let id = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        guidItem: windows::core::GUID::from_u128(guid),
        ..Default::default()
    };
    // SAFETY: read-only shell query with a fully initialised struct.
    unsafe { Shell_NotifyIconGetRect(&id) }.is_ok()
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
                crate::log!(
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
        remove_on_crash();
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

/// Linux: the desktop's menu with `entries`; a choice goes to `tx` (a capturing one once the menu had time
/// to leave the screen: GTK's menu lives in the shell, no handle here, so the settle delay only).
#[cfg(target_os = "linux")]
fn desktop_menu(tx: Triggers) -> Menu {
    let items: Vec<_> = (Entry::ALL.into_iter())
        .map(|e| (e, MenuItem::new(e.label(), true, None)))
        .collect();
    let ids: Vec<_> = (items.iter()).map(|(e, i)| (*e, i.id().clone())).collect();
    let built = Built {
        menu: Menu::new(),
        items,
        separator: PredefinedMenuItem::separator(),
    };
    fill(&built, &crate::config::current().labs);
    let menu = built.menu.clone();
    BUILT.set(Some(built));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let Some(entry) = (ids.iter()).find_map(|(e, id)| (*id == event.id).then_some(*e)) else {
            return;
        };
        // turned off in Labs meanwhile: nothing
        if !entries(&crate::config::current().labs).contains(&entry) {
            return;
        }
        let trigger = entry.trigger(Instant::now());
        let tx = tx.clone();
        let send = move || {
            let _ = tx.unbounded_send(trigger);
        };
        match entry.captures() {
            true => drop(std::thread::spawn(move || {
                std::thread::sleep(SETTLE);
                send();
            })),
            false => send(),
        }
    }));
    menu
}

/// Windows: no menu of the shell's: a right-click (let go) on the icon asks the
/// app for our popup there; a left-click opens Settings (owner, 2026-10-28).
#[cfg(windows)]
fn on_right_click(tx: Triggers) {
    use tray_icon::TrayIconEvent;
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button,
            button_state,
            rect,
            ..
        } = event
        {
            let at = [
                rect.position.x.round() as i32,
                rect.position.y.round() as i32,
                rect.size.width as i32,
                rect.size.height as i32,
            ];
            if let Some(trigger) = on_click(button, button_state, at) {
                let _ = tx.unbounded_send(trigger);
            }
        }
    }));
}

/// What a click on the tray icon does: let go of the right button = our menu at
/// the icon (`at` = x, y, width, height); of the left button = Settings.
#[cfg(windows)]
fn on_click(
    button: tray_icon::MouseButton,
    state: tray_icon::MouseButtonState,
    [x, y, w, h]: [i32; 4],
) -> Option<Trigger> {
    use tray_icon::{MouseButton, MouseButtonState};
    match (button, state) {
        (MouseButton::Right, MouseButtonState::Up) => Some(Trigger::TrayMenu([x, y, x + w, y + h])),
        (MouseButton::Left, MouseButtonState::Up) => Some(Trigger::OpenSettings),
        _ => None,
    }
}

fn build(tx: Triggers) -> Option<TrayIcon> {
    #[cfg(target_os = "linux")]
    let menu = desktop_menu(tx);
    #[cfg(windows)]
    on_right_click(tx);
    let icon = icon();
    let exe = std::env::current_exe().unwrap_or_default();
    #[cfg(windows)]
    let clear = delete_icon;
    #[cfg(not(windows))]
    let clear = |_| {};
    let tray = add_with_fallback(guid_for(&exe), clear, |guid| {
        let builder = TrayIconBuilder::new()
            .with_tooltip("OpenCircle")
            .with_icon(icon.clone());
        #[cfg(target_os = "linux")]
        let builder = builder.with_menu(Box::new(menu.clone()));
        let builder = match guid {
            Some(guid) => builder.with_guid(guid),
            None => builder,
        };
        let tray = builder.build().map_err(|e| e.to_string())?;
        // tray-icon ignores a refused add (Windows: GUID bound to another exe path), so ask the shell
        // whether the icon exists (the ghost under our GUID was deleted first, so it is ours).
        #[cfg(windows)]
        if guid.is_some_and(|g| !has_icon(g)) {
            // Not dropped: its drop would delete a GUID we do not hold, and the shell then ignores the next
            // (plain) icon too (VM, Windows 10: no icon at all). The leaked window shows nothing.
            std::mem::forget(tray);
            return Err("the shell did not take the icon".to_string());
        }
        Ok::<_, String>(tray)
    })
    .map_err(|e| crate::log!("[opencircle] tray unavailable: {e}"))
    .ok()?;
    #[cfg(windows)]
    if let Some(guid) = tray.1 {
        let _ = HELD_GUID.set(guid);
    }
    Some(tray.0)
}

/// Adds the icon under `guid`, after `clear` deleted what a killed run left under it: the shell refuses to add
/// a GUID a dead run's icon (a ghost) still holds, and then answers GetRect and NIM_MODIFY for that ghost, so the
/// new run had no icon of its own (VM, Windows 10). If the shell still refuses it (the GUID is bound to another
/// exe path), adds it without one: the icon shows, and only a force-kill can leave a ghost. `add` gets the GUID
/// to use; the result says which one the icon holds.
fn add_with_fallback<T, E: std::fmt::Display>(
    guid: u128,
    clear: impl FnOnce(u128),
    mut add: impl FnMut(Option<u128>) -> Result<T, E>,
) -> Result<(T, Option<u128>), E> {
    clear(guid);
    match add(Some(guid)) {
        Ok(icon) => Ok((icon, Some(guid))),
        Err(e) => {
            crate::log!("[opencircle] tray: icon identity refused ({e}); using a plain icon");
            add(None).map(|icon| (icon, None))
        }
    }
}

/// The 32 px app icon, embedded in the binary.
// ponytail: one size; pick 16/24/32 by tray DPI if it looks soft on scaled displays.
fn icon() -> Icon {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../../../assets/icons/32x32.png"
    )));
    let mut reader = decoder.read_info().expect("embedded icon is a valid PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("small icon")];
    let info = reader.next_frame(&mut rgba).expect("embedded icon decodes");
    rgba.truncate(info.buffer_size());
    Icon::from_rgba(rgba, info.width, info.height).expect("embedded icon is RGBA")
}

#[cfg(test)]
mod tests {
    use super::{Entry, add_with_fallback, entries, guid_for, place};
    use crate::config::Config;
    use crate::labs::{Feature, Labs};
    use std::path::Path;

    /// The design's rows in its order: a Labs feature's row only while it is on (Snip, Live translate, Song
    /// search); Translate screen always. Before: no Translate screen, Song search never.
    #[test]
    fn the_menu_shows_a_labs_row_only_while_it_is_on() {
        use Entry::*;
        // Snip is out of Labs (N-41): always there
        assert_eq!(
            entries(&Labs::default()),
            [Circle, Translate, Snip, Settings, Quit]
        );
        let mut on = Labs::default();
        on.set(Feature::Live, true);
        assert_eq!(
            entries(&on),
            [Circle, Translate, Snip, Live, Settings, Quit]
        );
        on.set(Feature::Song, true);
        assert_eq!(
            entries(&on),
            [Circle, Translate, Snip, Live, Song, Settings, Quit]
        );
        assert_eq!(
            Entry::ALL
                .iter()
                .filter(|e| e.separated())
                .collect::<Vec<_>>(),
            [&Settings]
        );
    }

    /// The hints are the user's own shortcuts, and Translate screen runs the one-shot translate even when the
    /// live one is bound to the same action.
    #[test]
    fn hints_are_the_users_shortcuts_and_translate_is_one_shot() {
        let mut cfg = Config::default();
        assert_eq!(
            Entry::Translate.shortcut(&cfg).as_deref(),
            Some("Ctrl+Shift+T")
        );
        assert_eq!(Entry::Live.shortcut(&cfg).as_deref(), Some("Ctrl+Shift+L"));
        assert_eq!(Entry::Song.shortcut(&cfg), None, "unbound: no hint");
        assert_eq!(Entry::Quit.shortcut(&cfg), None);
        cfg.shortcut = "Alt+Q".into();
        assert_eq!(Entry::Circle.shortcut(&cfg).as_deref(), Some("Alt+Q"));
        cfg.quick_actions
            .retain(|q| !(q.action == "translate" && !q.live));
        assert_eq!(Entry::Translate.shortcut(&cfg), None);
        match Entry::Translate.trigger(std::time::Instant::now()) {
            crate::Trigger::Quick(_, q) => assert!(q.action == "translate" && !q.live),
            other => panic!("{other:?}"),
        }
        assert!(!Entry::Settings.captures() && !Entry::Quit.captures());
        assert!(Entry::Circle.captures() && Entry::Translate.captures());
    }

    /// The menu opens off the taskbar on whichever edge it is, centred on the icon along it, and never
    /// leaves the work area. 1280x720 monitor, a 40 px taskbar, a 240x200 menu, gap 8.
    #[test]
    fn the_menu_opens_beside_the_icon_on_every_taskbar_edge() {
        let menu = (240, 200);
        // bottom: above the taskbar, centred on the icon
        let work = [0, 0, 1280, 680];
        assert_eq!(place([600, 684, 624, 708], work, menu, 8), (492, 472));
        // ... and an icon at the corner keeps it inside
        assert_eq!(place([1250, 684, 1274, 708], work, menu, 8), (1032, 472));
        // top
        let work = [0, 40, 1280, 720];
        assert_eq!(place([600, 8, 624, 32], work, menu, 8), (492, 48));
        // left
        let work = [40, 0, 1280, 720];
        assert_eq!(place([8, 600, 32, 624], work, menu, 8), (48, 512));
        // right
        let work = [0, 0, 1240, 720];
        assert_eq!(place([1248, 600, 1272, 624], work, menu, 8), (992, 512));
        // inside the work area (the overflow flyout): above the icon, else below
        let work = [0, 0, 1280, 680];
        assert_eq!(place([600, 600, 624, 624], work, menu, 8), (492, 392));
        assert_eq!(place([600, 20, 624, 44], work, menu, 8), (492, 52));
        // a second monitor at negative coordinates
        let work = [-1920, 0, 0, 1040];
        assert_eq!(place([-100, 1044, -76, 1068], work, menu, 8), (-248, 832));
    }

    /// A killed run's icon (a ghost) still holds our GUID and the shell refuses to add it again, so it is
    /// deleted before the add. Before: added straight away; the add failed and the ghost passed for our icon.
    #[test]
    fn the_identity_is_cleared_then_tried_first_and_used_when_accepted() {
        let tried = std::cell::RefCell::new(Vec::new());
        let r = add_with_fallback::<(), String>(
            7,
            |g| tried.borrow_mut().push(format!("clear {g}")),
            |g| {
                tried.borrow_mut().push(format!("add {g:?}"));
                Ok(())
            },
        );
        assert_eq!(r, Ok(((), Some(7))));
        assert_eq!(*tried.borrow(), ["clear 7", "add Some(7)"]);
    }

    /// Windows refuses a GUID from any exe path but the one that first registered it, so the identity follows
    /// the path: stable for one exe (any letter case), different for every other copy. Before: one constant
    /// GUID, refused for every copy but one (no ghost replacement there).
    #[test]
    fn the_identity_follows_the_exe_path() {
        let a = guid_for(Path::new(r"C:\Apps\OpenCircle\opencircle.exe"));
        assert_eq!(a, guid_for(Path::new(r"c:\apps\opencircle\OPENCIRCLE.EXE")));
        assert_ne!(a, guid_for(Path::new(r"C:\Portable\opencircle.exe")));
        assert_ne!(a, guid_for(Path::new("")));
    }

    #[test]
    fn a_refused_identity_falls_back_to_a_plain_icon() {
        let mut tried = Vec::new();
        let r = add_with_fallback::<&str, String>(
            7,
            |_| {},
            |g| {
                tried.push(g);
                if g.is_some() {
                    Err("refused".to_string())
                } else {
                    Ok("plain")
                }
            },
        );
        // not holding the GUID: a crash path must not delete it (the shell then drops the plain icon too)
        assert_eq!(r, Ok(("plain", None)));
        assert_eq!(tried, [Some(7), None]);
    }

    /// A left-click on the tray icon opens Settings; a right-click our menu at
    /// the icon; a press alone does nothing (owner, 2026-10-28).
    #[test]
    #[cfg(windows)]
    fn a_left_click_opens_settings_and_a_right_click_the_menu() {
        use super::Trigger;
        use tray_icon::{MouseButton, MouseButtonState};
        let at = [10, 20, 30, 40];
        assert!(matches!(
            super::on_click(MouseButton::Left, MouseButtonState::Up, at),
            Some(Trigger::OpenSettings)
        ));
        assert!(matches!(
            super::on_click(MouseButton::Right, MouseButtonState::Up, at),
            Some(Trigger::TrayMenu([10, 20, 40, 60]))
        ));
        assert!(super::on_click(MouseButton::Left, MouseButtonState::Down, at).is_none());
    }
}
