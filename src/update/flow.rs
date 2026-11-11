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

/// When the last check was: the UI says it in its language (`Display` is
/// the English of the logs and tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Never,
    JustNow,
    Minutes(u64),
    Hours(u64),
    Yesterday,
    Days(u64),
}

impl std::fmt::Display for When {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            When::Never => f.write_str("not checked yet"),
            When::JustNow => f.write_str("checked just now"),
            When::Minutes(n) => write!(f, "checked {n} min ago"),
            When::Hours(n) => write!(f, "checked {n} h ago"),
            When::Yesterday => f.write_str("checked yesterday"),
            When::Days(n) => write!(f, "checked {n} days ago"),
        }
    }
}

/// "checked just now / 5 min ago / 2 h ago / 3 days ago".
pub fn checked_ago(at: SystemTime, now: SystemTime) -> When {
    let s = now.duration_since(at).unwrap_or_default().as_secs();
    match s {
        0..60 => When::JustNow,
        60..3600 => When::Minutes(s / 60),
        3600..86_400 => When::Hours(s / 3600),
        _ if s < 2 * 86_400 => When::Yesterday,
        _ => When::Days(s / 86_400),
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

/// The Updates row's title (the UI words it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Out(String),
    Ready(String),
    Updates,
}

/// The Updates row's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Say {
    /// "Update it with your package manager or the release page".
    GetIt,
    /// "You have <current> · checked …".
    Have(String, When),
    /// The last answer to Check now.
    Answer(super::Answer),
}

/// Settings > General's Updates row, for an update `waiting` (its version
/// and how it installs), this version `current`, the last check `checked`,
/// and the last answer to Check now `last`: typed, the UI words it.
pub fn status(
    waiting: Option<(&str, How)>,
    current: &str,
    checked: Option<SystemTime>,
    last: Option<&super::Answer>,
    now: SystemTime,
) -> (Head, Say) {
    let when = checked.map_or(When::Never, |at| checked_ago(at, now));
    let have = || Say::Have(current.to_string(), when);
    match waiting {
        Some((v, How::GetIt)) => (Head::Out(v.into()), Say::GetIt),
        Some((v, _)) => (Head::Ready(v.into()), have()),
        None => (
            Head::Updates,
            last.map_or_else(have, |a| Say::Answer(a.clone())),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::Answer;

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
        let have = |v: &str, w| Say::Have(v.into(), w);
        assert_eq!(
            status(Some(("0.1.5", How::Restart)), "0.1.4", ago(7200), None, now),
            (Head::Ready("0.1.5".into()), have("0.1.4", When::Hours(2)))
        );
        assert_eq!(
            status(Some(("0.1.5", How::Setup)), "0.1.4", ago(30), None, now).1,
            have("0.1.4", When::JustNow)
        );
        assert_eq!(
            status(Some(("0.1.5", How::GetIt)), "0.1.4", None, None, now),
            (Head::Out("0.1.5".into()), Say::GetIt)
        );
        assert_eq!(
            status(None, "0.1.4", ago(300), None, now),
            (Head::Updates, have("0.1.4", When::Minutes(5)))
        );
        assert_eq!(
            status(None, "0.1.4", None, None, now).1,
            have("0.1.4", When::Never)
        );
        let said = Answer::UpToDate("0.1.4".into());
        assert_eq!(
            status(None, "0.1.4", ago(300), Some(&said), now).1,
            Say::Answer(said)
        );
        assert_eq!(
            checked_ago(now - Duration::from_secs(90_000), now),
            When::Yesterday
        );
        assert_eq!(
            checked_ago(now - Duration::from_secs(300_000), now).to_string(),
            "checked 3 days ago"
        );
    }
}
