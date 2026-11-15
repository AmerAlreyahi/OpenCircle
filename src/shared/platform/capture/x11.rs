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

/// An area grabbed again and again through a shared-memory segment
/// (MIT-SHM): the server writes the pixels straight into our memory instead
/// of sending them down the socket (a 3440x1440 monitor: ~20 MB a grab).
/// Paced to at most `MAX_RATE` grabs a second (the display's rate). Detached
/// and freed when dropped.
pub struct Shm {
    conn: xcb::Connection,
    root: x::Window,
    seg: xcb::shm::Seg,
    addr: *mut u8,
    rect: (i16, i16, u16, u16),
    last: Option<std::time::Instant>,
}

/// Grabs a second at most (live translate's watch: the display's rate).
const MAX_RATE: u32 = 60;

impl Shm {
    /// For `region` (physical desktop px); fails where the server has no
    /// MIT-SHM (a remote display) or the area is off the X11 screen.
    pub fn start((x, y, w, h): (i32, i32, u32, u32)) -> Result<Shm, String> {
        use xcb::shm;
        if std::env::var_os("DISPLAY").is_none() {
            return Err("no X11 display".into());
        }
        let off = |_| "area off the X11 screen".to_string();
        let big = |_| "area too big for X11".to_string();
        let rect = (
            i16::try_from(x).map_err(off)?,
            i16::try_from(y).map_err(off)?,
            u16::try_from(w).map_err(big)?,
            u16::try_from(h).map_err(big)?,
        );
        if w == 0 || h == 0 {
            return Err("nothing to grab".into());
        }
        let (conn, screen) =
            xcb::Connection::connect_with_extensions(None, &[xcb::Extension::Shm], &[])
                .map_err(|e| format!("X11 connect failed: {e}"))?;
        let root = (conn.get_setup().roots().nth(screen as usize))
            .ok_or("no X11 screen")?
            .root();
        conn.wait_for_reply(conn.send_request(&shm::QueryVersion {}))
            .map_err(err)?;
        let size = w as usize * h as usize * 4;
        // SAFETY: a new private segment of `size` bytes, attached here; it is
        // marked for removal below (it goes once both sides detach).
        let (id, addr) = unsafe {
            let id = libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o600);
            if id < 0 {
                let e = std::io::Error::last_os_error();
                return Err(format!("no shared memory: {e}"));
            }
            let addr = libc::shmat(id, std::ptr::null(), 0);
            if addr as isize == -1 {
                let e = std::io::Error::last_os_error();
                libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
                return Err(format!("no shared memory: {e}"));
            }
            (id, addr as *mut u8)
        };
        let seg: shm::Seg = conn.generate_id();
        let attached = conn.send_and_check_request(&shm::Attach {
            shmseg: seg,
            shmid: id as u32,
            read_only: false,
        });
        // SAFETY: the id made above; both attachments keep it until detached.
        unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
        if let Err(e) = attached {
            // SAFETY: attached above, not used after this.
            unsafe { libc::shmdt(addr as *const libc::c_void) };
            return Err(format!("X11 shared memory refused: {e:?}"));
        }
        Ok(Shm {
            conn,
            root,
            seg,
            addr,
            rect,
            last: None,
        })
    }

    /// The area now, RGBA (after waiting out the pace).
    pub fn grab(&mut self) -> Result<Vec<u8>, String> {
        let every = std::time::Duration::from_secs(1) / MAX_RATE;
        if let Some(last) = self.last {
            std::thread::sleep(every.saturating_sub(last.elapsed()));
        }
        self.last = Some(std::time::Instant::now());
        let (x, y, w, h) = self.rect;
        let reply = (self.conn)
            .wait_for_reply(self.conn.send_request(&xcb::shm::GetImage {
                drawable: x::Drawable::Window(self.root),
                x,
                y,
                width: w,
                height: h,
                plane_mask: u32::MAX,
                format: x::ImageFormat::ZPixmap as u8,
                shmseg: self.seg,
                offset: 0,
            }))
            .map_err(err)?;
        let size = w as usize * h as usize * 4;
        if !matches!(reply.depth(), 24 | 32)
            || self.conn.get_setup().image_byte_order() != x::ImageOrder::LsbFirst
            || reply.size() as usize != size
        {
            return Err(format!(
                "unsupported X11 pixel format (depth {})",
                reply.depth()
            ));
        }
        // SAFETY: the server wrote `size` bytes into our segment before its
        // reply; nothing else writes it until the next request.
        let data = unsafe { std::slice::from_raw_parts(self.addr, size) };
        Ok(bgrx_to_rgba(data))
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        let _ = (self.conn).send_and_check_request(&xcb::shm::Detach { shmseg: self.seg });
        // SAFETY: attached in `start`, never used after this.
        unsafe { libc::shmdt(self.addr as *const libc::c_void) };
    }
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
