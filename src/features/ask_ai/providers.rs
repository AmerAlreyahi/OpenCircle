//! The AI providers by name (the Settings Provider list): which API each
//! speaks, its server, a suggested model, and its own keyring entry. The
//! stored `[ai]` (provider + base_url) maps back to one (`Preset::of`), so a
//! request and its key always come from the same provider.
use std::fmt;

use crate::shared::config::{AiConfig, Provider};

/// The API a provider speaks: chosen here, never shown (owner, 2026-10-28:
/// protocol names mislead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    /// The Anthropic Messages API (`provider = "anthropic"`).
    Anthropic,
    /// The OpenAI-compatible client (`provider = "openai_compatible"`).
    OpenAi,
}

/// An AI provider in the Provider dropdown, by its real name: what it writes
/// to `[ai]` (the config format is unchanged: `provider` and `base_url`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// Its own keyring entry (`ai-key-<id>`): a key is only ever sent to the
    /// provider it was saved for.
    pub id: &'static str,
    pub name: &'static str,
    pub api: Api,
    /// "" for Anthropic (its default) and for Custom server (typed by the user).
    pub base_url: &'static str,
    /// Suggested model, used only while the model field is empty.
    pub model: &'static str,
    /// Runs on this machine: no API key needed.
    pub local: bool,
}

impl Preset {
    /// The provider this preset writes.
    pub fn provider(&self) -> Provider {
        match self.api {
            Api::Anthropic => Provider::Anthropic,
            Api::OpenAi => Provider::OpenaiCompatible,
        }
    }

    /// Custom server: the one that shows the Server URL row.
    pub fn custom(&self) -> bool {
        *self == CUSTOM
    }

    /// The preset a stored `[ai]` shows as (`None`: the browser, or a value
    /// the file can't give): Anthropic; an OpenAI-compatible server by its
    /// URL (a trailing `/` or the letter case aside); any other URL is a
    /// Custom server.
    pub fn of(ai: &AiConfig) -> Option<Preset> {
        let same = |a: &str, b: &str| a.trim_end_matches('/').eq_ignore_ascii_case(b);
        match ai.provider {
            Provider::Anthropic => Some(ANTHROPIC),
            Provider::OpenaiCompatible => Some(
                (PRESETS.iter())
                    .find(|p| p.api == Api::OpenAi && !p.custom() && same(&ai.base_url, p.base_url))
                    .copied()
                    .unwrap_or(CUSTOM),
            ),
            _ => None,
        }
    }
}

/// Its name as the lists show it, in the app's language: a service's name
/// stays as it is ("Ollama (local)" adds the word), Custom server is words.
impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::shared::app_language::{S, fmt, tr};
        match (self.custom(), self.local) {
            (true, _) => f.write_str(tr(S::prov_custom)),
            (false, true) => f.write_str(&fmt(S::prov_local, &[&self.name])),
            _ => f.write_str(self.name),
        }
    }
}

const fn openai(
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    model: &'static str,
) -> Preset {
    Preset {
        id,
        name,
        api: Api::OpenAi,
        base_url,
        model,
        local: false,
    }
}

const fn local(
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    model: &'static str,
) -> Preset {
    Preset {
        local: true,
        ..openai(id, name, base_url, model)
    }
}

pub const ANTHROPIC: Preset = Preset {
    // the entry name it always had
    id: "anthropic",
    name: "Anthropic",
    api: Api::Anthropic,
    base_url: "",
    model: crate::features::ask_ai::DEFAULT_ANTHROPIC_MODEL,
    local: false,
};

pub const CUSTOM: Preset = openai("custom", "Custom server", "", "");

/// The Provider dropdown, in the owner's order (2026-10-28).
pub const PRESETS: &[Preset] = &[
    // Free, correct Arabic, ~3 s when measured. Fast paid option: anthropic/claude-haiku-5.5
    // (shown as the placeholder while the model field is empty).
    openai(
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        "openrouter/free",
    ),
    openai(
        "openai",
        "OpenAI",
        "https://api.openai.com/v1",
        "gpt-4o-mini",
    ),
    ANTHROPIC,
    openai(
        "gemini",
        "Google Gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "gemini-flash-lite-latest",
    ),
    openai(
        "groq",
        "Groq",
        "https://api.groq.com/openai/v1",
        "qwen/qwen3.8-27b",
    ),
    openai(
        "mistral",
        "Mistral",
        "https://api.mistral.ai/v1",
        "mistral-small-latest",
    ),
    openai(
        "deepseek",
        "DeepSeek",
        "https://api.deepseek.com/v1",
        "deepseek-flash",
    ),
    openai("xai", "xAI", "https://api.x.ai/v1", "grok-4.3"),
    local(
        "ollama",
        "Ollama",
        "http://localhost:11434/v1",
        "llama3.2-vision",
    ),
    local(
        "lmstudio",
        "LM Studio",
        "http://localhost:1234/v1",
        "local-model",
    ),
    CUSTOM,
];

/// The keyring entry of the provider `ai` sends to (`None`: the browser, or a
/// value the file can't give). Derived from the same `[ai]` the request
/// uses, so a key goes only to the provider it was saved for.
pub fn key_of(ai: &AiConfig) -> Option<crate::shared::secrets::AiKey> {
    Preset::of(ai).map(|p| crate::shared::secrets::AiKey(p.id))
}
