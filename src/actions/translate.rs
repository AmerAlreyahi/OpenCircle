//! Translate: the text in the selection (or the whole screen, Ctrl+Shift+T) is
//! replaced by its translation in place. `crate::translate` does the work,
//! `ui/translate.rs` shows it, `ui/live_layer.rs` repeats it (live mode).
use super::{Action, Key, Run, ask};
use crate::config;
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

/// The pipeline's OCR + AI from the running config and the keyring, and the
/// target language. Blocking (keyring): call off the UI thread. `Err` = what
/// to tell the user: its first line is the short reason.
pub fn deps() -> Result<(Real, String), String> {
    let cfg = config::current();
    let (ai, key) = ask::credentials()?;
    Ok((Real { ai, key }, cfg.translate.target))
}

/// The Text panel's Translate: `text` line by line into the configured
/// language (one AI call; empty lines stay empty). Blocking (keyring, HTTP).
pub fn text(text: &str) -> Result<String, String> {
    let (deps, target) = deps()?;
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let out = crate::ai::translate_lines(&deps.ai, deps.key.as_ref(), &lines, &target, "")
        .map_err(|e| e.to_string())?;
    Ok(out.join("\n"))
}
