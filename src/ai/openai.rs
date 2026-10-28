//! OpenAI-compatible chat completions (`POST {base_url}/chat/completions`, SSE).
//! `base_url` includes the version path, e.g. `https://api.openai.com/v1`,
//! `http://localhost:11434/v1` (Ollama), `http://localhost:1234/v1` (LM Studio),
//! `https://openrouter.ai/api/v1`, Gemini's `.../v1beta/openai`.

use serde_json::{Value, json};
use std::ops::ControlFlow;

use super::{AiError, Event, HttpRequest, Message, Role, join_url, png_b64, run_stream};
use crate::config::AiConfig;
use crate::secrets::Secret;

/// `fast` = Some(max answer tokens) for translate calls (see `fast_fields`).
pub fn ask_stream(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    fast: Option<u32>,
    on_delta: impl FnMut(&str) -> ControlFlow<()>,
) -> Result<String, AiError> {
    let req = build_request(cfg, key, messages, fast)?;
    run_stream(&req, key, on_delta, parse_event)
}

pub(crate) fn build_request(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    fast: Option<u32>,
) -> Result<HttpRequest, AiError> {
    if cfg.base_url.is_empty() {
        return Err(AiError::NotConfigured(
            "set the endpoint URL of your OpenAI-compatible server".into(),
        ));
    }
    if cfg.model.is_empty() {
        return Err(AiError::NotConfigured("choose a model".into()));
    }
    let key = key.filter(|k| !k.is_empty());
    super::check_endpoint(&cfg.base_url, key.is_some())?;

    let msgs: Vec<Value> = messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let content = match &m.image_png {
                // Text-only stays a plain string: the widest compatibility (Ollama, LM Studio).
                None => json!(m.text),
                Some(png) => {
                    let mut parts = vec![json!({"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", png_b64(png))}})];
                    if !m.text.is_empty() {
                        parts.push(json!({"type": "text", "text": m.text}));
                    }
                    json!(parts)
                }
            };
            json!({"role": role, "content": content})
        })
        .collect();

    let mut headers = vec![("content-type", "application/json".to_string())];
    if let Some(k) = key {
        headers.push(("authorization", format!("Bearer {}", k.expose())));
    }
    let mut body = json!({"model": cfg.model, "stream": true, "messages": msgs});
    if let Some(cap) = fast {
        fast_fields(&mut body, &cfg.base_url, &cfg.model, cap);
    }
    Ok(HttpRequest {
        url: join_url(&cfg.base_url, "/chat/completions"),
        headers,
        body,
    })
}

/// Translate calls only: cap the answer, make it deterministic, and ask the
/// server not to reason first. Each field goes only where it is documented:
/// - OpenRouter: `max_tokens`, `temperature`, and its unified
///   `reasoning: {enabled: false}` (openrouter.ai/docs/guides/best-practices/reasoning-tokens).
/// - api.openai.com: reasoning models (o-series, gpt-5) reject `temperature`,
///   count hidden reasoning against the cap, and accept different
///   `reasoning_effort` values per model, so they get nothing extra; other
///   models get `max_completion_tokens` and `temperature`.
/// - Any other server (Ollama, LM Studio, Gemini): `max_tokens` and
///   `temperature`, which the OpenAI chat format defines; nothing unknown.
fn fast_fields(body: &mut Value, base_url: &str, model: &str, cap: u32) {
    let host = host_of(base_url);
    if host == "openrouter.ai" || host.ends_with(".openrouter.ai") {
        body["reasoning"] = json!({"enabled": false});
    } else if host == "api.openai.com" {
        let m = model.to_ascii_lowercase();
        let reasoning = m.starts_with("gpt-5")
            || (m.starts_with('o') && m[1..].starts_with(|c: char| c.is_ascii_digit()));
        if !reasoning {
            body["max_completion_tokens"] = json!(cap);
            body["temperature"] = json!(0);
        }
        return;
    }
    body["max_tokens"] = json!(cap);
    body["temperature"] = json!(0);
}

/// Lower-case host of a URL (no scheme, credentials, port or path).
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    host_port
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// One SSE `data:` payload: `[DONE]`, an error object, or a chunk with `choices[0].delta.content`.
pub(crate) fn parse_event(data: &str) -> Result<Event, AiError> {
    if data.trim() == "[DONE]" {
        return Ok(Event::Done);
    }
    let v: Value = serde_json::from_str(data)
        .map_err(|_| AiError::BadResponse("unreadable stream event".into()))?;
    if let Some(err) = v.get("error") {
        let why = err["message"]
            .as_str()
            .or(err.as_str())
            .unwrap_or("unknown error");
        return Err(AiError::BadResponse(super::scrub(why, None)));
    }
    let choice = &v["choices"][0];
    if choice["finish_reason"] == "content_filter" {
        return Err(AiError::Refused);
    }
    let delta = &choice["delta"];
    match delta["content"].as_str() {
        Some(text) if !text.is_empty() => Ok(Event::Delta(text.to_string())),
        // DeepSeek / Ollama / LM Studio: `reasoning_content`; OpenRouter: `reasoning`
        _ if [&delta["reasoning_content"], &delta["reasoning"]]
            .iter()
            .any(|r| r.as_str().is_some_and(|r| !r.is_empty())) =>
        {
            Ok(Event::Thinking)
        }
        _ => Ok(Event::Skip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    const FAKE_KEY: &str = "sk-FAKE-000000";

    fn cfg() -> AiConfig {
        AiConfig {
            provider: Provider::OpenaiCompatible,
            base_url: "https://api.example.com/v1/".into(),
            model: "gpt-x".into(),
        }
    }

    // Recorded shape of a streamed answer (abridged).
    const STREAM: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}

data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hel"},"finish_reason":null}]}

data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":null}]}

data: {"id":"c1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]

data: {"id":"never read","choices":[{"delta":{"content":"X"}}]}

"#;

    #[test]
    fn request_shape_text_and_auth() {
        let msgs = [Message::new(Role::System, "Be brief."), Message::user("Hi")];
        let req = build_request(&cfg(), Some(&Secret::new(FAKE_KEY)), &msgs, None).unwrap();
        assert_eq!(req.url, "https://api.example.com/v1/chat/completions");
        assert!(
            req.headers
                .contains(&("authorization", format!("Bearer {FAKE_KEY}")))
        );
        assert_eq!(
            req.body,
            json!({
                "model": "gpt-x",
                "stream": true,
                "messages": [{"role": "system", "content": "Be brief."}, {"role": "user", "content": "Hi"}]
            })
        );
    }

    fn fast_body(base: &str, model: &str) -> Value {
        let mut c = cfg();
        c.base_url = base.into();
        c.model = model.into();
        let msgs = [Message::user("Hi")];
        build_request(&c, None, &msgs, Some(300)).unwrap().body
    }

    #[test]
    fn translate_request_openrouter_caps_answer_and_turns_reasoning_off() {
        for base in [
            "https://openrouter.ai/api/v1",
            "https://OpenRouter.ai/api/v1/",
        ] {
            let b = fast_body(base, "nvidia/x:free");
            assert_eq!(b["max_tokens"], 300);
            assert_eq!(b["temperature"], 0);
            assert_eq!(b["reasoning"], json!({"enabled": false}));
            assert!(b.get("reasoning_effort").is_none());
        }
    }

    #[test]
    fn translate_request_other_servers_get_no_reasoning_field() {
        for base in [
            "http://localhost:11434/v1",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "https://openrouter.ai.evil.com/v1",
        ] {
            let b = fast_body(base, "m");
            assert_eq!(b["max_tokens"], 300, "{base}");
            assert_eq!(b["temperature"], 0, "{base}");
            assert!(b.get("reasoning").is_none(), "{base}");
            assert!(b.get("reasoning_effort").is_none(), "{base}");
        }
    }

    #[test]
    fn translate_request_openai_host_depends_on_model_family() {
        let b = fast_body("https://api.openai.com/v1", "gpt-4o-mini");
        assert_eq!(b["max_completion_tokens"], 300);
        assert_eq!(b["temperature"], 0);
        assert!(b.get("max_tokens").is_none());
        for m in ["gpt-5", "GPT-5-mini", "o3", "o4-mini"] {
            let b = fast_body("https://api.openai.com/v1", m);
            for k in [
                "max_tokens",
                "max_completion_tokens",
                "temperature",
                "reasoning",
                "reasoning_effort",
            ] {
                assert!(b.get(k).is_none(), "{m} {k}");
            }
        }
    }

    #[test]
    fn ask_requests_carry_no_translate_fields() {
        let b = build_request(&cfg(), None, &[Message::user("Hi")], None)
            .unwrap()
            .body;
        for k in [
            "max_tokens",
            "max_completion_tokens",
            "temperature",
            "reasoning",
        ] {
            assert!(b.get(k).is_none(), "{k}");
        }
    }

    #[test]
    fn request_shape_with_image_data_uri() {
        let msgs = [Message::user("What is this?").with_image(vec![0x89, b'P', b'N', b'G'])];
        let req = build_request(&cfg(), None, &msgs, None).unwrap();
        assert_eq!(
            req.body["messages"][0]["content"],
            json!([
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw=="}},
                {"type": "text", "text": "What is this?"}
            ])
        );
        // No key: no Authorization header (local servers).
        assert!(req.headers.iter().all(|(k, _)| *k != "authorization"));
    }

    #[test]
    fn needs_endpoint_and_model() {
        let mut c = cfg();
        c.base_url.clear();
        assert!(matches!(
            build_request(&c, None, &[], None),
            Err(AiError::NotConfigured(_))
        ));
        let mut c = cfg();
        c.model.clear();
        assert!(matches!(
            build_request(&c, None, &[], None),
            Err(AiError::NotConfigured(_))
        ));
    }

    #[test]
    fn key_is_never_sent_over_plain_http_to_a_remote_host() {
        let key = Secret::new(FAKE_KEY);
        let mut c = cfg();
        for local in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:8080/v1",
            "HTTP://LocalHost:1234/v1",
        ] {
            c.base_url = local.into();
            assert!(build_request(&c, Some(&key), &[], None).is_ok(), "{local}");
        }
        for remote in [
            "http://192.168.1.5:1234/v1",
            "http://example.com/v1",
            "http://localhost@evil.com/v1",
            "HTTP://example.com/v1",
            "http://127.evil.com/v1",
        ] {
            c.base_url = remote.into();
            let err = build_request(&c, Some(&key), &[], None)
                .err()
                .expect(remote);
            assert!(!err.to_string().contains(FAKE_KEY));
            // Without a key (a LAN LM Studio) it is allowed.
            assert!(build_request(&c, None, &[], None).is_ok(), "{remote}");
        }
    }

    #[test]
    fn stream_text_deltas_are_collected_until_done() {
        let mut out = String::new();
        super::super::read_sse(STREAM.as_bytes(), |d| match parse_event(d).unwrap() {
            Event::Delta(t) => {
                out.push_str(&t);
                true
            }
            Event::Skip | Event::Thinking => true,
            Event::Done => false,
        })
        .unwrap();
        assert_eq!(out, "Hello");
    }

    #[test]
    fn stream_errors_and_filters_fail() {
        let e = parse_event(r#"{"error":{"message":"Model not found sk-x"}}"#)
            .err()
            .unwrap();
        assert!(matches!(e, AiError::BadResponse(ref m) if m.contains("Model not found")));
        let f = r#"{"choices":[{"delta":{},"finish_reason":"content_filter"}]}"#;
        assert_eq!(parse_event(f).err().unwrap(), AiError::Refused);
        assert!(matches!(
            parse_event("<html>"),
            Err(AiError::BadResponse(_))
        ));
        assert!(matches!(parse_event(r#"{"choices":[]}"#), Ok(Event::Skip)));
    }

    #[test]
    fn reasoning_deltas_say_thinking() {
        for ev in [
            r#"{"choices":[{"delta":{"content":null,"reasoning_content":"Let me"}}]}"#,
            r#"{"choices":[{"delta":{"content":"","reasoning":"Let me"}}]}"#,
        ] {
            assert!(matches!(parse_event(ev), Ok(Event::Thinking)), "{ev}");
        }
        let empty = r#"{"choices":[{"delta":{"content":"","reasoning":""}}]}"#;
        assert!(matches!(parse_event(empty), Ok(Event::Skip)));
    }
}
