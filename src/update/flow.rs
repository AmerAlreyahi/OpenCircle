//! When a waiting update installs, and the life of the update pills (owner,
//! 2026-11-10, "Update experience"). Pure decisions: the App feeds them the
//! clock and the session's state.

use std::time::{Duration, Instant, SystemTime};

use super::Install;

/// The moments an update may install at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// Restart now (the pill, the tray row, Settings).
    Button,
    /// OpenCircle quits, or the session logs off / shuts down.
    Quit,
    /// The session has been locked `LOCKED_FOR` with nothing of OpenCircle open.
    Locked,
}

/// Whether an update for `install` installs at `moment`: a swapped file
/// (portable exe, AppImage) at any of them; a copy the setup installed only on
/// the button (Windows asks for permission: that prompt never comes from
/// nowhere); deb / rpm never (the user gets it).
pub fn installs_at(install: &Install, moment: Moment) -> bool {
    match install {
        Install::Exe(_) | Install::AppImage(_) => true,
        Install::Setup(_) => moment == Moment::Button,
        Install::Manual => false,
    }
}

/// A pill stays this long, then fades (the tray dot and row stay).
pub const PILL_FOR: Duration = Duration::from_secs(30);
/// No input for this long counts as away.
pub const AWAY_AFTER: Duration = Duration::from_secs(120);
/// Locked this long (and OpenCircle idle): the update installs.
pub const LOCKED_FOR: Duration = Duration::from_secs(60);

/// The user isn't there: the session is locked, or nothing was typed or
/// clicked for `AWAY_AFTER`.
pub fn away(locked: bool, idle: Duration) -> bool {
    locked || idle >= AWAY_AFTER
}

/// What a pill should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Show,
    Hide,
}

/// A pill that waits while the user is away and goes after `PILL_FOR`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pill {
    /// Not shown yet (or hidden because the user left before reading it).
    #[default]
    Waiting,
    Shown(Instant),
    /// Read, dismissed or faded: never again.
    Done,
}

impl Pill {
    /// The pill's next step at `now`, the user `away` or not.
    pub fn step(&mut self, now: Instant, away: bool) -> Step {
        match *self {
            Pill::Waiting if away => Step::Nothing,
            Pill::Waiting => {
                *self = Pill::Shown(now);
                Step::Show
            }
            // left before reading it: shown again when back
            Pill::Shown(_) if away => {
                *self = Pill::Waiting;
                Step::Hide
            }
            Pill::Shown(at) if now.duration_since(at) >= PILL_FOR => {
                *self = Pill::Done;
                Step::Hide
            }
            Pill::Shown(_) | Pill::Done => Step::Nothing,
        }
    }

    /// A button on it was used (Later, What's new) or it was closed.
    pub fn dismiss(&mut self) {
        *self = Pill::Done;
    }

    pub fn is_shown(&self) -> bool {
        matches!(self, Pill::Shown(_))
    }
}

/// How long the session has been locked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lock {
    since: Option<Instant>,
}

impl Lock {
    /// `true` once the session has been locked for `LOCKED_FOR` (seen at `now`).
    pub fn long_enough(&mut self, now: Instant, locked: bool) -> bool {
        if !locked {
            self.since = None;
            return false;
        }
        now.duration_since(*self.since.get_or_insert(now)) >= LOCKED_FOR
    }
}

/// "checked just now / 5 min ago / 2 h ago / 3 days ago".
pub fn checked_ago(at: SystemTime, now: SystemTime) -> String {
    let s = now.duration_since(at).unwrap_or_default().as_secs();
    match s {
        0..60 => "checked just now".into(),
        60..3600 => format!("checked {} min ago", s / 60),
        3600..86_400 => format!("checked {} h ago", s / 3600),
        _ if s < 2 * 86_400 => "checked yesterday".into(),
        _ => format!("checked {} days ago", s / 86_400),
    }
}

/// How the waiting update installs (what its buttons say and do).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// Restart now swaps to it (portable exe, AppImage).
    Restart,
    /// Restart now runs the setup (one Windows permission prompt).
    Setup,
    /// "Get it": the release page (deb, rpm, a copy that can't replace itself).
    GetIt,
}

impl How {
    pub fn of(install: &Install) -> How {
        match install {
            Install::Exe(_) | Install::AppImage(_) => How::Restart,
            Install::Setup(_) => How::Setup,
            Install::Manual => How::GetIt,
        }
    }
}

/// Settings > General's Updates row: its title and line, for an update
/// `waiting` (its version and how it installs), this version `current`, the
/// last check `checked`, and the last answer to Check now `last`.
pub fn status(
    waiting: Option<(&str, How)>,
    current: &str,
    checked: Option<SystemTime>,
    last: Option<&str>,
    now: SystemTime,
) -> (String, String) {
    let when = checked.map_or_else(|| "not checked yet".into(), |at| checked_ago(at, now));
    match waiting {
        Some((v, How::GetIt)) => (
            format!("OpenCircle {v} is out"),
            "Update it with your package manager or the release page".into(),
        ),
        Some((v, _)) => (
            format!("OpenCircle {v} is ready"),
            format!("You have {current} · {when}"),
        ),
        None => (
            "Updates".into(),
            last.map_or_else(|| format!("You have {current} · {when}"), str::to_string),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A portable / AppImage update installs at the button, at quit and when
    /// locked; a setup's only at the button (never a prompt from nowhere);
    /// deb / rpm never. Before: any idle minute, for every swap install.
    #[test]
    fn install_moments() {
        let exe = Install::Exe("C:/a/opencircle.exe".into());
        let app = Install::AppImage("/a/OpenCircle.AppImage".into());
        let setup = Install::Setup("C:/Program Files/OpenCircle/opencircle.exe".into());
        for m in [Moment::Button, Moment::Quit, Moment::Locked] {
            assert!(installs_at(&exe, m) && installs_at(&app, m), "{m:?}");
            assert!(!installs_at(&Install::Manual, m), "{m:?}");
        }
        assert!(installs_at(&setup, Moment::Button));
        assert!(!installs_at(&setup, Moment::Quit));
        assert!(!installs_at(&setup, Moment::Locked));
        assert_eq!(How::of(&setup), How::Setup);
        assert_eq!(How::of(&exe), How::Restart);
        assert_eq!(How::of(&Install::Manual), How::GetIt);
    }

    /// Locked under a minute: nothing; a minute: installs; unlocked meanwhile:
    /// the minute starts again.
    #[test]
    fn a_lock_installs_after_a_minute() {
        let t = Instant::now();
        let mut lock = Lock::default();
        assert!(!lock.long_enough(t, true));
        assert!(!lock.long_enough(t + Duration::from_secs(59), true));
        assert!(!lock.long_enough(t + Duration::from_secs(61), false));
        assert!(!lock.long_enough(t + Duration::from_secs(70), true));
        assert!(lock.long_enough(t + Duration::from_secs(130), true));
    }

    /// The pill waits while the user is away, stays 30 s, comes back if the
    /// user left before reading it, and never returns once dismissed or faded.
    #[test]
    fn the_pill_waits_while_away_and_fades_after_30_s() {
        let t = Instant::now();
        let s = Duration::from_secs;
        let mut p = Pill::default();
        assert_eq!(p.step(t, true), Step::Nothing, "away: waits");
        assert_eq!(p.step(t + s(5), false), Step::Show);
        assert!(p.is_shown());
        assert_eq!(p.step(t + s(20), false), Step::Nothing);
        assert_eq!(p.step(t + s(25), true), Step::Hide, "left: hidden");
        assert_eq!(p.step(t + s(300), false), Step::Show, "back: again");
        assert_eq!(p.step(t + s(329), false), Step::Nothing);
        assert_eq!(p.step(t + s(330), false), Step::Hide, "30 s: fades");
        assert_eq!(p.step(t + s(400), false), Step::Nothing, "for good");
        let mut p = Pill::default();
        p.step(t, false);
        p.dismiss();
        assert_eq!(p.step(t + s(1), false), Step::Nothing);
        assert!(away(true, Duration::ZERO));
        assert!(away(false, AWAY_AFTER));
        assert!(!away(false, s(119)));
    }

    /// The Updates row says what waits and when it last looked. Before: "Look
    /// for a new version now" until Check now was pressed.
    #[test]
    fn the_settings_status_says_what_waits_and_when_it_looked() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ago = |s| Some(now - Duration::from_secs(s));
        assert_eq!(
            status(Some(("0.1.5", How::Restart)), "0.1.4", ago(7200), None, now),
            (
                "OpenCircle 0.1.5 is ready".into(),
                "You have 0.1.4 · checked 2 h ago".into()
            )
        );
        assert_eq!(
            status(Some(("0.1.5", How::Setup)), "0.1.4", ago(30), None, now).1,
            "You have 0.1.4 · checked just now"
        );
        assert_eq!(
            status(Some(("0.1.5", How::GetIt)), "0.1.4", None, None, now).0,
            "OpenCircle 0.1.5 is out"
        );
        assert_eq!(
            status(None, "0.1.4", ago(300), None, now),
            (
                "Updates".into(),
                "You have 0.1.4 · checked 5 min ago".into()
            )
        );
        assert_eq!(
            status(None, "0.1.4", None, None, now).1,
            "You have 0.1.4 · not checked yet"
        );
        assert_eq!(
            status(None, "0.1.4", ago(300), Some("Checking…"), now).1,
            "Checking…"
        );
        assert_eq!(
            checked_ago(now - Duration::from_secs(90_000), now),
            "checked yesterday"
        );
        assert_eq!(
            checked_ago(now - Duration::from_secs(300_000), now),
            "checked 3 days ago"
        );
    }
}
