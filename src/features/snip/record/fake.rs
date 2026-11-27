//! The fake recorder (R-02): stands in for capture and encoding until R-04 so
//! the bar and the thumbnail can be driven in tests and goldens. It does what
//! the state machine's `Effect`s ask and answers the way the real one will:
//! a flush ends in `Event::Flushed`; a start can be told to fail.
use std::time::Duration;

use super::state::{Effect, Event, Fault};

#[derive(Debug, Default)]
pub struct Fake {
    /// Every effect it was asked to run, in order.
    pub log: Vec<Effect>,
    /// The next `Begin` fails with this.
    pub fail_start: Option<Fault>,
}

impl Fake {
    /// Runs `effect`; what it answers (the event the real recorder would send back).
    pub fn run(&mut self, effect: Effect) -> Option<Event> {
        self.log.push(effect);
        match effect {
            Effect::Begin => self.fail_start.take().map(Event::StartFailed),
            Effect::Flush => Some(Event::Flushed),
            Effect::Pause | Effect::Resume | Effect::DeleteTemp => None,
        }
    }

    /// Level bars at `at` (0..1): a fixed moving pattern.
    pub fn levels(at: Duration) -> Vec<f32> {
        let t = at.as_millis() as f32 / 120.0;
        (0..7)
            .map(|i| 0.5 + 0.45 * (t + i as f32 * 1.3).sin())
            .collect()
    }

    /// A waveform of `n` bars for a finished audio recording.
    pub fn wave(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.25 + 0.7 * ((i as f32 * 0.9).sin() * (i as f32 * 0.37).cos()).abs())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flush_answers_flushed_and_a_start_can_fail_once() {
        let mut f = Fake::default();
        assert_eq!(f.run(Effect::Flush), Some(Event::Flushed));
        assert_eq!(f.run(Effect::Begin), None);
        f.fail_start = Some(Fault::Mic);
        assert_eq!(f.run(Effect::Begin), Some(Event::StartFailed(Fault::Mic)));
        assert_eq!(f.run(Effect::Begin), None, "once");
        assert_eq!(f.log.len(), 4);
    }

    #[test]
    fn levels_move_and_stay_in_range() {
        let (a, b) = (
            Fake::levels(Duration::ZERO),
            Fake::levels(Duration::from_millis(300)),
        );
        assert_ne!(a, b);
        assert!(a.iter().chain(&b).all(|v| (0.0..=1.0).contains(v)));
        assert!(Fake::wave(30).iter().all(|v| (0.0..=1.0).contains(v)));
    }
}
