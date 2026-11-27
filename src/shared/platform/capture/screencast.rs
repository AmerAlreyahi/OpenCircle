//! GNOME before 46 (Ubuntu 22.04's 42): its Screenshot portal asks "Share this
//! screenshot?" on EVERY capture (#157). Its ScreenCast portal (version 4+) can
//! remember the user's one "Share" instead: `persist_mode = 2` hands back a restore
//! token, and a session started with it opens without a dialog. Each capture starts
//! such a session, takes ONE frame of its PipeWire stream(s) and closes it: nothing
//! streams between captures, the desktop's screen-share indicator goes off at once.
//!
//! The frame comes through GStreamer's `pipewiresrc` (GNOME's own recorder uses it,
//! so a GNOME desktop has it), loaded at run time with `dlopen`: no new crate and
//! nothing linked; where it's missing the Screenshot portal stays (`portal.rs`).
//! The restore token lives in the app's data folder (never the config file, never
//! logged); GNOME hands out a new one on every start.
use std::ffi::c_int;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use gtk::gio::prelude::*;
use gtk::gio::{Cancellable, DBusCallFlags, DBusConnection, UnixFDList};
use gtk::glib::{ToVariant, Variant, VariantDict, variant::ObjectPath};

use super::portal::{DESKTOP, PATH, handle_path, request};
use super::{CaptureError, Frame};
use crate::shared::platform::gst::{Gst, Pipe};

const IFACE: &str = "org.freedesktop.portal.ScreenCast";

/// Whether this desktop captures through ScreenCast: GNOME before 46 with a
/// ScreenCast portal that can persist (v4+) and GStreamer's PipeWire source at hand.
/// Asked once per process (the desktop doesn't change under us).
pub fn wanted(conn: &DBusConnection) -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let shell = property(
            conn,
            "org.gnome.Shell",
            "/org/gnome/Shell",
            "org.gnome.Shell",
            "ShellVersion",
        )
        .and_then(|v| v.str().map(str::to_string));
        let version = property(conn, DESKTOP, PATH, IFACE, "version").and_then(|v| v.get::<u32>());
        let wanted = asks_every_time(&desktop, shell.as_deref(), version.unwrap_or(0))
            && pipewire_gst().is_some();
        if wanted {
            crate::log!(
                "[opencircle] captures through the ScreenCast portal (asked once, then silent)"
            );
        }
        wanted
    })
}

/// GNOME's Screenshot portal asks on every capture before GNOME 46 (from 46 on it
/// remembers the permission); a ScreenCast portal from version 4 can remember instead.
/// No `XDG_CURRENT_DESKTOP` (started from ssh, a service, a bare terminal) with GNOME
/// Shell answering on the bus is GNOME too: else such a start asked on every capture.
fn asks_every_time(desktop: &str, shell: Option<&str>, screencast: u32) -> bool {
    let gnome = desktop.is_empty() || desktop.split(':').any(|d| d.eq_ignore_ascii_case("GNOME"));
    let major = shell.and_then(|v| v.split('.').next()?.parse::<u32>().ok());
    gnome && major.is_some_and(|m| m < 46) && screencast >= 4
}

fn property(
    conn: &DBusConnection,
    dest: &str,
    path: &str,
    iface: &str,
    name: &str,
) -> Option<Variant> {
    let answer = conn
        .call_sync(
            Some(dest),
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(iface, name).to_variant()),
            None,
            DBusCallFlags::NONE,
            2000,
            None::<&Cancellable>,
        )
        .ok()?;
    answer.child_value(0).as_variant()
}

/// One capture: the whole desktop (every shared monitor). Never allowed yet:
/// `Refused` at once (the permission window opens and calls `allow`). An expired
/// token makes the desktop show its dialog again; "Cancel" there is `Refused`.
pub fn capture(conn: &DBusConnection) -> Result<Frame, CaptureError> {
    if read_token(&token_file()).is_none() {
        // Never asked (or refused): our permission window asks, with no capture
        // waiting on the desktop's dialog meanwhile (`allow`).
        return Err(CaptureError::Refused(
            "Wayland screen cast: not allowed yet".into(),
        ));
    }
    let session = Session::start(conn, Source::Monitors)?;
    if session.asked {
        // GNOME fades its dialog out for ~150 ms: not into the frozen frame.
        std::thread::sleep(DIALOG_FADE);
    }
    let grabbed = session.grab();
    drop(session); // closed before the frames are stitched: the indicator goes off
    compose(grabbed?)
}

/// Onboarding's Allow: the dialog only (its "Share" is remembered); no frame taken.
pub fn allow(conn: &DBusConnection) -> Result<(), CaptureError> {
    forget_token(&token_file());
    Session::start(conn, Source::Monitors).map(drop)
}

/// Whether long capture can stream the screen here: a ScreenCast portal that can
/// persist (v4+, so the user's "Share" is asked once) and GStreamer's PipeWire
/// source. Any desktop (GNOME, KDE, ...); asked once per process.
pub fn can_stream(conn: &DBusConnection) -> bool {
    static CAN: OnceLock<bool> = OnceLock::new();
    *CAN.get_or_init(|| {
        let version = property(conn, DESKTOP, PATH, IFACE, "version").and_then(|v| v.get::<u32>());
        let can = version.unwrap_or(0) >= 4 && pipewire_gst().is_some();
        if !can {
            crate::log!(
                "[opencircle] no long capture: it needs the ScreenCast portal (v4+) and GStreamer's PipeWire source"
            );
        }
        can
    })
}

/// Long capture's frames on Wayland (no silent repeated screenshots there): one
/// ScreenCast session (the restore token: no dialog once allowed) streaming the
/// monitor that holds the area, as the screen shows it, while the capture runs;
/// the stream and the session close on drop (the indicator goes off).
pub struct Cast {
    // (dropped in this order: the pipeline before its session)
    pipe: Pipe,
    _session: Session,
    fd: c_int,
    /// The area in the stream's px.
    area: (i32, i32, u32, u32),
    /// A window's stream (`window`): the monitor size it must fill (a window
    /// stream says nothing of where the window sits).
    window: Option<(u32, u32)>,
    /// Frames read (the log line).
    pub frames: u32,
}

impl Cast {
    /// Streams the monitor holding `region` (physical desktop px, as the frozen
    /// frame's). The desktop shows its dialog only without a valid token.
    pub fn start(region: (i32, i32, u32, u32)) -> Result<Cast, String> {
        let gst = pipewire_gst().ok_or("GStreamer's PipeWire source is missing")?;
        let conn = super::portal::session_bus()?;
        let session = Session::start(&conn, Source::Monitors)?;
        let (stream, area) =
            holding(&session.streams, region).ok_or("the area is off the shared screens")?;
        let fd = session.remote()?;
        let pipe = match gst.launch(&streaming(fd, stream.node)) {
            Ok(pipe) => pipe,
            Err(e) => {
                // SAFETY: our own duplicate, not handed on.
                unsafe { libc::close(fd) };
                return Err(e);
            }
        };
        Ok(Cast {
            pipe,
            _session: session,
            fd,
            area,
            window: None,
            frames: 0,
        })
    }

    /// Streams the one window the user picks in the desktop's dialog (asked
    /// at every start where the desktop keeps no token: Plasma before 5.27),
    /// a `size` px monitor's full-screen window (a game): its own pixels,
    /// never what lies over it.
    pub fn window(size: (u32, u32)) -> Result<Cast, String> {
        let gst = pipewire_gst().ok_or("GStreamer's PipeWire source is missing")?;
        let conn = super::portal::session_bus()?;
        let session = Session::start(&conn, Source::Window)?;
        let node = session.streams.first().ok_or("no window shared")?.node;
        let fd = session.remote()?;
        let pipe = match gst.launch(&streaming(fd, node)) {
            Ok(pipe) => pipe,
            Err(e) => {
                // SAFETY: our own duplicate, not handed on.
                unsafe { libc::close(fd) };
                return Err(e);
            }
        };
        Ok(Cast {
            pipe,
            _session: session,
            fd,
            area: (0, 0, size.0, size.1),
            window: Some(size),
            frames: 0,
        })
    }

    /// A window's own pixels (`window`), not a monitor's.
    pub fn is_window(&self) -> bool {
        self.window.is_some()
    }

    /// The area out of the newest frame within `wait`, RGBA; none when the
    /// screen showed nothing new. An error once the stream ended (the user
    /// stopped sharing).
    pub fn next(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String> {
        let sample = self.pipe.pull(wait);
        if sample.is_null() {
            return match self.pipe.ended() {
                true => Err("the screen cast stopped".into()),
                false => Ok(None),
            };
        }
        self.frames += 1;
        let (area, window) = (self.area, self.window);
        // SAFETY: a live sample, unreffed after the read.
        let cut = unsafe {
            let cut = self.pipe.gst.read(sample, |b, w, h| match window {
                Some(size) if (w, h) != size => Err(not_full_screen((w, h), size)),
                _ => cut_bgrx(b, w, h, area),
            });
            (self.pipe.gst.mini_object_unref)(sample);
            cut
        };
        cut.map(Some)
    }
}

impl Drop for Cast {
    fn drop(&mut self) {
        // SAFETY: our own duplicate from the fd list (pipewiresrc dups it again).
        unsafe { libc::close(self.fd) };
    }
}

/// A window stream's frame isn't the monitor's size: where it sits is unknown (neither the
/// portal nor Wayland says), so Live can't place its patches over it. Said plainly, with the fix.
fn not_full_screen((w, h): (u32, u32), (mw, mh): (u32, u32)) -> String {
    format!(
        "Live needs the game full screen or borderless on this desktop (it can't tell where a window sits): \
         the shared window is {w}x{h} on a {mw}x{mh} screen. Make it full screen, then start Live again"
    )
}

/// Whether live translate can stream one window's own pixels here: the
/// ScreenCast portal offers the window source (KDE; xdg-desktop-portal-wlr
/// only in newer versions) and GStreamer's PipeWire source is at hand.
pub fn can_stream_window() -> bool {
    static CAN: OnceLock<bool> = OnceLock::new();
    *CAN.get_or_init(|| {
        let Ok(conn) = super::portal::session_bus() else {
            return false;
        };
        let types = property(&conn, DESKTOP, PATH, IFACE, "AvailableSourceTypes")
            .and_then(|v| v.get::<u32>())
            .unwrap_or(0);
        let can = types & Source::Window.types() != 0 && pipewire_gst().is_some();
        crate::log!(
            "[opencircle] screen cast of one window: {}",
            if can { "yes" } else { "no" }
        );
        can
    })
}

/// The stream holding most of `region` and the region in its px.
// ponytail: stream px = logical px (scale 1, the frozen frame's first-monitor rule
// in `compose`); a HiDPI multi-monitor desktop needs the per-stream scale.
fn holding(
    streams: &[Stream],
    region: (i32, i32, u32, u32),
) -> Option<(Stream, (i32, i32, u32, u32))> {
    let monitors: Vec<super::Monitor> = (streams.iter())
        .map(|s| super::Monitor {
            rect: (
                s.position.0,
                s.position.1,
                s.size.0.max(0) as u32,
                s.size.1.max(0) as u32,
            ),
            scale: 1.0,
            primary: false,
        })
        .collect();
    let at = match streams.len() {
        1 => 0,
        _ => super::choose(&monitors, Some(region), None)?,
    };
    let s = streams[at];
    let origin = match streams.len() {
        1 => (0, 0), // the frozen frame is this one stream, at 0,0 (`compose`)
        _ => {
            let left = streams.iter().map(|s| s.position.0).min().unwrap_or(0);
            let top = streams.iter().map(|s| s.position.1).min().unwrap_or(0);
            (s.position.0 - left, s.position.1 - top)
        }
    };
    Some((
        s,
        (region.0 - origin.0, region.1 - origin.1, region.2, region.3),
    ))
}

/// `area` of a `width` x `height` BGRx frame (rows maybe padded) as RGBA;
/// an error when the area isn't inside it.
pub(crate) fn cut_bgrx(
    bytes: &[u8],
    width: u32,
    height: u32,
    area: (i32, i32, u32, u32),
) -> Result<Vec<u8>, String> {
    let (x, y, w, h) = area;
    let inside = x >= 0
        && y >= 0
        && x as u32 + w <= width
        && y as u32 + h <= height
        && bytes.len() >= width as usize * 4 * height as usize;
    if !inside || height == 0 {
        return Err(format!("the area left the screen ({width}x{height})"));
    }
    let stride = bytes.len() / height as usize;
    let (x, y) = (x as usize, y as usize);
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for line in bytes.chunks(stride).skip(y).take(h as usize) {
        for px in line[x * 4..(x + w as usize) * 4].as_chunks::<4>().0 {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    Ok(rgba)
}

/// What a session shares: the monitors (captures, long capture) or the one
/// window the user picks (live translate: the game's own pixels, never our
/// layer over it). Each remembers its own "Share" (its token file).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Source {
    Monitors,
    Window,
}

impl Source {
    /// The portal's `types` bits.
    fn types(self) -> u32 {
        match self {
            Source::Monitors => 1,
            Source::Window => 2,
        }
    }

    fn token(self) -> PathBuf {
        match self {
            Source::Monitors => token_file(),
            Source::Window => crate::shared::paths::app_data().join("screencast-window-token"),
        }
    }
}

/// A started ScreenCast session; closed on drop.
struct Session {
    conn: DBusConnection,
    path: String,
    /// (PipeWire node, position, logical size) per shared monitor.
    streams: Vec<Stream>,
    /// The desktop showed its dialog (Start waited for the user).
    asked: bool,
}

/// The dialog's closing animation, with room (`App::leave_frame` waits as long).
const DIALOG_FADE: Duration = Duration::from_millis(250);
/// A restored start answers in ~20 ms; one that waited longer showed the dialog.
const ASKED_AFTER: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Stream {
    node: u32,
    position: (i32, i32),
    size: (i32, i32),
}

impl Session {
    fn start(conn: &DBusConnection, source: Source) -> Result<Self, CaptureError> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let err = |e: CaptureError| match e {
            // "Cancel" in the desktop's dialog: the user said no; ask again next time.
            CaptureError::Refused(_) => {
                forget_token(&source.token());
                CaptureError::Refused("Wayland screen cast: refused".into())
            }
            CaptureError::Failed(why) if why.contains("cancelled") => {
                forget_token(&source.token());
                CaptureError::Refused("Wayland screen cast: cancelled in the dialog".into())
            }
            CaptureError::Failed(why) => {
                CaptureError::Failed(format!("Wayland screen cast: {why}"))
            }
        };
        let name = format!(
            "opencircle_cast{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let sender = conn
            .unique_name()
            .ok_or("not connected to the session bus")?;
        let path = handle_path("session", &sender, &name);
        request(
            conn,
            IFACE,
            "CreateSession",
            Duration::from_secs(10),
            |handle| {
                let options = VariantDict::new(None);
                options.insert("handle_token", handle);
                options.insert("session_handle_token", name.as_str());
                (options,).to_variant()
            },
        )
        .map_err(err)?;
        // From here on the session exists: dropping it closes it, on every path.
        let mut session = Session {
            conn: conn.clone(),
            path,
            streams: Vec::new(),
            asked: false,
        };
        let object = || ObjectPath::try_from(session.path.as_str()).expect("valid session path");
        let token = read_token(&source.token());
        request(
            conn,
            IFACE,
            "SelectSources",
            Duration::from_secs(10),
            |handle| {
                let options = VariantDict::new(None);
                options.insert("handle_token", handle);
                options.insert("types", source.types());
                // every screen the user shares; one window
                options.insert("multiple", source == Source::Monitors);
                options.insert("cursor_mode", 1u32); // hidden, like the Screenshot portal
                options.insert("persist_mode", 2u32); // until revoked
                if let Some(token) = &token {
                    options.insert("restore_token", token.as_str());
                }
                (object(), options).to_variant()
            },
        )
        .map_err(err)?;
        // The dialog (first time only) waits for the user.
        let asking = std::time::Instant::now();
        let started = request(conn, IFACE, "Start", Duration::from_secs(120), |handle| {
            let options = VariantDict::new(None);
            options.insert("handle_token", handle);
            (object(), "", options).to_variant()
        })
        .map_err(err)?;
        session.asked = asking.elapsed() > ASKED_AFTER;
        if let Some(token) = started.lookup::<String>("restore_token").ok().flatten() {
            save_token(&source.token(), &token);
        }
        session.streams = started
            .lookup_value("streams", None)
            .map(|v| streams(&v))
            .unwrap_or_default();
        if session.streams.is_empty() {
            return Err(err("no screen in the answer".into()));
        }
        Ok(session)
    }

    /// One frame of each stream, in stream order.
    fn grab(&self) -> Result<Vec<(Stream, Frame)>, CaptureError> {
        let gst = pipewire_gst().ok_or("GStreamer's PipeWire source is missing")?;
        let fd = self.remote()?;
        let frames = (self.streams.iter())
            .map(|s| frame(gst, fd, s.node).map(|f| (*s, f)))
            .collect::<Result<Vec<_>, String>>()
            .map_err(|e| format!("Wayland screen cast: {e}"));
        // SAFETY: `fd` is our own duplicate from the fd list (pipewiresrc dups it again).
        unsafe { libc::close(fd) };
        Ok(frames?)
    }

    /// The session's PipeWire remote: an fd of our own (the caller closes it).
    fn remote(&self) -> Result<c_int, String> {
        let (_, fds) = self
            .conn
            .call_with_unix_fd_list_sync(
                Some(DESKTOP),
                PATH,
                IFACE,
                "OpenPipeWireRemote",
                Some(
                    &(
                        ObjectPath::try_from(self.path.as_str()).expect("valid session path"),
                        VariantDict::new(None),
                    )
                        .to_variant(),
                ),
                None,
                DBusCallFlags::NONE,
                5000,
                None::<&UnixFDList>,
                None::<&Cancellable>,
            )
            .map_err(|e| format!("Wayland screen cast: no PipeWire connection ({e})"))?;
        fds.get(0).map_err(|e| format!("Wayland screen cast: {e}"))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.conn.call_sync(
            Some(DESKTOP),
            &self.path,
            "org.freedesktop.portal.Session",
            "Close",
            None,
            None,
            DBusCallFlags::NONE,
            2000,
            None::<&Cancellable>,
        );
    }
}

/// Start's `streams` (`a(ua{sv})`): node id, `position` and `size` (logical px).
fn streams(list: &Variant) -> Vec<Stream> {
    (0..list.n_children())
        .filter_map(|i| {
            let item = list.child_value(i);
            let node = item.try_child_value(0)?.get::<u32>()?;
            let props = VariantDict::new(item.try_child_value(1).as_ref());
            let pair = |key| props.lookup::<(i32, i32)>(key).ok().flatten();
            Some(Stream {
                node,
                position: pair("position").unwrap_or((0, 0)),
                size: pair("size").unwrap_or((0, 0)),
            })
        })
        .collect()
}

/// The monitors as one desktop image, like the Screenshot portal's (origin 0,0).
// ponytail: physical px per logical px taken from the first monitor; mixed-scale
// monitors land slightly off. Per-monitor frames when the overlay goes per-monitor.
fn compose(mut frames: Vec<(Stream, Frame)>) -> Result<Frame, CaptureError> {
    if frames.len() == 1 {
        let (_, mut frame) = frames.remove(0);
        (frame.x, frame.y, frame.scale) = (0, 0, 1.0);
        return Ok(frame);
    }
    let (first, f0) = frames.first().ok_or("no screen captured")?;
    let k = match first.size.0 {
        w if w > 0 => f0.width as f32 / w as f32,
        _ => 1.0,
    };
    let at = |s: &Stream| {
        (
            (s.position.0 as f32 * k) as i32,
            (s.position.1 as f32 * k) as i32,
        )
    };
    let left = frames.iter().map(|(s, _)| at(s).0).min().unwrap_or(0);
    let top = frames.iter().map(|(s, _)| at(s).1).min().unwrap_or(0);
    let right = frames
        .iter()
        .map(|(s, f)| at(s).0 + f.width as i32)
        .max()
        .unwrap_or(0);
    let bottom = frames
        .iter()
        .map(|(s, f)| at(s).1 + f.height as i32)
        .max()
        .unwrap_or(0);
    let (width, height) = ((right - left) as u32, (bottom - top) as u32);
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for (s, f) in &frames {
        let (x, y) = at(s);
        let (x, y) = ((x - left) as usize, (y - top) as usize);
        let row = f.width as usize * 4;
        for (r, line) in f.rgba.chunks_exact(row).enumerate() {
            let start = ((y + r) * width as usize + x) * 4;
            rgba[start..start + row].copy_from_slice(line);
        }
    }
    Ok(Frame {
        rgba,
        width,
        height,
        x: 0,
        y: 0,
        scale: 1.0,
    })
}

fn token_file() -> PathBuf {
    crate::shared::paths::app_data().join("screencast-token")
}

fn read_token(path: &Path) -> Option<String> {
    let token = std::fs::read_to_string(path).ok()?;
    Some(token.trim().to_string()).filter(|t| !t.is_empty())
}

/// Owner-only file; a failure only means the next capture asks again.
fn save_token(path: &Path, token: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?
                .write_all(token.as_bytes())
        });
    if let Err(e) = written {
        crate::log!("[opencircle] could not keep the screen cast permission: {e}");
    }
}

fn forget_token(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// How long one frame may take to arrive (a screen cast's first frame takes ~0.1 s).
const FRAME_WAIT: Duration = Duration::from_secs(3);

/// GStreamer with its PipeWire source; `None` without either (logged once).
fn pipewire_gst() -> Option<&'static Gst> {
    static GST: OnceLock<Option<&'static Gst>> = OnceLock::new();
    *GST.get_or_init(|| {
        let gst = Gst::get().filter(|g| g.has("pipewiresrc"));
        if gst.is_none() {
            crate::log!("[opencircle] no GStreamer PipeWire source: screenshots ask every time");
        }
        gst
    })
}

/// One RGBA frame of PipeWire node `node` on the remote `fd`.
fn frame(gst: &'static Gst, fd: c_int, node: u32) -> Result<Frame, String> {
    let pipe = gst.launch(&pipeline(fd, node))?;
    let sample = pipe.pull(FRAME_WAIT);
    if sample.is_null() {
        return Err("no frame from the screen cast".to_string());
    }
    // SAFETY: a live sample, unreffed after the copy.
    unsafe {
        let frame = gst.read(sample, rgba_rows);
        (gst.mini_object_unref)(sample);
        frame
    }
}

/// pipewiresrc → RGBA → appsink, stopping after one buffer. `always-copy`: the
/// buffer outlives the PipeWire one we'd otherwise borrow.
fn pipeline(fd: c_int, node: u32) -> String {
    format!(
        "pipewiresrc fd={fd} path={node} always-copy=true num-buffers=1 ! videoconvert ! \
         video/x-raw,format=RGBA ! appsink name=sink sync=false max-buffers=1"
    )
}

/// pipewiresrc → BGRx (the screen cast's own format: no conversion) → appsink,
/// always the newest frame (an older one waiting is dropped).
fn streaming(fd: c_int, node: u32) -> String {
    format!(
        "pipewiresrc fd={fd} path={node} always-copy=true ! videoconvert ! \
         video/x-raw,format=BGRx ! appsink name=sink sync=false max-buffers=1 drop=true"
    )
}

/// Tight RGBA rows out of a mapped frame whose rows may be padded.
fn rgba_rows(bytes: &[u8], width: u32, height: u32) -> Result<Frame, String> {
    let row = width as usize * 4;
    if row == 0 || height == 0 || bytes.len() < row * height as usize {
        return Err(format!(
            "bad frame ({width}x{height}, {} bytes)",
            bytes.len()
        ));
    }
    let stride = bytes.len() / height as usize;
    let rgba = (bytes.chunks(stride).take(height as usize))
        .flat_map(|line| &line[..row])
        .copied()
        .collect();
    Ok(Frame {
        rgba,
        width,
        height,
        x: 0,
        y: 0,
        scale: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A windowed game can't be placed (Wayland never says where a window sits): the user is told
    /// plainly what to do, not "frame size mismatch".
    #[test]
    fn a_windowed_game_is_told_to_go_full_screen() {
        let said = not_full_screen((1280, 720), (1920, 1080));
        assert!(said.contains("full screen or borderless"), "{said}");
        assert!(
            said.contains("1280x720") && said.contains("1920x1080"),
            "{said}"
        );
        assert!(said.ends_with("start Live again"), "{said}");
    }

    #[test]
    fn only_gnome_before_46_with_a_persisting_screencast_portal() {
        assert!(asks_every_time("ubuntu:GNOME", Some("42.9"), 4));
        assert!(asks_every_time("GNOME", Some("45.2"), 5));
        // GNOME 46+ remembers the screenshot permission itself.
        assert!(!asks_every_time("ubuntu:GNOME", Some("46.0"), 5));
        assert!(!asks_every_time("GNOME", Some("50.1"), 5));
        // A portal that can't persist would ask every time too.
        assert!(!asks_every_time("ubuntu:GNOME", Some("42.9"), 3));
        // KDE, others, unknown shell: unchanged.
        assert!(!asks_every_time("KDE", Some("42.9"), 5));
        assert!(!asks_every_time("ubuntu:GNOME", None, 4));
        // #172: no XDG_CURRENT_DESKTOP, GNOME Shell answering: GNOME
        assert!(asks_every_time("", Some("42.9"), 4));
        assert!(!asks_every_time("", None, 4));
    }

    #[test]
    fn reads_the_started_streams() {
        let one = VariantDict::new(None);
        one.insert("position", (0i32, 0i32));
        one.insert("size", (1920i32, 1080i32));
        let two = VariantDict::new(None);
        two.insert("position", (1920i32, 0i32));
        two.insert("size", (1280i32, 1024i32));
        let list = vec![(35u32, one), (41u32, two)].to_variant();
        assert_eq!(
            streams(&list),
            [
                Stream {
                    node: 35,
                    position: (0, 0),
                    size: (1920, 1080)
                },
                Stream {
                    node: 41,
                    position: (1920, 0),
                    size: (1280, 1024)
                },
            ]
        );
    }

    fn solid(w: u32, h: u32, v: u8) -> Frame {
        Frame {
            rgba: vec![v; (w * h * 4) as usize],
            width: w,
            height: h,
            x: 5,
            y: 5,
            scale: 2.0,
        }
    }

    #[test]
    fn composes_every_monitor_into_one_desktop() {
        let at = |x, y, w, h| Stream {
            node: 1,
            position: (x, y),
            size: (w, h),
        };
        // One monitor: as is, at the origin.
        let one = compose(vec![(at(0, 0, 4, 2), solid(4, 2, 7))]).unwrap();
        assert_eq!(
            (one.width, one.height, one.x, one.y, one.scale),
            (4, 2, 0, 0, 1.0)
        );
        // Two side by side, the second lower and shorter; a gap stays black.
        let both = compose(vec![
            (at(0, 0, 4, 2), solid(4, 2, 1)),
            (at(4, 1, 2, 1), solid(2, 1, 9)),
        ])
        .unwrap();
        assert_eq!((both.width, both.height), (6, 2));
        let px = |x: usize, y: usize| both.rgba[(y * 6 + x) * 4];
        assert_eq!(
            (px(0, 0), px(3, 1), px(4, 0), px(4, 1), px(5, 1)),
            (1, 1, 0, 9, 9)
        );
        // A monitor left of the first (negative position) shifts everything right.
        let left = compose(vec![
            (at(0, 0, 2, 1), solid(2, 1, 1)),
            (at(-2, 0, 2, 1), solid(2, 1, 9)),
        ])
        .unwrap();
        assert_eq!(
            left.rgba.chunks(4).map(|p| p[0]).collect::<Vec<_>>(),
            [9, 9, 1, 1]
        );
    }

    #[test]
    fn drops_row_padding() {
        // 2x2 RGBA with 4 bytes of padding per row.
        let bytes: Vec<u8> = (0..24).collect();
        let f = rgba_rows(&bytes, 2, 2).unwrap();
        assert_eq!(
            f.rgba,
            [0, 1, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15, 16, 17, 18, 19]
        );
        assert!(rgba_rows(&bytes, 4, 2).is_err(), "too short");
        assert!(rgba_rows(&[], 0, 0).is_err());
    }

    #[test]
    fn the_pipeline_takes_one_rgba_frame_of_the_node() {
        let p = pipeline(7, 35);
        assert!(p.starts_with("pipewiresrc fd=7 path=35 "));
        assert!(
            p.contains("num-buffers=1")
                && p.contains("format=RGBA")
                && p.contains("appsink name=sink")
        );
    }

    #[test]
    fn the_area_is_cut_out_of_a_padded_bgrx_frame_as_rgba() {
        // 3x2 BGRx, 4 bytes of padding a row; pixel (x, y) = B x, G y, R 7
        let mut bytes = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                bytes.extend_from_slice(&[x, y, 7, 0]);
            }
            bytes.extend_from_slice(&[0; 4]);
        }
        let cut = cut_bgrx(&bytes, 3, 2, (1, 1, 2, 1)).unwrap();
        assert_eq!(cut, [7, 1, 1, 255, 7, 1, 2, 255]);
        assert!(
            cut_bgrx(&bytes, 3, 2, (2, 0, 2, 1)).is_err(),
            "past the edge"
        );
        assert!(cut_bgrx(&bytes, 3, 2, (-1, 0, 1, 1)).is_err());
        assert!(cut_bgrx(&[], 0, 0, (0, 0, 0, 0)).is_err());
    }

    #[test]
    fn the_cast_streams_the_screen_holding_the_area() {
        let s = |node, x, y, w, h| Stream {
            node,
            position: (x, y),
            size: (w, h),
        };
        // one screen: the frozen frame is it, at 0,0
        let one = [s(35, 0, 0, 1280, 720)];
        assert_eq!(
            holding(&one, (10, 20, 300, 200)),
            Some((one[0], (10, 20, 300, 200)))
        );
        // two side by side: the right one, the area in its px
        let two = [s(35, 0, 0, 1280, 720), s(41, 1280, 0, 1920, 1080)];
        assert_eq!(
            holding(&two, (1400, 100, 300, 200)),
            Some((two[1], (120, 100, 300, 200)))
        );
        assert_eq!(
            holding(&two, (100, 100, 300, 200)).map(|h| h.0.node),
            Some(35)
        );
        assert_eq!(holding(&[], (0, 0, 1, 1)), None);
    }

    #[test]
    fn the_cast_keeps_the_newest_frame_in_the_screens_own_format() {
        let p = streaming(7, 35);
        assert!(p.starts_with("pipewiresrc fd=7 path=35 "));
        assert!(!p.contains("num-buffers"), "it streams");
        assert!(
            p.contains("format=BGRx") && p.contains("drop=true") && p.contains("max-buffers=1")
        );
    }

    #[test]
    fn the_token_is_kept_owner_only_and_forgotten() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oc-cast-{}", std::process::id()));
        let file = dir.join("data/screencast-token");
        save_token(&file, "tok-1");
        assert_eq!(read_token(&file).as_deref(), Some("tok-1"));
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        save_token(&file, "tok-2");
        assert_eq!(
            read_token(&file).as_deref(),
            Some("tok-2"),
            "replaced, not appended"
        );
        forget_token(&file);
        assert_eq!(read_token(&file), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
