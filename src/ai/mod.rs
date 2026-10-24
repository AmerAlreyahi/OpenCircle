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
pub mod openai;

use std::fmt;
use std::io::BufRead;
use std::time::Duration;

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
    /// 401 / 403.
    Auth(u16),
    /// 429.
    RateLimited,
    /// 5xx (including Anthropic's 529 "overloaded").
    Server(u16),
    /// Any other HTTP error, with the provider's own (key-scrubbed) message.
    Rejected(u16, String),
    /// The model declined to answer (`stop_reason: refusal`).
    Refused,
    /// Could not reach the provider.
    Network(String),
    /// The provider answered, but not in the expected shape.
    BadResponse(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NotConfigured(what) => write!(f, "AI is not set up yet: {what}."),
            AiError::Auth(code) => write!(f, "The AI provider rejected the API key (HTTP {code}). Check the key and that it may use this model."),
            AiError::RateLimited => f.write_str("The AI provider says you are sending too much or the quota is used up (HTTP 429). Wait a moment and try again."),
            AiError::Server(code) => write!(f, "The AI provider has a problem right now (HTTP {code}). Try again shortly."),
            AiError::Rejected(code, why) if why.is_empty() => write!(f, "The AI provider rejected the request (HTTP {code})."),
            AiError::Rejected(code, why) => write!(f, "The AI provider rejected the request (HTTP {code}): {why}"),
            AiError::Refused => f.write_str("The AI model declined to answer this request."),
            AiError::Network(why) => write!(f, "Could not reach the AI provider: {why}"),
            AiError::BadResponse(why) => write!(f, "The AI provider sent an unexpected answer: {why}"),
        }
    }
}

impl std::error::Error for AiError {}

/// A fully built request. Not `Debug` on purpose: the headers hold the key.
pub(crate) struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

/// What one SSE event means.
pub(crate) enum Event {
    Delta(String),
    Done,
    Skip,
}

pub fn ask(cfg: &AiConfig, key: Option<&Secret>, messages: &[Message]) -> Result<String, AiError> {
    ask_stream(cfg, key, messages, |_| {})
}

/// Streams the answer; `on_delta` gets each new piece of text. Returns the full text.
pub fn ask_stream(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    on_delta: impl FnMut(&str),
) -> Result<String, AiError> {
    match cfg.provider {
        Provider::Browser => Err(AiError::NotConfigured(
            "this needs an AI provider with an API key (Settings, AI); Google AI Mode in your \
             browser cannot send an answer back"
                .into(),
        )),
        Provider::Anthropic => anthropic::ask_stream(cfg, key, messages, on_delta),
        Provider::OpenaiCompatible => openai::ask_stream(cfg, key, messages, on_delta),
    }
}

/// Translate many on-screen lines in ONE call. The answer must be a JSON array
/// of exactly `lines.len()` strings, in order, or this fails (never a silent
/// shift of lines onto the wrong boxes). `extra_instructions` ("" = none) is
/// added to the rules (e.g. translate's markup for highlighted names).
pub fn translate_lines(
    cfg: &AiConfig,
    key: Option<&Secret>,
    lines: &[String],
    target: &str,
    extra_instructions: &str,
) -> Result<Vec<String>, AiError> {
    translate_with(lines, target, extra_instructions, |msgs| {
        ask(cfg, key, msgs)
    })
}

pub(crate) fn translate_with(
    lines: &[String],
    target: &str,
    extra_instructions: &str,
    ask: impl FnOnce(&[Message]) -> Result<String, AiError>,
) -> Result<Vec<String>, AiError> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let input = serde_json::to_string(lines).map_err(|e| AiError::BadResponse(e.to_string()))?;
    let mut system = translate_system_prompt(target, lines.len());
    if !extra_instructions.is_empty() {
        system = format!("{system}\n{extra_instructions}");
    }
    let msgs = [Message::new(Role::System, system), Message::user(input)];
    parse_translation(&ask(&msgs)?, lines.len())
}

fn translate_system_prompt(target: &str, n: usize) -> String {
    format!(
        "You translate on-screen text from video games and apps into the language with code \"{target}\". \
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
/// pass text deltas to `on_delta`. Returns the concatenated text.
pub(crate) fn run_stream(
    req: &HttpRequest,
    key: Option<&Secret>,
    mut on_delta: impl FnMut(&str),
    parse: fn(&str) -> Result<Event, AiError>,
) -> Result<String, AiError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(300)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut builder = agent.post(&req.url);
    for (name, value) in &req.headers {
        builder = builder.header(*name, value.as_str());
    }
    let body = serde_json::to_string(&req.body).map_err(|e| AiError::BadResponse(e.to_string()))?;
    let mut resp = builder
        .send(body)
        .map_err(|e| AiError::Network(scrub(&e.to_string(), key)))?;

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
            on_delta(&piece);
            full.push_str(&piece);
            true
        }
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

/// Readable message for an HTTP error status. The provider's own text is kept
/// only for "other" 4xx (it names the bad field), one line, key removed.
pub(crate) fn map_status(status: u16, body: &str, key: Option<&Secret>) -> AiError {
    match status {
        401 | 403 => AiError::Auth(status),
        429 => AiError::RateLimited,
        500..=599 => AiError::Server(status),
        _ => {
            let msg = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            AiError::Rejected(status, scrub(&msg, key))
        }
    }
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
        let err = translate_with(&lines(3), "ar", "", |_| Ok(r#"["a","b"]"#.into())).unwrap_err();
        assert!(
            matches!(err, AiError::BadResponse(ref m) if m.contains("expected 3") && m.contains("got 2")),
            "{err}"
        );
        assert!(translate_with(&lines(1), "ar", "", |_| Ok(r#"["a","b"]"#.into())).is_err());
    }

    #[test]
    fn translate_rejects_non_arrays() {
        for bad in ["I cannot", "[1,2]", r#"{"a":"b"}"#, "[\"a\""] {
            assert!(
                translate_with(&lines(2), "ar", "", |_| Ok(bad.into())).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn translate_accepts_fenced_answer_and_keeps_order() {
        let out = translate_with(&lines(2), "ar", "", |_| {
            Ok("```json\n[\"واحد\", \"two\"]\n```".into())
        })
        .unwrap();
        assert_eq!(out, vec!["واحد".to_string(), "two".to_string()]);
    }

    #[test]
    fn translate_request_is_one_call_with_json_input() {
        let mut seen = Vec::new();
        let input = vec!["Start Game".to_string(), "Potion \"X\"".to_string()];
        translate_with(&input, "ar", "", |m| {
            seen = m.to_vec();
            Ok(r#"["a","b"]"#.into())
        })
        .unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].role, Role::System);
        assert!(seen[0].text.contains("\"ar\"") && seen[0].text.contains("exactly 2 strings"));
        assert!(seen[0].text.contains("consistent"));
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&seen[1].text).unwrap(),
            input
        );
    }

    #[test]
    fn empty_input_makes_no_call() {
        assert_eq!(
            translate_with(&[], "ar", "", |_| panic!("no call expected")).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn error_mapping_is_readable_and_never_echoes_the_key() {
        let key = Secret::new("sk-FAKE-SECRET-123456");
        let echo = r#"{"error":{"message":"Incorrect API key provided: sk-FAKE-SECRET-123456."}}"#;
        assert_eq!(map_status(401, echo, Some(&key)), AiError::Auth(401));
        assert_eq!(map_status(403, echo, Some(&key)), AiError::Auth(403));
        assert_eq!(map_status(429, "", Some(&key)), AiError::RateLimited);
        assert_eq!(map_status(500, "oops", None), AiError::Server(500));
        assert_eq!(map_status(529, "", None), AiError::Server(529));
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
        assert!(AiError::RateLimited.to_string().contains("429"));
        assert!(AiError::Auth(401).to_string().contains("API key"));
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
