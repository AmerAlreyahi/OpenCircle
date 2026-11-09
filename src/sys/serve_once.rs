//! A page handed to the user's own browser by a one-time web server on this PC:
//! 127.0.0.1 only, a random port, a secret 128-bit path, ONE GET of that path,
//! then closed (or after `LIFE`, fetched or not). The page lives in memory only:
//! nothing touches the disk, and a sandboxed browser (snap, Flatpak) that can't
//! read our temp folder still gets it (#156). The path and the page are never logged.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The server closes this long after it starts, even if the page was never fetched.
const LIFE: Duration = Duration::from_secs(30);
/// How long one connection may take to send its request / read the page.
const IO_WAIT: Duration = Duration::from_secs(5);
/// A request head bigger than this is refused.
const MAX_HEAD: usize = 8 * 1024;
/// At most this many connections are read at once (browsers preconnect a few).
const MAX_OPEN: usize = 8;

/// The page waiting for its one fetch (the full HTTP response), `None` once sent.
type Page = Arc<Mutex<Option<Vec<u8>>>>;

/// Serves `html` once with this Content-Security-Policy; returns its URL
/// (`http://127.0.0.1:<port>/<secret>`), already listening.
pub fn serve(html: &str, csp: &str) -> io::Result<String> {
    serve_for(html, csp, LIFE)
}

fn serve_for(html: &str, csp: &str, life: Duration) -> io::Result<String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?; // never 0.0.0.0
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let mut secret = [0u8; 16];
    getrandom::fill(&mut secret).map_err(|e| io::Error::other(e.to_string()))?;
    let secret: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    let want = Want {
        path: format!("/{secret}"),
        host: format!("127.0.0.1:{port}"),
    };
    let page: Page = Arc::new(Mutex::new(Some(response(html, csp))));
    std::thread::Builder::new()
        .name("page server".into())
        .spawn(move || run(&listener, &Arc::new(want), &page, life))?;
    Ok(format!("http://127.0.0.1:{port}/{secret}"))
}

/// The one request that gets the page.
struct Want {
    path: String,
    host: String,
}

fn run(listener: &TcpListener, want: &Arc<Want>, page: &Page, life: Duration) {
    let end = Instant::now() + life;
    // ponytail: polls accept every 20 ms for at most `LIFE`; a wake-up socket if it ever matters.
    while Instant::now() < end && page.lock().is_ok_and(|p| p.is_some()) {
        match listener.accept() {
            // One thread per connection, so a silent preconnect can't hold up the real one.
            Ok((stream, _)) if Arc::strong_count(page) <= MAX_OPEN => {
                let (want, page) = (want.clone(), page.clone());
                let _ = std::thread::Builder::new()
                    .name("page request".into())
                    .spawn(move || answer(stream, &want, &page));
            }
            Ok(_) => {} // too many at once: dropped
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
} // the listener drops here: the port is closed

fn answer(mut stream: TcpStream, want: &Want, page: &Page) {
    // An accepted socket can inherit the listener's non-blocking mode (Windows).
    let ready = stream.set_nonblocking(false).is_ok()
        && stream.set_read_timeout(Some(IO_WAIT)).is_ok()
        && stream.set_write_timeout(Some(IO_WAIT)).is_ok();
    let sent = ready
        .then(|| read_head(&mut stream))
        .flatten()
        .filter(|head| wanted(head, want))
        .and_then(|_| page.lock().ok()?.take());
    let out = sent.as_deref().unwrap_or(NOT_FOUND);
    let _ = stream.write_all(out).and_then(|()| stream.flush());
}

const NOT_FOUND: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";

/// The request head (up to the blank line), or `None` (too big, cut, not UTF-8).
fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.ends_with(b"\r\n\r\n") {
        let n = stream.read(&mut buf).ok().filter(|&n| n > 0)?;
        head.extend_from_slice(&buf[..n]);
        if head.len() > MAX_HEAD {
            return None;
        }
        if let Some(end) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            head.truncate(end + 4);
        }
    }
    String::from_utf8(head).ok()
}

/// Exactly `GET <secret path> HTTP/1.x` with `Host: 127.0.0.1:<port>` (no other
/// host: a page of another site can't reach it by a renamed DNS entry).
fn wanted(head: &str, want: &Want) -> bool {
    let mut lines = head.split("\r\n");
    let request: Vec<&str> = lines.next().unwrap_or("").split(' ').collect();
    let line_ok = matches!(request[..], ["GET", path, version]
        if path == want.path && version.starts_with("HTTP/1."));
    let host_ok = lines.any(|l| {
        l.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("host") && value.trim() == want.host
        })
    });
    line_ok && host_ok
}

fn response(html: &str, csp: &str) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Content-Security-Policy: {csp}\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Connection: close\r\n\r\n",
        html.len()
    )
    .into_bytes();
    out.extend_from_slice(html.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One request on a fresh connection; the whole answer, or the connect error.
    fn fetch(url: &str, request: impl Fn(&str, &str) -> String) -> io::Result<String> {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_at(rest.find('/').unwrap());
        let mut s = TcpStream::connect(host)?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        s.write_all(request(host, path).as_bytes())?;
        let mut out = String::new();
        s.read_to_string(&mut out)?;
        Ok(out)
    }

    fn get(host: &str, path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: text/html\r\n\r\n")
    }

    /// Waits until the server's port refuses connections (closed), or panics.
    fn closed(url: &str) {
        let host = &url["http://".len()..url.rfind('/').unwrap()];
        let end = Instant::now() + Duration::from_secs(3);
        while TcpStream::connect(host).is_ok() {
            assert!(Instant::now() < end, "still listening");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn only_loopback_with_a_secret_path() {
        let a = serve("<p>a</p>", "default-src 'none'").unwrap();
        let b = serve("<p>b</p>", "default-src 'none'").unwrap();
        for url in [&a, &b] {
            let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
            let (port, secret) = rest.split_once('/').unwrap();
            assert!(port.parse::<u16>().unwrap() > 0);
            assert_eq!(secret.len(), 32, "128 bits");
            assert!(secret.bytes().all(|c| c.is_ascii_hexdigit()));
            assert!(crate::sys::open::web_host(url).is_some(), "a plain link");
        }
        assert_ne!(a.rsplit('/').next(), b.rsplit('/').next(), "never reused");
        for url in [a, b] {
            assert!(fetch(&url, get).unwrap().starts_with("HTTP/1.1 200 OK"));
        }
    }

    #[test]
    fn one_fetch_then_closed() {
        let url = serve(
            "<p>hi</p>",
            "default-src 'none'; form-action https://x.test",
        )
        .unwrap();
        let first = fetch(&url, get).unwrap();
        let (head, body) = first.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 200 OK"));
        assert!(head.contains("Content-Type: text/html; charset=utf-8"));
        assert!(head.contains("Cache-Control: no-store"));
        assert!(
            head.contains(
                "Content-Security-Policy: default-src 'none'; form-action https://x.test"
            )
        );
        assert!(head.contains("Content-Length: 9"));
        assert!(
            !head.to_ascii_lowercase().contains("access-control"),
            "no CORS"
        );
        assert_eq!(body, "<p>hi</p>");
        // A second fetch never gets the page: a 404 or no server at all.
        if let Ok(second) = fetch(&url, get) {
            assert!(second.starts_with("HTTP/1.1 404"), "{second}");
        }
        closed(&url);
    }

    #[test]
    fn anything_but_the_one_get_is_404_and_the_page_waits() {
        let url = serve("<p>page</p>", "default-src 'none'").unwrap();
        let wrong: [fn(&str, &str) -> String; 9] = [
            |h, _| get(h, "/"),
            |h, p| get(h, &format!("{p}x")),
            |h, p| get(h, &p[..p.len() - 1]),
            |h, p| get(h, &format!("{p}/")),
            |h, p| get(h, &format!("{p}?a=1")),
            |h, p| get(h, &format!("/..{p}")),
            |h, p| get(h, &p.to_uppercase()),
            |h, p| format!("POST {p} HTTP/1.1\r\nHost: {h}\r\nContent-Length: 0\r\n\r\n"),
            |_, p| get("evil.test", p), // another host (DNS rebinding)
        ];
        for (i, req) in wrong.iter().enumerate() {
            let got = fetch(&url, req).unwrap();
            assert!(got.starts_with("HTTP/1.1 404 Not Found"), "{i}: {got}");
            assert!(!got.contains("page"), "{i}");
        }
        for req in [
            |_: &str, p: &str| format!("GET {p} HTTP/1.1\r\n\r\n"), // no Host
            |h: &str, p: &str| format!("HEAD {p} HTTP/1.1\r\nHost: {h}\r\n\r\n"),
        ] {
            assert!(fetch(&url, req).unwrap().starts_with("HTTP/1.1 404"));
        }
        // a head too big: a 404, or a reset when the rest was left unread
        let big = |h: &str, p: &str| {
            format!(
                "GET {p} HTTP/1.1\r\nHost: {h}\r\nX: {}\r\n\r\n",
                "a".repeat(9000)
            )
        };
        assert!(fetch(&url, big).map_or(true, |r| r.starts_with("HTTP/1.1 404")));
        // A silent connection (a browser's preconnect) doesn't hold up the real one.
        let host = &url["http://".len()..url.rfind('/').unwrap()];
        let _idle = TcpStream::connect(host).unwrap();
        let started = Instant::now();
        assert!(fetch(&url, get).unwrap().ends_with("<p>page</p>"));
        assert!(started.elapsed() < Duration::from_secs(2));
        closed(&url);
    }

    #[test]
    fn closes_after_its_life_even_if_never_fetched() {
        let url = serve_for("<p>x</p>", "default-src 'none'", Duration::from_millis(150)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        closed(&url);
        assert!(fetch(&url, get).is_err(), "nothing to fetch");
    }
}
