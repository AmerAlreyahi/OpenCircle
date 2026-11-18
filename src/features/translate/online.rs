//! The online engine (owner 2026-11-17; a normal engine choice since 0.3.0): Google's free web translator
//! (`translate_a/single`, client "gtx"): no key, no setup. Unofficial: it may
//! stop or refuse (429) at any time, and then the caller translates on this
//! PC. Only the text goes out (never the picture), only for one-shot
//! Translate, and only once the user picked this engine.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::shared::net::{self, Policy};

const URL: &str = "https://translate.googleapis.com/translate_a/single";

/// Characters per request: a page goes in a few calls, never one huge body.
const MAX_CHARS: usize = 4000;

/// After a refusal (429, 503) or an answer we can't read: no request for this
/// long (the next pages translate on this PC at once, no waiting on a timeout).
const COOL_DOWN: Duration = Duration::from_secs(600);
static COOL_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// `texts` from `source` ("auto" or a code) into `target`, one translation
/// per text. `Err` = why not (logged; the caller translates offline).
pub fn translate(texts: &[String], source: &str, target: &str) -> Result<Vec<String>, String> {
    translate_at(URL, net::TRANSLATE, texts, source, target)
}

fn translate_at(
    url: &str,
    policy: Policy,
    texts: &[String],
    source: &str,
    target: &str,
) -> Result<Vec<String>, String> {
    let until = *COOL_UNTIL.lock().unwrap_or_else(|e| e.into_inner());
    if until.is_some_and(|t| Instant::now() < t) {
        return Err("resting after a refusal".into());
    }
    let agent = net::agent(policy);
    let mut out = Vec::with_capacity(texts.len());
    for chunk in chunks(texts) {
        match call(&agent, url, chunk, code(source), code(target)) {
            Ok(got) => out.extend(got),
            Err(Failure { why, rest }) => {
                if rest {
                    *COOL_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(Instant::now() + COOL_DOWN);
                }
                return Err(why);
            }
        }
    }
    Ok(out)
}

struct Failure {
    why: String,
    /// Stop asking for a while (a refusal, or an answer in another shape).
    rest: bool,
}

/// Texts in runs of at most `MAX_CHARS` (a longer text goes alone).
fn chunks(texts: &[String]) -> Vec<&[String]> {
    let (mut out, mut start, mut chars) = (Vec::new(), 0, 0);
    for (i, t) in texts.iter().enumerate() {
        let n = t.chars().count() + 1;
        if i > start && chars + n > MAX_CHARS {
            out.push(&texts[start..i]);
            (start, chars) = (i, 0);
        }
        chars += n;
    }
    if start < texts.len() {
        out.push(&texts[start..]);
    }
    out
}

/// Our language codes in Google's spelling.
fn code(c: &str) -> &str {
    match c {
        "zh" => "zh-CN",
        "zh-Hant" => "zh-TW",
        "he" => "iw",
        c => c,
    }
}

/// One request: the texts one per line. A dropped connection or a server
/// error (not a refusal) is tried once more, a second later.
fn call(
    agent: &ureq::Agent,
    url: &str,
    texts: &[String],
    source: &str,
    target: &str,
) -> Result<Vec<String>, Failure> {
    // (a text's own line breaks would shift every line after it)
    let q = (texts.iter())
        .map(|t| t.replace(['\n', '\r'], " "))
        .collect::<Vec<_>>()
        .join("\n");
    let fail = |why: String, rest| Failure { why, rest };
    for again in [true, false] {
        let sent = agent
            .post(url)
            .query("client", "gtx")
            .query("sl", source)
            .query("tl", target)
            .query("dt", "t")
            .send_form([("q", q.as_str())]);
        let mut resp = match sent {
            Ok(r) => r,
            Err(e) if again && net::dropped(&e) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            Err(e) => return Err(fail(format!("no answer ({e})"), false)),
        };
        let status = resp.status().as_u16();
        match status {
            200 => {}
            429 | 503 => return Err(fail(format!("refused ({status})"), true)),
            500..=599 if again => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            _ => return Err(fail(format!("status {status}"), false)),
        }
        let body = (resp.body_mut().with_config().limit(1 << 20))
            .read_to_string()
            .map_err(|e| fail(format!("answer not read ({e})"), false))?;
        return parse(&body, texts.len()).ok_or_else(|| fail("unexpected answer".into(), true));
    }
    Err(fail("no answer".into(), false))
}

/// `[[["translated", "original", ...], ...], ...]`: the pieces joined, then
/// one line per text; `None` = another shape or another line count.
fn parse(body: &str, n: usize) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut all = String::new();
    for piece in v.get(0)?.as_array()? {
        all.push_str(piece.get(0)?.as_str()?);
    }
    let lines: Vec<String> = all.split('\n').map(|l| l.trim().to_string()).collect();
    (lines.len() == n).then_some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    /// A fake endpoint on 127.0.0.1 answering each request with the next of
    /// `answers` (status, body); returns its URL and what it received.
    fn fake(answers: Vec<(u16, String)>) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/translate_a/single",
            listener.local_addr().unwrap()
        );
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let (mut head, mut length) = (String::new(), 0);
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                let mut sent = vec![0; length];
                reader.read_exact(&mut sent).unwrap();
                let _ = tx.send(head + &String::from_utf8_lossy(&sent));
                let mut out = reader.into_inner();
                let _ = write!(
                    out,
                    "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (url, rx)
    }

    const TEST: Policy = Policy {
        https_only: false,
        ..net::TRANSLATE
    };

    fn texts(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    /// One shared cool-down: the tests that touch it run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn lines_come_back_one_per_text() {
        let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        *COOL_UNTIL.lock().unwrap() = None;
        let body = r#"[[["مرحبا. ","Hello. ",null],["كيف حالك؟\n","How are you?\n",null],["ثالث","Third",null]],null,"en"]"#;
        let (url, got) = fake(vec![(200, body.into())]);
        let out = translate_at(
            &url,
            TEST,
            &texts(&["Hello. How are you?", "Third"]),
            "auto",
            "zh",
        )
        .unwrap();
        assert_eq!(out, ["مرحبا. كيف حالك؟", "ثالث"]);
        let sent = got.recv().unwrap();
        assert!(
            sent.contains("sl=auto") && sent.contains("tl=zh-CN"),
            "{sent}"
        );
        assert!(
            sent.contains("q=Hello.+How+are+you%3F%0AThird"),
            "only the text: {sent}"
        );
    }

    /// A refusal rests the engine: the next page doesn't ask (and waits on
    /// nothing); a wrong line count is an error, never shifted lines.
    #[test]
    fn a_refusal_rests_and_a_bad_answer_fails() {
        let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        *COOL_UNTIL.lock().unwrap() = None;
        let (url, _got) = fake(vec![(200, r#"[[["one","a",null]]]"#.into())]);
        assert!(translate_at(&url, TEST, &texts(&["a", "b"]), "en", "ar").is_err());
        *COOL_UNTIL.lock().unwrap() = None;
        let (url, got) = fake(vec![(429, String::new())]);
        let e = translate_at(&url, TEST, &texts(&["a"]), "en", "ar").unwrap_err();
        assert!(e.contains("429"), "{e}");
        got.recv().unwrap();
        let e = translate_at(&url, TEST, &texts(&["a"]), "en", "ar").unwrap_err();
        assert!(e.contains("resting"), "{e}");
        assert!(got.try_recv().is_err(), "nothing sent while resting");
        *COOL_UNTIL.lock().unwrap() = None;
    }

    /// A server error is tried once more; it then translates.
    #[test]
    fn a_server_error_is_tried_again_once() {
        let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        *COOL_UNTIL.lock().unwrap() = None;
        let ok = r#"[[["سلام","hi",null]]]"#.to_string();
        let (url, got) = fake(vec![(500, String::new()), (200, ok)]);
        assert_eq!(
            translate_at(&url, TEST, &texts(&["hi"]), "en", "ar").unwrap(),
            ["سلام"]
        );
        assert_eq!(got.iter().take(2).count(), 2);
    }

    #[test]
    fn long_pages_go_in_runs() {
        let long = "x".repeat(MAX_CHARS - 2);
        let t = texts(&[&long, "a", "b", &long]);
        let runs: Vec<usize> = chunks(&t).iter().map(|c| c.len()).collect();
        assert_eq!(runs, [1, 2, 1]);
        assert!(chunks(&[]).is_empty());
    }

    /// The real endpoint (network; `cargo test real_endpoint -- --ignored`).
    #[test]
    #[ignore]
    fn real_endpoint() {
        let t = texts(&[
            "Settings",
            "Start the quest. Talk to the elder.",
            "Inventory full",
        ]);
        let out = translate(&t, "auto", "ar").unwrap();
        eprintln!("{out:?}");
        assert_eq!(out.len(), 3);
        assert!(
            out.iter()
                .all(|l| l.chars().any(|c| ('\u{600}'..='\u{6ff}').contains(&c)))
        );
    }
}
