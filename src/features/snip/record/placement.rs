//! Where the recording controls live (owner, Q1): the R1 bar when this OS can keep
//! our window out of what is recorded, the tray's recording state when it cannot
//! and the whole screen is recorded (Linux). Pure: the capability comes in as a
//! flag (`shared::platform::CAN_KEEP_WINDOWS_OUT_OF_CAPTURE`).
use super::state::State;

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
    /// Mic toggle and the device.
    Mic,
}

impl Row {
    /// Every row (the desktop's menu builds one item for each).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub const ALL: [Row; 5] = [Row::Pause, Row::Resume, Row::Stop, Row::Discard, Row::Mic];

    /// Its text; the mic row says what a press does.
    pub fn label(self, muted: bool) -> &'static str {
        match (self, muted) {
            (Row::Pause, _) => crate::t!(rec_pause),
            (Row::Resume, _) => crate::t!(rec_resume),
            (Row::Stop, _) => crate::t!(rec_stop),
            (Row::Discard, _) => crate::t!(rec_discard),
            (Row::Mic, false) => crate::t!(rec_mute),
            (Row::Mic, true) => crate::t!(rec_unmute),
        }
    }
}

/// The rows while `state` runs (none when nothing is recording).
pub fn tray_rows(state: State) -> Vec<Row> {
    match state {
        State::Recording { .. } => vec![Row::Pause, Row::Stop, Row::Discard, Row::Mic],
        State::Paused { .. } => vec![Row::Resume, Row::Stop, Row::Discard, Row::Mic],
        _ => Vec::new(),
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

    #[test]
    fn the_tray_menu_rows_follow_the_state() {
        let rec = State::Recording { audio_only: false };
        assert_eq!(
            tray_rows(rec),
            [Row::Pause, Row::Stop, Row::Discard, Row::Mic]
        );
        assert_eq!(
            tray_rows(State::Paused { audio_only: false })[0],
            Row::Resume
        );
        assert!(tray_rows(State::Idle).is_empty());
    }
}
