//! OpenAI-compatible chat completions (`POST {base_url}/chat/completions`, SSE).
//! `base_url` includes the version path, e.g. `https://api.openai.com/v1`,
//! `http://localhost:11434/v1` (Ollama), `http://localhost:1234/v1` (LM Studio),
//! `https://openrouter.ai/api/v1`, Gemini's `.../v1beta/openai`.

use serde_json::{Value, json};

use super::{AiError, Event, HttpRequest, Message, Role, join_url, png_b64, run_stream};
use crate::config::AiConfig;
use crate::secrets::Secret;

pub fn ask(cfg: &AiConfig, key: Option<&Secret>, messages: &[Message]) -> Result<String, AiError> {
    ask_stream(cfg, key, messages, |_| {})
}

pub fn ask_stream(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    on_delta: impl FnMut(&str),
) -> Result<String, AiError> {
    let req = build_request(cfg, key, messages)?;
    run_stream(&req, key, on_delta, parse_event)
}

pub(crate) fn build_request(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
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
    if key.is_some() && is_plain_http_to_remote(&cfg.base_url) {
        return Err(AiError::NotConfigured("the endpoint uses plain http:// with an API key; use https:// (http is allowed only for localhost)".into()));
    }

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
    Ok(HttpRequest {
        url: join_url(&cfg.base_url, "/chat/completions"),
        headers,
        body: json!({"model": cfg.model, "stream": true, "messages": msgs}),
    })
}

/// True for `http://` URLs whose host is not this machine.
fn is_plain_http_to_remote(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = if host_port.starts_with('[') {
        host_port
            .split(']')
            .next()
            .unwrap_or_default()
            .trim_start_matches('[')
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    !(host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host.starts_with("127."))
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
    match choice["delta"]["content"].as_str() {
        Some(text) if !text.is_empty() => Ok(Event::Delta(text.to_string())),
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
        let req = build_request(&cfg(), Some(&Secret::new(FAKE_KEY)), &msgs).unwrap();
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

    #[test]
    fn request_shape_with_image_data_uri() {
        let msgs = [Message::user("What is this?").with_image(vec![0x89, b'P', b'N', b'G'])];
        let req = build_request(&cfg(), None, &msgs).unwrap();
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
            build_request(&c, None, &[]),
            Err(AiError::NotConfigured(_))
        ));
        let mut c = cfg();
        c.model.clear();
        assert!(matches!(
            build_request(&c, None, &[]),
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
        ] {
            c.base_url = local.into();
            assert!(build_request(&c, Some(&key), &[]).is_ok(), "{local}");
        }
        for remote in [
            "http://192.168.1.5:1234/v1",
            "http://example.com/v1",
            "http://localhost@evil.com/v1",
        ] {
            c.base_url = remote.into();
            let err = build_request(&c, Some(&key), &[]).err().expect(remote);
            assert!(!err.to_string().contains(FAKE_KEY));
            // Without a key (a LAN LM Studio) it is allowed.
            assert!(build_request(&c, None, &[]).is_ok(), "{remote}");
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
            Event::Skip => true,
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
}
