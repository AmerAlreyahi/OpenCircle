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
/// The windows are listed on a thread while the monitors are grabbed: the
/// freeze waits for the slower one only. `chosen` gets the pointer's monitor
/// first (`every_monitor`): the overlay may show on it, unseen, meanwhile, so
/// the window the user is in is read before anything else (the overlay is
/// never listed, `window_rect::no_pick`, and a clear layered window is not in
/// a GDI grab).
pub fn snapshot(cursor: bool, chosen: impl FnOnce(&super::Monitor)) -> Result<Snapshot, String> {
    let start = Instant::now();
    let foreground = window_rect::foreground();
    let (grabbed, (windows, listed)) = std::thread::scope(|s| {
        let listing = s.spawn(|| (window_rect::list(), start.elapsed()));
        let grabbed = every_monitor(chosen).map(|g| (g, start.elapsed()));
        let listed = listing
            .join()
            .unwrap_or_else(|_| (Vec::new(), start.elapsed()));
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
