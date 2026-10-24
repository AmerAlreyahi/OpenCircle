//! `opencircle-sheet`: the Google results card's helper process. Shows Google's
//! own Lens results / AI Mode page in the system web view (WebView2 / WebKitGTK)
//! as the app's floating card, until the user closes it; then the process,
//! the web view's browser processes and all their memory are gone. The app
//! (`src/sheet.rs`) starts it; `proto.rs` is the contract between the two.
//! The only binary that links the web view: the resident app stays light.
//!
//! Look: `ui::widgets::card` (the Ask AI card): 20 px from the screen edges,
//! 20 px corners, thin border, soft shadow (Linux; Windows 10 clips it away);
//! header = icon, "Google · Lens" / "Google · AI Mode", the "google.com" chip,
//! Open in browser, ×; the page in the body; footer = the privacy line. On the
//! side away from the selection, full height (portrait or narrow: at the
//! bottom). The inner edge resizes it (its grip shows on hover; double-click =
//! default size); it never moves. Esc, ×, or a click outside closes it.
//!
//! Isolation: only https and our in-memory page load (no file://, no
//! downloads, every permission denied), new windows go to the user's browser
//! (through the app), the page's only message is "close", logs drop query
//! strings. Its own profile (`<data dir>/OpenCircle/webview`), never the
//! user's browser profile.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod linux;
#[allow(dead_code)] // the app's half (and per-platform parts)
mod proto;
#[cfg(windows)]
mod win;

use std::borrow::Cow;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use proto::{Args, Kind, Line, Place};
use wry::http::{Request, Response, header::CONTENT_TYPE};
use wry::{PageLoadEvent, PermissionResponse, WebContext, WebViewBuilder};

/// Links the card hands to the browser this session (`proto::Limit`).
pub type Links = std::rc::Rc<std::cell::RefCell<proto::Limit>>;

/// Decides a navigation (`new_window`: the page asked for a window) the web
/// view is about to make; `gesture`: the user's own click or key started it.
/// True = load it in the card. Google's pages load here; anything else goes to
/// the user's browser only on their click (rate-limited), else nowhere.
pub fn route(t0: Instant, url: &str, new_window: bool, gesture: bool, links: &Links) -> bool {
    let to = proto::go(
        url,
        &page_url(),
        new_window,
        gesture,
        &mut links.borrow_mut(),
        Instant::now(),
    );
    let what = match to {
        proto::Go::Here => "nav",
        proto::Go::Browser => "to the browser",
        proto::Go::Drop => "blocked",
    };
    log(t0, what, url);
    if to == proto::Go::Browser {
        say(Line::Open(url.into())); // the app opens plain http(s) links only
    }
    to == proto::Go::Here
}

/// Exit code: no web view runtime on this system (the app turns the card off).
const NO_RUNTIME: u8 = 3;
const SCHEME: &str = "ocsheet";
const SLIDE: Duration = Duration::from_millis(200);
/// How far the card travels while sliding in (logical px).
const SLIDE_BY: f64 = 48.0;
const CHIP: &str = "google.com";
/// Esc inside the page (the web view has the keyboard) closes the card.
const ESC_JS: &str =
    "addEventListener('keydown',e=>{if(e.key==='Escape')window.ipc.postMessage('close')},true)";

/// What the card opens.
pub enum Page {
    /// The auto-submit Lens page, served from memory at `page_url()`.
    Html(String),
    Url(String),
}

fn main() -> ExitCode {
    let t0 = Instant::now();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[sheet] {e}\nstarted by OpenCircle only");
            return ExitCode::from(2);
        }
    };
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("[sheet] no page: {e}");
        return ExitCode::from(2);
    }
    let (first, history) = proto::split_input(&input);
    let history = history.map(str::to_string);
    let page = match args.kind {
        Kind::Lens => Page::Html(first.into()),
        Kind::Ai => {
            let url = first.trim();
            // Only Google AI Mode, one plain link (the app built it).
            if !url.starts_with(proto::AI_PREFIX) || url.contains(char::is_whitespace) {
                eprintln!("[sheet] not an AI Mode link");
                return ExitCode::from(2);
            }
            Page::Url(url.into())
        }
    };
    let code = run(&args, page, history, t0);
    log(t0, "exit", "");
    hard_exit(code)
}

#[cfg(target_os = "linux")]
fn run(args: &Args, page: Page, history: Option<String>, t0: Instant) -> u8 {
    linux::run(args, page, history, t0)
}

#[cfg(windows)]
fn run(args: &Args, page: Page, history: Option<String>, t0: Instant) -> u8 {
    win::run(args, page, history, t0)
}

/// Ends the process outright: a normal exit could hang in WebView2's teardown
/// (seen after a Lens run, spike S9); the OS frees everything and the web
/// view's processes follow their host.
fn hard_exit(code: u8) -> ExitCode {
    let _ = std::io::stdout().flush();
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        // SAFETY: ends this process; nothing runs after it.
        let _ = unsafe { TerminateProcess(GetCurrentProcess(), code as u32) };
    }
    std::process::exit(code.into())
}

/// A line for the app (stdout).
pub fn say(line: Line) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", line.text());
    let _ = out.flush();
}

/// A log line (stderr; never the query: it can hold the user's text).
pub fn log(t0: Instant, what: &str, url: &str) {
    eprintln!(
        "[sheet] {:>6} ms {what} {}",
        t0.elapsed().as_millis(),
        proto::short(url)
    );
}

/// The web view's own profile, never the user's browser's.
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenCircle")
        .join("webview")
}

/// Where our in-memory page lives (wry maps custom schemes to http on Windows).
pub fn page_url() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/")
    } else {
        format!("{SCHEME}://localhost/")
    }
}

/// Our History page's address.
pub fn history_url() -> String {
    format!("{}{}", page_url(), proto::HISTORY)
}

/// The web view with every limit of the card (module docs); `close` runs on
/// the page's "close". `history`: the History page (the user keeps one); its
/// Clear is the only other message, and only from that page. `captcha` hears
/// whether each page is Google's human check.
pub fn builder<'a>(
    ctx: &'a mut WebContext,
    page: Page,
    history: Option<String>,
    t0: Instant,
    close: impl Fn() + 'static,
    captcha: impl Fn(bool) + 'static,
) -> WebViewBuilder<'a> {
    let own_ipc = page_url();
    let b = WebViewBuilder::new_with_web_context(ctx)
        .with_devtools(false)
        .with_initialization_script(ESC_JS)
        .with_ipc_handler(move |req: Request<String>| {
            let from_history = proto::is_history(&req.uri().to_string(), &own_ipc);
            match req.body().as_str() {
                proto::CLOSE => close(),
                proto::CLEAR if from_history => say(Line::ClearHistory),
                _ => {}
            }
        })
        // Navigations and new windows: `route`, hooked on the platform's web
        // view after it is built (wry's handlers can't tell a click from a script).
        .with_download_started_handler(|_, _| false)
        .with_permission_handler(|_| PermissionResponse::Deny)
        .with_on_page_load_handler(move |e, url| {
            // Google's human check shows a hint (the user answers it, never us).
            captcha(proto::is_captcha(&url));
            if let PageLoadEvent::Finished = e {
                log(t0, "loaded", &url);
                if url.starts_with("https://") {
                    say(Line::At(url)); // the history keeps Lens's results page
                }
            }
        });
    // Our pages, from memory: "/" (the Lens page), "/history".
    let (first, url) = match page {
        Page::Url(url) => (None, url),
        Page::Html(html) => (Some(html), page_url()),
    };
    let first: Option<Cow<'static, [u8]>> = first.map(|h| h.into_bytes().into());
    let history: Option<Cow<'static, [u8]>> = history.map(|h| h.into_bytes().into());
    let history_path = format!("/{}", proto::HISTORY);
    b.with_custom_protocol(SCHEME.into(), move |_, req| {
        let body = match req.uri().path() {
            "/" => first.clone(),
            p if p == history_path => history.clone(),
            _ => None,
        };
        let status = if body.is_some() { 200 } else { 404 };
        Response::builder()
            .status(status)
            .header(CONTENT_TYPE, "text/html; charset=utf-8")
            .body(body.unwrap_or(Cow::Borrowed(&b""[..])))
            .unwrap_or_default()
    })
    .with_url(url)
}

/// Ease-out share of the slide done at `t` since it began.
pub fn slid(t: Duration) -> f64 {
    let p = (t.as_secs_f64() / SLIDE.as_secs_f64()).min(1.0);
    1.0 - (1.0 - p).powi(3)
}

/// Offset (dx, dy) of the card `share` (0..1) into its slide.
pub fn slide_off(p: Place, share: f64) -> (f64, f64) {
    let d = SLIDE_BY * (1.0 - share);
    match p {
        Place::Right => (d, 0.0),
        Place::Left => (-d, 0.0),
        Place::Bottom => (0.0, d),
    }
}

/// The header's icon: a plain glyph (the app's icon font isn't here).
pub fn icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Lens => "⌕",
        Kind::Ai => "✦",
    }
}
