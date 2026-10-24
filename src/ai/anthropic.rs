//! Anthropic Messages API (`POST {base}/v1/messages`, SSE streaming).

use serde_json::{Value, json};

use super::{
    ANTHROPIC_API_VERSION, AiError, DEFAULT_ANTHROPIC_MODEL, Event, HttpRequest, Message, Role,
    join_url, png_b64, run_stream,
};
use crate::config::AiConfig;
use crate::secrets::Secret;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// Cap on the answer (not a cost). Streaming, so a big cap does not risk a timeout.
const MAX_TOKENS: u32 = 16_000;

pub fn ask(cfg: &AiConfig, key: Option<&Secret>, messages: &[Message]) -> Result<String, AiError> {
    ask_stream(cfg, key, messages, None, |_| {})
}

/// `fast` = Some(max answer tokens) for translate calls (see `build_request`).
pub fn ask_stream(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    fast: Option<u32>,
    on_delta: impl FnMut(&str),
) -> Result<String, AiError> {
    let req = build_request(cfg, key, messages, fast)?;
    run_stream(&req, key, on_delta, parse_event)
}

/// `fast` (translate calls): the answer is capped at that many tokens and the
/// model is told to think as little as possible. Never `temperature`: API
/// version 2026-10-21 rejects sampling parameters with a 400.
pub(crate) fn build_request(
    cfg: &AiConfig,
    key: Option<&Secret>,
    messages: &[Message],
    fast: Option<u32>,
) -> Result<HttpRequest, AiError> {
    let key = key
        .filter(|k| !k.is_empty())
        .ok_or_else(|| AiError::NotConfigured("add your Anthropic API key".into()))?;
    super::check_endpoint(
        if cfg.base_url.is_empty() {
            DEFAULT_BASE_URL
        } else {
            &cfg.base_url
        },
        true,
    )?;
    let model = if cfg.model.is_empty() {
        DEFAULT_ANTHROPIC_MODEL
    } else {
        cfg.model.as_str()
    };
    let base = if cfg.base_url.is_empty() {
        DEFAULT_BASE_URL
    } else {
        cfg.base_url.as_str()
    };
    // Accept a base given with or without the /v1 suffix.
    let url = join_url(
        base.trim_end_matches('/').trim_end_matches("/v1"),
        "/v1/messages",
    );

    let system = messages
        .iter()
        .filter(|m| m.role == Role::System && !m.text.is_empty())
        .map(|m| m.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let turns: Vec<Value> = messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(|m| {
            let mut content = Vec::new();
            // Image first, then text.
            if let Some(png) = &m.image_png {
                content.push(json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png_b64(png)}}));
            }
            if !m.text.is_empty() {
                content.push(json!({"type": "text", "text": m.text}));
            }
            json!({"role": if m.role == Role::User { "user" } else { "assistant" }, "content": content})
        })
        .collect();

    let mut body = json!({"model": model, "max_tokens": fast.unwrap_or(MAX_TOKENS), "stream": true, "messages": turns});
    if fast.is_some() {
        if model.starts_with("claude-sonnet-5-5") {
            // Claude Sonnet 5.5 400s on `disabled`; `between_tools` is its documented "off".
            body["thinking"] = json!({"type": "between_tools"});
        } else if !model.contains("-4-5") && !model.contains("-3-") {
            // Lowest effort = least thinking. Haiku 4.5 / Sonnet 4.5 / 3.x reject
            // `effort` (and don't think unless asked), so they get nothing.
            body["output_config"] = json!({"effort": "low"});
        }
    }
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    Ok(HttpRequest {
        url,
        headers: vec![
            ("x-api-key", key.expose().to_string()),
            ("anthropic-version", ANTHROPIC_API_VERSION.to_string()),
            ("content-type", "application/json".to_string()),
        ],
        body,
    })
}

/// One SSE `data:` payload. Only text deltas matter; thinking deltas, pings and
/// block markers are skipped. `stop_reason: refusal` and `error` events fail.
pub(crate) fn parse_event(data: &str) -> Result<Event, AiError> {
    let v: Value = serde_json::from_str(data)
        .map_err(|_| AiError::BadResponse("unreadable stream event".into()))?;
    match v["type"].as_str() {
        Some("content_block_delta") if v["delta"]["type"] == "text_delta" => Ok(Event::Delta(
            v["delta"]["text"].as_str().unwrap_or_default().to_string(),
        )),
        Some("message_delta") if v["delta"]["stop_reason"] == "refusal" => Err(AiError::Refused),
        Some("message_stop") => Ok(Event::Done),
        Some("error") => {
            let why = v["error"]["message"].as_str().unwrap_or("unknown error");
            // overloaded_error arrives mid-stream as an event, not as an HTTP status.
            Err(if v["error"]["type"] == "overloaded_error" {
                AiError::Server(529)
            } else {
                AiError::BadResponse(super::scrub(why, None))
            })
        }
        _ => Ok(Event::Skip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_KEY: &str = "sk-ant-FAKE-000000";

    fn cfg() -> AiConfig {
        AiConfig::default()
    }

    // Recorded shape of a streamed answer (abridged).
    const STREAM: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-sonnet-5-5","stop_reason":null,"usage":{"input_tokens":12,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: ping
data: {"type":"ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo مرحبا"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}

event: message_stop
data: {"type":"message_stop"}

"#;

    fn collect(stream: &str) -> Result<Vec<String>, AiError> {
        let mut out = Vec::new();
        let mut err = None;
        super::super::read_sse(stream.as_bytes(), |d| match parse_event(d) {
            Ok(Event::Delta(t)) => {
                out.push(t);
                true
            }
            Ok(Event::Skip) => true,
            Ok(Event::Done) => false,
            Err(e) => {
                err = Some(e);
                false
            }
        })
        .unwrap();
        err.map_or(Ok(out), Err)
    }

    #[test]
    fn request_shape_text_only() {
        let msgs = [
            Message::new(Role::System, "Be brief."),
            Message::user("What is this?"),
        ];
        let req = build_request(&cfg(), Some(&Secret::new(FAKE_KEY)), &msgs, None).unwrap();
        assert_eq!(req.url, "https://api.anthropic.com/v1/messages");
        let h = |n: &str| {
            req.headers
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(h("x-api-key"), Some(FAKE_KEY));
        assert_eq!(h("anthropic-version"), Some("2026-10-21"));
        assert_eq!(
            req.body,
            json!({
                "model": "claude-sonnet-5-5",
                "max_tokens": 16000,
                "stream": true,
                "system": "Be brief.",
                "messages": [{"role": "user", "content": [{"type": "text", "text": "What is this?"}]}]
            })
        );
        // No sampling parameters (rejected by API version 2026-10-21).
        for k in ["temperature", "top_p", "top_k"] {
            assert!(req.body.get(k).is_none());
        }
    }

    #[test]
    fn request_shape_with_image_and_history() {
        let msgs = [
            Message::user("Explain").with_image(vec![0x89, b'P', b'N', b'G']),
            Message::new(Role::Assistant, "It is a map."),
            Message::user("And the legend?"),
        ];
        let req = build_request(&cfg(), Some(&Secret::new(FAKE_KEY)), &msgs, None).unwrap();
        let m = &req.body["messages"];
        assert_eq!(
            m[0]["content"][0],
            json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw=="}})
        );
        assert_eq!(
            m[0]["content"][1],
            json!({"type": "text", "text": "Explain"})
        );
        assert_eq!(m[1]["role"], "assistant");
        assert_eq!(m[2]["content"][0]["text"], "And the legend?");
        assert!(req.body.get("system").is_none());
    }

    fn fast_body(model: &str) -> Value {
        let mut c = cfg();
        c.model = model.into();
        build_request(
            &c,
            Some(&Secret::new(FAKE_KEY)),
            &[Message::user("x")],
            Some(300),
        )
        .unwrap()
        .body
    }

    #[test]
    fn translate_request_caps_answer_and_minimises_thinking() {
        // Default model (Sonnet 5.5): thinking off via `between_tools`.
        let b = build_request(&cfg(), Some(&Secret::new(FAKE_KEY)), &[], Some(300))
            .unwrap()
            .body;
        assert_eq!(b["max_tokens"], 300);
        assert_eq!(b["thinking"], json!({"type": "between_tools"}));
        assert!(b.get("output_config").is_none());
        // Other current models: lowest effort, no thinking block.
        let b = fast_body("claude-haiku-5-5");
        assert_eq!(b["max_tokens"], 300);
        assert_eq!(b["output_config"], json!({"effort": "low"}));
        assert!(b.get("thinking").is_none());
        // Models that reject `effort`.
        let b = fast_body("claude-haiku-4-5");
        assert!(b.get("thinking").is_none() && b.get("output_config").is_none());
        // Never sampling parameters.
        for m in ["claude-sonnet-5-5", "claude-haiku-5-5", "claude-haiku-4-5"] {
            for k in ["temperature", "top_p", "top_k"] {
                assert!(fast_body(m).get(k).is_none());
            }
        }
    }

    #[test]
    fn ask_request_is_unchanged_by_translate_options() {
        let b = build_request(&cfg(), Some(&Secret::new(FAKE_KEY)), &[], None)
            .unwrap()
            .body;
        assert_eq!(b["max_tokens"], 16000);
        assert!(b.get("thinking").is_none() && b.get("output_config").is_none());
    }

    #[test]
    fn custom_base_url_and_model() {
        let mut c = cfg();
        c.base_url = "https://proxy.example.com/v1/".into();
        c.model = "claude-haiku-5-5".into();
        let req = build_request(
            &c,
            Some(&Secret::new(FAKE_KEY)),
            &[Message::user("x")],
            None,
        )
        .unwrap();
        assert_eq!(req.url, "https://proxy.example.com/v1/messages");
        assert_eq!(req.body["model"], "claude-haiku-5-5");
        c.model.clear();
        assert_eq!(
            build_request(&c, Some(&Secret::new(FAKE_KEY)), &[], None)
                .unwrap()
                .body["model"],
            DEFAULT_ANTHROPIC_MODEL
        );
    }

    #[test]
    fn key_is_never_sent_over_plain_http_to_a_remote_host() {
        let key = Secret::new(FAKE_KEY);
        let mut c = cfg();
        for remote in [
            "http://example.com",
            "HTTP://example.com/v1",
            "http://127.evil.com",
            "ftp://x",
        ] {
            c.base_url = remote.into();
            let err = build_request(&c, Some(&key), &[], None)
                .err()
                .expect(remote);
            assert!(matches!(err, AiError::NotConfigured(_)), "{remote}");
            assert!(!err.to_string().contains(FAKE_KEY));
        }
        for ok in [
            "https://api.anthropic.com",
            "http://127.0.0.1:1234",
            "http://[::1]:11434",
        ] {
            c.base_url = ok.into();
            assert!(build_request(&c, Some(&key), &[], None).is_ok(), "{ok}");
        }
    }

    #[test]
    fn missing_key_is_a_setup_error_not_a_request() {
        for key in [None, Some(&Secret::new(""))] {
            assert!(matches!(
                build_request(&cfg(), key, &[Message::user("x")], None),
                Err(AiError::NotConfigured(_))
            ));
        }
    }

    #[test]
    fn stream_text_deltas_are_collected() {
        assert_eq!(collect(STREAM).unwrap(), ["Hel", "lo مرحبا"]);
    }

    #[test]
    fn stream_refusal_and_error_events_fail() {
        let refusal = r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"}}"#;
        assert_eq!(
            collect(&format!("{refusal}\n\n")).unwrap_err(),
            AiError::Refused
        );
        let overloaded =
            r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert_eq!(
            collect(&format!("{overloaded}\n\n")).unwrap_err(),
            AiError::Server(529)
        );
        let bad = r#"data: {"type":"error","error":{"type":"invalid_request_error","message":"max_tokens too big"}}"#;
        assert_eq!(
            collect(&format!("{bad}\n\n")).unwrap_err(),
            AiError::BadResponse("max_tokens too big".into())
        );
        assert!(matches!(
            parse_event("not json"),
            Err(AiError::BadResponse(_))
        ));
    }

    #[test]
    fn thinking_deltas_are_not_shown() {
        let ev = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#;
        assert!(matches!(parse_event(ev), Ok(Event::Skip)));
    }
}
