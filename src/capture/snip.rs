//! What snip mode captures before the overlay shows: every monitor frozen
//! together, the windows (topmost first) and the one the user is in.

use std::time::Instant;

use super::{Frame, every_monitor, window_rect};

pub struct Snapshot {
    pub frames: Vec<Frame>,
    /// Index of the frame the pointer is on.
    pub on: usize,
    pub windows: Vec<window_rect::Win>,
    pub foreground: Option<isize>,
}

/// Every monitor and every window; the pointer is drawn on the frames when `cursor`.
/// The windows are listed on a thread while the monitors are grabbed (the
/// overlay is still hidden for both): the freeze waits for the slower one only.
pub fn snapshot(cursor: bool) -> Result<Snapshot, String> {
    let start = Instant::now();
    let (grabbed, (windows, foreground, listed)) = std::thread::scope(|s| {
        let listing = s.spawn(|| {
            let foreground = window_rect::foreground();
            let windows = window_rect::list();
            (windows, foreground, start.elapsed())
        });
        let grabbed = every_monitor().map(|g| (g, start.elapsed()));
        let listed = listing
            .join()
            .unwrap_or_else(|_| (Vec::new(), None, start.elapsed()));
        (grabbed, listed)
    });
    let ((mut frames, on), grab) = grabbed?;
    if cursor {
        frames.iter_mut().for_each(super::cursor::draw);
    }
    crate::log!(
        "[opencircle] snip snapshot: monitors {} ms, windows {} ms ({})",
        grab.as_millis(),
        listed.as_millis(),
        windows.len()
    );
    Ok(Snapshot {
        frames,
        on,
        windows,
        foreground,
    })
}
