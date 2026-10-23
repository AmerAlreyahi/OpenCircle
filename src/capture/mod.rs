//! Screen capture: one frozen frame of the monitor under the cursor.
use xcap::Monitor;

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

/// Captures the monitor under the cursor (the primary one if the cursor is unknown).
pub fn monitor_under_cursor() -> Result<Frame, String> {
    // ponytail: test hook. WSLg and headless CI can't capture the screen, so a PNG
    // stands in; keep it for UI snapshot tests, never document it for users.
    if let Some(path) = std::env::var_os("OPENCIRCLE_TEST_FRAME") {
        return from_png(&path);
    }
    // xcap's Wayland path panics on compositors without wlr-screencopy (e.g. WSLg);
    // a failed capture must not kill the resident app.
    std::panic::catch_unwind(capture).unwrap_or_else(|_| Err("capture backend panicked".into()))
}

fn capture() -> Result<Frame, String> {
    let monitor = cursor_position()
        .and_then(|(x, y)| Monitor::from_point(x, y).ok())
        .map_or_else(primary_monitor, Ok)?;
    let image = monitor.capture_image().map_err(|e| e.to_string())?;
    Ok(Frame {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        x: monitor.x().map_err(|e| e.to_string())?,
        y: monitor.y().map_err(|e| e.to_string())?,
        scale: monitor.scale_factor().unwrap_or(1.0),
    })
}

fn from_png(path: &std::ffi::OsStr) -> Result<Frame, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut rgba = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader.next_frame(&mut rgba).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("test frame must be an 8-bit RGBA PNG".into());
    }
    rgba.truncate(info.buffer_size());
    Ok(Frame {
        rgba,
        width: info.width,
        height: info.height,
        x: 0,
        y: 0,
        scale: 1.0,
    })
}

fn primary_monitor() -> Result<Monitor, String> {
    let monitors = Monitor::all().map_err(|e| e.to_string())?;
    let primary = monitors
        .iter()
        .position(|m| m.is_primary().unwrap_or(false))
        .unwrap_or(0);
    monitors
        .into_iter()
        .nth(primary)
        .ok_or_else(|| "no monitor found".into())
}

/// Global cursor position in physical desktop pixels.
// ponytail: on Linux this is X11/XWayland only; native Wayland has no global
// cursor query (the S2 portal path picks the monitor there).
#[cfg(target_os = "linux")]
fn cursor_position() -> Option<(i32, i32)> {
    let (conn, screen) = xcb::Connection::connect(None).ok()?;
    let root = conn.get_setup().roots().nth(screen as usize)?.root();
    let cookie = conn.send_request(&xcb::x::QueryPointer { window: root });
    let reply = conn.wait_for_reply(cookie).ok()?;
    Some((reply.root_x().into(), reply.root_y().into()))
}

#[cfg(windows)]
fn cursor_position() -> Option<(i32, i32)> {
    let mut p = windows::Win32::Foundation::POINT::default();
    // SAFETY: GetCursorPos only writes into the POINT we own.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn cursor_position() -> Option<(i32, i32)> {
    None
}
