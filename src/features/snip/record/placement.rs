//! Where the recording controls live (owner, Q1): the R1 bar when this OS can keep
//! our window out of what is recorded, the tray's recording state when it cannot
//! and the whole screen is recorded (Linux). Pure: the capability comes in as a
//! flag (`shared::platform::CAN_KEEP_WINDOWS_OUT_OF_CAPTURE`).
use super::state::{Fault, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    WholeScreen,
    #[cfg_attr(not(test), allow(dead_code))] // R-04's pick
    Area,
    #[cfg_attr(not(test), allow(dead_code))]
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The draggable bar (top centre of the recorded screen).
    Bar,
    /// The tray icon: red dot + live time, and its menu rows.
    Tray,
}

/// An area or a window can leave the bar outside the picture on every platform;
/// the whole screen only where our window can be kept out of a capture.
pub fn surface(can_keep_out: bool, target: Target) -> Surface {
    match (can_keep_out, target) {
        (false, Target::WholeScreen) => Surface::Tray,
        _ => Surface::Bar,
    }
}

/// The rows of the tray's recording menu, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Pause,
    Resume,
    /// "Stop and save" (shows the start key).
    Stop,
    Discard,
    /// Mic toggle.
    Mic,
    /// The microphone in use (a note).
    Device,
    /// Ready: start recording / cancel the pick.
    Record,
    Cancel,
    /// Esc's question: keep what was recorded (Discard is the row above).
    Keep,
    /// Error card: what went wrong (notes), then the ways out.
    ErrMic,
    ErrRefused,
    Retry,
    /// Record without the microphone.
    Fix,
    Dismiss,
}

/// What the row texts depend on.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    pub muted: bool,
    pub audio_only: bool,
    /// The microphone's name ("Off" / empty: none).
    pub device: String,
}

impl Row {
    /// Every row (the desktop's menu builds one item for each).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub const ALL: [Row; 14] = [
        Row::Pause,
        Row::Resume,
        Row::Stop,
        Row::Discard,
        Row::Mic,
        Row::Device,
        Row::Record,
        Row::Cancel,
        Row::Keep,
        Row::ErrMic,
        Row::ErrRefused,
        Row::Retry,
        Row::Fix,
        Row::Dismiss,
    ];

    /// A note, not a button: shown greyed.
    pub fn is_note(self) -> bool {
        matches!(self, Row::Device | Row::ErrMic | Row::ErrRefused)
    }

    /// Its text; the mic row says what a press does, Stop shows the start key.
    pub fn label(self, ctx: &Ctx) -> String {
        match self {
            Row::Pause => crate::t!(rec_pause).into(),
            Row::Resume => crate::t!(rec_resume).into(),
            Row::Stop => {
                let key = if ctx.audio_only {
                    "Alt+Shift+M"
                } else {
                    "Alt+Shift+R"
                };
                format!("{} ({key})", crate::t!(rec_stop))
            }
            Row::Discard => crate::t!(rec_discard).into(),
            Row::Mic if ctx.muted => crate::t!(rec_unmute).into(),
            Row::Mic => crate::t!(rec_mute).into(),
            Row::Device if ctx.muted || ctx.device.is_empty() => crate::t!(rec_off).into(),
            Row::Device => ctx.device.clone(),
            Row::Record => crate::t!(rec_record).into(),
            Row::Cancel | Row::Dismiss => crate::t!(rec_cancel).into(),
            Row::Keep => crate::t!(rec_keep).into(),
            Row::ErrMic => crate::t!(rec_err_mic).into(),
            Row::ErrRefused => crate::t!(rec_err_refused).into(),
            Row::Retry => crate::t!(rec_try_again).into(),
            Row::Fix => crate::t!(rec_err_mic_fix).into(),
        }
    }
}

/// The rows while `state` runs (none when nothing is on screen: idle, saving, the thumbnail).
pub fn tray_rows(state: State) -> Vec<Row> {
    match state {
        State::Picking | State::Asking | State::Countdown(_) => vec![Row::Record, Row::Cancel],
        State::Recording { .. } => vec![Row::Pause, Row::Stop, Row::Discard, Row::Mic, Row::Device],
        State::Paused { .. } => vec![Row::Resume, Row::Stop, Row::Discard, Row::Mic, Row::Device],
        State::AskKeep { .. } => vec![Row::Keep, Row::Discard],
        State::Error(Fault::Mic) => vec![Row::ErrMic, Row::Retry, Row::Fix, Row::Dismiss],
        State::Error(_) => vec![Row::ErrRefused, Row::Retry, Row::Dismiss],
        State::Idle | State::Stopping { .. } | State::Result { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_or_tray_by_capability_and_target() {
        use Surface::*;
        use Target::*;
        let table = [
            (true, WholeScreen, Bar),
            (true, Area, Bar),
            (true, Window, Bar),
            (false, WholeScreen, Tray),
            (false, Area, Bar),
            (false, Window, Bar),
        ];
        for (can, target, want) in table {
            assert_eq!(surface(can, target), want, "{can} {target:?}");
        }
    }

    /// R02-21: every state that shows something has a way out in the tray.
    #[test]
    fn the_tray_menu_rows_follow_the_state() {
        let rec = State::Recording { audio_only: false };
        let keep = State::AskKeep {
            audio_only: false,
            was_paused: false,
        };
        let table: [(State, &[Row]); 8] = [
            (
                rec,
                &[Row::Pause, Row::Stop, Row::Discard, Row::Mic, Row::Device],
            ),
            (
                State::Paused { audio_only: false },
                &[Row::Resume, Row::Stop, Row::Discard, Row::Mic, Row::Device],
            ),
            (State::Picking, &[Row::Record, Row::Cancel]),
            (State::Countdown(2), &[Row::Record, Row::Cancel]),
            (keep, &[Row::Keep, Row::Discard]),
            (
                State::Error(Fault::Mic),
                &[Row::ErrMic, Row::Retry, Row::Fix, Row::Dismiss],
            ),
            (
                State::Error(Fault::Refused),
                &[Row::ErrRefused, Row::Retry, Row::Dismiss],
            ),
            (State::Idle, &[]),
        ];
        for (state, want) in table {
            assert_eq!(tray_rows(state), want, "{state:?}");
        }
        assert!(tray_rows(State::Stopping { audio_only: false }).is_empty());
    }

    /// R02-22: Stop shows the start key (R for video, M for audio); the device row names the mic.
    #[test]
    fn stop_shows_its_key_and_the_device_row_the_mic() {
        let video = Ctx::default();
        let audio = Ctx {
            audio_only: true,
            device: "Built-in".into(),
            ..Ctx::default()
        };
        assert!(Row::Stop.label(&video).ends_with("(Alt+Shift+R)"));
        assert!(Row::Stop.label(&audio).ends_with("(Alt+Shift+M)"));
        assert_eq!(Row::Device.label(&audio), "Built-in");
        let muted = Ctx {
            muted: true,
            ..audio
        };
        assert_eq!(Row::Device.label(&muted), crate::t!(rec_off));
    }
}
