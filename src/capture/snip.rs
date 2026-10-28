//! What snip mode captures before the overlay shows: every monitor frozen
//! together, the windows (topmost first) and the one the user is in.

use super::{Frame, every_monitor, window_rect};

pub struct Snapshot {
    pub frames: Vec<Frame>,
    /// Index of the frame the pointer is on.
    pub on: usize,
    pub windows: Vec<window_rect::Win>,
    pub foreground: Option<isize>,
}

/// Every monitor and every window; the pointer is drawn on the frames when `cursor`.
pub fn snapshot(cursor: bool) -> Result<Snapshot, String> {
    let (mut frames, on) = every_monitor()?;
    if cursor {
        frames.iter_mut().for_each(super::cursor::draw);
    }
    let foreground = window_rect::foreground(); // before the list, as the overlay is still hidden
    Ok(Snapshot {
        frames,
        on,
        windows: window_rect::list(),
        foreground,
    })
}
