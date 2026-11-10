//! The Model list of Settings > Ask AI: a built-in list of each provider's
//! current chat models (shown at once, offline), plus what the provider's own
//! list-models endpoint answers when the user clicks "Load models" (with the
//! user's key, only then; kept in memory for the session). A model may carry
//! its PROVIDER's price: "Free tier at Google", "Paid at OpenAI", only where
//! that provider's own pricing page (or, for OpenRouter, its list) says so;
//! unknown = no tag. OpenCircle itself has nothing paid.
use std::sync::Mutex;

use serde_json::Value;

use super::providers::{Api, Preset};
use super::{AiError, HttpRequest, join_url, map_status};
use crate::config::AiConfig;
use crate::secrets::Secret;

/// What the provider charges for a model, by its own word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Price {
    Free,
    Paid,
}

/// A row of the list: the id sent to the provider and its price tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub tag: Option<String>,
}

use Price::{Free, Paid};

/// Each provider's current chat models, checked against its own pages on
/// 2026-11-10 ("-latest" aliases where it has them). Prices only where that
/// page says so.
fn builtin(preset: &str) -> &'static [(&'static str, Option<Price>)] {
    match preset {
        // https://openrouter.ai/api/v1/models (each model's `pricing`; ":free" = 0)
        // ("openrouter/free" = the Free Models Router: picks a free model, price 0;
        // "openrouter/auto" = the paid router)
        "openrouter" => &[
            ("openrouter/free", Some(Free)),
            ("nvidia/nemotron-3-ultra-550b-a55b:free", Some(Free)),
            ("nvidia/nemotron-3.5-super-vl:free", Some(Free)),
            ("google/gemma-4-31b-it:free", Some(Free)),
            ("anthropic/claude-haiku-5.5", Some(Paid)),
            ("anthropic/claude-sonnet-5.5", Some(Paid)),
            ("openai/gpt-6-luna", Some(Paid)),
            ("google/gemini-3.8-flash", Some(Paid)),
            ("openrouter/auto", Some(Paid)),
        ],
        // https://developers.openai.com/api/docs/models and .../docs/pricing
        // (every chat model priced per token; no free chat model)
        "openai" => &[
            ("gpt-4o-mini", Some(Paid)),
            ("gpt-6-luna", Some(Paid)),
            ("gpt-6.1-terra", Some(Paid)),
            ("gpt-6.1-sol", Some(Paid)),
            ("gpt-6-astra", Some(Paid)),
            ("gpt-5.4-mini", Some(Paid)),
        ],
        // https://platform.claude.com/docs/en/about-claude/models/overview (prices per MTok)
        "anthropic" => &[
            ("claude-sonnet-5-5", Some(Paid)),
            ("claude-haiku-5-5", Some(Paid)),
            ("claude-opus-5-5", Some(Paid)),
            ("claude-fable-5-2", Some(Paid)),
        ],
        // https://aistudio.google.com/docs/models and .../docs/pricing ("Free Tier":
        // "Free of charge" = Free, "Not available" = Paid); the -latest aliases
        // are in Google's list but not on its pricing page: no tag.
        "gemini" => &[
            ("gemini-flash-lite-latest", None),
            ("gemini-flash-latest", None),
            ("gemini-pro-latest", None),
            ("gemini-3.8-flash", Some(Free)),
            ("gemini-3.5-flash-lite", Some(Free)),
            ("gemini-3.1-flash-lite", Some(Free)),
            ("gemini-3-flash-preview", Some(Free)),
            ("gemini-4-argon", Some(Paid)),
            ("gemini-3.1-pro-preview", Some(Paid)),
        ],
        // https://console.groq.com/docs/models, /docs/vision (qwen3.8 reads images);
        // its free plan names no models: no tag
        "groq" => &[
            ("qwen/qwen3.8-27b", None),
            ("openai/gpt-oss-120b", None),
            ("openai/gpt-oss-20b", None),
            ("llama-3.3-70b-versatile", None),
            ("llama-3.1-8b-instant", None),
        ],
        // https://docs.mistral.ai/getting-started/models/ (its pages give no prices per tier)
        "mistral" => &[
            ("mistral-small-latest", None),
            ("mistral-large-latest", None),
        ],
        // https://api-docs.deepseek.com/quick_start/pricing (no free tier)
        "deepseek" => &[
            ("deepseek-flash", Some(Paid)),
            ("deepseek-v4-pro", Some(Paid)),
        ],
        // https://docs.x.ai/docs/models (every model priced per token)
        "xai" => &[
            ("grok-4.3", Some(Paid)),
            ("grok-4.7", Some(Paid)),
            ("grok-4.6", Some(Paid)),
        ],
        // a local server's models are the user's own: "Load models" lists them
        "ollama" => &[("llama3.2-vision", None)],
        _ => &[],
    }
}

/// Who sets the prices, as the tags say it.
fn owner(preset: &Preset) -> &'static str {
    match preset.id {
        "gemini" => "Google",
        _ => preset.name,
    }
}

fn tag(preset: &Preset, price: Price) -> String {
    match (price, preset.id) {
        (Free, "openrouter") => "Free at OpenRouter".into(),
        (Free, _) => format!("Free tier at {}", owner(preset)),
        (Paid, _) => format!("Paid at {}", owner(preset)),
    }
}

/// The list's first line, when any row has a tag.
pub fn pricing_note(preset: &Preset) -> String {
    format!("Pricing is set by {}", owner(preset))
}

/// What "Load models" brought, per provider (and a Custom server's URL).
static LOADED: Mutex<Vec<(String, Vec<Model>)>> = Mutex::new(Vec::new());

fn slot(preset: &Preset, cfg: &AiConfig) -> String {
    format!("{} {}", preset.id, cfg.base_url.trim_end_matches('/'))
}

/// The list as shown: the built-in models first, then the loaded ones not
/// among them (A-Z); a loaded price wins over the built-in one.
pub fn known(preset: &Preset, cfg: &AiConfig) -> Vec<Model> {
    let loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    let loaded = (loaded.iter())
        .find(|(s, _)| *s == slot(preset, cfg))
        .map_or(&[][..], |(_, m)| m.as_slice());
    merge(preset, loaded)
}

fn merge(preset: &Preset, loaded: &[Model]) -> Vec<Model> {
    let mut out: Vec<Model> = (builtin(preset.id).iter())
        .map(|(id, price)| {
            let tag = loaded
                .iter()
                .find(|m| m.id == *id)
                .and_then(|m| m.tag.clone());
            Model {
                id: id.to_string(),
                tag: tag.or_else(|| price.map(|p| self::tag(preset, p))),
            }
        })
        .collect();
    let mut more: Vec<Model> = (loaded.iter())
        .filter(|m| out.iter().all(|b| b.id != m.id))
        .cloned()
        .collect();
    more.sort_by(|a, b| a.id.cmp(&b.id));
    out.extend(more);
    out
}

/// "Load models from <provider>": asks its list-models endpoint (the user
/// clicked; never before) and keeps the answer for the session. The number
/// of models it listed.
pub fn load(cfg: &AiConfig, key: Option<&Secret>) -> Result<usize, AiError> {
    let answer = fetch(cfg, key);
    if let Err(e) = &answer {
        super::logged(cfg, e);
    }
    let (preset, models) = answer?;
    let n = models.len();
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    let at = slot(&preset, cfg);
    loaded.retain(|(s, _)| *s != at);
    loaded.push((at, models));
    Ok(n)
}

fn fetch(cfg: &AiConfig, key: Option<&Secret>) -> Result<(Preset, Vec<Model>), AiError> {
    let preset = Preset::of(cfg)
        .ok_or_else(|| AiError::NotConfigured("choose an AI provider first".into()))?;
    let req = request(&preset, cfg, key)?;
    let mut resp = super::send(key, |agent| {
        let mut get = agent.get(&req.url);
        for (name, value) in &req.headers {
            get = get.header(*name, value.as_str());
        }
        get.call()
    })?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    if status >= 400 {
        return Err(map_status(status, &body, key));
    }
    Ok((preset, parse(&preset, &body)?))
}

/// The list-models request: `GET {base}/models` (OpenAI-compatible; the key
/// as a Bearer when there is one) or Anthropic's `GET /v1/models` with its
/// own headers. Not `Debug`: the headers hold the key.
pub(crate) fn request(
    preset: &Preset,
    cfg: &AiConfig,
    key: Option<&Secret>,
) -> Result<HttpRequest, AiError> {
    let key = key.filter(|k| !k.is_empty());
    let mut headers = Vec::new();
    let url = match preset.api {
        Api::Anthropic => {
            let key =
                key.ok_or_else(|| AiError::NotConfigured("add your Anthropic API key".into()))?;
            let base = match cfg.base_url.as_str() {
                "" => "https://api.anthropic.com",
                b => b,
            };
            super::check_endpoint(base, true)?;
            headers.push(("x-api-key", key.expose().to_string()));
            headers.push(("anthropic-version", super::ANTHROPIC_API_VERSION.into()));
            let base = base.trim_end_matches('/').trim_end_matches("/v1");
            join_url(base, "/v1/models?limit=1000")
        }
        Api::OpenAi => {
            if cfg.base_url.is_empty() {
                return Err(AiError::NotConfigured("set the server URL first".into()));
            }
            super::check_endpoint(&cfg.base_url, key.is_some())?;
            if let Some(k) = key {
                headers.push(("authorization", format!("Bearer {}", k.expose())));
            }
            join_url(&cfg.base_url, "/models")
        }
    };
    Ok(HttpRequest {
        url,
        headers,
        body: Value::Null,
    })
}

/// `{"data": [{"id": ...}, ...]}` (every provider here). Left out where the
/// provider says a model can't chat: OpenRouter's `output_modalities`
/// without text, Mistral's `capabilities.completion_chat: false`. Google
/// writes ids as `models/<id>`; the id alone is what chat takes too.
fn parse(preset: &Preset, body: &str) -> Result<Vec<Model>, AiError> {
    let bad = || AiError::BadResponse("the model list was not readable".into());
    let v: Value = serde_json::from_str(body).map_err(|_| bad())?;
    let data = v["data"].as_array().ok_or_else(bad)?;
    let mut out = Vec::new();
    for m in data {
        let Some(id) = m["id"].as_str() else { continue };
        let id = id.strip_prefix("models/").unwrap_or(id);
        let outputs = m
            .pointer("/architecture/output_modalities")
            .and_then(Value::as_array);
        if outputs.is_some_and(|o| !o.iter().any(|x| x == "text")) {
            continue;
        }
        if m.pointer("/capabilities/completion_chat") == Some(&Value::Bool(false)) {
            continue;
        }
        // OpenRouter's own prices (USD per token, as strings)
        let price = |k: &str| m["pricing"][k].as_str().and_then(|p| p.parse::<f64>().ok());
        let tag = match (price("prompt"), price("completion")) {
            (Some(a), Some(b)) => Some(tag(preset, if a == 0.0 && b == 0.0 { Free } else { Paid })),
            _ => None,
        };
        if out.iter().all(|o: &Model| o.id != id) {
            out.push(Model {
                id: id.to_string(),
                tag,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::mock;
    use super::super::providers::{ANTHROPIC, PRESETS};
    use super::*;
    use crate::config::Provider;

    fn preset(id: &str) -> Preset {
        *PRESETS.iter().find(|p| p.id == id).unwrap()
    }

    fn custom(url: &str) -> AiConfig {
        AiConfig {
            provider: Provider::OpenaiCompatible,
            base_url: url.into(),
            model: String::new(),
        }
    }

    /// Every provider's default model is in its own list, and Gemini's is the
    /// one Google still serves (2.0 Flash answered 404 "no longer available").
    #[test]
    fn defaults_are_current_and_listed() {
        assert_eq!(preset("gemini").model, "gemini-flash-lite-latest");
        for p in PRESETS {
            if !p.model.is_empty() && p.id != "lmstudio" {
                assert_eq!(builtin(p.id)[0].0, p.model, "{}", p.name);
            }
        }
    }

    /// The built-in list shows offline; a loaded list adds what it has (A-Z,
    /// after the built-in ones) and its prices win; the typed id is the
    /// caller's (never dropped by a merge).
    #[test]
    fn a_loaded_list_merges_with_the_built_in_one() {
        let or = preset("openrouter");
        let loaded = vec![
            Model {
                id: "zeta/m".into(),
                tag: Some(tag(&or, Paid)),
            },
            Model {
                id: "anthropic/claude-haiku-5.5".into(),
                tag: Some(tag(&or, Free)),
            },
            Model {
                id: "alpha/m:free".into(),
                tag: None,
            },
        ];
        let list = merge(&or, &loaded);
        let ids: Vec<_> = list.iter().map(|m| m.id.as_str()).collect();
        let n = builtin("openrouter").len();
        assert_eq!(ids.len(), n + 2);
        assert_eq!(ids[0], or.model);
        assert_eq!(ids[0], "openrouter/free");
        assert_eq!(
            merge(&or, &[])[0].tag.as_deref(),
            Some("Free at OpenRouter")
        );
        assert!(ids.contains(&"openrouter/auto"));
        assert_eq!(&ids[n..], ["alpha/m:free", "zeta/m"]);
        let haiku = list
            .iter()
            .find(|m| m.id == "anthropic/claude-haiku-5.5")
            .unwrap();
        assert_eq!(haiku.tag.as_deref(), Some("Free at OpenRouter"));
        assert_eq!(merge(&or, &[]).len(), n);
    }

    /// Tags name the provider as the one who charges, never OpenCircle; no
    /// "Pro" / "Premium" wording; unknown = none.
    #[test]
    fn price_tags_come_from_the_provider() {
        let gemini = merge(&preset("gemini"), &[]);
        assert_eq!(gemini[0].tag, None); // the alias: not on Google's pricing page
        let flash = gemini.iter().find(|m| m.id == "gemini-3.8-flash").unwrap();
        assert_eq!(flash.tag.as_deref(), Some("Free tier at Google"));
        assert_eq!(pricing_note(&preset("gemini")), "Pricing is set by Google");
        assert_eq!(
            merge(&preset("openai"), &[])[0].tag.as_deref(),
            Some("Paid at OpenAI")
        );
        for p in PRESETS {
            for m in merge(p, &[]) {
                let t = m.tag.unwrap_or_default().to_lowercase();
                assert!(!t.contains("pro") && !t.contains("premium") && !t.contains("opencircle"));
            }
        }
        assert!(merge(&preset("groq"), &[]).iter().all(|m| m.tag.is_none()));
    }

    #[test]
    fn the_list_drops_what_cant_chat_and_reads_openrouter_prices() {
        let or = preset("openrouter");
        let body = r#"{"data":[
            {"id":"a/free:free","pricing":{"prompt":"0","completion":"0"},"architecture":{"output_modalities":["text"]}},
            {"id":"b/paid","pricing":{"prompt":"0.000001","completion":"0.000002"}},
            {"id":"c/image","architecture":{"output_modalities":["image"]}},
            {"id":"embed","capabilities":{"completion_chat":false}},
            {"id":"models/gemini-x"},
            {"id":"models/gemini-x"}
        ]}"#;
        let got = parse(&or, body).unwrap();
        let ids: Vec<_> = got.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["a/free:free", "b/paid", "gemini-x"]);
        assert_eq!(got[0].tag.as_deref(), Some("Free at OpenRouter"));
        assert_eq!(got[1].tag.as_deref(), Some("Paid at OpenRouter"));
        assert_eq!(got[2].tag, None);
        assert!(parse(&or, "<html>").is_err());
    }

    /// Anthropic's list: its own path and headers (the key, the version), no
    /// Bearer; an OpenAI-compatible list: `{base}/models` with the Bearer.
    #[test]
    fn list_requests_carry_each_providers_headers() {
        let key = super::super::tests::fake_key();
        let req = request(&ANTHROPIC, &AiConfig::default(), Some(&key)).unwrap();
        assert_eq!(req.url, "https://api.anthropic.com/v1/models?limit=1000");
        assert!(
            req.headers
                .contains(&("x-api-key", key.expose().to_string()))
        );
        assert!(req.headers.contains(&(
            "anthropic-version",
            super::super::ANTHROPIC_API_VERSION.to_string()
        )));
        assert!(req.headers.iter().all(|(h, _)| *h != "authorization"));
        assert!(request(&ANTHROPIC, &AiConfig::default(), None).is_err());
        let gemini = preset("gemini");
        let cfg = custom(gemini.base_url);
        let req = request(&gemini, &cfg, Some(&key)).unwrap();
        assert_eq!(
            req.url,
            "https://generativelanguage.googleapis.com/v1beta/openai/models"
        );
        assert!(
            req.headers
                .contains(&("authorization", format!("Bearer {}", key.expose())))
        );
        // a key never goes to a remote host over plain http
        assert!(request(&gemini, &custom("http://example.com/v1"), Some(&key)).is_err());
    }

    /// A click loads the server's list (a GET of /models) and the list shows it
    /// at once after; a failure says the provider's reason.
    #[test]
    fn load_asks_the_server_and_keeps_its_list() {
        let body = r#"{"object":"list","data":[{"id":"w45-local-a"},{"id":"w45-local-b"}]}"#;
        let (url, seen) = mock::serve(vec![mock::reply(200, body)]);
        let cfg = custom(&url);
        assert!(known(&super::super::providers::CUSTOM, &cfg).is_empty());
        assert_eq!(load(&cfg, None).unwrap(), 2);
        let head = seen.try_recv().unwrap();
        assert!(head.starts_with("GET /v1/models "), "{head}");
        let ids: Vec<_> = known(&super::super::providers::CUSTOM, &cfg)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["w45-local-a", "w45-local-b"]);
        let (url, _) = mock::serve(vec![mock::reply(401, r#"{"error":{"message":"bad key"}}"#)]);
        let err = load(&custom(&url), None).unwrap_err();
        assert!(err.to_string().contains("HTTP 401: bad key"), "{err}");
    }
}
