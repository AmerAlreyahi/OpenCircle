//! Screen capture: one frozen frame of the monitor under the cursor.
//! One file per platform: `x11.rs` (Linux X11/XWayland), `portal.rs` (Linux Wayland),
//! `win.rs` (Windows, xcap). Scrolling capture: `stitch.rs` + `scroll.rs`.
#[cfg(target_os = "linux")]
pub mod portal;
pub mod scroll;
pub mod stitch;
pub mod window_rect;

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
    #[cfg(target_os = "linux")]
    if wayland_session() {
        return portal::capture();
    }
    platform::capture()
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

/// A native Wayland session: X11 calls there only see XWayland windows.
#[cfg(target_os = "linux")]
pub fn wayland_session() -> bool {
    is_wayland(
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("XDG_SESSION_TYPE").as_deref(),
    )
}

#[cfg(target_os = "linux")]
fn is_wayland(display: Option<&std::ffi::OsStr>, session_type: Option<&std::ffi::OsStr>) -> bool {
    display.is_some_and(|d| !d.is_empty()) && session_type.is_some_and(|t| t == "wayland")
}

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

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_a_wayland_session() {
        use std::ffi::OsStr;
        let w = Some(OsStr::new("wayland-0"));
        assert!(is_wayland(w, Some(OsStr::new("wayland"))));
        assert!(!is_wayland(w, Some(OsStr::new("x11"))));
        assert!(!is_wayland(w, None)); // WSLg: no session type
        assert!(!is_wayland(None, Some(OsStr::new("wayland"))));
        assert!(!is_wayland(
            Some(OsStr::new("")),
            Some(OsStr::new("wayland"))
        ));
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
