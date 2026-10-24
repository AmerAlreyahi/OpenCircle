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
use wry::{NewWindowResponse, PageLoadEvent, PermissionResponse, WebContext, WebViewBuilder};

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
    let page = match args.kind {
        Kind::Lens => Page::Html(input),
        Kind::Ai => {
            let url = input.trim();
            // Only Google AI Mode, one plain link (the app built it).
            if !url.starts_with(proto::AI_PREFIX) || url.contains(char::is_whitespace) {
                eprintln!("[sheet] not an AI Mode link");
                return ExitCode::from(2);
            }
            Page::Url(url.into())
        }
    };
    let code = run(&args, page, t0);
    log(t0, "exit", "");
    hard_exit(code)
}

#[cfg(target_os = "linux")]
fn run(args: &Args, page: Page, t0: Instant) -> u8 {
    linux::run(args, page, t0)
}

#[cfg(windows)]
fn run(args: &Args, page: Page, t0: Instant) -> u8 {
    win::run(args, page, t0)
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

/// The web view with every limit of the card (module docs); `close` runs on
/// the page's one message.
pub fn builder<'a>(
    ctx: &'a mut WebContext,
    page: Page,
    t0: Instant,
    close: impl Fn() + 'static,
) -> WebViewBuilder<'a> {
    let own = page_url();
    let b = WebViewBuilder::new_with_web_context(ctx)
        .with_devtools(false)
        .with_initialization_script(ESC_JS)
        .with_ipc_handler(move |req: Request<String>| {
            if req.body() == proto::CLOSE {
                close();
            }
        })
        .with_navigation_handler(move |url| {
            let ok = proto::allowed(&url, &own);
            log(t0, if ok { "nav" } else { "nav BLOCKED" }, &url);
            ok
        })
        .with_new_window_req_handler(|url, _| {
            say(Line::Open(url)); // the app opens plain http(s) links only
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|_, _| false)
        .with_permission_handler(|_| PermissionResponse::Deny)
        .with_on_page_load_handler(move |e, url| {
            if let PageLoadEvent::Finished = e {
                log(t0, "loaded", &url);
            }
        });
    match page {
        Page::Url(url) => b.with_url(url),
        Page::Html(html) => {
            let html: Cow<'static, [u8]> = html.into_bytes().into();
            b.with_custom_protocol(SCHEME.into(), move |_, req| {
                let (status, body) = if req.uri().path() == "/" {
                    (200, html.clone())
                } else {
                    (404, Cow::Borrowed(&b""[..]))
                };
                Response::builder()
                    .status(status)
                    .header(CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(body)
                    .unwrap_or_default()
            })
            .with_url(page_url())
        }
    }
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
