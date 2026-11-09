//! A page handed to the user's own browser by a one-time web server on this PC:
//! 127.0.0.1 only, a random port, a secret 128-bit path, ONE GET of that path,
//! then closed (or after `LIFE`, fetched or not). The page lives in memory only:
//! nothing touches the disk, and a sandboxed browser (snap, Flatpak) that can't
//! read our temp folder still gets it (#156). The path and the page are never logged.
//!
//! The link can be seen by other accounts on this PC (a browser's command line),
//! so a connection is taken only from a process of OUR user (`peer_is_ours`,
//! checked at accept, before it gets a slot or can touch the page); anything
//! else, or a check that can't run, is dropped and the page keeps waiting.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The server closes this long after it starts, even if the page was never fetched.
const LIFE: Duration = Duration::from_secs(120);
/// One deadline per connection, from accept, for its whole request head (a slow
/// sender can't keep a slot alive by trickling bytes). A browser sends at once.
const HEAD_WAIT: Duration = Duration::from_secs(3);
/// How long the browser may take to read the page.
const WRITE_WAIT: Duration = Duration::from_secs(5);
/// A request head bigger than this is refused.
const MAX_HEAD: usize = 8 * 1024;
/// At most this many connections are read at once (browsers preconnect a few).
/// Only our own user's processes get a slot (`peer_is_ours`), and such a
/// process could read the screen itself, so a full house just drops the new one.
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
    let Ok(ours) = listener.local_addr() else {
        return;
    };
    // ponytail: polls accept every 20 ms for at most `LIFE`; a wake-up socket if it ever matters.
    while Instant::now() < end && page.lock().is_ok_and(|p| p.is_some()) {
        match listener.accept() {
            // One thread per connection, so a silent preconnect can't hold up the real one.
            Ok((stream, peer))
                if Arc::strong_count(page) <= MAX_OPEN && admit(peer_is_ours(peer, ours)) =>
            {
                let deadline = Instant::now() + HEAD_WAIT;
                let (want, page) = (want.clone(), page.clone());
                let _ = std::thread::Builder::new()
                    .name("page request".into())
                    .spawn(move || answer(stream, deadline, &want, &page));
            }
            Ok(_) => {} // too many at once, or not our user: dropped, the page untouched
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
} // the listener drops here: the port is closed

fn answer(mut stream: TcpStream, deadline: Instant, want: &Want, page: &Page) {
    // An accepted socket can inherit the listener's non-blocking mode (Windows).
    let ready =
        stream.set_nonblocking(false).is_ok() && stream.set_write_timeout(Some(WRITE_WAIT)).is_ok();
    let sent = ready
        .then(|| read_head(&mut stream, deadline))
        .flatten()
        .filter(|head| wanted(head, want))
        .and_then(|_| page.lock().ok()?.take());
    let out = sent.as_deref().unwrap_or(NOT_FOUND);
    let _ = stream.write_all(out).and_then(|()| stream.flush());
}

const NOT_FOUND: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";

/// The request head (up to the blank line), or `None` (too big, cut, not UTF-8,
/// or not all there by `deadline`).
fn read_head(stream: &mut TcpStream, deadline: Instant) -> Option<String> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.ends_with(b"\r\n\r\n") {
        let left = deadline.checked_duration_since(Instant::now())?;
        stream
            .set_read_timeout(Some(left.max(Duration::from_millis(1))))
            .ok()?;
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

// ---- whose connection is it -----------------------------------------------

/// 127.0.0.1 as the OS tables print it (`/proc/net/tcp`, `MIB_TCPROW`): the
/// address bytes in network order, read as a native integer.
const LOOPBACK: u32 = u32::from_ne_bytes([127, 0, 0, 1]);

/// One TCP connection of the OS table: local address and port, remote port,
/// and its owner (Linux: the uid; Windows: the process id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    addr: u32,
    local: u16,
    remote: u16,
    owner: u32,
}

/// The owner of the peer's end of our connection: the row whose LOCAL port is
/// the peer's and whose remote port is ours (our own end is the mirror row).
fn peer_owner(rows: &[Row], peer: SocketAddr, ours: SocketAddr) -> Option<u32> {
    let loopback = |a: SocketAddr| a.ip() == std::net::IpAddr::V4(Ipv4Addr::LOCALHOST);
    if !loopback(peer) || !loopback(ours) {
        return None;
    }
    rows.iter()
        .find(|r| r.addr == LOOPBACK && r.local == peer.port() && r.remote == ours.port())
        .map(|r| r.owner)
}

/// Take the connection? Only when it is surely our user's; a check that
/// couldn't run refuses it too (fail closed), logged once.
fn admit(ours: Result<bool, String>) -> bool {
    ours.unwrap_or_else(|e| {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| crate::log!("[page server] connection refused: owner unknown ({e})"));
        false
    })
}

/// `/proc/net/tcp` as rows (header and odd lines skipped).
#[cfg(any(target_os = "linux", test))]
fn proc_rows(table: &str) -> Vec<Row> {
    let hex_pair = |f: &str| {
        let (a, p) = f.split_once(':')?;
        Some((
            u32::from_str_radix(a, 16).ok()?,
            u16::from_str_radix(p, 16).ok()?,
        ))
    };
    table
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (addr, local) = hex_pair(f.get(1)?)?;
            let (_, remote) = hex_pair(f.get(2)?)?;
            let owner = f.get(7)?.parse().ok()?;
            Some(Row {
                addr,
                local,
                remote,
                owner,
            })
        })
        .collect()
}

/// Is the process at the other end of this loopback connection run by our user?
#[cfg(target_os = "linux")]
fn peer_is_ours(peer: SocketAddr, ours: SocketAddr) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    let me = std::fs::metadata("/proc/self")
        .map_err(|e| e.to_string())?
        .uid();
    let table = std::fs::read_to_string("/proc/net/tcp").map_err(|e| e.to_string())?;
    let owner = peer_owner(&proc_rows(&table), peer, ours).ok_or("connection not in the table")?;
    Ok(owner == me)
}

#[cfg(windows)]
fn peer_is_ours(peer: SocketAddr, ours: SocketAddr) -> Result<bool, String> {
    let pid = peer_owner(&win::tcp_rows()?, peer, ours).ok_or("connection not in the table")?;
    // SAFETY: plain token queries on our own process and the peer's.
    unsafe {
        let me = win::user_sid(windows::Win32::System::Threading::GetCurrentProcess())?;
        Ok(win::process_user(pid)? == me)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn peer_is_ours(_: SocketAddr, _: SocketAddr) -> Result<bool, String> {
    Err("no owner check on this OS".into())
}

#[cfg(windows)]
mod win {
    use super::Row;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows::Win32::Security::{
        GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const AF_INET: u32 = 2;

    /// The IPv4 TCP table with each connection's process id.
    pub(super) fn tcp_rows() -> Result<Vec<Row>, String> {
        let mut size = 0u32;
        for _ in 0..4 {
            let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
            size = (buf.len() * 8) as u32;
            // SAFETY: `buf` holds `size` bytes, 8-aligned; the call fills it or
            // says the size it needs (then we try again, the table may grow).
            let r = unsafe {
                GetExtendedTcpTable(
                    Some(buf.as_mut_ptr().cast()),
                    &mut size,
                    false,
                    AF_INET,
                    TCP_TABLE_OWNER_PID_ALL,
                    0,
                )
            };
            if r == 0 {
                // SAFETY: on success the buffer starts with the table and holds
                // `dwNumEntries` rows.
                let rows = unsafe {
                    let t = &*buf.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
                    std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize)
                };
                return Ok(rows
                    .iter()
                    .map(|r| Row {
                        addr: r.dwLocalAddr,
                        local: u16::from_be(r.dwLocalPort as u16),
                        remote: u16::from_be(r.dwRemotePort as u16),
                        owner: r.dwOwningPid,
                    })
                    .collect());
            }
            if r != 122 {
                // not ERROR_INSUFFICIENT_BUFFER
                return Err(format!("GetExtendedTcpTable failed ({r})"));
            }
            size += 4096;
        }
        Err("the TCP table kept growing".into())
    }

    /// The user SID of process `pid`.
    pub(super) unsafe fn process_user(pid: u32) -> Result<Vec<u8>, String> {
        // SAFETY: the handle is ours to close.
        unsafe {
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .map_err(|e| e.to_string())?;
            let sid = user_sid(p);
            let _ = CloseHandle(p);
            sid
        }
    }

    /// The user SID of `process`'s token, as bytes.
    pub(super) unsafe fn user_sid(process: HANDLE) -> Result<Vec<u8>, String> {
        // SAFETY: the token is ours to close; TokenUser writes a TOKEN_USER (8-aligned
        // buffer of the size it asked for) whose SID points into the same buffer.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|e| e.to_string())?;
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
            let read = GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                (buf.len() * 8) as u32,
                &mut len,
            );
            let _ = CloseHandle(token);
            read.map_err(|e| e.to_string())?;
            let sid = (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let n = GetLengthSid(sid) as usize;
            Ok(std::slice::from_raw_parts(sid.0.cast::<u8>(), n).to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slow "How do you want to open this?" prompt (Windows) must not outlive the page.
    #[test]
    fn the_page_waits_two_minutes_for_the_browser() {
        assert_eq!(LIFE, Duration::from_secs(120));
    }

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

    #[test]
    fn a_slow_sender_loses_its_slot_at_one_deadline_and_the_page_waits() {
        let url = serve("<p>page</p>", "default-src 'none'").unwrap();
        let host = &url["http://".len()..url.rfind('/').unwrap()];
        let path = &url[url.rfind('/').unwrap()..];
        let mut slow = TcpStream::connect(host).unwrap();
        slow.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let started = Instant::now();
        // a byte every 250 ms: each read is quick, the whole head never comes
        for b in get(host, path).bytes().take(30) {
            if slow.write_all(&[b]).is_err()
                || started.elapsed() > HEAD_WAIT + Duration::from_secs(1)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let mut got = String::new();
        let _ = slow.read_to_string(&mut got);
        let took = started.elapsed();
        assert!(took < HEAD_WAIT + Duration::from_secs(2), "{took:?}");
        assert!(!got.contains("page"), "{got}");
        assert!(fetch(&url, get).unwrap().ends_with("<p>page</p>"));
    }

    /// The OS tables as the owner check reads them: the PEER's row (its port
    /// local, ours remote), never our own mirror row; loopback only.
    #[test]
    fn the_owner_is_read_from_the_peers_row() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
            0: 0100007F:A1B2 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 11 1\n\
            1: 0100007F:A1B2 0100007F:C3D4 01 00000000:00000000 00:00000000 00000000  1000        0 12 1\n\
            2: 0100007F:C3D4 0100007F:A1B2 01 00000000:00000000 00:00000000 00000000  1001        0 13 1\n\
            3: 0F02000A:C3D5 0100007F:A1B2 01 00000000:00000000 00:00000000 00000000     7        0 14 1\n\
            junk\n";
        let rows = proc_rows(table);
        assert_eq!(rows.len(), 4);
        if cfg!(target_endian = "little") {
            assert_eq!(rows[2].addr, LOOPBACK);
        }
        let at = |port: u16| SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let (ours, peer) = (at(0xA1B2), at(0xC3D4));
        assert_eq!(
            peer_owner(&rows, peer, ours),
            Some(1001),
            "the peer's, not ours (1000)"
        );
        assert_eq!(peer_owner(&rows, at(0xC3D5), ours), None, "not on loopback");
        assert_eq!(peer_owner(&rows, at(0x1234), ours), None, "gone");
        let lan = SocketAddr::from(([10, 0, 2, 15], 0xC3D4));
        assert_eq!(peer_owner(&rows, lan, ours), None);
        // fail closed
        assert!(admit(Ok(true)));
        assert!(!admit(Ok(false)));
        assert!(!admit(Err("no table".into())));
    }

    /// The real check on this OS: our own connection is ours.
    #[test]
    fn our_own_connection_is_ours() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let ours = l.local_addr().unwrap();
        let _c = TcpStream::connect(ours).unwrap();
        let (_s, peer) = l.accept().unwrap();
        assert_eq!(peer_is_ours(peer, ours), Ok(true));
    }

    /// VM proof by hand (another account fetches first): serves a page, writes
    /// its link to `$OC_W24_DIR/url`, waits for `$OC_W24_DIR/other.done` (the other
    /// user's fetch), then fetches it itself: still there.
    #[test]
    #[ignore]
    fn another_user_cannot_take_the_page() {
        let dir = std::path::PathBuf::from(std::env::var("OC_W24_DIR").unwrap());
        let url = serve("<p>the crop</p>", "default-src 'none'").unwrap();
        std::fs::write(dir.join("url"), &url).unwrap();
        let end = Instant::now() + Duration::from_secs(25);
        while !dir.join("other.done").exists() {
            assert!(Instant::now() < end, "no other-user fetch");
            std::thread::sleep(Duration::from_millis(100));
        }
        let got = fetch(&url, get).unwrap();
        println!(
            "own fetch after the other user's: {}",
            got.lines().next().unwrap()
        );
        assert!(got.ends_with("<p>the crop</p>"));
    }
}
