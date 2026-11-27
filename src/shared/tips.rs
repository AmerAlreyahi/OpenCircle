//! The one-time tips (R-17): which hint may show now. Pure: the app says what became
//! relevant and whether the user is in a fast flow; this says yes or no and what to
//! write down. Each tip shows ONCE per user, existing users included (nothing is
//! pre-marked seen), at most one per session, never in a fast flow (a pick running, a
//! drag), and never while "Show tips" is off. A tip that fades after `FADE_MS` counts as
//! seen, like Got it and the x; "Don't show tips" turns every tip off.
use super::config::TipsConfig;

/// How long a tip stays before it fades away by itself (and counts as seen).
pub const FADE_MS: u64 = 8_000;

/// The tips, each tied to the widget it hints at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tip {
    /// Snip toolbar: the first drag over a shape ("Hold Ctrl and drag to move a shape").
    SnipMove,
    /// The translate pill: the first finished translation ("Original shows the source text").
    Original,
    /// The main bar: the first time it opens ("Press F1 to see what you can do").
    Bar,
}

impl Tip {
    #[cfg(test)]
    pub const ALL: [Tip; 3] = [Tip::SnipMove, Tip::Original, Tip::Bar];

    /// The id kept in the config (never renamed).
    pub fn id(self) -> &'static str {
        match self {
            Tip::SnipMove => "snip_move",
            Tip::Original => "translate_original",
            Tip::Bar => "bar_f1",
        }
    }
}

/// The session's tip state: at most one tip per run of the app.
#[derive(Debug, Default)]
pub struct Session {
    shown: bool,
}

impl Session {
    /// `tip` became relevant: may it show now? `fast` = the user is in a fast flow (the
    /// Alt+A pick is running, a drag is going). A yes uses up the session's one tip.
    pub fn offer(&mut self, cfg: &TipsConfig, tip: Tip, fast: bool) -> bool {
        let yes = cfg.show && !self.shown && !fast && !seen(cfg, tip);
        self.shown |= yes;
        yes
    }
}

pub fn seen(cfg: &TipsConfig, tip: Tip) -> bool {
    cfg.seen.iter().any(|id| id == tip.id())
}

/// Got it, the x or the fade: `tip` is seen, for good.
pub fn mark_seen(cfg: &mut TipsConfig, tip: Tip) {
    if !seen(cfg, tip) {
        cfg.seen.push(tip.id().to_string());
    }
}

/// "Don't show tips": every tip off (Settings > General turns them back on).
pub fn hide_all(cfg: &mut TipsConfig) {
    cfg.show = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::config::Config;

    /// R17-AC1: a tip shows once per user: once seen (Got it, x or fade), never again,
    /// in this session or any later one (the seen list is in the config file).
    #[test]
    fn a_tip_shows_once_per_user() {
        let mut cfg = TipsConfig::default();
        assert!(Session::default().offer(&cfg, Tip::Bar, false));
        mark_seen(&mut cfg, Tip::Bar);
        mark_seen(&mut cfg, Tip::Bar);
        assert_eq!(cfg.seen, ["bar_f1"], "written once");
        // a new session (a restart) with the same file: no
        assert!(!Session::default().offer(&cfg, Tip::Bar, false));
        // the others are still unseen
        assert!(Session::default().offer(&cfg, Tip::Original, false));
    }

    /// R17-AC2: one tip per session, even when another becomes relevant meanwhile.
    #[test]
    fn one_tip_per_session() {
        let cfg = TipsConfig::default();
        let mut s = Session::default();
        assert!(s.offer(&cfg, Tip::Bar, false));
        assert!(!s.offer(&cfg, Tip::SnipMove, false));
        assert!(!s.offer(&cfg, Tip::Original, false));
    }

    /// R17-AC3: never in a fast flow, and that does not use up the session's tip.
    #[test]
    fn never_in_a_fast_flow() {
        let cfg = TipsConfig::default();
        let mut s = Session::default();
        assert!(!s.offer(&cfg, Tip::SnipMove, true));
        assert!(s.offer(&cfg, Tip::SnipMove, false), "the tip was not spent");
    }

    /// R17-AC4: a tip that fades counts as seen (`mark_seen` is what the fade calls); with
    /// it written, the next session does not show it again.
    #[test]
    fn a_faded_tip_counts_as_seen() {
        let mut cfg = TipsConfig::default();
        let mut s = Session::default();
        assert!(s.offer(&cfg, Tip::Original, false));
        // 8 s pass: the app marks it
        assert_eq!(FADE_MS, 8_000);
        mark_seen(&mut cfg, Tip::Original);
        assert!(seen(&cfg, Tip::Original));
        assert!(!Session::default().offer(&cfg, Tip::Original, false));
    }

    /// R17-AC5: "Show tips" off hides every tip; "Don't show tips" turns it off.
    #[test]
    fn show_off_hides_all() {
        let mut cfg = TipsConfig::default();
        for tip in Tip::ALL {
            assert!(Session::default().offer(&cfg, tip, false));
        }
        hide_all(&mut cfg);
        assert!(!cfg.show);
        for tip in Tip::ALL {
            assert!(!Session::default().offer(&cfg, tip, false), "{tip:?}");
        }
    }

    /// R17-AC6: an existing user (a config file from before tips, no [tips] section) sees
    /// every tip once: nothing is pre-marked seen. And the section round-trips, written
    /// only once changed.
    #[test]
    fn an_existing_user_sees_the_tips() {
        let (old, warnings) = Config::parse("version = 15\ntheme = \"system\"\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(old.tips.show && old.tips.seen.is_empty());
        for tip in Tip::ALL {
            assert!(Session::default().offer(&old.tips, tip, false));
        }
        let plain = toml::to_string(&Config::default()).unwrap();
        assert!(
            !plain.contains("[tips]"),
            "the default file stays as frozen"
        );
        let mut cfg = Config::default();
        mark_seen(&mut cfg.tips, Tip::Bar);
        hide_all(&mut cfg.tips);
        let text = toml::to_string(&cfg).unwrap();
        let (back, warnings) = Config::parse(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back.tips, cfg.tips);
    }

    #[test]
    fn the_ids_are_unique() {
        let mut ids: Vec<_> = Tip::ALL.iter().map(|t| t.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Tip::ALL.len());
    }
}
