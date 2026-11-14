//! Where the glossary applies (owner, 2026-11-14): only on a game screen, and
//! each game with its own glossary.
//!
//! One decision, [`game`], for every place that shows or uses a glossary (the
//! pill's button, the word card's names, name suggestions, the translation
//! itself): no browser address bar on the frame (`pipeline::has_address_bar`,
//! every OS) and, where the OS tells the focused window (Windows, X11), that
//! window covers its screen and is no browser. Wayland tells no window: the
//! frame test alone decides. A window a glossary names is a game.
//!
//! Which glossary: the player's pick on the pill, else the one whose "windows"
//! list names the window in front; with no match, where titles are known, none
//! (never the previous game's); on Wayland the saved pick stays.

#[cfg(test)]
use std::cell::RefCell;
#[cfg(not(test))]
use std::sync::Mutex;

use super::Glossary;
use super::store;
use crate::shared::platform::capture::window_rect::{self, Focused};

/// A browser's window class (Windows, X11): never a game's window.
pub(crate) fn is_browser(class: &str) -> bool {
    const CLASSES: &[&str] = &[
        "chrome_widgetwin", // Chrome, Edge, Brave, Opera, Vivaldi (and Electron apps)
        "mozillawindowclass",
        "mozilla", // Firefox on X11
        "navigator",
        "firefox",
        "google-chrome",
        "chromium",
        "microsoft-edge",
        "brave-browser",
        "opera",
        "vivaldi",
        "epiphany",
        "falkon",
        "librewolf",
        "zen",
    ];
    let class = class.to_lowercase();
    CLASSES.iter().any(|c| class.starts_with(c))
}

/// Is this screen a game's? `address_bar`: the frame shows a browser's;
/// `window`: the focused window where the OS tells one; `named`: a glossary
/// names that window.
pub(crate) fn game(address_bar: bool, window: Option<&Focused>, named: bool) -> bool {
    named || (!address_bar && window.is_none_or(|w| w.covers_screen && !is_browser(&w.class)))
}

/// The glossary to use: the pick (`""` = none), else the window's own; with
/// no match it is none where titles are known (`titles`), else the saved
/// choice.
pub(crate) fn resolve<'a>(
    all: &'a [(String, Glossary)],
    chosen: &str,
    title: Option<&str>,
    titles: bool,
    pick: Option<&str>,
) -> Option<&'a str> {
    let slug = pick
        .or_else(|| store::for_window(all, title))
        .or((!titles).then_some(chosen))?;
    all.iter().find(|(s, _)| s == slug).map(|(s, _)| s.as_str())
}

/// What this capture has shown so far.
#[derive(Default)]
struct Seen {
    /// The decision, made once the frame was read.
    game: bool,
    /// The player's pick on the pill for this capture.
    pick: Option<String>,
}

// Tests run on many threads: each has its own screen, a game by default.
#[cfg(test)]
thread_local!(static SEEN: RefCell<Seen> = const { RefCell::new(Seen { game: true, pick: None }) });
#[cfg(not(test))]
static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

fn seen<R>(f: impl FnOnce(&mut Seen) -> R) -> R {
    #[cfg(test)]
    return SEEN.with(|s| f(&mut s.borrow_mut()));
    #[cfg(not(test))]
    return f(SEEN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(Seen::default));
}

/// A new capture: nothing is known about it until its frame is read.
pub fn begin() {
    seen(|s| *s = Seen::default());
}

/// The frame was read (`address_bar`: a browser's was found): decides the
/// capture's screen.
pub fn saw_frame(address_bar: bool) {
    let all = store::load_all(&store::dir());
    let focus = window_rect::captured();
    let title = focus.as_ref().map(|f| f.title.as_str());
    let named = store::for_window(&all, title).is_some();
    seen(|s| s.game = game(address_bar, focus.as_ref(), named));
}

/// This capture's screen is a game's.
pub fn is_game() -> bool {
    seen(|s| s.game)
}

/// The player picked `slug` on the pill (`""` = none) for this capture.
pub fn pick(slug: &str) {
    seen(|s| s.pick = Some(slug.to_string()));
}

/// What `active` depends on besides the files (the cache key).
pub(crate) fn key() -> String {
    seen(|s| format!("{}{:?}", s.game, s.pick))
}

/// The glossary the pick or the window in front gives, game or not (what the
/// pill's button shows).
pub(crate) fn resolved<'a>(all: &'a [(String, Glossary)], chosen: &str) -> Option<&'a str> {
    let pick = seen(|s| s.pick.clone());
    let title = window_rect::captured().map(|f| f.title);
    resolve(
        all,
        chosen,
        title.as_deref(),
        window_rect::titles_available(),
        pick.as_deref(),
    )
}

/// The glossary in use now, `None` off a game screen.
pub(crate) fn active<'a>(all: &'a [(String, Glossary)], chosen: &str) -> Option<&'a str> {
    if is_game() {
        resolved(all, chosen)
    } else {
        None
    }
}
