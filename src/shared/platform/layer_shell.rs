//! Live translate's layer on KDE / wlroots Wayland: a `wlr-layer-shell`
//! OVERLAY surface of our own (winit can't make one), over every window
//! (full-screen games too), with an empty input region (clicks go to the game)
//! and per-pixel alpha. It shows the edge glow (drawn once) and the patches;
//! only what changed is redrawn and sent (`wl_surface.damage_buffer`).
//! It runs on a thread of its own with its own Wayland connection; the UI
//! sends it what to show. GNOME has no layer-shell: `available` says no there.
//! Part of the UI (a window; `app::arch_tests`).
//! Lab-proven first on Plasma 5.24 (docs/internal/reports-0.3.0/w21-kde-wlroots-layer.md).
#![cfg_attr(not(target_os = "linux"), allow(dead_code))] // (Wayland only)

/// Rectangles in surface px with their pixels (RGBA, straight alpha): the glow.
pub type Strips = Vec<([u32; 4], Vec<u8>)>;

/// What the UI asks of the layer.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, PartialEq)]
pub(crate) enum Op {
    /// A patch (x, y, w, h; RGBA straight alpha), first see-through: `Alpha` fades it in.
    Show(u64, [u32; 4], Vec<u8>),
    Alpha(u64, f32),
    Hide(u64),
    /// The glow's opacity (its fade in and out).
    Edge(f32),
}

/// The layer surface (dropping it closes it).
pub struct LayerShell {
    #[cfg(target_os = "linux")]
    tx: Option<std::sync::mpsc::Sender<Op>>,
    #[cfg(target_os = "linux")]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for LayerShell {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("LayerShell")
    }
}

/// This desktop lets an app put a click-through layer over other apps
/// (`zwlr_layer_shell_v1`: KDE, wlroots; not GNOME). Asked once per process.
pub fn available() -> bool {
    #[cfg(target_os = "linux")]
    {
        static HAS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *HAS.get_or_init(|| {
            let has = wl::has_layer_shell();
            crate::log!(
                "[opencircle] wayland layer-shell: {}",
                if has { "yes" } else { "no" }
            );
            has
        })
    }
    #[cfg(not(target_os = "linux"))]
    false
}

impl LayerShell {
    /// The layer over the output the desktop picks (the focused one), `size`
    /// px (the monitor's), showing `strips` at opacity 0 (`edge` fades it in).
    #[cfg(target_os = "linux")]
    pub fn open(size: (u32, u32), strips: Strips) -> Result<LayerShell, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("live layer".into())
            .spawn(move || match wl::Surface::open(size) {
                Ok(mut surface) => {
                    let _ = ready_tx.send(Ok(()));
                    surface.run(Canvas::new(size, &strips), &rx);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        let ready = ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "the layer surface didn't come up".to_string())?;
        ready.map(|()| LayerShell {
            tx: Some(tx),
            thread: Some(thread),
        })
    }

    /// A layer with no surface: what it is sent comes out of the receiver (tests).
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn recording() -> (LayerShell, std::sync::mpsc::Receiver<Op>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let layer = LayerShell {
            tx: Some(tx),
            thread: None,
        };
        (layer, rx)
    }

    #[cfg(target_os = "linux")]
    fn send(&self, op: Op) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(op);
        }
    }
    #[cfg(not(target_os = "linux"))]
    fn send(&self, _: Op) {}

    pub fn show(&self, id: u64, at: [u32; 4], rgba: Vec<u8>) {
        self.send(Op::Show(id, at, rgba));
    }
    pub fn alpha(&self, id: u64, alpha: f32) {
        self.send(Op::Alpha(id, alpha));
    }
    pub fn hide(&self, id: u64) {
        self.send(Op::Hide(id));
    }
    pub fn edge(&self, alpha: f32) {
        self.send(Op::Edge(alpha));
    }
}

impl Drop for LayerShell {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            self.tx = None; // the thread ends at its next look
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

/// What the layer shows, in the surface's own format (`wl_shm` ARGB8888 =
/// BGRA bytes, premultiplied): the glow at its opacity, the patches over it
/// in the order they came.
struct Canvas {
    w: u32,
    h: u32,
    glow: Vec<u8>,
    edge: f32,
    patches: Vec<Patch>,
}

struct Patch {
    id: u64,
    at: [u32; 4],
    /// Premultiplied BGRA.
    px: Vec<u8>,
    alpha: f32,
}

impl Canvas {
    fn new((w, h): (u32, u32), strips: &Strips) -> Canvas {
        let mut glow = vec![0; w as usize * h as usize * 4];
        super::alpha_window::premultiplied_bgra(&mut glow, w, strips);
        Canvas {
            w,
            h,
            glow,
            edge: 0.0,
            patches: Vec::new(),
        }
    }

    /// Takes `op`; what must be redrawn (x, y, w, h), if anything.
    fn apply(&mut self, op: Op) -> Option<[u32; 4]> {
        let (sw, sh) = (self.w, self.h);
        let clip = move |[x, y, w, h]: [u32; 4]| {
            let (x1, y1) = ((x + w).min(sw), (y + h).min(sh));
            (x < x1 && y < y1).then_some([x, y, x1 - x, y1 - y])
        };
        match op {
            Op::Show(id, at, rgba) => {
                let old = self.take(id);
                if rgba.len() != (at[2] * at[3] * 4) as usize {
                    return old.and_then(clip);
                }
                let mut px = vec![0; rgba.len()];
                super::alpha_window::premultiplied_bgra(
                    &mut px,
                    at[2],
                    &[([0, 0, at[2], at[3]], rgba)],
                );
                self.patches.push(Patch {
                    id,
                    at,
                    px,
                    alpha: 0.0,
                });
                // (see-through until its first `Alpha`: only an old one to clear)
                old.and_then(clip)
            }
            Op::Alpha(id, alpha) => {
                let p = self.patches.iter_mut().find(|p| p.id == id)?;
                let alpha = alpha.clamp(0.0, 1.0);
                if p.alpha == alpha {
                    return None;
                }
                p.alpha = alpha;
                clip(p.at)
            }
            Op::Hide(id) => self.take(id).and_then(clip),
            Op::Edge(alpha) => {
                let alpha = alpha.clamp(0.0, 1.0);
                if self.edge == alpha {
                    return None;
                }
                self.edge = alpha;
                clip([0, 0, self.w, self.h])
            }
        }
    }

    fn take(&mut self, id: u64) -> Option<[u32; 4]> {
        let at = self.patches.iter().position(|p| p.id == id)?;
        Some(self.patches.remove(at).at)
    }

    /// `r` of the picture into `out` (the whole surface, `w` px a row).
    fn paint(&self, out: &mut [u8], [rx, ry, rw, rh]: [u32; 4]) {
        let mul = |c: u8, a: f32| (f32::from(c) * a + 0.5) as u8;
        for y in ry..ry + rh {
            for x in rx..rx + rw {
                let i = ((y * self.w + x) * 4) as usize;
                let g = &self.glow[i..i + 4];
                let mut px = [
                    mul(g[0], self.edge),
                    mul(g[1], self.edge),
                    mul(g[2], self.edge),
                    mul(g[3], self.edge),
                ];
                for p in &self.patches {
                    let [px0, py0, pw, ph] = p.at;
                    if p.alpha <= 0.0 || x < px0 || y < py0 || x >= px0 + pw || y >= py0 + ph {
                        continue;
                    }
                    let j = (((y - py0) * pw + (x - px0)) * 4) as usize;
                    let s = &p.px[j..j + 4];
                    let a = f32::from(s[3]) * p.alpha / 255.0;
                    for c in 0..4 {
                        px[c] = (f32::from(s[c]) * p.alpha + f32::from(px[c]) * (1.0 - a) + 0.5)
                            .min(255.0) as u8;
                    }
                }
                out[i..i + 4].copy_from_slice(&px);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod wl {
    use std::os::fd::{AsFd, FromRawFd, OwnedFd};
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::{Duration, Instant};

    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{
        wl_buffer::{self, WlBuffer},
        wl_compositor::WlCompositor,
        wl_region::WlRegion,
        wl_registry::WlRegistry,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    };
    use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
    use wayland_protocols_wlr::layer_shell::v1::client::{
        zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
        zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
    };

    use super::{Canvas, Op};

    const LAYER_SHELL: &str = "zwlr_layer_shell_v1";
    /// Nothing to do: the queue is looked at this often (the stop is seen).
    const IDLE: Duration = Duration::from_millis(100);
    /// The longest wait for the compositor to let the buffer go before a redraw.
    const RELEASE_WAIT: Duration = Duration::from_millis(50);

    #[derive(Default)]
    struct State {
        configured: Option<(u32, u32)>,
        closed: bool,
        busy: bool,
    }

    pub fn has_layer_shell() -> bool {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return false;
        }
        let Ok(conn) = Connection::connect_to_env() else {
            return false;
        };
        let Ok((globals, _)) = registry_queue_init::<State>(&conn) else {
            return false;
        };
        globals
            .contents()
            .with_list(|list| list.iter().any(|g| g.interface == LAYER_SHELL))
    }

    pub struct Surface {
        _conn: Connection,
        queue: EventQueue<State>,
        state: State,
        surface: WlSurface,
        layer: ZwlrLayerSurfaceV1,
        buffer: WlBuffer,
        pool: WlShmPool,
        map: *mut u8,
        len: usize,
    }

    impl Surface {
        /// The overlay surface, configured, see-through, click-through; a
        /// buffer of `size` px for it.
        pub fn open((w, h): (u32, u32)) -> Result<Surface, String> {
            let e = |e: &dyn std::fmt::Display| format!("Wayland layer: {e}");
            let conn = Connection::connect_to_env().map_err(|x| e(&x))?;
            let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|x| e(&x))?;
            let qh = queue.handle();
            let compositor: WlCompositor = globals.bind(&qh, 4..=4, ()).map_err(|x| e(&x))?;
            let shm: WlShm = globals.bind(&qh, 1..=1, ()).map_err(|x| e(&x))?;
            let shell: ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ()).map_err(|x| e(&x))?;
            let surface = compositor.create_surface(&qh, ());
            let layer = shell.get_layer_surface(
                &surface,
                None, // the output the desktop picks: the focused one
                Layer::Overlay,
                "opencircle-live".into(),
                &qh,
                (),
            );
            layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            // click-through: an empty input region
            let region = compositor.create_region(&qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();
            surface.commit();
            let mut state = State::default();
            while state.configured.is_none() && !state.closed {
                queue.blocking_dispatch(&mut state).map_err(|x| e(&x))?;
            }
            let (cw, _) = state.configured.ok_or("Wayland layer: closed at once")?;
            // a HiDPI output: our px are physical (ponytail: an integer scale only)
            if cw > 0 && w > cw && w % cw == 0 {
                surface.set_buffer_scale((w / cw) as i32);
            }
            let len = w as usize * h as usize * 4;
            // SAFETY: plain libc calls; every result is checked before use.
            let (fd, map) = unsafe {
                let fd = libc::memfd_create(c"opencircle-live".as_ptr(), libc::MFD_CLOEXEC);
                if fd < 0 || libc::ftruncate(fd, len as libc::off_t) != 0 {
                    return Err("Wayland layer: no shared memory".into());
                }
                let fd = OwnedFd::from_raw_fd(fd);
                let map = libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    std::os::fd::AsRawFd::as_raw_fd(&fd),
                    0,
                );
                if map == libc::MAP_FAILED {
                    return Err("Wayland layer: no shared memory".into());
                }
                (fd, map as *mut u8)
            };
            let pool = shm.create_pool(fd.as_fd(), len as i32, &qh, ());
            let buffer = pool.create_buffer(
                0,
                w as i32,
                h as i32,
                (w * 4) as i32,
                wl_shm::Format::Argb8888,
                &qh,
                (),
            );
            // see-through from the first frame (memfd pages start zeroed)
            surface.attach(Some(&buffer), 0, 0);
            surface.commit();
            state.busy = true;
            conn.flush().map_err(|x| e(&x))?;
            crate::log!("[opencircle] live layer: wayland layer-shell overlay ({w}x{h})");
            Ok(Surface {
                _conn: conn,
                queue,
                state,
                surface,
                layer,
                buffer,
                pool,
                map,
                len,
            })
        }

        /// Draws what `rx` asks until it closes (or the desktop closes the layer).
        pub fn run(&mut self, mut canvas: Canvas, rx: &Receiver<Op>) {
            loop {
                let mut damage: Option<[u32; 4]> = None;
                let mut take = |op: Op, canvas: &mut Canvas| {
                    if let Some(r) = canvas.apply(op) {
                        damage = Some(damage.map_or(r, |d| union(d, r)));
                    }
                };
                match rx.recv_timeout(IDLE) {
                    Ok(op) => take(op, &mut canvas),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                loop {
                    match rx.try_recv() {
                        Ok(op) => take(op, &mut canvas),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                self.pump(Duration::ZERO);
                if self.state.closed {
                    crate::log!("[opencircle] live layer: closed by the desktop");
                    return;
                }
                let Some(d) = damage else { continue };
                let until = Instant::now() + RELEASE_WAIT;
                while self.state.busy && Instant::now() < until {
                    self.pump(RELEASE_WAIT);
                }
                // SAFETY: our own mapping of `len` bytes, alive as long as `self`.
                let out = unsafe { std::slice::from_raw_parts_mut(self.map, self.len) };
                canvas.paint(out, d);
                self.surface.attach(Some(&self.buffer), 0, 0);
                let [x, y, w, h] = d;
                self.surface
                    .damage_buffer(x as i32, y as i32, w as i32, h as i32);
                self.surface.commit();
                self.state.busy = true;
                let _ = self.queue.flush();
            }
        }

        /// Reads what the compositor sent (waiting at most `wait`) and handles it.
        fn pump(&mut self, wait: Duration) {
            let _ = self.queue.flush();
            if let Some(guard) = self.queue.prepare_read() {
                let mut fd = libc::pollfd {
                    fd: std::os::fd::AsRawFd::as_raw_fd(&guard.connection_fd()),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: one valid pollfd.
                let ready = unsafe { libc::poll(&mut fd, 1, wait.as_millis() as i32) };
                if ready > 0 {
                    let _ = guard.read();
                }
            }
            let _ = self.queue.dispatch_pending(&mut self.state);
        }
    }

    impl Drop for Surface {
        fn drop(&mut self) {
            self.layer.destroy();
            self.surface.destroy();
            self.buffer.destroy();
            self.pool.destroy();
            let _ = self.queue.flush();
            // SAFETY: our own mapping, unmapped once.
            unsafe { libc::munmap(self.map as *mut libc::c_void, self.len) };
        }
    }

    fn union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
        let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
        let x1 = (a[0] + a[2]).max(b[0] + b[2]);
        let y1 = (a[1] + a[3]).max(b[1] + b[3]);
        [x0, y0, x1 - x0, y1 - y0]
    }

    impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
        fn event(
            state: &mut State,
            layer: &ZwlrLayerSurfaceV1,
            event: zwlr_layer_surface_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<State>,
        ) {
            match event {
                zwlr_layer_surface_v1::Event::Configure {
                    serial,
                    width,
                    height,
                } => {
                    layer.ack_configure(serial);
                    state.configured = Some((width, height));
                }
                zwlr_layer_surface_v1::Event::Closed => state.closed = true,
                _ => {}
            }
        }
    }

    impl Dispatch<WlBuffer, ()> for State {
        fn event(
            state: &mut State,
            _: &WlBuffer,
            event: wl_buffer::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<State>,
        ) {
            if let wl_buffer::Event::Release = event {
                state.busy = false;
            }
        }
    }

    impl Dispatch<WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut State,
            _: &WlRegistry,
            _: wayland_client::protocol::wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<State>,
        ) {
        }
    }

    delegate_noop!(State: ignore WlCompositor);
    delegate_noop!(State: ignore WlShm);
    delegate_noop!(State: ignore WlShmPool);
    delegate_noop!(State: ignore WlSurface);
    delegate_noop!(State: ignore WlRegion);
    delegate_noop!(State: ignore ZwlrLayerShellV1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Canvas {
        // 8x4, the glow: a half-clear blue strip along the top row
        let strip = [0, 0, 255, 128].repeat(8);
        Canvas::new((8, 4), &vec![([0, 0, 8, 1], strip)])
    }

    fn picture(c: &Canvas) -> Vec<u8> {
        let mut out = vec![0; (c.w * c.h * 4) as usize];
        c.paint(&mut out, [0, 0, c.w, c.h]);
        out
    }

    fn px(out: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [out[i], out[i + 1], out[i + 2], out[i + 3]]
    }

    /// The layer's contract with the live core (as the other layers keep it):
    /// a patch is see-through until it fades in, only the patch that changed
    /// is redrawn, a hidden patch leaves what was under it, the glow fades as a
    /// whole and is premultiplied (what the compositor blends).
    #[test]
    fn only_what_changed_is_redrawn_and_a_patch_fades_in() {
        let mut c = canvas();
        assert_eq!(
            c.apply(Op::Edge(1.0)),
            Some([0, 0, 8, 4]),
            "the glow: all of it, once"
        );
        assert_eq!(c.apply(Op::Edge(1.0)), None, "the same again: nothing");
        assert_eq!(
            px(&picture(&c), 8, 3, 0),
            [128, 0, 0, 128],
            "BGRA premultiplied"
        );
        let red = [255, 0, 0, 255].repeat(4);
        assert_eq!(
            c.apply(Op::Show(1, [2, 1, 2, 2], red)),
            None,
            "see-through until it fades in"
        );
        assert_eq!(px(&picture(&c), 8, 2, 1), [0, 0, 0, 0]);
        assert_eq!(
            c.apply(Op::Alpha(1, 0.5)),
            Some([2, 1, 2, 2]),
            "only the patch"
        );
        assert_eq!(px(&picture(&c), 8, 2, 1), [0, 0, 128, 128], "half in");
        c.apply(Op::Alpha(1, 1.0));
        assert_eq!(px(&picture(&c), 8, 3, 2), [0, 0, 255, 255]);
        assert_eq!(c.apply(Op::Alpha(9, 1.0)), None, "no such patch");
        // a new picture for the same patch clears the old one's place
        let green = [0, 255, 0, 255].to_vec();
        assert_eq!(
            c.apply(Op::Show(1, [7, 3, 1, 1], green)),
            Some([2, 1, 2, 2])
        );
        assert_eq!(c.apply(Op::Alpha(1, 1.0)), Some([7, 3, 1, 1]));
        assert_eq!(c.apply(Op::Hide(1)), Some([7, 3, 1, 1]), "hidden: its rect");
        assert_eq!(
            picture(&c)[(8 * 4) as usize..],
            vec![0; 8 * 3 * 4][..],
            "nothing left"
        );
        // fading the glow out: every pixel follows
        c.apply(Op::Edge(0.0));
        assert!(picture(&c).iter().all(|&b| b == 0));
    }

    #[test]
    fn a_patch_past_the_surface_is_clipped() {
        let mut c = canvas();
        c.apply(Op::Show(2, [6, 2, 4, 4], [9; 64].to_vec()));
        assert_eq!(c.apply(Op::Alpha(2, 1.0)), Some([6, 2, 2, 2]));
        let _ = picture(&c); // (paints inside the surface only)
        assert_eq!(
            c.apply(Op::Show(3, [1, 1, 2, 2], vec![1; 3])),
            None,
            "a wrong size: dropped"
        );
    }
}
