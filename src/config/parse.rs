//! Config text -> `Config`: validated field by field, a bad value takes its default
//! and adds a warning.

use serde::de::DeserializeOwned;
use toml::{Table, Value};

use super::*;

impl Config {
    /// Validate TOML text field by field.
    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut w = Vec::new();
        let root: Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                let line = e.to_string().lines().next().unwrap_or_default().to_string();
                return (
                    Self::unreadable(),
                    vec![format!(
                        "config is not valid TOML ({line}: {}); using defaults{AI_OFF}",
                        e.message()
                    )],
                );
            }
        };
        let d = Config::default();
        let mut c = Config {
            version: VERSION,
            ..d.clone()
        };

        if let Some(v) = root.get("version") {
            match v.as_integer() {
                Some(n) if (1..=VERSION as i64).contains(&n) => {}
                Some(n) if n > VERSION as i64 => w.push(format!(
                    "version: written by a newer OpenCircle (schema {n}); unknown keys are ignored and lost on save"
                )),
                _ => w.push(format!(
                    "version: expected 1 to {VERSION}; reading it as version {VERSION}"
                )),
            }
        }
        c.theme = get(&root, "", "theme", d.theme, &mut w);
        c.widget_theme = get(&root, "", "widget_theme", d.widget_theme, &mut w);
        c.glass_effect = get(&root, "", "glass_effect", d.glass_effect, &mut w);
        c.shortcut = get_checked(&root, "", "shortcut", d.shortcut, &mut w, |s: &String| {
            parse_shortcut(s).map(drop)
        });
        // The file's schema (missing = 1): older ones are carried forward below.
        let file_version = root.get("version").map_or(Some(1), Value::as_integer);

        if let Some(t) = section(&root, "appearance", &mut w) {
            let (p, d) = ("appearance.", &d.appearance);
            let one_of = |ids: &'static [&str]| {
                move |s: &String| match ids.contains(&s.as_str()) {
                    true => Ok(()),
                    false => Err(format!("expected one of {}", ids.join(", "))),
                }
            };
            c.appearance.dark =
                get_checked(t, p, "dark", d.dark.clone(), &mut w, one_of(&DARK_THEMES));
            c.appearance.light = get_checked(
                t,
                p,
                "light",
                d.light.clone(),
                &mut w,
                one_of(&LIGHT_THEMES),
            );
            c.appearance.logo = get(t, p, "logo", d.logo, &mut w);
            c.appearance.hidden_previews = get(t, p, "hidden_previews", vec![], &mut w);
        } else if file_version < Some(4) {
            // Schema 4: the old widget theme becomes that mode's app theme.
            match c.widget_theme {
                WidgetTheme::Frost => c.appearance.light = "frost".into(),
                WidgetTheme::Graphite => c.appearance.dark = "graphite".into(),
                WidgetTheme::Default => {}
            }
        }

        if let Some(t) = section(&root, "general", &mut w) {
            c.general.default_action = get(
                t,
                "general.",
                "default_action",
                d.general.default_action,
                &mut w,
            );
            c.general.save_folder = get_checked(
                t,
                "general.",
                "save_folder",
                d.general.save_folder,
                &mut w,
                |s: &String| check_folder(s),
            );
            c.general.search_history = get(
                t,
                "general.",
                "search_history",
                d.general.search_history,
                &mut w,
            );
            c.general.autostart = get(t, "general.", "autostart", d.general.autostart, &mut w);
        }
        // `[results]` (schema 1-2: the web view card's side and size) is ignored.
        if let Some(t) = section(&root, "bar", &mut w) {
            c.bar.anchor = get(t, "bar.", "anchor", d.bar.anchor, &mut w);
        }
        if let Some(t) = section(&root, "selection", &mut w) {
            let (p, d) = ("selection.", d.selection);
            c.selection.style = get(t, p, "style", d.style, &mut w);
            c.selection.after = get(t, p, "after", d.after, &mut w);
            // Schema 3: nothing is sent before the user picks (owner, 2026-10-26).
            // An older file's "search" becomes "wait"; Settings opts back in.
            if file_version < Some(3) {
                c.selection.after = After::Wait;
            }
            c.selection.search_with = get(t, p, "search_with", d.search_with, &mut w);
            c.selection.text_reader = get(t, p, "text_reader", d.text_reader, &mut w);
        }
        if let Some(t) = section(&root, "look", &mut w) {
            c.look.theme = get(t, "look.", "theme", d.look.theme, &mut w);
            if let Some(custom) = sub_section(t, "look.", "custom", &mut w) {
                c.look.custom = read_look(custom, d.look.custom, &mut w);
            }
        }
        if let Some(t) = section(&root, "editor", &mut w) {
            c.editor.line_assist = get(t, "editor.", "line_assist", d.editor.line_assist, &mut w);
        }
        if root.get("ai").is_some_and(|v| !v.is_table()) {
            w.push(format!("ai: expected a table; using defaults{AI_OFF}"));
            c.ai.provider = Provider::Unreadable;
        } else if let Some(t) = section(&root, "ai", &mut w) {
            c.ai.provider = Provider::read(t, &mut w);
            c.ai.base_url =
                get_checked(t, "ai.", "base_url", d.ai.base_url, &mut w, |s: &String| {
                    check_base_url(s)
                });
            c.ai.model = get_checked(t, "ai.", "model", d.ai.model, &mut w, |s: &String| {
                check_model(s)
            });
        }
        if let Some(t) = section(&root, "translate", &mut w) {
            c.translate.source = get_checked(
                t,
                "translate.",
                "source",
                d.translate.source,
                &mut w,
                |s: &String| match s.as_str() {
                    crate::translate::lang::AUTO => Ok(()),
                    s => check_lang(s).map_err(|e| format!("{e}, or \"auto\"")),
                },
            );
            c.translate.target = get_checked(
                t,
                "translate.",
                "target",
                d.translate.target,
                &mut w,
                |s: &String| check_lang(s),
            );
            c.translate.engine = get(t, "translate.", "engine", d.translate.engine, &mut w);
            c.translate.live_interval_ms = get_checked(
                t,
                "translate.",
                "live_interval_ms",
                d.translate.live_interval_ms,
                &mut w,
                |n: &u32| {
                    LIVE_INTERVAL_MS.contains(n).then_some(()).ok_or_else(|| {
                        format!(
                            "expected {} to {} milliseconds",
                            LIVE_INTERVAL_MS.start(),
                            LIVE_INTERVAL_MS.end()
                        )
                    })
                },
            );
            c.translate.live_note_shown = get(t, "translate.", "live_note_shown", false, &mut w);
            c.translate.whole_screen = get(t, "translate.", "whole_screen", false, &mut w);
        }
        if let Some(t) = section(&root, "song", &mut w) {
            c.song.source = get(t, "song.", "source", d.song.source, &mut w);
            c.song.history = get(t, "song.", "history", d.song.history, &mut w);
        }
        if let Some(t) = section(&root, "chat", &mut w) {
            c.chat.stays_open = get(t, "chat.", "stays_open", d.chat.stays_open, &mut w);
            c.chat.keep_history = get(t, "chat.", "keep_history", d.chat.keep_history, &mut w);
        }
        if let Some(t) = section(&root, "update", &mut w) {
            c.update.auto = get(t, "update.", "auto", None, &mut w);
        }
        if let Some(t) = section(&root, "snip", &mut w) {
            let p = "snip.";
            c.snip.after = get(t, p, "after", d.snip.after, &mut w);
            c.snip.max_edge = get_checked(t, p, "max_edge", d.snip.max_edge, &mut w, |n: &u32| {
                (*n == 0 || MAX_EDGE.contains(n))
                    .then_some(())
                    .ok_or_else(|| {
                        format!(
                            "expected 0 (original) or {} to {} px",
                            MAX_EDGE.start(),
                            MAX_EDGE.end()
                        )
                    })
            });
            c.snip.format = get(t, p, "format", d.snip.format, &mut w);
            c.snip.save_original = get(t, p, "save_original", false, &mut w);
            c.snip.cursor = get(t, p, "cursor", false, &mut w);
            c.snip.history = get(t, p, "history", false, &mut w);
            c.snip.delay = get_checked(t, p, "delay", 0, &mut w, |n: &u8| {
                [0, 3, 5, 10]
                    .contains(n)
                    .then_some(())
                    .ok_or_else(|| "expected 0 (off), 3, 5 or 10 seconds".to_string())
            });
        }
        if let Some(t) = section(&root, "labs", &mut w) {
            let (p, d) = ("labs.", d.labs);
            c.labs.snip = get(t, p, "snip", d.snip, &mut w);
            c.labs.editor = get(t, p, "editor", d.editor, &mut w);
            c.labs.song = get(t, p, "song", d.song, &mut w);
            c.labs.history = get(t, p, "history", d.history, &mut w);
            c.labs.pins = get(t, p, "pins", d.pins, &mut w);
            c.labs.live = get(t, p, "live", d.live, &mut w);
        }
        if let Some(v) = root.get("quick_actions") {
            c.quick_actions = parse_quick_actions(v, &mut w);
        }
        // Schema 4: translate is on-device unless the user chose AI for it.
        // Before, "ai" was the default and only ever ran with an API provider
        // (the browser one fell back to on-device); kept that way, picking a
        // chat provider later would start sending page text to it.
        if file_version < Some(4)
            && !matches!(
                c.ai.provider,
                Provider::Anthropic | Provider::OpenaiCompatible
            )
        {
            c.translate.engine = Engine::Offline;
        }

        // One physical shortcut = one job: the first user of a combination keeps it.
        let mut taken: HashSet<HotKey> = HashSet::new();
        taken.insert(parse_shortcut(&c.shortcut).expect("validated above"));
        for qa in &mut c.quick_actions {
            if let Ok(hk) = parse_shortcut(&qa.shortcut)
                && !taken.insert(hk)
            {
                w.push(format!(
                    "quick_actions: \"{}\" is already used; \"{}\" is left unbound",
                    qa.shortcut, qa.action
                ));
                qa.shortcut.clear();
            }
        }
        (c, w)
    }
}

pub(super) fn section<'a>(root: &'a Table, name: &str, w: &mut Vec<String>) -> Option<&'a Table> {
    sub_section(root, "", name, w)
}

pub(super) fn sub_section<'a>(
    t: &'a Table,
    prefix: &str,
    name: &str,
    w: &mut Vec<String>,
) -> Option<&'a Table> {
    match t.get(name)? {
        Value::Table(t) => Some(t),
        _ => {
            w.push(format!("{prefix}{name}: expected a table; using defaults"));
            None
        }
    }
}

/// `n` is in `range` (`unit` names it in the warning).
pub(super) fn within(
    range: std::ops::RangeInclusive<u8>,
    unit: &'static str,
) -> impl Fn(&u8) -> Result<(), String> {
    move |n| {
        (range.contains(n))
            .then_some(())
            .ok_or_else(|| format!("expected {} to {}{unit}", range.start(), range.end()))
    }
}

pub(super) fn get<T: DeserializeOwned>(
    t: &Table,
    prefix: &str,
    key: &str,
    default: T,
    w: &mut Vec<String>,
) -> T {
    get_checked(t, prefix, key, default, w, |_| Ok(()))
}

pub(super) fn get_checked<T: DeserializeOwned>(
    t: &Table,
    prefix: &str,
    key: &str,
    default: T,
    w: &mut Vec<String>,
    check: impl Fn(&T) -> Result<(), String>,
) -> T {
    let Some(v) = t.get(key) else { return default };
    match v
        .clone()
        .try_into::<T>()
        .map_err(|e| e.message().to_string())
        .and_then(|x| check(&x).map(|()| x))
    {
        Ok(x) => x,
        Err(why) => {
            w.push(format!("{prefix}{key}: {why}; using the default"));
            default
        }
    }
}

pub(super) fn check_base_url(s: &str) -> Result<(), String> {
    let ok = s.is_empty()
        || ((s.starts_with("http://") || s.starts_with("https://"))
            && !s.contains(char::is_whitespace));
    ok.then_some(())
        .ok_or_else(|| "expected empty or an http(s):// URL".into())
}

/// Empty (system choice) or an absolute path; it need not exist (checked on use).
pub(super) fn check_folder(s: &str) -> Result<(), String> {
    (s.is_empty() || Path::new(s).is_absolute())
        .then_some(())
        .ok_or_else(|| "expected empty or an absolute folder path".into())
}

pub(super) fn check_model(s: &str) -> Result<(), String> {
    let ok = s.len() <= 200 && !s.contains(char::is_whitespace);
    ok.then_some(())
        .ok_or_else(|| "expected a model id without spaces".into())
}

/// "ar", "en", "pt-BR", "zh-Hant": 2-3 letters, then `-` parts of 2-8 alphanumerics.
pub(super) fn check_lang(s: &str) -> Result<(), String> {
    let mut parts = s.split('-');
    let first = parts.next().unwrap_or_default();
    let ok = (2..=3).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_alphabetic())
        && parts
            .all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()));
    ok.then_some(())
        .ok_or_else(|| "expected a language code like \"ar\" or \"pt-BR\"".into())
}

pub(super) fn parse_quick_actions(v: &Value, w: &mut Vec<String>) -> Vec<QuickAction> {
    let Some(items) = v.as_array() else {
        w.push("quick_actions: expected a list of tables; using the defaults".into());
        return Config::default().quick_actions;
    };
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(t) = item.as_table() else {
            w.push(format!(
                "quick_actions[{i}]: expected a table; entry skipped"
            ));
            continue;
        };
        let prefix = format!("quick_actions[{i}].");
        let action: String = get_checked(t, &prefix, "action", String::new(), w, |s: &String| {
            let ok = !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            ok.then_some(())
                .ok_or_else(|| "expected an action id like \"ai_explain\"".into())
        });
        if action.is_empty() {
            w.push(format!(
                "quick_actions[{i}]: no valid action; entry skipped"
            ));
            continue;
        }
        // Empty or invalid shortcut = unbound (never guess a binding).
        let shortcut = get_checked(t, &prefix, "shortcut", String::new(), w, |s: &String| {
            if s.is_empty() {
                Ok(())
            } else {
                parse_shortcut(s).map(drop)
            }
        });
        out.push(QuickAction {
            action: QuickId::from_name(&action),
            shortcut,
            target: get(t, &prefix, "target", Target::FullScreen, w),
            live: get(t, &prefix, "live", false, w),
        });
    }
    out
}
