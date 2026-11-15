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

/// A private shared-memory segment the X server writes images into (MIT-SHM):
/// the pixels skip the socket (a 3440x1440 monitor: ~20 MB a grab).
struct Segment {
    seg: xcb::shm::Seg,
    addr: *mut u8,
    size: usize,
}

impl Segment {
    fn new(conn: &xcb::Connection, size: usize) -> Result<Segment, String> {
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
        let seg: xcb::shm::Seg = conn.generate_id();
        let attached = conn.send_and_check_request(&xcb::shm::Attach {
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
        Ok(Segment { seg, addr, size })
    }

    /// `drawable`'s `rect` written into the segment, as B, G, R, unused.
    fn get(
        &self,
        conn: &xcb::Connection,
        drawable: x::Drawable,
        (x, y, w, h): (i16, i16, u16, u16),
    ) -> Result<&[u8], String> {
        let size = w as usize * h as usize * 4;
        if size > self.size {
            return Err("the image outgrew its segment".into());
        }
        let reply = conn
            .wait_for_reply(conn.send_request(&xcb::shm::GetImage {
                drawable,
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
        // ponytail: only 32-bit little-endian BGRX (every current X server).
        if !matches!(reply.depth(), 24 | 32)
            || conn.get_setup().image_byte_order() != x::ImageOrder::LsbFirst
            || reply.size() as usize != size
        {
            return Err(format!(
                "unsupported X11 pixel format (depth {})",
                reply.depth()
            ));
        }
        // SAFETY: the server wrote `size` bytes into our segment before its
        // reply; nothing else writes it until the next request.
        Ok(unsafe { std::slice::from_raw_parts(self.addr, size) })
    }

    fn free(&self, conn: &xcb::Connection) {
        let _ = conn.send_and_check_request(&xcb::shm::Detach { shmseg: self.seg });
        // SAFETY: attached in `new`, never used after this.
        unsafe { libc::shmdt(self.addr as *const libc::c_void) };
    }
}

/// Grabs a second at most (live translate's watch: the display's rate).
const MAX_RATE: u32 = 60;

/// Sleeps out the rest of `1 / MAX_RATE` since `last`, then stamps it.
fn pace(last: &mut Option<std::time::Instant>) {
    let every = std::time::Duration::from_secs(1) / MAX_RATE;
    if let Some(last) = *last {
        std::thread::sleep(every.saturating_sub(last.elapsed()));
    }
    *last = Some(std::time::Instant::now());
}

/// An area of the screen grabbed again and again through a shared-memory
/// segment, at most `MAX_RATE` grabs a second. Freed when dropped.
pub struct Shm {
    conn: xcb::Connection,
    root: x::Window,
    seg: Segment,
    rect: (i16, i16, u16, u16),
    last: Option<std::time::Instant>,
}

impl Shm {
    /// For `region` (physical desktop px); fails where the server has no
    /// MIT-SHM (a remote display) or the area is off the X11 screen.
    pub fn start((x, y, w, h): (i32, i32, u32, u32)) -> Result<Shm, String> {
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
        conn.wait_for_reply(conn.send_request(&xcb::shm::QueryVersion {}))
            .map_err(err)?;
        let seg = Segment::new(&conn, w as usize * h as usize * 4)?;
        Ok(Shm {
            conn,
            root,
            seg,
            rect,
            last: None,
        })
    }

    /// The area now, RGBA (after waiting out the pace).
    pub fn grab(&mut self) -> Result<Vec<u8>, String> {
        pace(&mut self.last);
        let data = (self.seg).get(&self.conn, x::Drawable::Window(self.root), self.rect)?;
        Ok(bgrx_to_rgba(data))
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        self.seg.free(&self.conn);
    }
}

/// The frames of the window the user is in (live translate reads the game,
/// never what lies over it): its own pixels through XComposite (a window
/// redirected off screen has a pixmap of its own, as OBS's window capture
/// reads it; no prompt), grabbed only when XDamage says it drew, through
/// shared memory, at most `MAX_RATE` a second. Placed where the window sits
/// on the monitor (the rest black), so frame px = monitor px. It follows the
/// focus to another window that covers a quarter of the monitor or more; a
/// smaller one (a toast, a menu) or one of ours never takes it. With a
/// compositor the window is redirected already; without one the redirect is
/// ours (the server then copies the window to the screen: the game's cost).
pub struct FrontWindow {
    conn: xcb::Connection,
    root: x::Window,
    active_atom: x::Atom,
    monitor: (i32, i32, u32, u32),
    on: Option<Watched>,
    /// The active window changed: look at it.
    look: bool,
    /// The window drew since the last grab.
    drew: bool,
    last: Option<std::time::Instant>,
}

/// The window being read.
struct Watched {
    window: x::Window,
    pixmap: x::Pixmap,
    damage: xcb::damage::Damage,
    seg: Segment,
    /// Root px: x, y, width, height; its border's width.
    rect: (i32, i32, u32, u32),
    border: u16,
}

/// A frame of the front window, none new, or no window to read now.
pub enum Got {
    Frame(Vec<u8>),
    Same,
    NoWindow,
}

impl FrontWindow {
    /// For the monitor at `monitor` (physical px); fails without XComposite
    /// 0.2 / XDamage / MIT-SHM (then the screen is read instead).
    pub fn start(monitor: (i32, i32, u32, u32)) -> Result<FrontWindow, String> {
        use xcb::{Extension, composite, damage, xfixes};
        if std::env::var_os("DISPLAY").is_none() {
            return Err("no X11 display".into());
        }
        let (conn, screen) = xcb::Connection::connect_with_extensions(
            None,
            &[
                Extension::Shm,
                Extension::Composite,
                Extension::Damage,
                Extension::XFixes,
            ],
            &[],
        )
        .map_err(|e| format!("no XComposite / XDamage / MIT-SHM: {e}"))?;
        let root = (conn.get_setup().roots().nth(screen as usize))
            .ok_or("no X11 screen")?
            .root();
        let composite = conn
            .wait_for_reply(conn.send_request(&composite::QueryVersion {
                client_major_version: 0,
                client_minor_version: 4,
            }))
            .map_err(err)?;
        if (composite.major_version(), composite.minor_version()) < (0, 2) {
            return Err("XComposite older than 0.2".into());
        }
        conn.wait_for_reply(conn.send_request(&xfixes::QueryVersion {
            client_major_version: 5,
            client_minor_version: 0,
        }))
        .map_err(err)?;
        conn.wait_for_reply(conn.send_request(&damage::QueryVersion {
            client_major_version: 1,
            client_minor_version: 1,
        }))
        .map_err(err)?;
        conn.wait_for_reply(conn.send_request(&xcb::shm::QueryVersion {}))
            .map_err(err)?;
        let active_atom = conn
            .wait_for_reply(conn.send_request(&x::InternAtom {
                only_if_exists: false,
                name: b"_NET_ACTIVE_WINDOW",
            }))
            .map_err(err)?
            .atom();
        // (this connection's own interest: other clients' are untouched)
        conn.send_and_check_request(&x::ChangeWindowAttributes {
            window: root,
            value_list: &[x::Cw::EventMask(x::EventMask::PROPERTY_CHANGE)],
        })
        .map_err(|e| format!("{e:?}"))?;
        Ok(FrontWindow {
            conn,
            root,
            active_atom,
            monitor,
            on: None,
            look: true,
            drew: false,
            last: None,
        })
    }

    /// The window read now (for the log).
    pub fn window(&self) -> Option<u32> {
        use xcb::Xid;
        self.on.as_ref().map(|w| w.window.resource_id())
    }

    /// The next frame, waiting at most `wait` for the window to draw.
    pub fn next(&mut self, wait: std::time::Duration) -> Got {
        self.events();
        if !self.drew && self.on.is_some() && !self.look {
            let mut fd = libc::pollfd {
                fd: std::os::fd::AsRawFd::as_raw_fd(&self.conn),
                events: libc::POLLIN,
                revents: 0,
            };
            let ms = wait.as_millis().min(i32::MAX as u128) as i32;
            // SAFETY: one valid pollfd for the call.
            unsafe { libc::poll(&mut fd, 1, ms) };
            self.events();
        }
        if std::mem::take(&mut self.look) {
            self.follow();
        }
        let Some(on) = &self.on else {
            return Got::NoWindow;
        };
        if !std::mem::take(&mut self.drew) {
            return Got::Same;
        }
        pace(&mut self.last);
        use xcb::Xid;
        // what it draws from now on comes as the next notice
        self.conn.send_request(&xcb::damage::Subtract {
            damage: on.damage,
            repair: xcb::xfixes::Region::none(),
            parts: xcb::xfixes::Region::none(),
        });
        let (_, _, w, h) = on.rect;
        let area = (on.border as i16, on.border as i16, w as u16, h as u16);
        match on.seg.get(&self.conn, x::Drawable::Pixmap(on.pixmap), area) {
            Ok(bgrx) => Got::Frame(place(bgrx, on.rect, self.monitor)),
            Err(e) => {
                crate::log!("[opencircle] live: the window can't be read ({e}); the screen");
                self.drop_window();
                self.look = true;
                Got::NoWindow
            }
        }
    }

    /// Takes what the server sent: drawing, the window moved / resized /
    /// went, the focus moved.
    fn events(&mut self) {
        use xcb::Event;
        loop {
            let event = match self.conn.poll_for_event() {
                Ok(Some(e)) => e,
                Ok(None) => return,
                // (a request on a window that just went: nothing to do)
                Err(xcb::Error::Protocol(_)) => continue,
                Err(xcb::Error::Connection(_)) => {
                    self.on = None; // (nothing to free on a dead connection)
                    return;
                }
            };
            let ours = |w: x::Window| self.on.as_ref().is_some_and(|on| on.window == w);
            match event {
                Event::Damage(xcb::damage::Event::Notify(_)) => self.drew = true,
                Event::X(x::Event::PropertyNotify(e)) if e.atom() == self.active_atom => {
                    self.look = true;
                }
                Event::X(x::Event::ConfigureNotify(e)) if ours(e.window()) => self.reshaped(),
                Event::X(x::Event::UnmapNotify(e)) if ours(e.window()) => {
                    self.drop_window();
                    self.look = true;
                }
                Event::X(x::Event::DestroyNotify(e)) if ours(e.window()) => {
                    self.on = None; // (its pixmap and damage went with it)
                    self.look = true;
                }
                _ => {}
            }
        }
    }

    /// The window moved or changed size: its place, and a new pixmap (the
    /// old one keeps the old size).
    fn reshaped(&mut self) {
        let Some(on) = &self.on else { return };
        let window = on.window;
        match self.geometry(window) {
            Some((rect, border)) if covers_a_quarter(rect, self.monitor) => {
                let Some(on) = &mut self.on else { return };
                let resized = (rect.2, rect.3, border) != (on.rect.2, on.rect.3, on.border);
                (on.rect, on.border) = (rect, border);
                if resized {
                    self.drop_window();
                    self.watch(window);
                }
                self.drew = true;
            }
            _ => {
                self.drop_window();
                self.look = true;
            }
        }
    }

    /// The active window, if it is a new one to read.
    fn follow(&mut self) {
        let Some(active) = super::window_rect::x11_active(&self.conn, self.root, false) else {
            return;
        };
        if self.on.as_ref().is_some_and(|on| on.window == active) {
            return;
        }
        if !self
            .geometry(active)
            .is_some_and(|(r, _)| covers_a_quarter(r, self.monitor))
        {
            return; // a small window (a toast, a menu): the one read stays
        }
        self.drop_window();
        self.watch(active);
    }

    /// Starts reading `window`: redirected, its pixmap named, its drawing
    /// watched, a segment its size.
    fn watch(&mut self, window: x::Window) {
        use xcb::{Xid, composite, damage};
        let Some((rect, border)) = self.geometry(window) else {
            return;
        };
        let c = &self.conn;
        let _ = c.send_and_check_request(&x::ChangeWindowAttributes {
            window,
            value_list: &[x::Cw::EventMask(x::EventMask::STRUCTURE_NOTIFY)],
        });
        let redirect = c.send_and_check_request(&composite::RedirectWindow {
            window,
            update: composite::Redirect::Automatic,
        });
        let pixmap: x::Pixmap = c.generate_id();
        let named = redirect.and_then(|()| {
            c.send_and_check_request(&composite::NameWindowPixmap { window, pixmap })
        });
        let seg = (named.map_err(|e| format!("{e:?}")))
            .and_then(|()| Segment::new(c, rect.2 as usize * rect.3 as usize * 4));
        let seg = match seg {
            Ok(seg) => seg,
            Err(e) => {
                crate::log!(
                    "[opencircle] live: window {:#x} can't be read ({e}); the screen",
                    window.resource_id()
                );
                let _ = c.send_and_check_request(&x::FreePixmap { pixmap });
                let _ = c.send_and_check_request(&composite::UnredirectWindow {
                    window,
                    update: composite::Redirect::Automatic,
                });
                return;
            }
        };
        let damage: damage::Damage = c.generate_id();
        let _ = c.send_and_check_request(&damage::Create {
            damage,
            drawable: x::Drawable::Window(window),
            level: damage::ReportLevel::NonEmpty,
        });
        crate::log!(
            "[opencircle] live: reading window {:#x} ({}x{} at {},{}), its own pixels",
            window.resource_id(),
            rect.2,
            rect.3,
            rect.0,
            rect.1
        );
        self.on = Some(Watched {
            window,
            pixmap,
            damage,
            seg,
            rect,
            border,
        });
        self.drew = true;
    }

    /// `window`'s place on the root (inside its border) and its border width.
    fn geometry(&self, window: x::Window) -> Option<((i32, i32, u32, u32), u16)> {
        let c = &self.conn;
        let g = c.send_request(&x::GetGeometry {
            drawable: x::Drawable::Window(window),
        });
        let at = c.send_request(&x::TranslateCoordinates {
            src_window: window,
            dst_window: self.root,
            src_x: 0,
            src_y: 0,
        });
        let (g, at) = (c.wait_for_reply(g).ok()?, c.wait_for_reply(at).ok()?);
        let rect = (
            i32::from(at.dst_x()),
            i32::from(at.dst_y()),
            u32::from(g.width()),
            u32::from(g.height()),
        );
        Some((rect, g.border_width()))
    }

    /// Stops reading the window (it stays as its app left it; the redirect
    /// is ours to undo).
    fn drop_window(&mut self) {
        use xcb::composite;
        let Some(on) = self.on.take() else { return };
        let c = &self.conn;
        let _ = c.send_and_check_request(&xcb::damage::Destroy { damage: on.damage });
        let _ = c.send_and_check_request(&x::FreePixmap { pixmap: on.pixmap });
        let _ = c.send_and_check_request(&composite::UnredirectWindow {
            window: on.window,
            update: composite::Redirect::Automatic,
        });
        let _ = c.send_and_check_request(&x::ChangeWindowAttributes {
            window: on.window,
            value_list: &[x::Cw::EventMask(x::EventMask::NO_EVENT)],
        });
        on.seg.free(c);
    }
}

impl Drop for FrontWindow {
    fn drop(&mut self) {
        self.drop_window();
    }
}

/// `rect` (root px) covers at least a quarter of `monitor`.
fn covers_a_quarter(rect: (i32, i32, u32, u32), monitor: (i32, i32, u32, u32)) -> bool {
    let (w, h) = overlap(rect, monitor);
    4 * w as u64 * h as u64 >= monitor.2 as u64 * monitor.3 as u64
}

/// The size of the part two rects (x, y, w, h) share.
fn overlap(a: (i32, i32, u32, u32), b: (i32, i32, u32, u32)) -> (u32, u32) {
    let span = |a0: i32, al: u32, b0: i32, bl: u32| {
        let end = (a0 + al as i32).min(b0 + bl as i32);
        (end - a0.max(b0)).max(0) as u32
    };
    (span(a.0, a.2, b.0, b.2), span(a.1, a.3, b.1, b.3))
}

/// A window's BGRX pixels (`rect` on the root) as a frame of `monitor`:
/// opaque RGBA where the window covers it, black elsewhere.
fn place(bgrx: &[u8], rect: (i32, i32, u32, u32), monitor: (i32, i32, u32, u32)) -> Vec<u8> {
    let (mx, my, mw, mh) = monitor;
    let mut out = vec![0u8; mw as usize * mh as usize * 4];
    let (w, h) = overlap(rect, monitor);
    let (x0, y0) = (rect.0.max(mx), rect.1.max(my));
    for row in 0..h as i32 {
        let src = ((y0 + row - rect.1) as usize * rect.2 as usize + (x0 - rect.0) as usize) * 4;
        let dst = ((y0 + row - my) as usize * mw as usize + (x0 - mx) as usize) * 4;
        let n = w as usize * 4;
        bgrx_into_rgba(&bgrx[src..src + n], &mut out[dst..dst + n]);
    }
    out
}

/// X11 ZPixmap (B, G, R, unused) → opaque RGBA.
fn bgrx_to_rgba(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0; data.len() / 4 * 4];
    bgrx_into_rgba(data, &mut out);
    out
}

/// The same into `out` (as long as `data`).
fn bgrx_into_rgba(data: &[u8], out: &mut [u8]) {
    for (o, &[b, g, r, _]) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(data.as_chunks::<4>().0)
    {
        *o = [r, g, b, 255];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_lands_where_it_sits_on_the_monitor() {
        // a 2x2 window at (11, 21) on a 3x3 monitor at (10, 20): its pixel
        // (x, y) = B, G, R = x, y, 7
        let bgrx: Vec<u8> = (0..2u8)
            .flat_map(|y| (0..2u8).flat_map(move |x| [x, y, 7, 0]))
            .collect();
        let frame = place(&bgrx, (11, 21, 2, 2), (10, 20, 3, 3));
        let px = |x: usize, y: usize| &frame[(y * 3 + x) * 4..][..4];
        assert_eq!(px(0, 0), [0, 0, 0, 0]); // (not the window: black)
        assert_eq!(px(1, 1), [7, 0, 0, 255]);
        assert_eq!(px(2, 2), [7, 1, 1, 255]);
        // half off the monitor: the part on it
        let frame = place(&bgrx, (9, 20, 2, 2), (10, 20, 3, 3));
        assert_eq!(&frame[..4], [7, 0, 1, 255]);
        assert_eq!(&frame[4..8], [0, 0, 0, 0]);
    }

    #[test]
    fn only_a_window_over_a_quarter_of_the_monitor_is_followed() {
        let m = (0, 0, 100, 100);
        assert!(covers_a_quarter((0, 0, 100, 100), m));
        assert!(covers_a_quarter((50, 50, 80, 80), m)); // 50x50 on it
        assert!(!covers_a_quarter((0, 0, 40, 40), m)); // a toast
        assert!(!covers_a_quarter((100, 0, 100, 100), m)); // the next monitor
    }

    #[test]
    fn converts_bgrx_to_opaque_rgba() {
        assert_eq!(
            bgrx_to_rgba(&[1, 2, 3, 0, 4, 5, 6, 9]),
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
    }
}
