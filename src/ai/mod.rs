//! AI: exactly two API clients, picked by `config.ai.provider`: the Anthropic
//! Messages API (`anthropic`) and OpenAI-compatible chat completions
//! (`openai`: OpenAI, Gemini's OpenAI endpoint, OpenRouter, Ollama, LM Studio).
//! The third provider, `browser`, has no client: it opens Google AI Mode in the
//! user's own browser and nothing comes back.
//!
//! Blocking (ureq + rustls), no async runtime: call from a worker thread.
//! Nothing here runs unless the caller does; the key comes from
//! `crate::secrets` and never appears in an error, a log or a `Debug`.

pub mod anthropic;
pub mod browser;
pub mod models;
pub mod openai;
pub mod providers;

use std::fmt;
use std::io::BufRead;
use std::ops::ControlFlow;

use serde_json::Value;

use crate::config::{AiConfig, Provider};
use crate::secrets::Secret;

/// The one place default model ids live. Anthropic: current Sonnet (DESIGN.md);
/// an OpenAI-compatible server has no sensible default, the user picks one.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-5-5";
/// Sent as `anthropic-version`.
pub const ANTHROPIC_API_VERSION: &str = "2026-10-21";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Instructions. Anthropic: the top-level `system` field; OpenAI: a system message.
    System,
    User,
    Assistant,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
    /// A PNG, sent inline (base64). Only the caller decides to attach one.
    pub image_png: Option<Vec<u8>>,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        Message {
            role,
            text: text.into(),
            image_png: None,
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, text)
    }

    #[cfg(test)]
    pub fn with_image(mut self, png: Vec<u8>) -> Self {
        self.image_png = Some(png);
        self
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let image = self
            .image_png
            .as_ref()
            .map(|i| format!("<{} bytes>", i.len()));
        f.debug_struct("Message")
            .field("role", &self.role)
            .field("text", &self.text)
            .field("image_png", &image)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// Something the user must set first (key, model, endpoint).
    NotConfigured(String),
    /// 401 / 403. Each HTTP error keeps the provider's own (key-scrubbed)
    /// reason, "" when it gave none.
    Auth(u16, String),
    /// 429.
    RateLimited(String),
    /// 5xx (including Anthropic's 529 "overloaded").
    Server(u16, String),
    /// Any other HTTP error.
    Rejected(u16, String),
    /// The model declined to answer (`stop_reason: refusal`).
    Refused,
    /// Could not reach the provider.
    Network(String),
    /// The provider answered, but not in the expected shape.
    BadResponse(String),
    /// The offline translation engine (`crate::mt`) failed; no AI involved.
    Offline(String),
    /// Nothing to translate: the text is in the target already (its name).
    /// Not a failure: shown as a quiet note, never an error card.
    AlreadyIn(&'static str),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NotConfigured(what) => write!(f, "AI is not set up yet: {what}."),
            AiError::Auth(code, why) => write!(
                f,
                "The AI provider rejected the API key ({}). Check the key and that it may use this model.",
                http(*code, why)
            ),
            AiError::RateLimited(why) => write!(
                f,
                "The AI provider says you are sending too much or the quota is used up ({}). Wait a moment and try again.",
                http(429, why)
            ),
            AiError::Server(code, why) => write!(
                f,
                "The AI provider has a problem right now ({}). Try again shortly.",
                http(*code, why)
            ),
            // a model the provider retired or never had: its list has the ones it has now
            AiError::Rejected(404, why) => write!(
                f,
                "The AI provider rejected the request ({}). Choose another model from the list (Settings, Ask AI, Model).",
                http(404, why)
            ),
            AiError::Rejected(code, why) => write!(
                f,
                "The AI provider rejected the request ({}).",
                http(*code, why)
            ),
            AiError::Refused => f.write_str("The AI model declined to answer this request."),
            AiError::Network(why) => write!(f, "Could not reach the AI provider: {why}"),
            AiError::BadResponse(why) => {
                write!(f, "The AI provider sent an unexpected answer: {why}")
            }
            AiError::Offline(why) => write!(f, "Local translation failed: {why}"),
            AiError::AlreadyIn(name) => write!(f, "Already in {name}"),
        }
    }
}

impl std::error::Error for AiError {}

/// "HTTP 503", or "HTTP 503: <the provider's reason>".
fn http(code: u16, why: &str) -> String {
    match why.trim_end_matches('.') {
        "" => format!("HTTP {code}"),
        why => format!("HTTP {code}: {why}"),
    }
}

/// A fully built request. Not `Debug` on purpose: the headers hold the key.
pub(crate) struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

/// What one SSE event means.
pub(crate) enum Event {
    Delta(String),
    /// The model is reasoning before it answers (its text is not shown).
    Thinking,
    Done,
    Skip,
}

/// Streams the answer; `on_delta` gets each new piece of text and returns `Break`
/// to stop (the connection is dropped). An empty piece means the model is
/// thinking (the provider streams its reasoning, which is not shown). Returns
/// the text so far.
pub fn ask_stream(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    on_delta: impl FnMut(&str) -> ControlFlow<()>,
) -> Result<String, AiError> {
    stream(cfg, key, messages, None, on_delta)
}

/// Like `ask_stream`, for short mechanical answers (translation): the answer is capped
/// at `max_tokens` and the provider is asked not to "think" first. Only the
/// request differs; the answer is read the same way.
pub(crate) fn ask_fast(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    max_tokens: u32,
) -> Result<String, AiError> {
    stream(cfg, key, messages, Some(max_tokens), |_| {
        ControlFlow::Continue(())
    })
}

fn stream(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    fast: Option<u32>,
    on_delta: impl FnMut(&str) -> ControlFlow<()>,
) -> Result<String, AiError> {
    let answer = match &cfg.provider {
        Provider::Browser => Err(AiError::NotConfigured(
            "this needs an AI provider with an API key (Settings, Ask AI); Google AI Mode in your \
             browser cannot send an answer back"
                .into(),
        )),
        Provider::Invalid(raw) => Err(AiError::NotConfigured(format!(
            "the AI provider setting \"{raw}\" isn't recognised (Settings, Ask AI)"
        ))),
        Provider::Unreadable => Err(AiError::NotConfigured(
            "the settings file couldn't be read, so AI is off (Settings, Ask AI)".into(),
        )),
        Provider::Anthropic => anthropic::ask_stream(cfg, key, messages, fast, on_delta),
        Provider::OpenaiCompatible => openai::ask_stream(cfg, key, messages, fast, on_delta),
    };
    if let Err(e) = &answer {
        logged(cfg, e);
    }
    answer
}

/// Every provider error goes to the log, so Copy diagnostics carries it: the
/// provider's name and the error as shown (key-scrubbed, never the prompt).
pub(crate) fn logged(cfg: &AiConfig, e: &AiError) {
    let who = providers::Preset::of(cfg).map_or("AI", |p| p.name);
    crate::log!("[ai] {who}: {e}");
}

/// Translate many on-screen lines in ONE call. The answer must be a JSON array
/// of exactly `lines.len()` strings, in order, or this fails (never a silent
/// shift of lines onto the wrong boxes). `extra_instructions` ("" = none) is
/// added to the rules (e.g. translate's markup for highlighted names).
pub fn translate_lines(
    cfg: &AiConfig,
    key: Option<&Secret>,
    lines: &[String],
    source: &str,
    target: &str,
    extra_instructions: &str,
) -> Result<Vec<String>, AiError> {
    let cap = translate_max_tokens(lines);
    translate_with(lines, source, target, extra_instructions, |msgs| {
        ask_fast(cfg, key, msgs, cap)
    })
}

/// Answer cap for a translation: about the input's size (the JSON array is
/// roughly as long as its source in bytes; Arabic needs more tokens than
/// English, so this is generous) plus room for the brackets and quotes.
/// Clamped to 256..=4096 so one huge screen can't run away.
pub(crate) fn translate_max_tokens(lines: &[String]) -> u32 {
    let bytes: usize = lines.iter().map(String::len).sum();
    (bytes + 256).clamp(256, 4096) as u32
}

pub(crate) fn translate_with(
    lines: &[String],
    source: &str,
    target: &str,
    extra_instructions: &str,
    ask: impl FnOnce(&[Message]) -> Result<String, AiError>,
) -> Result<Vec<String>, AiError> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let input = serde_json::to_string(lines).map_err(|e| AiError::BadResponse(e.to_string()))?;
    let mut system = translate_system_prompt(source, target, lines.len());
    if !extra_instructions.is_empty() {
        system = format!("{system}\n{extra_instructions}");
    }
    let msgs = [Message::new(Role::System, system), Message::user(input)];
    parse_translation(&ask(&msgs)?, lines.len())
}

/// The languages by their English names (the table's), with the code: any
/// target works, a code outside the table is sent as is. Source "auto" = the
/// model detects it.
fn translate_system_prompt(source: &str, target: &str, n: usize) -> String {
    use crate::translate::lang;
    let from = match source {
        lang::AUTO => String::new(),
        s => format!(" written in {} (\"{s}\")", lang::name(s)),
    };
    let to = lang::name(target);
    format!(
        "You translate on-screen text{from} from video games and apps into {to} (language code \"{target}\"). \
The user message is a JSON array of {n} strings. Each string is one separate text element read from the SAME screen \
(subtitle, menu label, item name, button, tooltip, ...), in reading order.\n\
Rules:\n\
- Translate each element on its own; never merge, split, reorder, add or drop elements.\n\
- Be consistent: the same source term (character name, place, item, skill, UI label) gets the same translation everywhere in the array. \
Keep proper names and made-up game terms in their usual transliteration or untranslated form.\n\
- Keep numbers, symbols, key names, placeholders (like {{0}}, %s, $name$) and markup tags exactly as they are.\n\
- Keep UI labels short; a translation should not be much longer than its source.\n\
- Elements that are not translatable text (numbers, codes, already in the target language, OCR noise) come back unchanged.\n\
Answer with ONLY a JSON array of exactly {n} strings, in the same order. No commentary, no code fences."
    )
}

/// Accepts the array alone or wrapped in prose/code fences (first `[` to last `]`).
fn parse_translation(answer: &str, expected: usize) -> Result<Vec<String>, AiError> {
    let json = match (answer.find('['), answer.rfind(']')) {
        (Some(a), Some(b)) if a < b => &answer[a..=b],
        _ => {
            return Err(AiError::BadResponse(
                "the translation was not a JSON array".into(),
            ));
        }
    };
    let out: Vec<String> = serde_json::from_str(json).map_err(|_| {
        AiError::BadResponse("the translation was not a JSON array of strings".into())
    })?;
    if out.len() != expected {
        return Err(AiError::BadResponse(format!(
            "expected {expected} translated lines, got {}",
            out.len()
        )));
    }
    Ok(out)
}

// ---- shared HTTP plumbing ----

/// POST, check the status, then feed each SSE `data:` payload to `parse` and
/// pass text deltas to `on_delta` (`Break` ends the stream). Returns the concatenated text.
pub(crate) fn run_stream(
    req: &HttpRequest,
    key: Option<&Secret>,
    mut on_delta: impl FnMut(&str) -> ControlFlow<()>,
    parse: fn(&str) -> Result<Event, AiError>,
) -> Result<String, AiError> {
    let agent = crate::net::agent(crate::net::AI);
    let body = serde_json::to_string(&req.body).map_err(|e| AiError::BadResponse(e.to_string()))?;
    let send = || {
        let mut builder = agent.post(&req.url);
        for (name, value) in &req.headers {
            builder = builder.header(*name, value.as_str());
        }
        builder
            .send(body.as_str())
            .map_err(|e| AiError::Network(scrub(&e.to_string(), key)))
    };
    let mut resp = send()?;
    // busy (503) or overloaded (529): once more after a moment, silently
    if matches!(resp.status().as_u16(), 503 | 529) {
        std::thread::sleep(RETRY_AFTER);
        resp = send()?;
    }
    let status = resp.status().as_u16();
    if status >= 400 {
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        return Err(map_status(status, &text, key));
    }

    let mut full = String::new();
    let mut failure = None;
    let reader = std::io::BufReader::new(resp.into_body().into_reader());
    read_sse(reader, |data| match parse(data) {
        Ok(Event::Delta(piece)) => {
            full.push_str(&piece);
            on_delta(&piece).is_continue()
        }
        Ok(Event::Thinking) => on_delta("").is_continue(),
        Ok(Event::Skip) => true,
        Ok(Event::Done) => false,
        Err(e) => {
            failure = Some(e);
            false
        }
    })
    .map_err(|e| AiError::Network(scrub(&e.to_string(), key)))?;
    match failure {
        Some(e) => Err(e),
        None => Ok(full),
    }
}

/// Server-sent events: calls `on_data` with each event's joined `data:` lines;
/// stops when it returns false. Other fields (`event:`, `id:`, comments) are ignored.
pub(crate) fn read_sse(
    reader: impl BufRead,
    mut on_data: impl FnMut(&str) -> bool,
) -> std::io::Result<()> {
    let mut data = String::new();
    let mut has_data = false;
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            if has_data && !on_data(&data) {
                return Ok(());
            }
            data.clear();
            has_data = false;
        } else if let Some(rest) = line.strip_prefix("data:") {
            if has_data {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            has_data = true;
        }
    }
    if has_data {
        on_data(&data);
    }
    Ok(())
}

/// How long a busy provider gets before the one retry.
#[cfg(not(test))]
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_millis(20);

/// Readable message for an HTTP error status, with the provider's own reason
/// (one line, key removed).
pub(crate) fn map_status(status: u16, body: &str, key: Option<&Secret>) -> AiError {
    let why = scrub(&reason(body), key);
    match status {
        401 | 403 => AiError::Auth(status, why),
        429 => AiError::RateLimited(why),
        500..=599 => AiError::Server(status, why),
        _ => AiError::Rejected(status, why),
    }
}

/// The provider's reason in an error body: `{"error": {"message"}}` (OpenAI,
/// Anthropic, most others), the same in an array (Google), `{"error": "..."}`
/// (Ollama), `{"message"}`; "" for anything else (an HTML page, nothing).
fn reason(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return String::new();
    };
    ["/error/message", "/0/error/message", "/error", "/message"]
        .iter()
        .find_map(|p| v.pointer(p).and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// One line, at most 300 chars, with the key (if it appears) replaced.
pub(crate) fn scrub(text: &str, key: Option<&Secret>) -> String {
    let mut s = text.to_string();
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        s = s.replace(k.expose(), "[key]");
    }
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(300).collect()
}

/// The one transport check, run by both clients before any key header exists.
/// Scheme is matched case-insensitively. `https://` always passes; `http://`
/// only for this machine (host `localhost` / `*.localhost`, or an IP literal
/// that `is_loopback()`), unless there is no key to leak; any other scheme fails.
pub(crate) fn check_endpoint(url: &str, has_key: bool) -> Result<(), AiError> {
    let bad = |why: &str| Err(AiError::NotConfigured(why.to_string()));
    let Some((scheme, rest)) = url.split_once("://") else {
        return bad("the endpoint must be an http(s):// URL");
    };
    if scheme.eq_ignore_ascii_case("https") || (!has_key && scheme.eq_ignore_ascii_case("http")) {
        return Ok(());
    }
    if !scheme.eq_ignore_ascii_case("http") {
        return bad("the endpoint must be an http(s):// URL");
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Backslashes and spaces are parsed differently by different URL parsers: refuse.
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    };
    let host_lc = host.to_ascii_lowercase();
    let local = !authority.contains(['\\', ' '])
        && (host_lc == "localhost"
            || host_lc.ends_with(".localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()));
    if local {
        Ok(())
    } else {
        bad(
            "the endpoint uses plain http:// with an API key; use https:// (http is allowed only for localhost)",
        )
    }
}

/// `base` without trailing slashes.
pub(crate) fn join_url(base: &str, path: &str) -> String {
    format!("{}{path}", base.trim_end_matches('/'))
}

pub(crate) fn png_b64(png: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(png)
}

/// A provider on 127.0.0.1 for tests: answers each connection with the next
/// canned reply and hands back each request's head (request line + headers).
#[cfg(test)]
pub(crate) mod mock {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{Receiver, channel};

    pub fn reply(status: u16, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    pub fn stream(sse: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{sse}"
        )
    }

    /// The base URL (`http://127.0.0.1:<port>/v1`) and the requests' heads.
    pub fn serve(replies: Vec<String>) -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let (mut head, mut length) = (String::new(), 0);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&line);
                }
                let _ = reader.by_ref().take(length).read_to_end(&mut Vec::new());
                let _ = tx.send(head);
                let mut out = reader.into_inner();
                let _ = out.write_all(reply.as_bytes());
                let _ = out.flush();
            }
        });
        (url, rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn endpoint_check_with_a_key() {
        for ok in [
            "https://api.anthropic.com",
            "HTTPS://api.openai.com/v1",
            "http://localhost:11434/v1",
            "HTTP://LOCALHOST/v1",
            "http://app.localhost/v1",
            "http://127.0.0.1:1234",
            "http://127.1.2.3/v1",
            "http://[::1]:11434",
            "http://user:pw@127.0.0.1:1234/v1",
        ] {
            assert!(check_endpoint(ok, true).is_ok(), "{ok}");
        }
        for bad in [
            "HTTP://example.com",
            "http://example.com/v1",
            "http://127.evil.com",
            "http://localhost.evil.com",
            "http://localhost@evil.com/v1",
            "http://127.0.0.1@evil.com/v1",
            "http://127.0.0.1\\@evil.com/v1",
            "http://192.168.1.5:1234/v1",
            "http://[2001:db8::1]/v1",
            "http://0.0.0.0:1234",
            "ftp://x",
            "file:///etc/passwd",
            "api.anthropic.com",
            "",
        ] {
            assert!(check_endpoint(bad, true).is_err(), "{bad}");
        }
    }

    #[test]
    fn endpoint_check_without_a_key_only_needs_http_or_https() {
        assert!(check_endpoint("http://192.168.1.5:1234/v1", false).is_ok());
        assert!(check_endpoint("HTTP://example.com", false).is_ok());
        assert!(check_endpoint("ftp://x", false).is_err());
    }

    #[test]
    fn translate_rejects_wrong_length() {
        let err =
            translate_with(&lines(3), "auto", "ar", "", |_| Ok(r#"["a","b"]"#.into())).unwrap_err();
        assert!(
            matches!(err, AiError::BadResponse(ref m) if m.contains("expected 3") && m.contains("got 2")),
            "{err}"
        );
        assert!(
            translate_with(&lines(1), "auto", "ar", "", |_| Ok(r#"["a","b"]"#.into())).is_err()
        );
    }

    #[test]
    fn translate_rejects_non_arrays() {
        for bad in ["I cannot", "[1,2]", r#"{"a":"b"}"#, "[\"a\""] {
            assert!(
                translate_with(&lines(2), "auto", "ar", "", |_| Ok(bad.into())).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn translate_accepts_fenced_answer_and_keeps_order() {
        let out = translate_with(&lines(2), "auto", "ar", "", |_| {
            Ok("```json\n[\"واحد\", \"two\"]\n```".into())
        })
        .unwrap();
        assert_eq!(out, vec!["واحد".to_string(), "two".to_string()]);
    }

    #[test]
    fn translate_request_is_one_call_with_json_input() {
        let mut seen = Vec::new();
        let input = vec!["Start Game".to_string(), "Potion \"X\"".to_string()];
        translate_with(&input, "fr", "ar", "", |m| {
            seen = m.to_vec();
            Ok(r#"["a","b"]"#.into())
        })
        .unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].role, Role::System);
        assert!(
            seen[0].text.contains("into Arabic (language code \"ar\")"),
            "{}",
            seen[0].text
        );
        assert!(seen[0].text.contains("written in French (\"fr\")"));
        assert!(seen[0].text.contains("exactly 2 strings"));
        assert!(seen[0].text.contains("consistent"));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&seen[1].text).unwrap(),
            input
        );
    }

    #[test]
    fn translate_prompt_names_the_languages() {
        let auto = translate_system_prompt("auto", "ja", 1);
        assert!(auto.contains("text from video games") && !auto.contains("written in"));
        assert!(
            auto.contains("into Japanese (language code \"ja\")"),
            "{auto}"
        );
        // a code outside the table goes as is
        let other = translate_system_prompt("ar-EG", "sw", 1);
        assert!(other.contains("written in Arabic (\"ar-EG\")") && other.contains("into sw"));
    }

    #[test]
    fn translate_cap_grows_with_input_and_is_clamped() {
        assert_eq!(translate_max_tokens(&["Hi".into()]), 258);
        assert_eq!(translate_max_tokens(&[]), 256);
        assert_eq!(translate_max_tokens(&["x".repeat(100_000)]), 4096);
    }

    #[test]
    fn empty_input_makes_no_call() {
        assert_eq!(
            translate_with(&[], "auto", "ar", "", |_| panic!("no call expected")).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn error_mapping_is_readable_and_never_echoes_the_key() {
        let key = Secret::new("sk-FAKE-SECRET-123456");
        let echo = r#"{"error":{"message":"Incorrect API key provided: sk-FAKE-SECRET-123456."}}"#;
        let said = "Incorrect API key provided: [key].".to_string();
        assert_eq!(map_status(401, echo, Some(&key)), AiError::Auth(401, said));
        assert!(matches!(
            map_status(403, echo, Some(&key)),
            AiError::Auth(403, _)
        ));
        assert_eq!(
            map_status(429, "", Some(&key)),
            AiError::RateLimited(String::new())
        );
        assert_eq!(
            map_status(500, "oops", None),
            AiError::Server(500, String::new())
        );
        assert_eq!(
            map_status(529, "", None),
            AiError::Server(529, String::new())
        );
        // A 400 keeps the provider's message, minus the key, on one line.
        let e = map_status(
            400,
            r#"{"error":{"message":"bad key sk-FAKE-SECRET-123456\nmodel: x"}}"#,
            Some(&key),
        );
        assert_eq!(e, AiError::Rejected(400, "bad key [key] model: x".into()));
        for e in [
            map_status(401, echo, Some(&key)),
            e,
            map_status(404, "not json", None),
            AiError::Network(scrub("boom sk-FAKE-SECRET-123456", Some(&key))),
        ] {
            let shown = e.to_string();
            assert!(!shown.contains("FAKE-SECRET"), "{shown}");
            assert!(!shown.is_empty());
        }
        assert!(
            AiError::RateLimited(String::new())
                .to_string()
                .contains("429")
        );
        assert!(
            AiError::Auth(401, String::new())
                .to_string()
                .contains("API key")
        );
    }

    /// Google's bodies as recorded (2026-11-10): an array around the error.
    const BUSY: &str = r#"[{"error":{"code":503,"message":"This model is currently experiencing high demand. Spikes in demand are usually temporary.","status":"UNAVAILABLE"}}]"#;
    const GONE: &str = r#"[{"error":{"code":404,"message":"This model models/gemini-2.0-flash is no longer available.","status":"NOT_FOUND"}}]"#;

    /// A 5xx says the provider's own reason, a 404 points at the model list.
    #[test]
    fn a_server_error_shows_the_providers_reason() {
        let busy = map_status(503, BUSY, None).to_string();
        assert!(
            busy.contains("HTTP 503: This model is currently experiencing high demand"),
            "{busy}"
        );
        let gone = map_status(404, GONE, None).to_string();
        assert!(gone.contains("no longer available"), "{gone}");
        assert!(
            gone.contains("Choose another model from the list"),
            "{gone}"
        );
        // Ollama's plain string, a bare message, an HTML page
        let ollama = map_status(404, r#"{"error":"model \"x\" not found"}"#, None);
        assert_eq!(
            ollama,
            AiError::Rejected(404, "model \"x\" not found".into())
        );
        let bare = map_status(500, r#"{"message":"boom"}"#, None);
        assert_eq!(bare, AiError::Server(500, "boom".into()));
        let page = map_status(502, "<html>Bad gateway</html>", None).to_string();
        assert!(page.contains("(HTTP 502)"), "{page}");
    }

    /// A fake key made at run time (never a key-shaped literal in the source).
    pub(crate) fn fake_key() -> Secret {
        Secret::new(format!("{}-{}", "test", "k3y9z-w45-not-real"))
    }

    /// Every provider error is in the log (the provider's name, the reason
    /// as shown), with the key gone even when the provider echoes it.
    #[test]
    fn a_provider_error_is_logged_without_the_key() {
        let key = fake_key();
        let body = format!(
            r#"{{"error":{{"message":"w45 unique reason for {}"}}}}"#,
            key.expose()
        );
        let url = mock::serve(vec![mock::reply(400, &body)]).0;
        let cfg = AiConfig {
            provider: Provider::OpenaiCompatible,
            base_url: url,
            model: "m".into(),
        };
        let err = ask_stream(&cfg, Some(&key), &[Message::user("secret prompt")], |_| {
            ControlFlow::Continue(())
        })
        .unwrap_err();
        assert!(matches!(err, AiError::Rejected(400, _)), "{err}");
        let line = crate::log::tail(200)
            .into_iter()
            .find(|l| l.contains("w45 unique reason"))
            .expect("the error is logged");
        assert!(
            line.contains("[ai] Custom server:") && line.contains("HTTP 400"),
            "{line}"
        );
        assert!(
            !line.contains(key.expose()) && !line.contains("secret prompt"),
            "{line}"
        );
    }

    /// A busy provider (503) is asked once more, silently; a second 503 shows.
    #[test]
    fn a_busy_provider_is_retried_once() {
        let ok = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\ndata: [DONE]\n\n";
        let (url, seen) = mock::serve(vec![mock::reply(503, BUSY), mock::stream(ok)]);
        let cfg = AiConfig {
            provider: Provider::OpenaiCompatible,
            base_url: url,
            model: "m".into(),
        };
        let go = |cfg: &AiConfig| {
            ask_stream(cfg, None, &[Message::user("q")], |_| {
                ControlFlow::Continue(())
            })
        };
        assert_eq!(go(&cfg).unwrap(), "Hi");
        assert_eq!(seen.try_iter().count(), 2);
        // twice busy: the reason shows, and no third try
        let (url, seen) = mock::serve(vec![mock::reply(503, BUSY), mock::reply(503, BUSY)]);
        let err = go(&AiConfig {
            base_url: url,
            ..cfg
        })
        .unwrap_err();
        assert!(err.to_string().contains("high demand"), "{err}");
        assert_eq!(seen.try_iter().count(), 2);
    }

    #[test]
    fn sse_reader_joins_data_lines_and_ignores_other_fields() {
        let sse = ": comment\nevent: a\ndata: one\n\nid: 7\ndata:two\ndata: three\n\ndata: tail";
        let mut got = Vec::new();
        read_sse(sse.as_bytes(), |d| {
            got.push(d.to_string());
            true
        })
        .unwrap();
        assert_eq!(got, ["one", "two\nthree", "tail"]);
        // CRLF line endings.
        let mut got = Vec::new();
        read_sse("data: x\r\n\r\ndata: y\r\n\r\n".as_bytes(), |d| {
            got.push(d.to_string());
            true
        })
        .unwrap();
        assert_eq!(got, ["x", "y"]);
    }

    #[test]
    fn message_debug_hides_image_bytes() {
        let m = Message::user("hi").with_image(vec![1, 2, 3]);
        let shown = format!("{m:?}");
        assert!(shown.contains("<3 bytes>"), "{shown}");
    }
}
