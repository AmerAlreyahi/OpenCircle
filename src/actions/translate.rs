//! Translate: the text in the selection (or the whole screen, Ctrl+Shift+T) is
//! replaced by its translation in place. `crate::translate` does the work,
//! `ui/translate.rs` shows it, `ui/live_layer.rs` repeats it (live mode).
use super::{Action, Key, Run, ask};
use crate::config::{self, AiConfig, Engine, Provider};
use crate::mt;
use crate::secrets::Secret;
use crate::translate::lang;
use crate::translate::pipeline::{Real, Via};

pub const ACTION: Action = Action {
    id: "translate",
    label: "Translate",
    tip: "Translate the text in the selection, in place",
    key: Key {
        ctrl: false,
        letter: 'r',
    },
    icon: '\u{e103}', // lucide "languages"
    run: Run::Translate,
};

/// The pipeline's OCR + translation engine from the running config and the
/// keyring, and the target language. Blocking (keyring): call off the UI
/// thread. `Err` = what to tell the user: its first line is the short reason.
pub fn deps() -> Result<(Real, String), String> {
    let cfg = config::current();
    deps_for(&cfg.translate.source, &cfg.translate.target)
}

/// `deps` for another source / target than the config's (a picker's).
pub fn deps_for(source: &str, target: &str) -> Result<(Real, String), String> {
    let cfg = config::current();
    let via = choose(
        cfg.translate.engine,
        &cfg.ai.provider,
        source,
        target,
        ask::credentials,
    )?;
    Ok((Real::new(source, via), target.to_string()))
}

/// The engine, without the keyring: `Some` when translation will surely be
/// offline (chosen, or the browser provider, which can't translate), with
/// the pair it will most likely need (English for "auto"). With an API
/// provider whose key is missing, only `deps` finds out.
pub fn surely_offline(source: &str, target: &str) -> Option<&'static mt::Pair> {
    let source = if source == lang::AUTO { "en" } else { source };
    will_be_offline()
        .then(|| mt::pair(source, target))
        .flatten()
}

/// Offline is chosen, or the provider is the browser (which can't translate).
pub fn will_be_offline() -> bool {
    let cfg = config::current();
    cfg.translate.engine == Engine::Offline || cfg.ai.provider == Provider::Browser
}

/// Offline when chosen, or when AI isn't set up (the browser provider, no
/// key) and an offline model exists for `source` -> `target`: live and
/// one-shot translate then need no key. A configured AI provider is used as
/// is; an unknown or unreadable provider still fails closed (the user must
/// fix it). No offline model and no AI: an error that says so (nothing is
/// ever sent to an AI the user didn't choose).
fn choose(
    engine: Engine,
    provider: &Provider,
    source: &str,
    target: &str,
    credentials: impl FnOnce() -> Result<(AiConfig, Option<Secret>), String>,
) -> Result<Via, String> {
    let offline = mt::available(source, target);
    if engine == Engine::Offline {
        return match offline {
            true => Ok(Via::Offline),
            false => {
                let why = mt::unavailable(source, target);
                let mut chars = why.chars();
                let first = chars.next().map(|c| c.to_ascii_uppercase());
                Err(format!("{}{}.", first.unwrap_or_default(), chars.as_str()))
            }
        };
    }
    match credentials() {
        Ok((ai, key)) => Ok(Via::Ai { ai, key }),
        Err(e) if matches!(provider, Provider::Invalid(_) | Provider::Unreadable) => Err(e),
        Err(_) if offline => Ok(Via::Offline),
        Err(e) => Err(e),
    }
}

/// The Text panel's Translate: `text` line by line into the configured
/// language (one call; empty lines stay empty). Blocking (keyring, HTTP or
/// the helper process).
pub fn text(text: &str) -> Result<String, String> {
    let (deps, target) = deps()?;
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let out = deps.lines(&lines, &target, "").map_err(|e| e.to_string())?;
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ai() -> Result<(AiConfig, Option<Secret>), String> {
        Ok((AiConfig::default(), None))
    }

    fn no_key() -> Result<(AiConfig, Option<Secret>), String> {
        Err("AI is not set up yet: no API key is saved.".into())
    }

    fn engine(r: Result<Via, String>) -> &'static str {
        match r {
            Ok(Via::Ai { .. }) => "ai",
            Ok(Via::Offline) => "offline",
            Err(_) => "error",
        }
    }

    #[test]
    fn engine_choice() {
        let (anthropic, browser) = (Provider::Anthropic, Provider::Browser);
        let auto = lang::AUTO;
        // AI set up: AI, live and one-shot alike, any pair
        assert_eq!(engine(choose(Engine::Ai, &anthropic, auto, "ar", ai)), "ai");
        assert_eq!(engine(choose(Engine::Ai, &anthropic, "ko", "th", ai)), "ai");
        // no key / the browser provider: offline is the default (live included)
        let offline = |src, tgt| engine(choose(Engine::Ai, &browser, src, tgt, no_key));
        assert_eq!(offline(auto, "ar"), "offline");
        assert_eq!(offline(auto, "ar-SA"), "offline");
        assert_eq!(offline("fr", "en"), "offline");
        assert_eq!(offline(auto, "en"), "offline", "ar/fr/zh/ja -> en exist");
        // ... unless there is no offline model for the pair: the AI's reason
        let e = choose(Engine::Ai, &browser, "fr", "de", no_key)
            .err()
            .unwrap();
        assert!(e.starts_with("AI is not set up"), "{e}");
        assert_eq!(offline(auto, "ko"), "error");
        // a broken provider setting fails closed
        for bad in [Provider::Invalid("antropic".into()), Provider::Unreadable] {
            assert_eq!(
                engine(choose(Engine::Ai, &bad, auto, "ar", no_key)),
                "error"
            );
        }
        // Offline chosen: offline even with a key, and the keyring isn't read
        let never = || -> Result<(AiConfig, Option<Secret>), String> { panic!("keyring read") };
        assert_eq!(
            engine(choose(Engine::Offline, &anthropic, "en", "ar", never)),
            "offline"
        );
        let e = choose(Engine::Offline, &anthropic, "fr", "ja", never)
            .err()
            .unwrap();
        assert!(
            e.starts_with("There is no offline model from French into Japanese"),
            "{e}"
        );
        assert!(e.contains("Choose AI"), "offers AI: {e}");
    }
}
