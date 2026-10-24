//! Translate: the text in the selection (or the whole screen, Ctrl+Shift+T) is
//! replaced by its translation in place. `crate::translate` does the work,
//! `ui/translate.rs` shows it, `ui/live_layer.rs` repeats it (live mode).
use super::{Action, Key, Run, ask};
use crate::config::{self, AiConfig, Engine, Provider};
use crate::mt;
use crate::secrets::Secret;
use crate::translate::pipeline::Real;

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
    let target = cfg.translate.target;
    let real = choose(
        cfg.translate.engine,
        &cfg.ai.provider,
        &target,
        ask::credentials,
    )?;
    Ok((real, target))
}

/// The engine, without the keyring: `Some` when translation will surely be
/// offline (chosen, or the browser provider, which can't translate). With an
/// API provider whose key is missing, only `deps` finds out.
pub fn surely_offline() -> Option<&'static mt::Pair> {
    let cfg = config::current();
    let offline = cfg.translate.engine == Engine::Offline || cfg.ai.provider == Provider::Browser;
    offline.then(|| mt::pair(&cfg.translate.target)).flatten()
}

/// Offline when chosen, or when AI isn't set up (the browser provider, no
/// key) and an offline model exists for `target`: live and one-shot
/// translate then need no key. A configured AI provider is used as is; an
/// unknown or unreadable provider still fails closed (the user must fix it).
fn choose(
    engine: Engine,
    provider: &Provider,
    target: &str,
    credentials: impl FnOnce() -> Result<(AiConfig, Option<Secret>), String>,
) -> Result<Real, String> {
    let offline = mt::pair(target);
    if engine == Engine::Offline {
        return offline.map(Real::Offline).ok_or_else(|| {
            format!(
                "Offline translation goes from English into Arabic only for now, not \"{target}\".\n\n\
                 Pick Arabic in Settings > Translate, or choose AI as the engine."
            )
        });
    }
    match credentials() {
        Ok((ai, key)) => Ok(Real::Ai { ai, key }),
        Err(e) if matches!(provider, Provider::Invalid(_) | Provider::Unreadable) => Err(e),
        Err(e) => offline.map(Real::Offline).ok_or(e),
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

    fn engine(r: Result<Real, String>) -> &'static str {
        match r {
            Ok(Real::Ai { .. }) => "ai",
            Ok(Real::Offline(p)) => p.target,
            Err(_) => "error",
        }
    }

    #[test]
    fn engine_choice() {
        let (anthropic, browser) = (Provider::Anthropic, Provider::Browser);
        // AI set up: AI, live and one-shot alike
        assert_eq!(engine(choose(Engine::Ai, &anthropic, "ar", ai)), "ai");
        // no key / the browser provider: offline is the default (live included)
        assert_eq!(engine(choose(Engine::Ai, &anthropic, "ar", no_key)), "ar");
        assert_eq!(engine(choose(Engine::Ai, &browser, "ar-SA", no_key)), "ar");
        // ... unless there is no offline model for the target: the AI's reason
        let e = choose(Engine::Ai, &browser, "fr", no_key).err().unwrap();
        assert!(e.starts_with("AI is not set up"), "{e}");
        // a broken provider setting fails closed
        for bad in [Provider::Invalid("antropic".into()), Provider::Unreadable] {
            assert_eq!(engine(choose(Engine::Ai, &bad, "ar", no_key)), "error");
        }
        // Offline chosen: offline even with a key, and the keyring isn't read
        let never = || -> Result<(AiConfig, Option<Secret>), String> { panic!("keyring read") };
        assert_eq!(
            engine(choose(Engine::Offline, &anthropic, "ar", never)),
            "ar"
        );
        let e = choose(Engine::Offline, &anthropic, "fr", never)
            .err()
            .unwrap();
        assert!(e.contains("Arabic only") && e.contains("\"fr\""), "{e}");
    }
}
