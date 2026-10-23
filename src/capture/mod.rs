//! Screen capture: one frozen frame of the monitor under the cursor.
//! One file per platform: `x11.rs` (Linux X11/XWayland), `win.rs` (Windows, xcap).
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

/// Captures the monitor under the cursor (the primary one if the cursor is unknown).
pub fn monitor_under_cursor() -> Result<Frame, String> {
    // ponytail: test hook. WSLg and headless CI can't capture the screen, so a PNG
    // stands in; keep it for UI snapshot tests, never document it for users.
    if let Some(path) = std::env::var_os("OPENCIRCLE_TEST_FRAME") {
        return from_png(&path);
    }
    platform::capture()
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
