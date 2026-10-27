//! Screen capture: one frozen frame of a monitor (`choose`: the active window's,
//! else the pointer's), or of every monitor at once (snip).
//! One file per platform: `x11.rs` (Linux X11/XWayland), `portal.rs` (Linux Wayland),
//! `win.rs` (Windows, xcap). Scrolling capture: `stitch.rs` + `scroll.rs`.
pub mod cursor;
#[cfg(target_os = "linux")]
pub mod portal;
pub mod scroll;
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
pub fn monitor_under_cursor() -> Result<Frame, String> {
    monitor_of(None)
}

/// Captures the monitor of the active window (most of it), for Circle to Search,
/// translate and the quick actions; the pointer's monitor when no real window is
/// active (the desktop, the taskbar, one of ours).
pub fn active_monitor() -> Result<Frame, String> {
    monitor_of(window_rect::active_rect())
}

/// Snip: every monitor at once (frozen together), and which one the pointer is on.
pub fn every_monitor() -> Result<(Vec<Frame>, usize), String> {
    if let Some(frame) = whole_desktop() {
        return Ok((vec![frame?], 0));
    }
    let monitors = platform::monitors()?;
    let on = choose(&monitors, None, platform::pointer()).ok_or("no monitor found")?;
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

fn whole_desktop() -> Option<Result<Frame, String>> {
    // ponytail: test hook. WSLg and headless CI can't capture the screen, so a PNG
    // stands in; keep it for UI snapshot tests, never document it for users.
    if let Some(path) = std::env::var_os("OPENCIRCLE_TEST_FRAME") {
        return Some(from_png(&path));
    }
    #[cfg(target_os = "linux")]
    if wayland_session() {
        return Some(portal::capture());
    }
    None
}

/// The monitor holding most of `area` (physical desktop px), else the pointer's, captured.
pub fn monitor_of(area: Option<WinRect>) -> Result<Frame, String> {
    if let Some(frame) = whole_desktop() {
        return frame;
    }
    let monitors = platform::monitors()?;
    let at = choose(&monitors, area, platform::pointer()).ok_or("no monitor found")?;
    platform::grab(&monitors[at])
}

/// The monitor holding most of the `active` window, else the one under the
/// `pointer`, else the primary one, else the first.
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

/// The desktop refused the capture (`monitor_under_cursor`'s error): on Wayland it
/// wants permission, and GNOME asks only while one of our windows has the focus.
pub fn refused(error: &str) -> bool {
    #[cfg(target_os = "linux")]
    return error.contains(portal::REFUSED);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = error;
        false
    }
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
    fn the_active_windows_monitor_wins_then_the_pointers_then_the_primary() {
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
        let on_left = Some((-1000, 700));
        assert_eq!(choose(&monitors, None, on_left), Some(1));
        assert_eq!(choose(&monitors, None, Some((100, 100))), Some(0));
        // Focus in a window on screen 2, the pointer on screen 1: screen 2.
        let window = (-1800, 300, 1200, 800);
        assert_eq!(choose(&monitors, Some(window), Some((100, 100))), Some(1));
        // Across both: the one holding most of it.
        assert_eq!(
            choose(&monitors, Some((-300, 300, 1000, 500)), on_left),
            Some(0)
        );
        // No active window (the desktop), or one off every screen: the pointer.
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
