//! One-click "Connect OpenRouter": OpenRouter's documented OAuth PKCE flow
//! (no app registration, a loopback callback on any port, RFC 8252). The
//! browser opens the consent page; its redirect comes back to a one-time
//! server on 127.0.0.1 (`serve_once::receive`); the code and our verifier
//! (which never leaves the PC) are swapped for a key, which the caller saves
//! in the keyring. Neither the code nor the key is ever logged.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::shared::app_language::{S, tr};
use crate::shared::net::{self, Policy};
use crate::shared::platform::sys::serve_once;
use crate::shared::secrets::Secret;

const AUTH: &str = "https://openrouter.ai/auth";
const EXCHANGE: &str = "https://openrouter.ai/api/v1/auth/keys";
/// The user has this long to finish in the browser (a closed tab or a
/// "deny" sends nothing back: the wait ends here).
const WAIT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInError {
    /// The user pressed Cancel.
    Cancelled,
    /// A plain sentence for the Settings row (never a raw status alone).
    Failed(String),
}

/// One sign-in for a worker thread: `run` it there; `cancel` stops it.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: u32,
    pub cancel: Arc<AtomicBool>,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Job {
    pub fn run(self) -> (u32, Result<Secret, SignInError>) {
        let open = |url: &str| crate::shared::platform::sys::open::open_link(url);
        let got = connect(&self.cancel, open, EXCHANGE, net::OAUTH, WAIT);
        if let Err(SignInError::Failed(e)) = &got {
            crate::log!("[ai] Connect OpenRouter: {e}");
        }
        (self.id, got)
    }
}

/// The PKCE pair and the `state` the callback must echo.
struct Pkce {
    verifier: String,
    challenge: String,
    state: String,
}

fn pkce() -> Result<Pkce, SignInError> {
    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let mut bytes = [0u8; 48];
    getrandom::fill(&mut bytes).map_err(|e| SignInError::Failed(e.to_string()))?;
    let verifier = b64(&bytes[..32]); // 43 chars, the RFC 7636 minimum
    Ok(Pkce {
        challenge: b64(&hmac_sha256::Hash::hash(verifier.as_bytes())),
        state: b64(&bytes[32..]),
        verifier,
    })
}

/// `s` with every byte but RFC 3986 unreserved ones as `%XX`.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn auth_url(callback: &str, p: &Pkce) -> String {
    format!(
        "{AUTH}?callback_url={}&code_challenge={}&code_challenge_method=S256&state={}",
        encode(callback),
        p.challenge,
        p.state
    )
}

/// The code from the callback's query, if its `state` is ours.
fn code_of(query: &str, state: &str) -> Result<String, SignInError> {
    let get = |name: &str| {
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == name)
            .and_then(|(_, v)| decode(v))
    };
    if get("state").as_deref() != Some(state) {
        return Err(SignInError::Failed(tr(S::signin_mismatch).into()));
    }
    get("code")
        .filter(|c| !c.is_empty())
        .ok_or_else(|| SignInError::Failed(tr(S::signin_denied).into()))
}

/// Swaps the code for a key at `url`.
fn exchange(url: &str, policy: Policy, code: &str, verifier: &str) -> Result<Secret, SignInError> {
    let failed = |s: S| SignInError::Failed(tr(s).into());
    let body = serde_json::json!({
        "code": code,
        "code_verifier": verifier,
        "code_challenge_method": "S256",
    });
    let mut reply = net::agent(policy)
        .post(url)
        .header("Content-Type", "application/json")
        .send(body.to_string().as_str())
        .map_err(|_| failed(S::signin_offline))?;
    match reply.status().as_u16() {
        200..=299 => {}
        400..=499 => return Err(failed(S::signin_refused)),
        _ => return Err(failed(S::signin_busy)),
    }
    let text = reply.body_mut().read_to_string().unwrap_or_default();
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| failed(S::signin_busy))?;
    json["key"]
        .as_str()
        .map(Secret::new)
        .filter(|k| !k.is_empty())
        .ok_or_else(|| failed(S::signin_busy))
}

/// The whole flow: callback server, consent page in the browser (`open`),
/// the wait (Cancel or `wait`), the exchange at `exchange_url`.
fn connect(
    cancel: &AtomicBool,
    open: impl Fn(&str) -> std::io::Result<()>,
    exchange_url: &str,
    policy: Policy,
    wait: Duration,
) -> Result<Secret, SignInError> {
    let failed = |s: S| SignInError::Failed(tr(s).into());
    let page = format!(
        "<!doctype html><meta charset=utf-8><title>OpenCircle</title>\
         <p dir=auto style=\"font:16px system-ui,sans-serif;margin:3em\">{}</p>",
        tr(S::signin_page)
    );
    let callback = serve_once::receive(&page, wait).map_err(|_| failed(S::signin_busy))?;
    let p = pkce()?;
    open(&auth_url(&callback.url, &p)).map_err(|_| failed(S::signin_browser))?;
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if cancel.load(Ordering::Relaxed) {
            return Err(SignInError::Cancelled);
        }
        if let Some(query) = callback.wait(Duration::from_millis(200)) {
            let code = code_of(&query, &p.state)?;
            return exchange(exchange_url, policy, &code, &p.verifier);
        }
    }
    Err(failed(S::signin_timeout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    const TEST: Policy = Policy {
        https_only: false,
        ..net::OAUTH
    };

    #[test]
    fn pkce_is_s256_of_a_fresh_verifier() {
        use base64::Engine as _;
        let (a, b) = (pkce().unwrap(), pkce().unwrap());
        assert_eq!(a.verifier.len(), 43);
        assert_ne!(a.verifier, b.verifier, "fresh each time");
        assert_ne!(a.state, b.state);
        let want = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(hmac_sha256::Hash::hash(a.verifier.as_bytes()));
        assert_eq!(a.challenge, want);
        let url = auth_url("http://localhost:5/x", &a);
        assert!(url.starts_with(
            "https://openrouter.ai/auth?callback_url=http%3A%2F%2Flocalhost%3A5%2Fx&code_challenge="
        ));
        assert!(url.contains("&code_challenge_method=S256&state="));
        assert!(
            !url.contains(&a.verifier),
            "the verifier never leaves the PC"
        );
    }

    #[test]
    fn the_callback_must_carry_our_state() {
        assert_eq!(code_of("code=ab%2Bc&state=s1", "s1"), Ok("ab+c".into()));
        assert!(
            code_of("code=abc&state=s2", "s1").is_err(),
            "another sign-in's"
        );
        assert!(code_of("code=abc", "s1").is_err());
        assert!(code_of("state=s1", "s1").is_err(), "no code: denied");
        assert!(code_of("state=s1&code=", "s1").is_err());
    }

    /// A fake OpenRouter key endpoint: answers one POST with `status` and
    /// `body` (`{verifier}` replaced by the challenge check's result) and
    /// hands back the request body.
    fn fake_exchange(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/v1/auth/keys", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if rest.len() >= len || n == 0 {
                        let reply = format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        s.write_all(reply.as_bytes()).unwrap();
                        return rest.to_string();
                    }
                }
            }
        });
        (url, h)
    }

    /// The fake key: built at run time (no key-shaped literal in the source).
    fn fake_key() -> String {
        ["fake", "key", "for", "tests"].join("-")
    }

    /// The whole flow against a fake OpenRouter: the test plays the browser
    /// (follows the consent URL's callback with a code and the state).
    #[test]
    fn connect_end_to_end_with_a_fake_openrouter() {
        let body: &'static str =
            Box::leak(format!("{{\"key\":\"{}\",\"user_id\":null}}", fake_key()).into_boxed_str());
        let (url, server) = fake_exchange(200, body);
        let browser = |auth: &str| {
            let q = auth.split_once('?').unwrap().1;
            let param = |n: &str| {
                q.split('&')
                    .find_map(|kv| kv.strip_prefix(&format!("{n}=")))
                    .map(|v| decode(v).unwrap())
                    .unwrap()
            };
            let (cb, state) = (param("callback_url"), param("state"));
            let rest = cb.strip_prefix("http://localhost:").unwrap();
            let (port, path) = rest.split_at(rest.find('/').unwrap());
            std::thread::spawn({
                let (port, path) = (port.to_string(), path.to_string());
                move || {
                    let mut s =
                        TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
                    write!(
                        s,
                        "GET {path}?code=the-code&state={state} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"
                    )
                    .unwrap();
                    let mut page = String::new();
                    let _ = s.read_to_string(&mut page);
                    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
                }
            });
            Ok(())
        };
        let key = connect(&AtomicBool::new(false), browser, &url, TEST, WAIT).unwrap();
        assert_eq!(key.expose(), fake_key());
        let sent: serde_json::Value = serde_json::from_str(&server.join().unwrap()).unwrap();
        assert_eq!(sent["code"], "the-code");
        assert_eq!(sent["code_challenge_method"], "S256");
        assert_eq!(sent["code_verifier"].as_str().unwrap().len(), 43);
    }

    #[test]
    fn refused_and_broken_exchanges_say_so_plainly() {
        for (status, body) in [
            (403, "{\"error\":\"Invalid code\"}"),
            (500, "oops"),
            (200, "{}"),
        ] {
            let (url, server) = fake_exchange(status, body);
            let got = exchange(&url, TEST, "c", "v");
            server.join().unwrap();
            let Err(SignInError::Failed(why)) = got else {
                panic!("{status}: {got:?}")
            };
            assert!(!why.contains(&status.to_string()), "no raw code: {why}");
        }
        // nobody listening
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let got = exchange(&format!("http://127.0.0.1:{port}/k"), TEST, "c", "v");
        assert_eq!(got, Err(SignInError::Failed(tr(S::signin_offline).into())));
    }

    #[test]
    fn cancel_timeout_and_no_browser_end_the_wait() {
        let no_browser = |_: &str| Err(std::io::Error::other("none"));
        let quiet = |_: &str| Ok(());
        let got = connect(&AtomicBool::new(false), no_browser, "x", TEST, WAIT);
        assert_eq!(got, Err(SignInError::Failed(tr(S::signin_browser).into())));
        let t = Instant::now();
        let got = connect(&AtomicBool::new(true), quiet, "x", TEST, WAIT);
        assert_eq!(got, Err(SignInError::Cancelled));
        assert!(t.elapsed() < Duration::from_secs(2));
        let got = connect(
            &AtomicBool::new(false),
            quiet,
            "x",
            TEST,
            Duration::from_millis(300),
        );
        assert_eq!(got, Err(SignInError::Failed(tr(S::signin_timeout).into())));
    }
}
