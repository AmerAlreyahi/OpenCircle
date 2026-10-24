//! The Google results card (owner, 2026-10-24): Google's own Lens / AI Mode
//! page in the system web view, as a floating card beside the selection.
//!
//! The web view never runs in the resident app: it lives in the short-lived
//! helper `opencircle-sheet` next to this exe (`src/bin/opencircle-sheet`, the
//! only binary that links wry / WebView2 / WebKitGTK), so the app's idle memory
//! is unchanged and everything of the card goes when the helper exits (on ×,
//! Esc, an outside click, or when the overlay drops the panel: `Guard`).
//! Windows and Linux: Lens and AI Mode. On Linux Google often answers
//! WebKitGTK's Lens with its "unusual traffic" check (spike S9): the card shows
//! a hint, the user solves it there or opens the browser (we never touch it);
//! `results.lens_linux = "browser"` keeps Lens in the browser.
//! No helper, no web view runtime (WebView2, libwebkit2gtk-4.1) or the user's
//! choice (Settings › General): the browser, as before.
pub mod history;
#[allow(dead_code)] // the helper uses the rest
#[path = "bin/opencircle-sheet/proto.rs"]
mod proto;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::{GoogleResults, LensLinux};
pub use proto::Kind;
use proto::{Args, Line};

/// The helper's file name, next to the app's exe.
const HELPER: &str = if cfg!(windows) {
    "opencircle-sheet.exe"
} else {
    "opencircle-sheet"
};
/// The helper's exit code when the system has no web view runtime.
const NO_RUNTIME: i32 = 3;
/// What the panel says when the card could not open and the browser did instead.
pub const FALLBACK: &str =
    "The built-in viewer isn't available on this PC, so Google opened in your browser.";

/// The helper could not start this session (missing runtime): the browser from now on.
static BROKEN: AtomicBool = AtomicBool::new(false);
/// The size the user last resized the card to, this session (never the position).
static SIZE: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// Show `kind`'s results in the card (else in the browser).
pub fn wanted(kind: Kind) -> bool {
    let cfg = crate::config::current();
    let lens_here = !cfg!(target_os = "linux") || cfg.results.lens_linux == LensLinux::InCard;
    choose(
        cfg.general.google_results,
        kind,
        unavailable().is_none(),
        lens_here,
    )
}

/// The card only when the user wants it, the platform can show `kind`, and
/// the helper and its web view runtime are there. `lens_here`: Lens may open
/// in the card (Linux: `results.lens_linux`; Google may ask the user there to
/// prove they're human, and only the user ever answers it).
fn choose(choice: GoogleResults, kind: Kind, available: bool, lens_here: bool) -> bool {
    let platform = cfg!(any(windows, target_os = "linux")); // macOS: no helper yet
    choice == GoogleResults::App && platform && available && (kind == Kind::Ai || lens_here)
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

fn helper() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.with_file_name(HELPER);
    path.is_file().then_some(path)
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
/// closes it. `near`: the selection's centre (share of the screen width);
/// `colours` from [`colours`]. `on` gets each `Event` (false = stop listening).
/// Blocking: call off the UI thread. `Err` = it never showed (the caller opens
/// the browser instead); a missing runtime turns the card off for the session.
/// `entry`: this search for the history (kept only when the user turned it on;
/// off deletes the old one).
pub fn run(
    kind: Kind,
    mut page: Vec<u8>,
    near: Option<f32>,
    (colours, shadow): ([u32; 5], f64),
    mut entry: Option<history::Entry>,
    mut on: impl FnMut(Event) -> bool,
) -> Result<(), String> {
    let path = helper().ok_or("no helper")?;
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
    let args = Args {
        kind,
        near: near.map(f64::from),
        size: *SIZE.lock().unwrap_or_else(|e| e.into_inner()),
        colours,
        shadow,
    };
    let mut command = Command::new(path);
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
            // Google's human check: never the check itself to the browser, the search.
            Some(Line::Open(url)) if proto::is_captcha(&url) => match &ai_url {
                Some(ai) => open(kind, ai),
                None => {
                    if !on(Event::ToBrowser) {
                        return Ok(());
                    }
                }
            },
            Some(Line::Open(url)) => open(kind, &url),
            Some(Line::Size(size)) => *SIZE.lock().unwrap_or_else(|e| e.into_inner()) = size,
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
            for near in [None, Some(0.2), Some(0.8)] {
                let screen = Size::new(w as f32, h as f32);
                let sel = near.map(|x: f64| {
                    Rectangle::new(Point::new((x * w) as f32 - 1.0, 0.0), Size::new(2.0, 2.0))
                });
                let shared = card::place(screen, sel, None);
                let [x, y, cw, ch] = proto::place((w, h), near, None).1;
                assert_eq!(
                    [x as f32, y as f32, cw as f32, ch as f32],
                    [shared.x, shared.y, shared.width, shared.height],
                    "{w}x{h} near {near:?}"
                );
            }
        }
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
    fn the_browser_is_the_fallback() {
        let app = crate::config::Config::default().general.google_results;
        assert_eq!(app, GoogleResults::App, "the card is the default");
        assert!(choose(app, Kind::Ai, true, true) && choose(app, Kind::Lens, true, true));
        // The user's choice, a missing helper or runtime: the browser.
        assert!(!choose(GoogleResults::Browser, Kind::Ai, true, true));
        assert!(!choose(app, Kind::Ai, false, true));
        assert!(!choose(app, Kind::Lens, false, true));
        // Linux, results.lens_linux = browser: Lens in the browser, AI Mode still here.
        assert!(!choose(app, Kind::Lens, true, false));
        assert!(choose(app, Kind::Ai, true, false));
        assert_eq!(
            crate::config::Config::default().results.lens_linux,
            LensLinux::InCard
        );
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
        // No helper next to the test binary: the browser, and the reason.
        assert!(helper().is_none());
        assert!(!wanted(Kind::Ai) && unavailable().is_some());
        let r = run(Kind::Ai, Vec::new(), None, ([0; 5], 0.0), None, |_| true);
        assert!(r.is_err());
    }
}
