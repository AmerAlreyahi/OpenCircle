//! The Google results card (owner, 2026-10-24): Google's own Lens / AI Mode
//! page in the system web view, as a floating card beside the selection.
//!
//! The web view never runs in the resident app: it lives in a short-lived
//! helper process (`helper/`: on Windows `opencircle.exe sheet-helper`, a hidden
//! mode of this very exe, one file to ship; on Linux the separate binary
//! `opencircle-sheet` next to it, the only one that links WebKitGTK), so the
//! app's idle memory is unchanged and everything of the card goes when the helper exits (on ×,
//! Esc, an outside click, or when the overlay drops the panel: `Guard`).
//! Windows and Linux: Lens and AI Mode. On Linux Google often answers
//! WebKitGTK's Lens with its "unusual traffic" check (spike S9): the card shows
//! a hint, the user solves it there or opens the browser (we never touch it).
//! The card sits on the user's side (`[results] side`, moved by dragging its
//! header) at the width they last gave it; both are kept in the config.
//! Nothing opens in the browser by itself (owner, 2026-10-24): only the user's
//! "Open in browser". No helper or web view runtime (WebView2,
//! libwebkit2gtk-4.1): the panel says so and offers that button.
/// Windows: the helper's code, run by `main` for `ARG` (Linux: the separate bin).
#[cfg(all(windows, feature = "sheet"))]
pub mod helper;
pub mod history;
#[allow(dead_code)] // the helper uses the rest
mod proto;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::CardSide;
pub use proto::Kind;
use proto::{Args, Line, Place};

/// The hidden first argument that runs this exe as the Windows helper.
#[cfg(windows)]
pub const ARG: &str = "sheet-helper";
/// The Linux helper's file name, next to the app's exe.
#[cfg(not(windows))]
const HELPER: &str = "opencircle-sheet";
/// The helper's exit code when the system has no web view runtime.
const NO_RUNTIME: i32 = 3;
/// What the panel says when the card could not open (an "Open in browser"
/// button goes with it; nothing opens by itself).
pub const FALLBACK: &str = "The built-in viewer isn't available on this PC.";

/// The helper could not start this session (missing runtime): the browser from now on.
static BROKEN: AtomicBool = AtomicBool::new(false);
/// Results show in the card: always, where it can open (the helper and its
/// web view runtime are there).
pub fn wanted() -> bool {
    unavailable().is_none()
}

/// Why the card can't open on this PC, or `None`. (Settings › General shows it.)
pub fn unavailable() -> Option<&'static str> {
    if !cfg!(any(windows, target_os = "linux")) {
        return Some("Not available on this system yet: results open in your browser.");
    }
    if helper().is_none() || BROKEN.load(Ordering::Relaxed) {
        return Some(if cfg!(windows) {
            "Needs Microsoft Edge WebView2, which isn't on this PC: results open in your browser."
        } else {
            "Needs libwebkit2gtk-4.1, which isn't installed: results open in your browser."
        });
    }
    None
}

/// What to run for the card: the program and its leading arguments.
fn helper() -> Option<(PathBuf, &'static [&'static str])> {
    helper_of(&std::env::current_exe().ok()?)
}

/// `exe` = the running app. Windows: itself in helper mode (when built with the
/// web view); elsewhere: the `opencircle-sheet` file beside it, if there.
fn helper_of(exe: &std::path::Path) -> Option<(PathBuf, &'static [&'static str])> {
    #[cfg(windows)]
    return cfg!(feature = "sheet").then(|| (exe.to_path_buf(), &[ARG][..]));
    #[cfg(not(windows))]
    {
        let path = exe.with_file_name(HELPER);
        path.is_file().then_some((path, &[][..]))
    }
}

/// The language Google's page should be in: the app's UI language, which is
/// the system's (`hl=`; not the region of the PC's IP address).
pub fn language() -> String {
    crate::lens::locale()
}

/// The AI Mode page for `question`, in the user's language.
pub fn ai_url(question: &str) -> String {
    format!("{}&hl={}", crate::ai::browser::url(question).0, language())
}

/// The running helper. Dropping it ends the helper (and its web view processes).
pub struct Guard(Mutex<Child>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(child) = self.0.get_mut() {
            let _ = child.kill(); // a hard exit, as the spike found it must be
            let _ = child.wait();
        }
    }
}

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("sheet::Guard")
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The helper runs: hold this while the card may show.
    Started(Arc<Guard>),
    /// The card is on screen.
    Shown,
    /// The user closed it.
    Closed,
    /// Lens: the user wants the search in the browser (Google's human check,
    /// or Open in browser before results): the panel opens Lens there.
    ToBrowser,
    /// The user took the results to the browser ("Open in browser"): the card
    /// closed; the overlay gets out of the way too.
    Left,
}

/// The card's colours: the floating palette of the app's look (`theme::floating`).
/// Call on the UI thread (the theme is per thread).
pub fn colours() -> ([u32; 5], f64) {
    let p = crate::ui::theme::floating();
    let bg = p.panel_solid;
    let hex = |c: iced::Color, under: iced::Color| {
        let ch = |x: f32, y: f32| ((x * c.a + y * (1.0 - c.a)) * 255.0).round() as u32;
        ch(c.r, under.r) << 16 | ch(c.g, under.g) << 8 | ch(c.b, under.b)
    };
    let black = iced::Color::BLACK;
    let colours = [
        hex(bg, black),
        hex(p.border, bg),
        hex(p.hover, bg),
        hex(p.text, bg),
        hex(p.muted, bg),
    ];
    (colours, p.shadow.a as f64)
}

/// Shows `page` (Lens: the HTML page; AI: the URL) in the card until the user
/// closes it, on the user's side at their size (`[results]`, updated when
/// they move or resize it). `colours` from [`colours`]. `on` gets each
/// `Event` (false = stop listening). Blocking: call off the UI thread. `Err` =
/// it never showed (the caller offers "Open in browser", never opens it by
/// itself); a missing runtime turns the card off for the session.
/// `entry`: this search for the history (kept only when the user turned it on;
/// off deletes the old one).
pub fn run(
    kind: Kind,
    mut page: Vec<u8>,
    (colours, shadow): ([u32; 5], f64),
    mut entry: Option<history::Entry>,
    avoid: Option<Avoid>,
    mut on: impl FnMut(Event) -> bool,
) -> Result<(), String> {
    let (path, lead) = helper().ok_or("no helper")?;
    let ai_url = (kind == Kind::Ai).then(|| String::from_utf8_lossy(&page).into_owned());
    let keep = history::path().filter(|_| history::enabled());
    match &keep {
        // The History page rides along after the page (proto::split_input).
        Some(p) => {
            let now = (std::time::SystemTime::now())
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            page.push(0);
            page.extend(history::page(&history::read(p), colours, now).into_bytes());
        }
        None => history::clear_default(),
    }
    let mut keep_url = |url: &str| {
        if let (Some(p), Some(mut e)) = (&keep, entry.take()) {
            e.url = url.into();
            history::record(p, true, e);
        }
    };
    let args = args(kind, (colours, shadow), avoid);
    let mut command = Command::new(path);
    command.args(lead);
    // An AppImage bundles an older GLib/GTK than the system's WebKitGTK needs
    // (seen: symbol lookup error on Ubuntu 26.04): the helper takes the
    // system's libraries first.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some()
        && let Some(dir) = system_webkit_dir()
    {
        command.env("LD_LIBRARY_PATH", dir);
    }
    let mut child = command
        .args(args.to_vec())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            BROKEN.store(true, Ordering::Relaxed);
            e.to_string()
        })?;
    let (stdin, stdout) = (child.stdin.take(), child.stdout.take());
    let guard = Arc::new(Guard(Mutex::new(child)));
    let wait = Arc::downgrade(&guard);
    if !on(Event::Started(guard)) {
        return Ok(()); // dropped: the guard ended it
    }
    // The page goes through the pipe, never a file; closing stdin = all sent.
    if let Some(mut stdin) = stdin {
        let _ = stdin.write_all(&page);
    }
    let mut shown = false;
    for line in BufReader::new(stdout.ok_or("no pipe")?).lines() {
        let Ok(line) = line else { break };
        match Line::parse(&line) {
            Some(Line::Ready) => {
                shown = true;
                if let Some(url) = &ai_url {
                    keep_url(url);
                }
                if !on(Event::Shown) {
                    return Ok(());
                }
            }
            // The user's "Open in browser": never Google's check itself, the search.
            Some(Line::Leave(url)) => {
                match leave(ai_url.as_deref(), &url) {
                    Some(page) => open(kind, page),
                    None => {
                        if !on(Event::ToBrowser) {
                            return Ok(());
                        }
                    }
                }
                if !on(Event::Left) {
                    return Ok(());
                }
            }
            Some(Line::Open(url)) => open(kind, &url),
            Some(Line::Size(size)) => remember(|r| sized(r, size)),
            Some(Line::Side(side)) => {
                let side = match side {
                    Place::Left => CardSide::Left,
                    _ => CardSide::Right,
                };
                remember(|r| r.side = side)
            }
            // Lens: the results page is what reopens the search.
            Some(Line::At(url)) if kind == Kind::Lens && lens_results(&url) => keep_url(&url),
            Some(Line::ClearHistory) => history::clear_default(),
            Some(Line::At(_)) | None => {}
        }
    }
    if shown {
        on(Event::Closed);
        return Ok(());
    }
    // It ended before showing: why? (The guard may be gone: the panel closed.)
    let code = wait.upgrade().and_then(|g| {
        let mut child = g.0.lock().ok()?;
        child.wait().ok()?.code()
    });
    // 127: the loader found no libwebkit2gtk (Linux); NO_RUNTIME: no WebView2.
    if matches!(code, Some(NO_RUNTIME | 127)) {
        BROKEN.store(true, Ordering::Relaxed);
    }
    Err(format!("the results card did not open (exit {code:?})"))
}

/// The selection a search is about, for its card to stay off it: the screen
/// (w, h) and the selection's left and right edges, logical px.
pub type Avoid = ((f64, f64), (f64, f64));

/// The helper's command line: the user's side and size (`[results]`), the
/// side flipped for this search when the card would cover the selection.
fn args(kind: Kind, (colours, shadow): ([u32; 5], f64), avoid: Option<Avoid>) -> Args {
    let results = crate::config::current().results;
    let saved = match results.side {
        CardSide::Left => Place::Left,
        CardSide::Right => Place::Right,
    };
    let size = (results.width > 0 || results.height > 0)
        .then_some((f64::from(results.width), f64::from(results.height)));
    let side = match avoid {
        Some((screen, sel)) => proto::side_for(screen, saved, size, Some(sel)),
        None => saved,
    };
    Args {
        kind,
        side,
        size,
        colours,
        shadow,
    }
}

/// What the browser gets when the user takes the card's page `url` there:
/// AI Mode's page (its question, not Google's human check); `None` for Lens =
/// post the crop to Lens again in the browser (its results page is tied to
/// the card's upload: the browser would show a broken image).
fn leave<'a>(ai_url: Option<&'a str>, url: &'a str) -> Option<&'a str> {
    match ai_url {
        Some(ai) if proto::is_captcha(url) => Some(ai),
        Some(_) => Some(url),
        None => None,
    }
}

/// `results` after the user resized the card: `None` = back to the default
/// size; a 0 in `size` = that dimension wasn't the one resized.
fn sized(results: &mut crate::config::ResultsConfig, size: Option<(f64, f64)>) {
    let px = |v: f64| v.round().clamp(200.0, 10_000.0) as u32;
    match size {
        None => (results.width, results.height) = (0, 0),
        Some((w, h)) => {
            if w > 0.0 {
                results.width = px(w);
            }
            if h > 0.0 {
                results.height = px(h);
            }
        }
    }
}

/// Changes the card's `[results]` and saves the config when they changed.
// ponytail: writes the whole config from this thread; a Settings page open
// at the same time saves its own copy over it (the card's place reverts).
fn remember(change: impl FnOnce(&mut crate::config::ResultsConfig)) {
    let mut config = crate::config::current();
    let before = config.results;
    change(&mut config.results);
    if config.results != before {
        if let Err(e) = config.save() {
            eprintln!("[sheet] could not keep the card's place: {e}");
        }
        crate::config::set_current(config);
    }
}

/// The system folder that holds libwebkit2gtk-4.1 (Debian/Ubuntu multiarch,
/// Fedora/openSUSE lib64, Arch lib), if any.
#[cfg(target_os = "linux")]
fn system_webkit_dir() -> Option<String> {
    let arch = std::env::consts::ARCH;
    [
        format!("/usr/lib/{arch}-linux-gnu"),
        "/usr/lib64".into(),
        "/usr/lib".into(),
    ]
    .into_iter()
    .find(|d| {
        std::path::Path::new(d)
            .join("libwebkit2gtk-4.1.so.0")
            .exists()
    })
}

/// A Lens results page (what the history reopens), as one plain https link.
fn lens_results(url: &str) -> bool {
    url.starts_with("https://lens.google.com/search?")
        && crate::lens::web_host(url).as_deref() == Some("lens.google.com")
}

/// A link the card hands to the user's browser: only a plain http(s) link
/// (`lens::open_link`), never a file or another app's scheme.
fn open(kind: Kind, url: &str) {
    if let Err(e) = crate::lens::open_link(url) {
        eprintln!("[sheet] {}: not opened ({e})", kind.arg());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::widgets::card;
    use iced::{Point, Rectangle, Size};

    #[test]
    fn helper_card_is_the_shared_cards_place() {
        assert_eq!(proto::GAP as f32, card::GAP);
        assert_eq!(proto::SHARE as f32, card::SHARE);
        assert_eq!(proto::WIDTH as f32, card::WIDTH);
        assert_eq!(
            (proto::MIN.0 as f32, proto::MIN.1 as f32),
            (card::MIN.width, card::MIN.height)
        );
        assert_eq!(proto::NARROW as f32, card::NARROW);
        for (w, h) in [
            (1920.0, 1080.0),
            (1280.0, 800.0),
            (1080.0, 1920.0),
            (800.0, 600.0),
        ] {
            // The shared card goes away from a selection: put one on the other side.
            for (side, sel_x) in [(Place::Right, 0.2), (Place::Left, 0.8)] {
                let screen = Size::new(w as f32, h as f32);
                let sel = Rectangle::new(
                    Point::new((sel_x * w) as f32 - 1.0, 0.0),
                    Size::new(2.0, 2.0),
                );
                let shared = card::place(screen, Some(sel), None);
                let [x, y, cw, ch] = proto::place((w, h), side, None).1;
                assert_eq!(
                    [x as f32, y as f32, cw as f32, ch as f32],
                    [shared.x, shared.y, shared.width, shared.height],
                    "{w}x{h} {side:?}"
                );
            }
        }
    }

    #[test]
    fn a_card_over_the_selection_opens_on_the_other_side_the_saved_side_stays() {
        let saved = || crate::config::current().results.side;
        assert_eq!(saved(), CardSide::Right, "the default");
        let look = ([0; 5], 0.3);
        let screen = (1716.0, 1271.0);
        let side = |sel| args(Kind::Lens, look, Some((screen, sel))).side;
        assert_eq!(
            side((1015.0, 1235.0)),
            Place::Left,
            "the right-hand picture"
        );
        assert_eq!(side((470.0, 1000.0)), Place::Right, "middle-left text");
        assert_eq!(args(Kind::Ai, look, None).side, Place::Right);
        assert_eq!(saved(), CardSide::Right, "unchanged");
    }

    #[test]
    fn open_in_browser_takes_the_search_not_the_cards_session() {
        let ai = "https://www.google.com/search?udm=50&q=x";
        let lens = "https://www.google.com/search?vsrid=abc&lns_mode=un";
        let check = "https://www.google.com/sorry/index?continue=x";
        assert_eq!(leave(None, lens), None, "Lens: the crop again");
        assert_eq!(leave(None, check), None);
        let followed = "https://www.google.com/search?udm=50&q=y";
        assert_eq!(leave(Some(ai), followed), Some(followed));
        assert_eq!(leave(Some(ai), check), Some(ai), "never the check itself");
    }

    #[test]
    fn the_cards_size_is_kept_per_dimension() {
        let mut r = crate::config::ResultsConfig::default();
        sized(&mut r, Some((640.0, 0.0))); // a side card's width
        assert_eq!((r.width, r.height), (640, 0));
        sized(&mut r, Some((0.0, 333.4))); // a bottom card's height
        assert_eq!((r.width, r.height), (640, 333));
        sized(&mut r, Some((50.0, 0.0)));
        assert_eq!(r.width, 200, "within bounds");
        sized(&mut r, None); // double-click: the default size
        assert_eq!((r.width, r.height), (0, 0));
    }

    #[test]
    fn ai_url_is_ai_mode_in_the_users_language() {
        let url = ai_url("What is this?");
        assert!(url.starts_with(proto::AI_PREFIX));
        assert!(url.starts_with("https://www.google.com/search?udm=50&q=What%20is%20this%3F&hl="));
        let hl = url.rsplit_once("&hl=").unwrap().1;
        assert_eq!(hl, language());
        assert!(!hl.is_empty() && hl.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
        // still one plain link (what the helper and the browser opener accept)
        assert_eq!(
            crate::lens::web_host(&url).as_deref(),
            Some("www.google.com")
        );
        // the user's text can't add parameters
        assert!(!ai_url("x&hl=fr&udm=1").contains("&udm=1"));
    }

    #[test]
    fn the_helper_is_this_exe_on_windows_and_a_sibling_file_elsewhere() {
        let exe = std::path::Path::new("/nowhere/opencircle");
        #[cfg(windows)]
        {
            let want = cfg!(feature = "sheet").then(|| (exe.to_path_buf(), &[ARG][..]));
            assert_eq!(helper_of(exe), want);
        }
        #[cfg(not(windows))]
        {
            assert_eq!(helper_of(exe), None); // no such file beside it
            let dir = std::env::temp_dir().join(format!("oc-sheet-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("opencircle-sheet"), b"").unwrap();
            let got = helper_of(&dir.join("opencircle"));
            std::fs::remove_dir_all(&dir).unwrap();
            let (path, lead) = got.unwrap();
            assert!(path.ends_with("opencircle-sheet") && lead.is_empty());
        }
    }

    #[test]
    fn no_helper_no_card_and_nothing_opens_by_itself() {
        // Google's human check, and what the browser gets instead.
        assert!(proto::is_captcha(
            "https://www.google.com/sorry/index?continue=x"
        ));
        assert!(proto::is_captcha("https://ipv4.google.com/sorry/index?q=1"));
        assert!(!proto::is_captcha("https://www.google.com/search?q=sorry/"));
        assert!(!proto::is_captcha("https://evil.test/sorry/x?google.com"));
        assert!(lens_results("https://lens.google.com/search?ep=gsbubb&p=x"));
        assert!(!lens_results("https://lens.google.com/v3/upload?x"));
        assert!(!lens_results(
            "https://www.google.com/sorry/index?continue=x"
        ));
        // No helper next to the test binary: no card, the reason, and an
        // error for the panel (which offers "Open in browser"; `run` opens nothing).
        #[cfg(not(all(windows, feature = "sheet")))]
        assert!(helper().is_none());
        #[cfg(not(all(windows, feature = "sheet")))]
        assert!(!wanted() && unavailable().is_some());
        let mut events = 0;
        let r = run(Kind::Ai, Vec::new(), ([0; 5], 0.0), None, None, |_| {
            events += 1;
            true
        });
        assert!(r.is_err() && events == 0);
    }
}
