//! The recording's elapsed time: pause gaps are not counted. Pure: the caller
//! passes `now` (time since any fixed start), so tests need no sleeping.
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    started: Option<Duration>,
    /// Set while paused: when the pause began.
    paused_at: Option<Duration>,
    /// Every finished pause, summed.
    paused: Duration,
}

impl Clock {
    pub fn start(now: Duration) -> Clock {
        Clock {
            started: Some(now),
            ..Clock::default()
        }
    }

    pub fn pause(&mut self, now: Duration) {
        if self.started.is_some() && self.paused_at.is_none() {
            self.paused_at = Some(now);
        }
    }

    pub fn resume(&mut self, now: Duration) {
        if let Some(at) = self.paused_at.take() {
            self.paused += now.saturating_sub(at);
        }
    }

    /// Recorded time at `now`: frozen while paused, never negative.
    pub fn elapsed(&self, now: Duration) -> Duration {
        let Some(start) = self.started else {
            return Duration::ZERO;
        };
        let end = self.paused_at.unwrap_or(now);
        end.saturating_sub(start).saturating_sub(self.paused)
    }
}

/// `0:12`, `00:12` as the bar shows it (`mm:ss`), `1:02:03` past an hour.
pub fn format(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    match s >= 3600 {
        true => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
        false => format!("{:02}:{:02}", s / 60, s % 60),
    }
}

/// The thumbnail's short form: `0:12`, `1:05`.
pub fn short(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn pause_gaps_are_not_counted() {
        let mut c = Clock::start(s(100));
        assert_eq!(c.elapsed(s(112)), s(12));
        c.pause(s(112));
        assert_eq!(c.elapsed(s(500)), s(12), "frozen while paused");
        c.resume(s(130));
        assert_eq!(c.elapsed(s(135)), s(17));
        c.pause(s(135));
        c.resume(s(140));
        assert_eq!(c.elapsed(s(141)), s(18), "two pauses add up");
    }

    #[test]
    fn a_double_pause_or_resume_changes_nothing() {
        let mut c = Clock::start(s(0));
        c.pause(s(10));
        c.pause(s(20));
        c.resume(s(30));
        c.resume(s(40));
        assert_eq!(c.elapsed(s(50)), s(30));
        assert_eq!(Clock::default().elapsed(s(9)), Duration::ZERO);
    }

    #[test]
    fn the_time_reads_as_the_bar_shows_it() {
        assert_eq!(format(s(12)), "00:12");
        assert_eq!(format(s(754)), "12:34");
        assert_eq!(format(s(3723)), "1:02:03");
        assert_eq!(short(s(12)), "0:12");
        assert_eq!(short(s(65)), "1:05");
    }
}
