//! Linux capture via X11 / XWayland: GetImage of the root window, cropped to the
//! RandR monitor under the cursor. Native Wayland goes through the portal (S2).
use xcb::{randr, x};

use super::Frame;

pub fn capture() -> Result<Frame, String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("no X11 display; Wayland capture comes via the desktop portal (S2)".into());
    }
    let err = |e: xcb::Error| format!("X11 capture failed: {e:?}");
    let (conn, screen) =
        xcb::Connection::connect_with_extensions(None, &[xcb::Extension::RandR], &[])
            .map_err(|e| format!("X11 connect failed: {e}"))?;
    let setup = conn.get_setup();
    let root = setup
        .roots()
        .nth(screen as usize)
        .ok_or("no X11 screen")?
        .root();

    let pointer = conn
        .wait_for_reply(conn.send_request(&x::QueryPointer { window: root }))
        .map_err(err)?;
    let monitors = conn
        .wait_for_reply(conn.send_request(&randr::GetMonitors {
            window: root,
            get_active: true,
        }))
        .map_err(err)?;
    let rects: Vec<Rect> = monitors
        .monitors()
        .map(|m| (m.x(), m.y(), m.width(), m.height(), m.primary()))
        .collect();
    let (mx, my, width, height, _) = rects
        [pick_monitor(&rects, pointer.root_x(), pointer.root_y()).ok_or("no monitor found")?];

    let image = conn
        .wait_for_reply(conn.send_request(&x::GetImage {
            format: x::ImageFormat::ZPixmap,
            drawable: x::Drawable::Window(root),
            x: mx,
            y: my,
            width,
            height,
            plane_mask: u32::MAX,
        }))
        .map_err(err)?;
    let (width, height) = (u32::from(width), u32::from(height));
    // ponytail: only 32-bit little-endian BGRX (every current X server); add the
    // 16-bit and big-endian cases if someone reports a garbled frame.
    if !matches!(image.depth(), 24 | 32)
        || setup.image_byte_order() != x::ImageOrder::LsbFirst
        || image.data().len() != (width * height * 4) as usize
    {
        return Err(format!(
            "unsupported X11 pixel format (depth {})",
            image.depth()
        ));
    }
    Ok(Frame {
        rgba: bgrx_to_rgba(image.data()),
        width,
        height,
        x: mx.into(),
        y: my.into(),
        // ponytail: X11 has no per-monitor scale; read Xft.dpi if HiDPI X11 matters.
        scale: 1.0,
    })
}

/// x, y, width, height, primary.
type Rect = (i16, i16, u16, u16, bool);

/// The monitor containing the point, else the primary one, else the first.
fn pick_monitor(rects: &[Rect], px: i16, py: i16) -> Option<usize> {
    let contains = |&(x, y, w, h, _): &Rect| {
        let (px, py) = (i32::from(px), i32::from(py));
        (i32::from(x)..i32::from(x) + i32::from(w)).contains(&px)
            && (i32::from(y)..i32::from(y) + i32::from(h)).contains(&py)
    };
    rects
        .iter()
        .position(contains)
        .or_else(|| rects.iter().position(|r| r.4))
        .or((!rects.is_empty()).then_some(0))
}

/// X11 ZPixmap (B, G, R, unused) → opaque RGBA.
fn bgrx_to_rgba(data: &[u8]) -> Vec<u8> {
    let (pixels, _) = data.as_chunks::<4>();
    pixels
        .iter()
        .flat_map(|&[b, g, r, _]| [r, g, b, 255])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_monitor_under_cursor_then_primary_then_first() {
        let rects = [(0, 0, 1920, 1080, false), (1920, 0, 2560, 1440, true)];
        assert_eq!(pick_monitor(&rects, 100, 100), Some(0));
        assert_eq!(pick_monitor(&rects, 1920, 0), Some(1));
        assert_eq!(pick_monitor(&rects, -5, -5), Some(1));
        assert_eq!(pick_monitor(&rects[..1], -5, -5), Some(0));
        assert_eq!(pick_monitor(&[], 0, 0), None);
    }

    #[test]
    fn converts_bgrx_to_opaque_rgba() {
        assert_eq!(
            bgrx_to_rgba(&[1, 2, 3, 0, 4, 5, 6, 9]),
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
    }
}
