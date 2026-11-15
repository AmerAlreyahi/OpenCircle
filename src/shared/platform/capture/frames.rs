//! An area of the screen frame after frame, as it is shown (long capture,
//! live translate): one interface, the OS calls behind it.
//! - Windows: a Desktop Duplication of the area's monitor (every frame the
//!   screen shows, none while it is still; windows that keep out of captures
//!   are left out); where it can't run (an area across monitors, a rotated
//!   one, a remote session) or stops, a GDI grab of the area each time.
//! - X11: the area through shared memory (`x11::Shm`, a few ms even for a
//!   big monitor), else a grab of its monitor cut to the area.
//! - Wayland: a ScreenCast stream of the area's monitor (`screencast::Cast`,
//!   the restore token: no dialog once allowed).
use std::io;
use std::time::Duration;

/// Where the frames of one area come from.
pub struct Frames {
    region: (i32, i32, u32, u32),
    #[cfg(windows)]
    dup: Option<super::win::Duplication>,
    #[cfg(target_os = "linux")]
    cast: Option<super::screencast::Cast>,
    #[cfg(target_os = "linux")]
    shm: Option<super::x11::Shm>,
    /// Why there is no duplication (it never started, or stopped).
    why: Option<String>,
    first: bool,
}

impl Frames {
    /// For `region` (physical desktop px: x, y, width, height).
    pub fn open(region: (i32, i32, u32, u32)) -> Frames {
        #[cfg(windows)]
        let (dup, why) = match super::win::Duplication::start(region) {
            Ok(dup) => (Some(dup), None),
            Err(e) => (None, Some(e)),
        };
        #[cfg(target_os = "linux")]
        let (cast, shm, why) = match super::wayland_session() {
            true => match super::screencast::Cast::start(region) {
                Ok(cast) => (Some(cast), None, None),
                Err(e) => (None, None, Some(e)),
            },
            false => match super::x11::Shm::start(region) {
                Ok(shm) => (None, Some(shm), None),
                Err(e) => (None, None, Some(e)),
            },
        };
        #[cfg(not(any(windows, target_os = "linux")))]
        let why = None;
        Frames {
            region,
            #[cfg(windows)]
            dup,
            #[cfg(target_os = "linux")]
            cast,
            #[cfg(target_os = "linux")]
            shm,
            why,
            first: true,
        }
    }

    /// Frames come as the screen shows them, none while it is still (no
    /// input check, no period needed).
    pub fn fast(&self) -> bool {
        #[cfg(windows)]
        return self.dup.is_some();
        #[cfg(target_os = "linux")]
        return self.cast.is_some();
        #[cfg(not(any(windows, target_os = "linux")))]
        false
    }

    /// The next frame (RGBA), waiting at most `wait` for one where frames
    /// come by themselves; none when nothing new was shown. The first is a
    /// grab (duplication only says what changes).
    pub fn next(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        let first = std::mem::take(&mut self.first);
        #[cfg(windows)]
        if !first && let Some(dup) = &mut self.dup {
            match dup.next(wait.as_millis().min(u32::MAX as u128) as u32) {
                Ok(frame) => return Ok(frame),
                Err(e) => {
                    crate::log!("[opencircle] screen frames stopped ({e}); grabs");
                    (self.why, self.dup) = (Some(e), None);
                }
            }
        }
        let _ = (first, wait);
        #[cfg(target_os = "linux")]
        if let Some(cast) = &mut self.cast {
            return cast.next(wait).map_err(io::Error::other);
        }
        #[cfg(target_os = "linux")]
        if super::wayland_session() {
            // (a screenshot each time would be a portal session each, or a dialog)
            let why = self.why.as_deref().unwrap_or("no screen cast");
            return Err(io::Error::other(why.to_string()));
        }
        #[cfg(target_os = "linux")]
        if let Some(shm) = &mut self.shm {
            match shm.grab() {
                Ok(frame) => return Ok(Some(frame)),
                Err(e) => {
                    crate::log!("[opencircle] shared-memory grabs stopped ({e}); grabs");
                    (self.why, self.shm) = (Some(e), None);
                }
            }
        }
        grab_area(self.region).map(Some)
    }

    /// For the log line.
    pub fn describe(&self) -> String {
        #[cfg(windows)]
        if let Some(dup) = &self.dup {
            return format!("screen frames ({} read)", dup.frames);
        }
        #[cfg(target_os = "linux")]
        if let Some(cast) = &self.cast {
            return format!("screen cast ({} frames read)", cast.frames);
        }
        #[cfg(target_os = "linux")]
        if self.shm.is_some() {
            return "shared-memory grabs".into();
        }
        match &self.why {
            Some(why) => format!("grabs (no screen frames: {why})"),
            None => "grabs".into(),
        }
    }

    /// Ms a frame took to read: the copy alone for duplicated frames (the
    /// caller's time includes waiting for them).
    pub fn ms_each(&self, total: Duration, n: u32) -> f64 {
        #[cfg(windows)]
        if let Some(dup) = &self.dup {
            return dup.copy.as_secs_f64() * 1000.0 / f64::from(dup.frames.max(1));
        }
        total.as_secs_f64() * 1000.0 / f64::from(n.max(1))
    }
}

/// The area alone, from the screen as shown (GDI; windows that keep out of
/// captures, ours among them, are not in it), RGBA.
#[cfg(windows)]
fn grab_area(region: (i32, i32, u32, u32)) -> io::Result<Vec<u8>> {
    super::win::grab_rect(region).map_err(|_| io::Error::other("the area could not be grabbed"))
}

/// The area cut out of its monitor's capture.
#[cfg(not(windows))]
fn grab_area(region: (i32, i32, u32, u32)) -> io::Result<Vec<u8>> {
    let (x, y, w, h) = region;
    let frame = super::monitor_of(Some(region)).map_err(io::Error::other)?;
    cut(&frame.rgba, frame.width, (x - frame.x, y - frame.y), (w, h))
        .ok_or_else(|| io::Error::other("the area left the monitor"))
}

/// `size` pixels at `at` out of a `width`-wide RGBA frame; `None` if not inside it.
#[cfg(any(not(windows), test))]
fn cut(rgba: &[u8], width: u32, at: (i32, i32), size: (u32, u32)) -> Option<Vec<u8>> {
    let (x, y) = (u32::try_from(at.0).ok()?, u32::try_from(at.1).ok()?);
    let height = rgba.len() as u32 / 4 / width.max(1);
    if x + size.0 > width || y + size.1 > height {
        return None;
    }
    let row = |r: u32| {
        let start = (((y + r) * width + x) * 4) as usize;
        &rgba[start..start + size.0 as usize * 4]
    };
    Some((0..size.1).flat_map(row).copied().collect())
}

#[test]
fn cut_takes_an_area_inside_the_frame_only() {
    // 3x2 frame, pixel (x, y) = [x, y, 0, 255].
    let frame: Vec<u8> = (0..2u8)
        .flat_map(|y| (0..3u8).flat_map(move |x| [x, y, 0, 255]))
        .collect();
    assert_eq!(
        cut(&frame, 3, (1, 1), (2, 1)),
        Some(vec![1, 1, 0, 255, 2, 1, 0, 255])
    );
    assert_eq!(cut(&frame, 3, (2, 0), (2, 1)), None);
    assert_eq!(cut(&frame, 3, (-1, 0), (1, 1)), None);
}
