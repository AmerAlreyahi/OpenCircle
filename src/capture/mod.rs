//! Screen capture: one frozen frame of a monitor (`choose`: the active window's,
//! else the pointer's) first and then the others (Circle to Search), or of every
//! monitor at once (snip).
//! One file per platform: `x11.rs` (Linux X11/XWayland), `portal.rs` (Linux Wayland),
//! `win.rs` (Windows, xcap). Scrolling capture: `stitch.rs` + `scroll.rs`.
pub mod cursor;
pub mod live;
#[cfg(target_os = "linux")]
pub mod portal;
pub mod scroll;
pub mod snip;
pub mod stitch;
pub mod uia;
pub mod window_rect;

use window_rect::WinRect;

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
use x11 as platform;

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as platform;

/// Why a capture failed. `Refused` is the one the UI acts on: the desktop said no
/// (Wayland wants permission, and GNOME asks only while one of our windows has the
/// focus), so the permission prompt opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    // Only the Wayland portal refuses.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Refused(String),
    Failed(String),
}

impl CaptureError {
    pub fn is_refused(&self) -> bool {
        matches!(self, CaptureError::Refused(_))
    }
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::Refused(why) | CaptureError::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<String> for CaptureError {
    fn from(why: String) -> Self {
        CaptureError::Failed(why)
    }
}

impl From<&str> for CaptureError {
    fn from(why: &str) -> Self {
        CaptureError::Failed(why.into())
    }
}

impl From<CaptureError> for String {
    fn from(e: CaptureError) -> String {
        e.to_string()
    }
}

/// A captured monitor: RGBA pixels plus where the monitor sits on the desktop.
pub struct Frame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Monitor origin in physical desktop pixels.
    pub x: i32,
    pub y: i32,
    pub scale: f32,
}

/// A monitor: where it sits on the desktop (physical px), physical px per
/// logical px, and whether it is the primary one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub rect: WinRect,
    pub scale: f32,
    pub primary: bool,
}

/// Every monitor (diagnostics); empty where the platform can't list them.
pub fn monitor_list() -> Vec<Monitor> {
    platform::monitors().unwrap_or_default()
}

/// Captures the monitor under the cursor (the primary one if the cursor is unknown).
pub fn monitor_under_cursor() -> Result<Frame, CaptureError> {
    monitor_of(None)
}

/// Circle to Search: the monitor the MOUSE is on (owner, 2026-10-28; not the
/// active window's: the primary when the pointer is unknown) handed to `each`
/// as soon as it is grabbed (its overlay, glow and bar open on it, as fast as
/// with one screen), then with `all` every other monitor, grabbed together
/// after it. A monitor that fails is left out (logged); only the pointer's
/// failing is an error, and then nothing else is grabbed. Wayland and the
/// test frame: the one whole desktop, as before.
pub fn mouse_first(all: bool, each: impl FnMut(Result<Frame, CaptureError>)) {
    let mut each = each;
    if let Some(frame) = whole_desktop() {
        return each(frame);
    }
    let monitors = match platform::monitors() {
        Ok(monitors) => monitors,
        Err(e) => return each(Err(e.into())),
    };
    match choose(&monitors, None, platform::pointer()) {
        Some(at) => grab_first(&monitors, at, all, platform::grab, each),
        None => each(Err("no monitor found".into())),
    }
}

/// `mouse_first` past choosing: `monitors[at]` first, then (`all`) the rest.
fn grab_first(
    monitors: &[Monitor],
    at: usize,
    all: bool,
    grab: impl Fn(&Monitor) -> Result<Frame, String> + Sync,
    mut each: impl FnMut(Result<Frame, CaptureError>),
) {
    let active = grab(&monitors[at]);
    let failed = active.is_err();
    each(active.map_err(CaptureError::from));
    if failed || !all {
        return;
    }
    // A thread each: one capture takes tens of ms.
    let grab = &grab;
    let rest = std::thread::scope(|s| {
        let grabs: Vec<_> = (monitors.iter().enumerate())
            .filter(|&(i, _)| i != at)
            .map(|(_, m)| s.spawn(move || grab(m)))
            .collect();
        (grabs.into_iter())
            .map(|g| g.join().unwrap_or_else(|_| Err("capture failed".into())))
            .collect::<Vec<_>>()
    });
    for frame in rest {
        match frame {
            Ok(frame) => each(Ok(frame)),
            Err(e) => crate::log!("[opencircle] a monitor's capture failed: {e}"),
        }
    }
}

/// Snip: every monitor at once (frozen together), and which one the pointer is on.
/// `chosen` gets the pointer's monitor before any is grabbed (the overlay can
/// get ready on it meanwhile; not called for the test frame or a portal).
pub fn every_monitor(chosen: impl FnOnce(&Monitor)) -> Result<(Vec<Frame>, usize), CaptureError> {
    if let Some(frame) = whole_desktop() {
        return Ok((vec![frame?], 0));
    }
    let monitors = platform::monitors()?;
    let on = choose(&monitors, None, platform::pointer()).ok_or("no monitor found")?;
    chosen(&monitors[on]);
    // A thread each: one capture takes tens of ms.
    let frames = std::thread::scope(|s| {
        let grabs: Vec<_> = (monitors.iter())
            .map(|m| s.spawn(move || platform::grab(m)))
            .collect();
        (grabs.into_iter())
            .map(|g| g.join().unwrap_or_else(|_| Err("capture failed".into())))
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok((frames, on))
}

/// The pointer in physical desktop px (`None`: unknown, e.g. on Wayland).
pub fn pointer() -> Option<(i32, i32)> {
    if whole_desktop_only() {
        return None;
    }
    platform::pointer()
}

/// The test frame, or (Wayland) the portal's whole desktop: no monitor to choose.
fn whole_desktop_only() -> bool {
    #[cfg(target_os = "linux")]
    if wayland_session() {
        return true;
    }
    std::env::var_os("OPENCIRCLE_TEST_FRAME").is_some()
}

fn whole_desktop() -> Option<Result<Frame, CaptureError>> {
    // ponytail: test hook. WSLg and headless CI can't capture the screen, so a PNG
    // stands in; keep it for UI snapshot tests, never document it for users.
    if let Some(path) = std::env::var_os("OPENCIRCLE_TEST_FRAME") {
        return Some(from_png(&path).map_err(CaptureError::from));
    }
    #[cfg(target_os = "linux")]
    if wayland_session() {
        return Some(portal::capture());
    }
    None
}

/// The monitor holding most of `area` (physical desktop px), else the pointer's, captured.
pub fn monitor_of(area: Option<WinRect>) -> Result<Frame, CaptureError> {
    if let Some(frame) = whole_desktop() {
        return frame;
    }
    let monitors = platform::monitors()?;
    let at = choose(&monitors, area, platform::pointer()).ok_or("no monitor found")?;
    Ok(platform::grab(&monitors[at])?)
}

/// The monitor holding most of the area `active` (a scrolling capture's
/// region; Circle to Search passes none), else the one under the `pointer`,
/// else the primary one, else the first.
pub fn choose(
    monitors: &[Monitor],
    active: Option<WinRect>,
    pointer: Option<(i32, i32)>,
) -> Option<usize> {
    let span = |a: i32, al: u32, b: i32, bl: u32| {
        let (a, b) = (i64::from(a), i64::from(b));
        ((a + i64::from(al)).min(b + i64::from(bl)) - a.max(b)).max(0)
    };
    let shared = |m: &Monitor, (x, y, w, h): WinRect| {
        let (mx, my, mw, mh) = m.rect;
        span(x, w, mx, mw) * span(y, h, my, mh)
    };
    let by_window = active.and_then(|r| {
        // the first of equals
        let (at, area) = (monitors.iter().map(|m| shared(m, r)).enumerate())
            .rev()
            .max_by_key(|&(_, area)| area)?;
        (area > 0).then_some(at)
    });
    let by_pointer = || {
        let (x, y) = pointer?;
        monitors.iter().position(|m| shared(m, (x, y, 1, 1)) > 0)
    };
    by_window
        .or_else(by_pointer)
        .or_else(|| monitors.iter().position(|m| m.primary))
        .or((!monitors.is_empty()).then_some(0))
}

#[cfg(target_os = "linux")]
pub use crate::sys::session::wayland_session;

pub(crate) fn from_png(path: &std::ffi::OsStr) -> Result<Frame, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    decode_png(std::io::BufReader::new(file))
}

/// An RGB or RGBA PNG as RGBA (also the song card's cover).
pub(crate) fn decode_png(png: impl std::io::BufRead + std::io::Seek) -> Result<Frame, String> {
    let mut decoder = png::Decoder::new(png);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    // Screenshot portals write RGB or RGBA PNGs.
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let (pixels, _) = buf.as_chunks::<3>();
            pixels
                .iter()
                .flat_map(|&[r, g, b]| [r, g, b, 255])
                .collect()
        }
        other => return Err(format!("unsupported PNG colour type {other:?}")),
    };
    Ok(Frame {
        rgba,
        width: info.width,
        height: info.height,
        x: 0,
        y: 0,
        scale: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointers_monitor_opens_and_an_area_takes_the_monitor_holding_most_of_it() {
        let m = |rect, primary| Monitor {
            rect,
            scale: 1.0,
            primary,
        };
        // A laptop (primary, 150 %) and, LEFT of it, a 1080p screen.
        let monitors = [
            m((0, 0, 2880, 1800), true),
            m((-1920, 200, 1920, 1080), false),
        ];
        // Circle to Search (`mouse_first`: no area): the pointer's screen.
        let on_left = Some((-1000, 700));
        assert_eq!(choose(&monitors, None, on_left), Some(1));
        assert_eq!(choose(&monitors, None, Some((100, 100))), Some(0));
        // A scrolling capture's region on screen 2, the pointer on screen 1: screen 2.
        let window = (-1800, 300, 1200, 800);
        assert_eq!(choose(&monitors, Some(window), Some((100, 100))), Some(1));
        // Across both: the one holding most of it.
        assert_eq!(
            choose(&monitors, Some((-300, 300, 1000, 500)), on_left),
            Some(0)
        );
        // An area off every screen: the pointer.
        assert_eq!(choose(&monitors, Some((9000, 0, 10, 10)), on_left), Some(1));
        // Pointer unknown or in a gap (above screen 2): the primary.
        assert_eq!(choose(&monitors, None, Some((-1000, 0))), Some(0));
        assert_eq!(choose(&monitors, None, None), Some(0));
        let no_primary = [m((0, 0, 10, 10), false), m((10, 0, 10, 10), false)];
        assert_eq!(choose(&no_primary, None, None), Some(0));
        // Equal shares: the first.
        assert_eq!(choose(&no_primary, Some((5, 0, 10, 10)), None), Some(0));
        assert_eq!(choose(&[], None, None), None);
    }

    #[test]
    fn the_mouses_monitor_comes_first_and_a_failed_one_keeps_the_rest() {
        let m = |x| Monitor {
            rect: (x, 0, 10, 10),
            scale: 1.0,
            primary: x == 0,
        };
        let monitors = [m(-10), m(0), m(10), m(20)];
        let grab = |mon: &Monitor| match mon.rect.0 {
            10 => Err("gone".to_string()),
            x => {
                if x == 20 {
                    // slow: the others still wait for the active one
                    std::thread::sleep(std::time::Duration::from_millis(30));
                }
                Ok(Frame {
                    rgba: Vec::new(),
                    width: 10,
                    height: 10,
                    x,
                    y: 0,
                    scale: 1.0,
                })
            }
        };
        let run = |at, all| {
            let mut got = Vec::new();
            grab_first(&monitors, at, all, grab, |f| got.push(f.map(|f| f.x)));
            got
        };
        // the slow first one first, then the rest; the failed one is left out
        assert_eq!(run(3, true), [Ok(20), Ok(-10), Ok(0)]);
        assert_eq!(run(1, true), [Ok(0), Ok(-10), Ok(20)]);
        // the first one failing: that error, nothing else grabbed
        assert_eq!(run(2, true), [Err(CaptureError::Failed("gone".into()))]);
        // one screen asked: the first one only
        assert_eq!(run(0, false), [Ok(-10)]);
    }

    #[test]
    fn reads_rgb_pngs_as_opaque_rgba() {
        let path = std::env::temp_dir().join(format!("opencircle-test-{}.png", std::process::id()));
        let mut encoder = png::Encoder::new(std::fs::File::create(&path).unwrap(), 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[1, 2, 3, 4, 5, 6]).unwrap();
        writer.finish().unwrap();
        let frame = from_png(path.as_os_str());
        std::fs::remove_file(&path).unwrap();
        let frame = frame.unwrap();
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(frame.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
    }
}
