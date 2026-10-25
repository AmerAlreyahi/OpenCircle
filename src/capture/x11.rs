//! Linux capture via X11 / XWayland: GetImage of the root window, cropped to one
//! RandR monitor. Native Wayland goes through the portal (S2).
use xcb::{randr, x};

use super::{Frame, Monitor};

fn connect() -> Result<(xcb::Connection, x::Window), String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("no X11 display; Wayland capture comes via the desktop portal (S2)".into());
    }
    let (conn, screen) =
        xcb::Connection::connect_with_extensions(None, &[xcb::Extension::RandR], &[])
            .map_err(|e| format!("X11 connect failed: {e}"))?;
    let root = conn
        .get_setup()
        .roots()
        .nth(screen as usize)
        .ok_or("no X11 screen")?
        .root();
    Ok((conn, root))
}

fn err(e: xcb::Error) -> String {
    format!("X11 capture failed: {e:?}")
}

pub fn monitors() -> Result<Vec<Monitor>, String> {
    let (conn, root) = connect()?;
    let monitors = conn
        .wait_for_reply(conn.send_request(&randr::GetMonitors {
            window: root,
            get_active: true,
        }))
        .map_err(err)?;
    Ok((monitors.monitors())
        .map(|m| Monitor {
            rect: (
                m.x().into(),
                m.y().into(),
                m.width().into(),
                m.height().into(),
            ),
            // ponytail: X11 has no per-monitor scale; read Xft.dpi if HiDPI X11 matters.
            scale: 1.0,
            primary: m.primary(),
        })
        .collect())
}

pub fn pointer() -> Option<(i32, i32)> {
    let (conn, root) = connect().ok()?;
    let pointer = conn
        .wait_for_reply(conn.send_request(&x::QueryPointer { window: root }))
        .ok()?;
    Some((pointer.root_x().into(), pointer.root_y().into()))
}

pub fn grab(m: &Monitor) -> Result<Frame, String> {
    let (conn, root) = connect()?;
    let (x, y, width, height) = m.rect;
    let short = |v: i32| i16::try_from(v).map_err(|_| "monitor off the X11 screen".to_string());
    let long = |v: u32| u16::try_from(v).map_err(|_| "monitor too big for X11".to_string());
    let image = conn
        .wait_for_reply(conn.send_request(&x::GetImage {
            format: x::ImageFormat::ZPixmap,
            drawable: x::Drawable::Window(root),
            x: short(x)?,
            y: short(y)?,
            width: long(width)?,
            height: long(height)?,
            plane_mask: u32::MAX,
        }))
        .map_err(err)?;
    // ponytail: only 32-bit little-endian BGRX (every current X server); add the
    // 16-bit and big-endian cases if someone reports a garbled frame.
    if !matches!(image.depth(), 24 | 32)
        || conn.get_setup().image_byte_order() != x::ImageOrder::LsbFirst
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
        x,
        y,
        scale: m.scale,
    })
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
    fn converts_bgrx_to_opaque_rgba() {
        assert_eq!(
            bgrx_to_rgba(&[1, 2, 3, 0, 4, 5, 6, 9]),
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
    }
}
