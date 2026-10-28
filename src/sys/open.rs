//! Opening things for the user: a web link or address in the browser, the Lens
//! hand-off page, a mail window. One launcher for every caller, no shell: the
//! target is always one argv entry. Linux routes through the desktop portal.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

fn bad(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, e.to_string())
}

/// The only web address we open by URL: Google, over https.
const GOOGLE: &str = "https://www.google.com/";

/// Open `path` in the default browser. No shell, no URL: the path is one argv
/// entry and must be absolute, so it can never read as an option.
pub fn open_file(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(bad("page path must be absolute"));
    }
    spawn(path.as_os_str())
}

/// Open a `https://www.google.com/...` URL in the default browser (Google AI
/// Mode). Same launcher as the Lens page: no shell, the URL is one argv entry.
/// Any other host, scheme or a URL that is not one plain link is refused.
pub fn open_google(url: &str) -> io::Result<()> {
    if !url.starts_with(GOOGLE) || web_host(url).as_deref() != Some("www.google.com") {
        return Err(bad("only https://www.google.com/ URLs can be opened"));
    }
    spawn(std::ffi::OsStr::new(url))
}

/// The host of `url` (lower case, for the chip to show) if `url` is one plain
/// http(s) link, else `None`. The one URL check every opener and chip uses.
///
/// Strict on purpose, so a browser can't read another host than the one we show:
/// scheme http or https (any case), then only `host[:port]`: no userinfo ('@'),
/// '%', '\\' or quotes; the host is LDH labels, IPv4 or a bracketed IPv6 literal.
/// The path, query and fragment hold only RFC 3986 unreserved/reserved bytes and
/// `%XX` escapes (so no spaces, controls, '\\', '"' or non-ASCII anywhere).
pub fn web_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")) {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let tail_ok = tail.bytes().enumerate().all(|(i, b)| match b {
        b'%' => tail
            .get(i + 1..i + 3)
            .is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit())),
        _ => crate::actions::entities::url_byte(b),
    });
    tail_ok.then(|| crate::actions::entities::host_name(authority))?
}

/// Open a link the user clicked (link chip, QR chip) in the default
/// browser. Only http(s): never a file, a script or another app's scheme.
pub fn open_link(url: &str) -> io::Result<()> {
    if web_host(url).is_none() {
        return Err(bad("only http(s) links can be opened"));
    }
    spawn(std::ffi::OsStr::new(url))
}

/// Open a new message to `address` (the email chip) in the default
/// mail app. `address` must be a plain address: no `?subject=`, `&`/`=` headers,
/// `%` escapes that could smuggle them in, or anything but printable ASCII.
pub fn open_mailto(address: &str) -> io::Result<()> {
    let plain = address
        .bytes()
        .all(|b| b.is_ascii_graphic() && !b"%?&=#".contains(&b));
    if !plain || !crate::actions::entities::is_email(address) {
        return Err(bad("not an email address"));
    }
    spawn(std::ffi::OsStr::new(&format!("mailto:{address}")))
}

fn spawn(target: &std::ffi::OsStr) -> io::Result<()> {
    #[cfg(windows)]
    hand_off_foreground();
    // Debug builds only: tests and manual runs point this at a script that
    // gets the target as its one argument, so no real browser opens.
    #[cfg(debug_assertions)]
    let test_browser = std::env::var_os("OPENCIRCLE_TEST_BROWSER");
    #[cfg(not(debug_assertions))]
    let test_browser: Option<std::ffi::OsString> = None;
    if let Some(program) = test_browser {
        return reap(Command::new(program).arg(target));
    }
    #[cfg(target_os = "linux")]
    if open_on_linux(target) {
        return Ok(());
    }
    reap(&mut launcher(target))
}

/// How Linux hands `target` to the desktop.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// A web link or `mailto:`: the desktop portal (OpenURI), which finds the
    /// browser itself. `xdg-open` fetches the page and opens its source in a text
    /// editor when no https handler is set (Ubuntu 26.04 out of the box).
    Portal,
    /// A local file (the Lens hand-off page): the browser, by name. The file's
    /// own default (text/html) can be a text editor.
    Browser,
    /// Anything else: the plain launcher.
    Command,
}

#[cfg(any(target_os = "linux", test))]
fn route(target: &str) -> Route {
    if target.starts_with('/') {
        Route::Browser
    } else if web_host(target).is_some() || target.starts_with("mailto:") {
        Route::Portal
    } else {
        Route::Command
    }
}

/// Opens `target` by its [`Route`]; `false` = fall back to `xdg-open` (logged).
#[cfg(target_os = "linux")]
fn open_on_linux(target: &std::ffi::OsStr) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let opened = match route(target) {
        Route::Portal => open_uri(target),
        Route::Browser => open_in_browser(target),
        Route::Command => return false,
    };
    if let Err(e) = &opened {
        crate::log!("[opencircle] cannot open a link through the desktop ({e}); trying xdg-open");
    }
    opened.is_ok()
}

#[cfg(target_os = "linux")]
fn open_uri(uri: &str) -> Result<(), String> {
    use crate::capture::portal;
    use gtk::gio::{Cancellable, DBusCallFlags};
    use gtk::glib::{ToVariant, VariantDict};

    let args = ("", uri, VariantDict::new(None)).to_variant();
    portal::session_bus()?
        .call_sync(
            Some(portal::DESKTOP),
            portal::PATH,
            "org.freedesktop.portal.OpenURI",
            "OpenURI",
            Some(&args),
            None,
            DBusCallFlags::NONE,
            5000,
            None::<&Cancellable>,
        )
        .map(drop)
        .map_err(|e| e.to_string())
}

/// The user's https handler (else any registered browser) opens the local file.
#[cfg(target_os = "linux")]
fn open_in_browser(path: &str) -> Result<(), String> {
    use gtk::gio::{self, prelude::AppInfoExt};

    let uri = gtk::glib::filename_to_uri(path, None).map_err(|e| e.to_string())?;
    let browser = gio::AppInfo::default_for_uri_scheme("https")
        .or_else(|| {
            gio::AppInfo::all_for_type("x-scheme-handler/https")
                .into_iter()
                .next()
        })
        .ok_or("no browser is registered for https")?;
    browser
        .launch_uris(&[uri.as_str()], None::<&gio::AppLaunchContext>)
        .map_err(|e| e.to_string())
}

/// Windows keeps a new window behind the always-on-top overlay, and only lets a
/// process take the foreground if the foreground process allows it. Before the
/// browser starts: allow any process to take the foreground, and drop our own
/// full-screen overlay out of the topmost band (the next `show` re-asserts it).
/// The overlay then closes (`Ui::close`), so nothing of ours stays on screen.
/// The live layer (click-through) and small pins are left alone.
#[cfg(windows)]
fn hand_off_foreground() {
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        ASFW_ANY, AllowSetForegroundWindow, EnumWindows, GWL_EXSTYLE, GetSystemMetrics,
        GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, HWND_NOTOPMOST, IsWindowVisible,
        SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, WS_EX_TOPMOST,
        WS_EX_TRANSPARENT,
    };
    use windows::core::BOOL;

    unsafe extern "system" fn each(hwnd: HWND, _: LPARAM) -> BOOL {
        // SAFETY: plain queries on a window handle EnumWindows just gave us.
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let mut r = RECT::default();
            let big = GetWindowRect(hwnd, &mut r).is_ok()
                && (r.right - r.left) * 10 >= GetSystemMetrics(SM_CXSCREEN) * 9;
            if pid == std::process::id()
                && IsWindowVisible(hwnd).as_bool()
                && ex & WS_EX_TOPMOST.0 != 0
                && ex & WS_EX_TRANSPARENT.0 == 0
                && big
            {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_NOTOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
        BOOL(1)
    }

    // SAFETY: plain Win32 calls; `each` keeps no state.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let _ = EnumWindows(Some(each), LPARAM(0));
    }
}

/// A Windows Settings page (`ms-settings:...`), through the same launcher as links.
#[cfg(windows)]
pub fn open_system_settings(uri: &str) -> io::Result<()> {
    reap(&mut launcher(std::ffi::OsStr::new(uri)))
}

/// The OS command that opens `target` in the default app. `target` is its one
/// argument: no shell, so nothing in it can be read as an option or a command.
fn launcher(target: &std::ffi::OsStr) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(target);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("xdg-open");
        c.arg(target);
        c
    }
}

fn reap(cmd: &mut Command) -> io::Result<()> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap it without blocking: xdg-open can stay alive while the browser runs.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_file_rejects_relative_path() {
        assert!(open_file(Path::new("-x.html")).is_err());
    }

    #[test]
    fn open_link_takes_only_plain_http_urls() {
        for (ok, host) in [
            ("https://example.com", "example.com"),
            ("http://example.com/a?b=c&d=e#f", "example.com"),
            ("https://192.168.0.1:8080/x", "192.168.0.1"),
            ("HTTPS://Example.COM/Path", "example.com"),
            ("https://[::1]:8443/x", "[::1]"),
            ("https://medium.com/@user/post", "medium.com"),
            ("https://a.io/x%20y?q=%E2%82%AC#top", "a.io"),
            (
                "https://en.wikipedia.org/wiki/Rust_(language)",
                "en.wikipedia.org",
            ),
            ("https://example.com:65535", "example.com"),
        ] {
            assert_eq!(web_host(ok).as_deref(), Some(host), "{ok:?}");
        }
        for bad in [
            "ftp://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "mailto:a@b.com",
            "data:text/html,x",
            "https://",
            "https:// example.com",
            "https://exa mple.com",
            "https://example.com/a\nb",
            "https://example.com/\u{645}",
            "-x",
            "--help",
            "https:///etc",
            "",
            // userinfo, and the ways browsers and we could disagree on the host
            "https://google.com@evil.com/",
            "https://good.com\\@evil.com",
            "https://good.com%2f@evil.com",
            "https://good.com:443@evil.com",
            "https://@evil.com",
            "https://evil.com\\.good.com/",
            "https://\"good.com\"/",
            "https://good.com'/",
            "https://good.com/\"onmouseover",
            "https://good.com/a\\b",
            "https://good.com/a%2",
            "https://good.com/a%zz",
            "https://good.com/a<b>",
            "https://good.com/a`b",
            "https://ex%61mple.com",
            "https://example.com\u{202e}moc.live",
            "https://example.com:",
            "https://example.com:65536",
            "https://example.com:8a",
            "https://01.2.3.4/",
            "https://999.1.1.1/",
            "https://0x7f.0.0.1/",
            "https://[::1%25eth0]/",
            "https://[not-ip]/",
            "https://localhost/",
            "https://-bad-.com/",
        ] {
            assert_eq!(web_host(bad), None, "{bad:?}");
            assert!(open_link(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn mailto_takes_only_a_plain_address() {
        for bad in [
            "a@b.com?cc=c@d.com",
            "a b@c.com",
            "-oProxyCommand=x",
            "a@b",
            "",
            // percent escapes and query bytes that would add headers
            "a%3Fcc%3Dx@y.com",
            "a%40b@c.com",
            "x%26body%3Dhi@y.com",
            "a&b@c.com",
            "a=b@c.com",
            "a#b@c.com",
            "a@b.com&cc=x@y.com",
            "\u{645}@b.com",
            "a@b.com\n",
        ] {
            assert!(open_mailto(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_launcher_gets_the_target_as_its_one_argument() {
        // No shell: a hostile-looking link stays one argv entry.
        let target = "https://example.com/a b;rm -rf x&y|z$(id)";
        let cmd = launcher(std::ffi::OsStr::new(target));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args.last().copied(), Some(std::ffi::OsStr::new(target)));
        #[cfg(not(windows))]
        {
            assert_eq!(cmd.get_program(), "xdg-open");
            assert_eq!(args.len(), 1);
        }
        #[cfg(windows)]
        assert_eq!(args.len(), 2); // "url.dll,FileProtocolHandler" + target
    }

    #[test]
    fn links_go_to_the_portal_and_the_hand_off_page_to_a_browser() {
        // A web link or mailto: never goes through xdg-open on Linux (no https
        // handler = the page source in a text editor); the local page opens in a browser.
        for link in [
            "https://github.com/AmerAlreyahi/OpenCircle",
            "HTTP://example.com/a?b=c",
            "mailto:me@example.com",
        ] {
            assert_eq!(route(link), Route::Portal, "{link}");
        }
        assert_eq!(route("/tmp/opencircle-lens-1-2.html"), Route::Browser);
        for other in ["file:///etc/passwd", "-x", "ftp://example.com/", ""] {
            assert_eq!(route(other), Route::Command, "{other}");
        }
    }

    #[test]
    fn open_url_takes_only_google_https() {
        for bad in [
            "http://www.google.com/search?q=a",
            "https://www.google.com.evil.test/search?q=a",
            "https://evil.test/https://www.google.com/",
            "https://www.google.com/search?q=a b",
            "https://www.google.com/search?q=a\nb",
            "https://www.google.com/search?q=a\"b",
            "https://www.google.com/search?q=a\\b",
            "https://www.google.com/search?q=%zz",
            "file:///etc/passwd",
            "-x",
            "",
        ] {
            assert!(open_google(bad).is_err(), "{bad:?}");
        }
    }
}
