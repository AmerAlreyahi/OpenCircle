//! Translate: the text in the selection (or the whole screen, Alt+Shift+T) is
//! replaced by its translation in place. `crate::features::translate` does the work,
//! `features/translate/screens/pill/` shows it, `features/live_translate/screens/live_layer.rs` repeats it (live mode).
use crate::features::ask_ai::ask;
use crate::features::circle::actions::{Action, Key, Run};
use crate::features::text_reading::models;
use crate::features::translate::lang;
use crate::features::translate::mt;
use crate::features::translate::pipeline::{Real, Via};
use crate::shared::app_language::{S, fmt, language_name, tr};
use crate::shared::config::{self, AiConfig, Engine, Provider};
use crate::shared::secrets::Secret;

pub const ACTION: Action = Action {
    id: "translate",
    label: S::act_translate,
    key: Key {
        ctrl: false,
        letter: 'r',
    },
    icon: '\u{e103}', // lucide "languages"
    run: Run::Translate,
};

/// The pipeline's OCR + translation engine and the target for `source` -> `target`;
/// `one_shot` = Translate (Live never goes online). Blocking (keyring): call off
/// the UI thread. `Err` = what to tell the user: its first line is the short reason.
pub fn deps_for(source: &str, target: &str, one_shot: bool) -> Result<(Real, String), String> {
    let cfg = config::current();
    let via = choose(
        engine(&cfg, one_shot),
        &cfg.ai.provider,
        source,
        target,
        ask::credentials,
    )?;
    Ok((Real::new(source, via), target.to_string()))
}

/// Why `translate_text` gave no translation.
#[derive(Debug, Clone)]
pub enum TextError {
    /// The offline pair's models are not on disk: the user may download them (`Model`).
    Missing(Model),
    /// What to tell the user (its first line is the short reason).
    Failed(String),
}

/// An offline translation pair that can be downloaded: its size and the download.
#[derive(Debug, Clone, Copy)]
pub struct Model(&'static mt::Pair);

impl Model {
    /// The pair `source` -> `target`, if there is one.
    #[cfg(test)]
    pub fn of(source: &str, target: &str) -> Option<Model> {
        mt::pair(source, target).map(Model)
    }

    /// The whole pair's size (what the download card says).
    pub fn bytes(&self) -> u64 {
        self.0.full_size()
    }

    /// Downloads the pair's missing files (blocking; `progress(done, total)`); `Err` = why not.
    pub fn download(&self, progress: impl FnMut(u64, u64)) -> Result<(), String> {
        self.0
            .ensure(progress)
            .map_err(|e| crate::features::text_reading::model_download::why(&e))
    }
}

/// `text` (the right-click menu's selected words) translated into `target` with the configured
/// engine: offline by default, the AI provider or the online service as chosen. Blocking.
/// `Err` = what to tell the user. Text already in the target comes back as it is.
pub fn translate_text(text: &str, source: &str, target: &str) -> Result<String, TextError> {
    use crate::features::translate::Block;
    use crate::features::translate::pipeline::{Cache, Deps, translate_blocks};
    let (real, target) = deps_for(source, target, true).map_err(TextError::Failed)?;
    real.read(&[text]);
    let block = Block {
        lines: Vec::new(),
        text: text.split_whitespace().collect::<Vec<_>>().join(" "),
        bounds: [0.0; 4],
    };
    let out = translate_blocks(&[block], &target, &Cache::new(8), &real).map_err(|e| match real
        .missing()
    {
        Some(pair) => TextError::Missing(Model(pair)),
        None => TextError::Failed(e.to_string()),
    })?;
    Ok(out
        .into_iter()
        .next()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| text.to_string()))
}

/// The engine, without the keyring: `Some` when translation will surely be
/// offline (chosen, or the browser provider, which can't translate), with
/// the model it will most likely need next (`mt::next`; English for
/// "auto"). With an API
/// provider whose key is missing, only `deps` finds out.
pub fn surely_offline(source: &str, target: &str) -> Option<&'static mt::Pair> {
    let source = if source == lang::AUTO { "en" } else { source };
    will_be_offline()
        .then(|| mt::next(source, target))
        .flatten()
}

/// What translating `source` -> `target` must download before it can
/// run (sizes only): the offline pair's files with OCR's when translation
/// will surely be offline, else OCR's for the source. (bytes, what it is,
/// the pair to `Pair::ensure` or `None` = OCR's only); `None` = ready.
pub fn missing(source: &str, target: &str) -> Option<(u64, String, Option<&'static mt::Pair>)> {
    if let Some(pair) = surely_offline(source, target) {
        let bytes = pair.missing_bytes();
        if bytes > 0 {
            let what = fmt(
                S::lc_what_pair,
                &[&language_name(pair.source), &language_name(pair.target)],
            );
            return Some((bytes, what, Some(pair)));
        }
    }
    let rec = models::recognizer_for(source);
    let bytes = models::missing_bytes(&models::ocr_files(rec));
    let script = match rec.script {
        "Arabic" => S::sc_arabic,
        "Cyrillic" => S::sc_cyrillic,
        "Chinese and Japanese" => S::sc_cjk,
        _ => S::sc_latin,
    };
    let what = fmt(S::lc_what_ocr, &[&tr(script)]);
    (bytes > 0).then_some((bytes, what, None))
}

/// Downloads what `missing` found, after the user agreed. Blocking.
pub fn download(
    source: &str,
    pair: Option<&'static mt::Pair>,
    progress: impl FnMut(u64, u64),
) -> Result<(), crate::features::text_reading::OcrError> {
    match pair {
        Some(pair) => pair.ensure(progress),
        None => models::ensure(&models::ocr_files(models::recognizer_for(source)), progress),
    }
}

/// The readers for other scripts (Arabic, Cyrillic, Chinese and Japanese)
/// that aren't on disk, for a page read as noise.
pub fn download_readers(
    progress: impl FnMut(u64, u64),
) -> Result<(), crate::features::text_reading::OcrError> {
    models::ensure(&models::other_scripts_files(), progress)
}

/// What `download_readers` fetches (bytes).
pub fn readers_bytes() -> u64 {
    models::missing_bytes(&models::other_scripts_files())
}

/// The engine in effect: Online only for one-shot Translate; Live uses Offline.
fn engine(cfg: &config::Config, one_shot: bool) -> Engine {
    match cfg.translate.engine {
        Engine::Online if one_shot => Engine::Online,
        Engine::Online => Engine::Offline,
        e => e,
    }
}

/// Offline is chosen, or the provider is the browser (which can't translate).
/// (One-shot Translate's view: online is not offline.)
pub fn will_be_offline() -> bool {
    let cfg = config::current();
    match engine(&cfg, true) {
        Engine::Offline => true,
        Engine::Online => false,
        Engine::Ai => cfg.ai.provider == Provider::Browser,
    }
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
    if engine == Engine::Online {
        return Ok(Via::Online);
    }
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
            Ok(Via::Online) => "online",
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
        let e = choose(Engine::Ai, &browser, "fr", "ko", no_key)
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
            e.starts_with("There is no local model from French into Japanese"),
            "{e}"
        );
        assert!(e.contains("Choose AI"), "offers AI: {e}");
        // Online: any pair, and the keyring isn't read
        assert_eq!(
            engine(choose(Engine::Online, &anthropic, "ko", "th", never)),
            "online"
        );
    }

    /// Online is one-shot Translate's (no Labs switch since 0.3.0).
    #[test]
    fn online_is_one_shot_only() {
        let mut cfg = config::Config::default();
        cfg.translate.engine = Engine::Online;
        assert_eq!(super::engine(&cfg, true), Engine::Online);
        assert_eq!(super::engine(&cfg, false), Engine::Offline, "never Live");
        cfg.translate.engine = Engine::Ai;
        assert_eq!(super::engine(&cfg, true), Engine::Ai);
    }
}
