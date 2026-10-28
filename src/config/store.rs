//! The running app's config: one source of truth, and the one writer of the file.

use std::path::Path;
use std::sync::RwLock;

use toml::{Table, Value};

use super::*;

static CURRENT: RwLock<Option<Config>> = RwLock::new(None);

/// The active look, without cloning the whole config (the screen edge reads it
/// every frame, the widgets on every style).
pub fn look() -> Look {
    if let Some(c) = CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return c.look.active();
    }
    current().look.active()
}

/// The floating widgets' theme and whether they blur the screen behind them.
pub fn widgets() -> (WidgetTheme, bool) {
    let c = CURRENT.read().unwrap_or_else(|e| e.into_inner());
    c.as_ref().map_or((WidgetTheme::Default, true), |c| {
        (c.widget_theme, c.glass_effect)
    })
}

/// The config the running app uses (any thread). Read from disk on first use;
/// after that only `set_current` changes it (Settings saved, onboarding finished).
pub fn current() -> Config {
    if let Some(c) = CURRENT.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return c.clone();
    }
    let (config, warnings) = Config::load();
    for warning in warnings {
        crate::log!("[opencircle] config: {warning}");
    }
    CURRENT
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert(config)
        .clone()
}

pub fn set_current(config: Config) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(config);
}

static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The one way to change the saved config: under one lock, apply `f` to the
/// config as it is NOW (not to a copy somebody opened earlier), write it
/// atomically and make it the running app's config. Returns the result.
pub fn update(f: impl FnOnce(&mut Config)) -> std::io::Result<Config> {
    update_at(None, f)
}

/// `update` on another file (`Some`, Settings and onboarding tests); the
/// running app's config is then left alone.
pub fn update_at(path: Option<&Path>, f: impl FnOnce(&mut Config)) -> std::io::Result<Config> {
    let real = path.is_none();
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => Config::path().ok_or_else(|| std::io::Error::other("no user config directory"))?,
    };
    // ponytail: one process-wide lock, fsync inside it; a per-file lock if a second file ever needs writing.
    let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = if real {
        current()
    } else {
        Config::load_from(&path).0
    };
    f(&mut c);
    c.save_to(&path)?;
    if real {
        set_current(c.clone());
    }
    Ok(c)
}

/// `base` with the edits that turned `from` into `to` laid on it: what Settings
/// saves, so a value changed elsewhere while it was open survives. Compared
/// leaf by leaf in the file's own shape (a list counts as one leaf).
pub fn rebase(base: &Config, from: &Config, to: &Config) -> Config {
    fn table(c: &Config) -> Table {
        match Value::try_from(c) {
            Ok(Value::Table(t)) => t,
            _ => Table::new(),
        }
    }
    fn merge(base: &mut Table, from: &Table, to: &Table) {
        for (k, v) in to {
            match (from.get(k), v) {
                (Some(old), _) if old == v => {}
                (Some(Value::Table(f)), Value::Table(t))
                    if matches!(base.get(k), Some(Value::Table(_))) =>
                {
                    if let Some(Value::Table(b)) = base.get_mut(k) {
                        merge(b, f, t);
                    }
                }
                _ => {
                    base.insert(k.clone(), v.clone());
                }
            }
        }
        for k in from.keys().filter(|k| !to.contains_key(*k)) {
            base.remove(k);
        }
    }
    let mut t = table(base);
    merge(&mut t, &table(from), &table(to));
    toml::to_string(&t).map_or_else(|_| base.clone(), |s| Config::parse(&s).0)
}
