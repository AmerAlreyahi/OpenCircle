//! The recording state machine (R-01 section 5), pure: no capture, no clock, no
//! window. The App feeds it `Event`s and does what the returned `Effect`s say.
//!
//! ```text
//! Idle --StartVideo--> Picking --Picked--> (Asking) --Allowed--> Countdown(3..1) --> Recording
//! Idle --StartAudio--> Recording(audio)
//! Recording <--Pause / Resume--> Paused
//! Recording|Paused --Stop / the start key--> Stopping --Flushed--> Result --Dismissed--> Idle
//! Recording|Paused --Discard (bin)--> Idle
//! any --Fault--> Error (disk low / stream ended: Stopping, what we have is kept)
//! ```
//! Esc does nothing while recording or paused (owner, 2026-11-28). Choices the design is
//! silent on: a start key during the countdown is ignored (it must not start a second
//! recording); a second Stop while `Stopping` is ignored.

/// Where a recording is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Video: the user picks an area, a window or a screen.
    Picking,
    /// Wayland: the desktop's own permission dialog is up.
    Asking,
    /// 3, 2, 1 before the first frame.
    Countdown(u8),
    Recording {
        audio_only: bool,
    },
    Paused {
        audio_only: bool,
    },
    /// Flushing the file ("Saving").
    Stopping {
        audio_only: bool,
    },
    /// The corner thumbnail (S2) is up.
    Result {
        audio_only: bool,
    },
    Error(Fault),
}

/// What went wrong (the one error card says it plainly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The microphone is gone or was refused (recording goes on without it when
    /// the user chooses so).
    #[cfg_attr(not(test), allow(dead_code))] // R-04's recorder raises these
    Mic,
    /// The desktop said no (permission, window gone before the start).
    Refused,
    /// The disk is nearly full: stop and keep what we have.
    #[cfg_attr(not(test), allow(dead_code))]
    DiskLow,
    /// The captured stream ended (window closed, screen off): stop and keep.
    #[cfg_attr(not(test), allow(dead_code))]
    StreamEnded,
    /// This PC cannot encode (GStreamer's plugins are missing): nothing was recorded.
    Encoder,
    /// The file could not be written (disk error, encoder error): stop, keep what is valid.
    WriteFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Alt+Shift+R, the tray, or Video on the pick screen.
    StartVideo,
    /// Alt+Shift+M or the tray: audio only, no pick.
    StartAudio,
    /// The area, window or screen was picked (`ask`: Wayland asks the desktop).
    Picked {
        ask: bool,
    },
    #[cfg_attr(not(test), allow(dead_code))] // R-04: the desktop's answer, the countdown, the key
    Allowed,
    #[cfg_attr(not(test), allow(dead_code))]
    Denied,
    /// One second of the countdown went by.
    #[cfg_attr(not(test), allow(dead_code))]
    Tick,
    /// Skip / Enter on the countdown.
    Skip,
    Pause,
    Resume,
    /// The bar's Stop.
    Stop,
    /// The start key pressed again (Alt+Shift+R / M): stop and save.
    #[cfg_attr(not(test), allow(dead_code))]
    Key,
    Esc,
    /// The bar's bin.
    Discard,
    /// A recording an earlier run left behind is offered on the thumbnail.
    Offer {
        audio_only: bool,
    },
    /// The sink wrote and closed the file.
    Flushed,
    /// The file could not be written or closed cleanly: the card says so, what is valid stays.
    FlushFailed(Fault),
    /// An S2 action ran, or S2 slid away; or the error card was closed.
    Dismiss,
    #[cfg_attr(not(test), allow(dead_code))] // R-04's recorder raises it
    Fault(Fault),
    /// The capture could not start (nothing was recorded: the card, nothing to keep).
    StartFailed(Fault),
}

/// What the App does for a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Start capturing (the first frame).
    Begin,
    Pause,
    Resume,
    /// Stop capturing, drain the queue, close the file.
    Flush,
    /// Delete the temp file.
    DeleteTemp,
}

impl State {
    /// Recording or paused: a bar is on screen.
    pub fn is_live(self) -> bool {
        matches!(self, State::Recording { .. } | State::Paused { .. })
    }

    #[cfg(test)]
    pub fn audio_only(self) -> bool {
        matches!(
            self,
            State::Recording { audio_only: true }
                | State::Paused { audio_only: true }
                | State::Stopping { audio_only: true }
                | State::Result { audio_only: true }
        )
    }

    /// The next state and what to do. An event that means nothing here changes nothing.
    pub fn on(self, event: Event) -> (State, Option<Effect>) {
        use Event as E;
        use State as S;
        let same = (self, None);
        match (self, event) {
            // starting (also from a finished recording: its thumbnail goes)
            (S::Idle | S::Result { .. } | S::Error(_), E::StartVideo) => (S::Picking, None),
            (S::Idle | S::Result { .. } | S::Error(_), E::StartAudio) => {
                (S::Recording { audio_only: true }, Some(Effect::Begin))
            }
            (S::Picking, E::Picked { ask: true }) => (S::Asking, None),
            (S::Picking, E::Picked { ask: false }) | (S::Asking, E::Allowed) => {
                (S::Countdown(3), None)
            }
            (S::Picking | S::Asking, E::Esc) => (S::Idle, None),
            (S::Asking, E::Denied) => (S::Error(Fault::Refused), None),
            // 3, 2, 1
            (S::Countdown(n), E::Tick) if n > 1 => (S::Countdown(n - 1), None),
            (S::Countdown(_), E::Tick | E::Skip) => {
                (S::Recording { audio_only: false }, Some(Effect::Begin))
            }
            (S::Countdown(_), E::Esc) => (S::Idle, None),
            // the start key during the countdown is swallowed (no second recording)
            (S::Countdown(_), E::Key) => same,
            // recording
            (S::Recording { audio_only }, E::Pause) => {
                (S::Paused { audio_only }, Some(Effect::Pause))
            }
            (S::Paused { audio_only }, E::Resume) => {
                (S::Recording { audio_only }, Some(Effect::Resume))
            }
            (S::Recording { audio_only } | S::Paused { audio_only }, E::Stop | E::Key) => {
                (S::Stopping { audio_only }, Some(Effect::Flush))
            }
            // Esc does NOTHING while recording (owner, 2026-11-28): Stop is the record key, the
            // bar's Stop or the tray
            (S::Recording { .. } | S::Paused { .. }, E::Discard) => {
                (S::Idle, Some(Effect::DeleteTemp))
            }
            // stopping: a second Stop is nothing; the flush ends in the thumbnail
            (S::Stopping { audio_only }, E::Flushed) => (S::Result { audio_only }, None),
            (S::Stopping { .. }, E::FlushFailed(f)) => (S::Error(f), None),
            // leftovers of an earlier run, offered once (nothing is recorded)
            (S::Idle, E::Offer { audio_only }) => (S::Result { audio_only }, None),
            (S::Result { .. }, E::Dismiss | E::Esc) => (S::Idle, None),
            // the error card (a fault before recording: nothing was kept) closed
            (S::Error(_), E::Dismiss | E::Esc) => (S::Idle, Some(Effect::DeleteTemp)),
            // faults
            (S::Recording { .. }, E::StartFailed(f)) => (S::Error(f), None),
            // mid-recording, every fault ends in what we have (a lost mic or a desktop that
            // says no at minute 30 must not delete the recording)
            (S::Recording { audio_only } | S::Paused { audio_only }, E::Fault(_)) => {
                (S::Stopping { audio_only }, Some(Effect::Flush))
            }
            (S::Picking | S::Asking | S::Countdown(_), E::Fault(f)) => (S::Error(f), None),
            _ => same,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Effect as F;
    use super::Event as E;
    use super::State as S;
    use super::*;

    const REC: S = S::Recording { audio_only: false };
    const PAU: S = S::Paused { audio_only: false };
    const STOPPING: S = S::Stopping { audio_only: false };

    /// Every arrow of R-01's diagram, one row each: from, event, to, effect.
    #[test]
    fn every_arrow_of_the_diagram() {
        let rows: Vec<(S, E, S, Option<F>)> = vec![
            (S::Idle, E::StartVideo, S::Picking, None),
            (
                S::Idle,
                E::StartAudio,
                S::Recording { audio_only: true },
                Some(F::Begin),
            ),
            (S::Picking, E::Picked { ask: true }, S::Asking, None),
            (S::Picking, E::Picked { ask: false }, S::Countdown(3), None),
            (S::Asking, E::Allowed, S::Countdown(3), None),
            (S::Asking, E::Denied, S::Error(Fault::Refused), None),
            (S::Countdown(3), E::Tick, S::Countdown(2), None),
            (S::Countdown(2), E::Tick, S::Countdown(1), None),
            (S::Countdown(1), E::Tick, REC, Some(F::Begin)),
            (S::Countdown(3), E::Skip, REC, Some(F::Begin)),
            (REC, E::Pause, PAU, Some(F::Pause)),
            (PAU, E::Resume, REC, Some(F::Resume)),
            (REC, E::Stop, STOPPING, Some(F::Flush)),
            (PAU, E::Stop, STOPPING, Some(F::Flush)),
            (REC, E::Key, STOPPING, Some(F::Flush)),
            (PAU, E::Key, STOPPING, Some(F::Flush)),
            (STOPPING, E::Flushed, S::Result { audio_only: false }, None),
            (S::Result { audio_only: false }, E::Dismiss, S::Idle, None),
            (REC, E::Discard, S::Idle, Some(F::DeleteTemp)),
            (PAU, E::Discard, S::Idle, Some(F::DeleteTemp)),
            (REC, E::Fault(Fault::DiskLow), STOPPING, Some(F::Flush)),
            (PAU, E::Fault(Fault::StreamEnded), STOPPING, Some(F::Flush)),
            (REC, E::Fault(Fault::Mic), STOPPING, Some(F::Flush)),
            (REC, E::StartFailed(Fault::Mic), S::Error(Fault::Mic), None),
            (
                STOPPING,
                E::FlushFailed(Fault::WriteFailed),
                S::Error(Fault::WriteFailed),
                None,
            ),
            (REC, E::Fault(Fault::WriteFailed), STOPPING, Some(F::Flush)),
            (
                S::Idle,
                E::Offer { audio_only: false },
                S::Result { audio_only: false },
                None,
            ),
            (PAU, E::Fault(Fault::Refused), STOPPING, Some(F::Flush)),
            (
                S::Asking,
                E::Fault(Fault::Refused),
                S::Error(Fault::Refused),
                None,
            ),
            (
                S::Error(Fault::Mic),
                E::Dismiss,
                S::Idle,
                Some(F::DeleteTemp),
            ),
            (
                S::Error(Fault::Refused),
                E::Esc,
                S::Idle,
                Some(F::DeleteTemp),
            ),
        ];
        for (from, event, to, effect) in rows {
            assert_eq!(from.on(event), (to, effect), "{from:?} --{event:?}-->");
        }
    }

    /// Esc in every state: a pick or a countdown is cancelled, a recording or a pause does
    /// NOT move (it neither stops nor asks), nothing else moves.
    #[test]
    fn esc_in_each_state() {
        let cases = [
            (S::Idle, S::Idle),
            (S::Picking, S::Idle),
            (S::Asking, S::Idle),
            (S::Countdown(2), S::Idle),
            (REC, REC),
            (PAU, PAU),
            (STOPPING, STOPPING),
            (S::Result { audio_only: true }, S::Idle),
            (S::Error(Fault::Mic), S::Idle),
        ];
        for (from, to) in cases {
            assert_eq!(from.on(E::Esc).0, to, "Esc in {from:?}");
        }
        // no effect either: nothing is flushed, nothing deleted
        assert_eq!(REC.on(E::Esc).1, None);
        assert_eq!(PAU.on(E::Esc).1, None);
    }

    #[test]
    fn a_second_stop_does_nothing() {
        let (stopping, effect) = REC.on(E::Stop);
        assert_eq!(effect, Some(F::Flush));
        assert_eq!(stopping.on(E::Stop), (stopping, None));
        assert_eq!(stopping.on(E::Key), (stopping, None));
        // the flush finishing still lands in the thumbnail
        assert_eq!(stopping.on(E::Flushed).0, S::Result { audio_only: false });
    }

    #[test]
    fn the_start_key_during_the_countdown_starts_nothing() {
        for n in 1..=3 {
            assert_eq!(S::Countdown(n).on(E::Key), (S::Countdown(n), None));
            assert_eq!(S::Countdown(n).on(E::StartVideo), (S::Countdown(n), None));
            assert_eq!(S::Countdown(n).on(E::StartAudio), (S::Countdown(n), None));
        }
    }

    #[test]
    fn audio_only_stays_audio_only_to_the_thumbnail() {
        let (rec, _) = S::Idle.on(E::StartAudio);
        let (paused, _) = rec.on(E::Pause);
        let (stopping, _) = paused.on(E::Stop);
        let (result, _) = stopping.on(E::Flushed);
        assert_eq!(result, S::Result { audio_only: true });
        assert!(result.audio_only() && !REC.audio_only());
    }

    #[test]
    fn a_new_recording_starts_from_the_thumbnail_or_the_error_card() {
        let result = S::Result { audio_only: false };
        assert_eq!(result.on(E::StartVideo).0, S::Picking);
        assert_eq!(
            S::Error(Fault::Mic).on(E::StartAudio).0,
            S::Recording { audio_only: true }
        );
    }

    #[test]
    fn meaningless_events_change_nothing() {
        let all = [
            E::Pause,
            E::Resume,
            E::Stop,
            E::Key,
            E::Discard,
            E::Flushed,
            E::Tick,
            E::Skip,
            E::Allowed,
            E::Denied,
        ];
        for event in all {
            assert_eq!(S::Idle.on(event), (S::Idle, None), "Idle {event:?}");
        }
        assert_eq!(REC.on(E::Resume), (REC, None));
        assert_eq!(PAU.on(E::Pause), (PAU, None));
        assert_eq!(REC.on(E::StartVideo), (REC, None));
    }

    #[test]
    fn faults_before_recording_go_to_the_card() {
        for from in [S::Picking, S::Asking, S::Countdown(2)] {
            assert_eq!(
                from.on(E::Fault(Fault::Refused)).0,
                S::Error(Fault::Refused)
            );
        }
        assert!(REC.is_live() && PAU.is_live());
        assert!(!STOPPING.is_live() && !S::Idle.is_live());
    }
}
