//! A live pin's source (Windows): one window, captured by itself (Windows
//! Graphics Capture: it keeps updating when the window is covered or moved),
//! cropped to the part the user picked. The crop is kept relative to the
//! window's client area (it follows a move and a resize, and scales with the
//! window's DPI). The capture only delivers a frame when the window changed,
//! and a changed frame is read at most every `MIN_GAP` (~10 fps): a still
//! window costs nothing. Elsewhere: not supported (the UI hides it).
#![cfg_attr(not(windows), allow(dead_code))]
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use super::window_rect::WinRect;
use crate::actions::Crop;

/// Live pins exist here.
pub const SUPPORTED: bool = cfg!(windows);

/// At most this often a frame is read (~10 fps).
pub const MIN_GAP: Duration = Duration::from_millis(100);

/// The picked part of a window: its offset from the client area's top-left and
/// its size, physical px at the window's `dpi` then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub dpi: u32,
}

/// Where the window is now, physical desktop px: the captured frame's rect
/// (the DWM frame bounds: what the window capture shows), its client area, its DPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geom {
    pub frame: WinRect,
    pub client: WinRect,
    pub dpi: u32,
}

/// `a` ∩ `b` (x, y, w, h), `None` when empty.
fn meet(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> Option<(i64, i64, i64, i64)> {
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let (x1, y1) = ((a.0 + a.2).min(b.0 + b.2), (a.1 + a.3).min(b.1 + b.3));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

fn wide(r: WinRect) -> (i64, i64, i64, i64) {
    (r.0 as i64, r.1 as i64, r.2 as i64, r.3 as i64)
}

/// The `region` (physical desktop px) the user picked over the window at `g`:
/// its part inside the client area, relative to it. `None` = it misses it.
pub fn pick(region: WinRect, g: &Geom) -> Option<Pick> {
    let (x, y, w, h) = meet(wide(region), wide(g.client))?;
    Some(Pick {
        x: (x - g.client.0 as i64) as i32,
        y: (y - g.client.1 as i64) as i32,
        w: w as u32,
        h: h as u32,
        dpi: g.dpi,
    })
}

/// The pick in the frame captured of the window at `g`, whose texture is
/// `content` px: x, y, width, height. A moved window keeps the crop (it is
/// relative to the client area); a resized one cuts it to what is left of the
/// client area; a DPI change scales it (the app redrew at the new scale).
/// `None` = nothing of it is left (shrunk away, minimised).
pub fn crop(p: &Pick, g: &Geom, content: (u32, u32)) -> Option<[u32; 4]> {
    let s = g.dpi.max(1) as f64 / p.dpi.max(1) as f64;
    let scaled = |v: f64| (v * s).round() as i64;
    let (cx, cy) = (
        g.client.0 as i64 - g.frame.0 as i64,
        g.client.1 as i64 - g.frame.1 as i64,
    );
    let want = (
        cx + scaled(p.x as f64),
        cy + scaled(p.y as f64),
        scaled(p.w as f64),
        scaled(p.h as f64),
    );
    let client = (cx, cy, g.client.2 as i64, g.client.3 as i64);
    let texture = (0, 0, content.0 as i64, content.1 as i64);
    let (x, y, w, h) = meet(meet(want, client)?, texture)?;
    Some([x as u32, y as u32, w as u32, h as u32])
}

/// How long to wait before reading a changed frame, the last read at `last`.
pub fn wait(last: Option<Instant>, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |t| {
        MIN_GAP.saturating_sub(now.saturating_duration_since(t))
    })
}

/// What a live source tells its pin.
#[derive(Debug, Clone)]
pub enum Event {
    /// The picked part, as it is now.
    Frame(Crop),
    /// The source window closed.
    Gone,
    /// The capture couldn't start or broke: the pin keeps its last picture.
    Failed(String),
}

/// Why the capture thread wakes.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug)]
enum Wake {
    Frame,
    Closed,
    Stop,
}

/// A running source: dropping it stops the capture (its thread ends, the
/// window capture closes).
#[derive(Debug)]
pub struct Source(Sender<Wake>);

impl Drop for Source {
    fn drop(&mut self) {
        let _ = self.0.send(Wake::Stop);
    }
}

/// A source and the run that captures for it: call `run` on a thread of its
/// own; it blocks until the source is dropped, the window closes or `send`
/// returns false (the pin is gone).
pub fn source(
    window: isize,
    pick: Pick,
    send: impl FnMut(Event) -> bool + Send + 'static,
) -> (Source, impl FnOnce() + Send) {
    let (tx, rx) = channel();
    let wake = tx.clone();
    (Source(tx), move || run(window, pick, wake, rx, send))
}

/// Where the window `window` is now.
pub fn geom(window: isize) -> Result<Geom, String> {
    #[cfg(windows)]
    return win::geom(window);
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("live pins need Windows".into())
    }
}

#[cfg(not(windows))]
fn run(
    _window: isize,
    _pick: Pick,
    _wake: Sender<Wake>,
    _rx: Receiver<Wake>,
    mut send: impl FnMut(Event) -> bool,
) {
    send(Event::Failed("live pins need Windows".into()));
}

#[cfg(windows)]
fn run(
    window: isize,
    pick: Pick,
    wake: Sender<Wake>,
    rx: Receiver<Wake>,
    mut send: impl FnMut(Event) -> bool,
) {
    let mut capture = match win::Capture::start(window, wake) {
        Ok(c) => c,
        Err(e) => {
            send(Event::Failed(e));
            return;
        }
    };
    let mut last = None;
    loop {
        match rx.recv() {
            Ok(Wake::Frame) => {}
            Ok(Wake::Closed) => {
                send(Event::Gone);
                return;
            }
            Ok(Wake::Stop) | Err(_) => return,
        }
        std::thread::sleep(wait(last, Instant::now()));
        // what came meanwhile: one read for all the frames
        for w in rx.try_iter() {
            match w {
                Wake::Frame => {}
                Wake::Closed => {
                    send(Event::Gone);
                    return;
                }
                Wake::Stop => return,
            }
        }
        last = Some(Instant::now());
        match capture.read(window, &pick) {
            Ok(Some(crop)) => {
                if !send(Event::Frame(crop)) {
                    return;
                }
            }
            Ok(None) => {}
            Err(e) => crate::log!("[opencircle] live pin frame: {e}"),
        }
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::sync::mpsc::Sender;

    use windows::Foundation::TypedEventHandler;
    use windows::Graphics::Capture::{
        Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
    };
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Graphics::SizeInt32;
    use windows::Win32::Foundation::{HMODULE, HWND, POINT, RECT};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    use windows::core::{IInspectable, Interface, factory};

    use super::{Geom, Pick, Wake};
    use crate::actions::Crop;

    const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;
    /// Two buffers: one being read, one the next change lands in.
    const BUFFERS: i32 = 2;

    pub fn geom(window: isize) -> Result<Geom, String> {
        let hwnd = HWND(window as *mut c_void);
        let mut frame = RECT::default();
        let mut client = RECT::default();
        let mut origin = POINT::default();
        // SAFETY: plain queries on a window handle; a dead one makes them fail.
        unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&raw mut frame).cast::<c_void>(),
                size_of::<RECT>() as u32,
            )
            .map_err(|e| e.to_string())?;
            GetClientRect(hwnd, &mut client).map_err(|e| e.to_string())?;
            if !ClientToScreen(hwnd, &mut origin).as_bool() {
                return Err("the window is gone".into());
            }
            let dpi = GetDpiForWindow(hwnd);
            let size = |r: &RECT| {
                (
                    (r.right - r.left).max(0) as u32,
                    (r.bottom - r.top).max(0) as u32,
                )
            };
            let (fw, fh) = size(&frame);
            let (cw, ch) = size(&client);
            Ok(Geom {
                frame: (frame.left, frame.top, fw, fh),
                client: (origin.x, origin.y, cw, ch),
                dpi: if dpi == 0 { 96 } else { dpi },
            })
        }
    }

    fn device(kind: D3D_DRIVER_TYPE) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
        let (mut device, mut context) = (None, None);
        // SAFETY: out pointers to locals; no adapter (the driver type picks).
        unsafe {
            D3D11CreateDevice(
                None,
                kind,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        Ok((device.unwrap(), context.unwrap()))
    }

    /// One window's capture: its frame pool and session, and the texture the
    /// picked part is copied to for reading.
    pub struct Capture {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        d3d: IDirect3DDevice,
        pool: Direct3D11CaptureFramePool,
        session: GraphicsCaptureSession,
        size: SizeInt32,
        staging: Option<(ID3D11Texture2D, u32, u32)>,
        _item: GraphicsCaptureItem,
    }

    impl Capture {
        /// Starts capturing `window`; every change wakes `wake` (`Wake::Frame`),
        /// its closing too (`Wake::Closed`).
        pub fn start(window: isize, wake: Sender<Wake>) -> Result<Capture, String> {
            let err = |e: windows::core::Error| e.message();
            // A GPU-less PC (the test VM) has only WARP.
            let (device, context) = device(D3D_DRIVER_TYPE_HARDWARE)
                .or_else(|_| device(D3D_DRIVER_TYPE_WARP))
                .map_err(err)?;
            let dxgi: IDXGIDevice = device.cast().map_err(err)?;
            // SAFETY: a live DXGI device of ours.
            let inspectable: IInspectable =
                unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(err)?;
            let d3d: IDirect3DDevice = inspectable.cast().map_err(err)?;
            let interop =
                factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(err)?;
            // SAFETY: a window handle; a dead one fails.
            let item: GraphicsCaptureItem =
                unsafe { interop.CreateForWindow(HWND(window as *mut c_void)) }.map_err(err)?;
            let size = item.Size().map_err(err)?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, FORMAT, BUFFERS, size)
                .map_err(err)?;
            let session = pool.CreateCaptureSession(&item).map_err(err)?;
            // Windows 10 2004+ / 11: no pointer in the pin, no yellow border
            // (a Windows 10 that can't drop the border keeps it).
            let _ = session.SetIsCursorCaptureEnabled(false);
            let _ = session.SetIsBorderRequired(false);
            let frames = wake.clone();
            pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
                let _ = frames.send(Wake::Frame);
                Ok(())
            }))
            .map_err(err)?;
            item.Closed(&TypedEventHandler::new(move |_, _| {
                let _ = wake.send(Wake::Closed);
                Ok(())
            }))
            .map_err(err)?;
            session.StartCapture().map_err(err)?;
            Ok(Capture {
                device,
                context,
                d3d,
                pool,
                session,
                size,
                staging: None,
                _item: item,
            })
        }

        /// The newest frame's picked part, if a frame is waiting.
        pub fn read(&mut self, window: isize, pick: &Pick) -> Result<Option<Crop>, String> {
            let err = |e: windows::core::Error| e.message();
            let mut newest = None;
            while let Ok(frame) = self.pool.TryGetNextFrame() {
                if let Some(old) = newest.replace(frame) {
                    let _ = old.Close();
                }
            }
            let Some(frame) = newest else {
                return Ok(None);
            };
            let read = self.read_frame(&frame, window, pick);
            let _ = frame.Close();
            // a resized window: the pool follows (the next frame has the new size)
            if let Ok(size) = frame.ContentSize()
                && (size.Width, size.Height) != (self.size.Width, self.size.Height)
                && size.Width > 0
                && size.Height > 0
            {
                self.pool
                    .Recreate(&self.d3d, FORMAT, BUFFERS, size)
                    .map_err(err)?;
                self.size = size;
            }
            read
        }

        fn read_frame(
            &mut self,
            frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
            window: isize,
            pick: &Pick,
        ) -> Result<Option<Crop>, String> {
            let err = |e: windows::core::Error| e.message();
            let access: IDirect3DDxgiInterfaceAccess =
                frame.Surface().map_err(err)?.cast().map_err(err)?;
            // SAFETY: the frame's surface is a D3D11 texture of our device.
            let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(err)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            // SAFETY: out pointer to a local.
            unsafe { texture.GetDesc(&mut desc) };
            let content = frame.ContentSize().map_err(err)?;
            let content = (
                (content.Width.max(0) as u32).min(desc.Width),
                (content.Height.max(0) as u32).min(desc.Height),
            );
            let Some([x, y, w, h]) = super::crop(pick, &geom(window)?, content) else {
                return Ok(None);
            };
            let staging = match &self.staging {
                Some((t, sw, sh)) if (*sw, *sh) == (w, h) => t.clone(),
                _ => {
                    let desc = D3D11_TEXTURE2D_DESC {
                        Width: w,
                        Height: h,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_STAGING,
                        BindFlags: 0,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        MiscFlags: 0,
                    };
                    let mut t = None;
                    // SAFETY: a valid description; out pointer to a local.
                    unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut t)) }
                        .map_err(err)?;
                    let t = t.ok_or("no texture")?;
                    self.staging = Some((t.clone(), w, h));
                    t
                }
            };
            let area = D3D11_BOX {
                left: x,
                top: y,
                front: 0,
                right: x + w,
                bottom: y + h,
                back: 1,
            };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let mut rgba = vec![0u8; w as usize * h as usize * 4];
            // SAFETY: `area` lies inside the frame (`crop` clips it to the
            // texture); the mapped rows are `RowPitch` apart and `w * 4` long.
            unsafe {
                self.context
                    .CopySubresourceRegion(&staging, 0, 0, 0, 0, &texture, 0, Some(&area));
                self.context
                    .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .map_err(err)?;
                let row = w as usize * 4;
                for (i, out) in rgba.chunks_exact_mut(row).enumerate() {
                    let src = (mapped.pData as *const u8).add(i * mapped.RowPitch as usize);
                    out.copy_from_slice(std::slice::from_raw_parts(src, row));
                }
                self.context.Unmap(&staging, 0);
            }
            for px in rgba.as_chunks_mut::<4>().0 {
                px.swap(0, 2); // BGRA -> RGBA
                px[3] = 255;
            }
            Ok(Some(Crop {
                width: w,
                height: h,
                rgba,
            }))
        }
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            let _ = self.session.Close();
            let _ = self.pool.Close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window with a 8 px frame edge and a 31 px title bar over its client area.
    fn at(x: i32, y: i32, w: u32, h: u32, dpi: u32) -> Geom {
        Geom {
            frame: (x, y, w + 16, h + 39),
            client: (x + 8, y + 31, w, h),
            dpi,
        }
    }

    #[test]
    fn the_crop_follows_the_client_area_when_the_window_moves() {
        let g = at(100, 50, 800, 600, 96);
        let p = pick((200, 150, 300, 200), &g).unwrap();
        assert_eq!((p.x, p.y, p.w, p.h), (92, 69, 300, 200));
        let content = (g.frame.2, g.frame.3);
        // in the frame: the client offset + the pick
        assert_eq!(crop(&p, &g, content), Some([100, 100, 300, 200]));
        // moved anywhere (another monitor, partly off screen): the same crop
        for (x, y) in [(900, 700), (-400, -20), (3000, 10)] {
            assert_eq!(
                crop(&p, &at(x, y, 800, 600, 96), content),
                Some([100, 100, 300, 200])
            );
        }
    }

    #[test]
    fn a_resized_window_cuts_the_crop_to_its_client_area() {
        let p = pick((200, 150, 300, 200), &at(100, 50, 800, 600, 96)).unwrap();
        // narrower: only what is left of the client area
        let g = at(100, 50, 250, 600, 96);
        assert_eq!(
            crop(&p, &g, (g.frame.2, g.frame.3)),
            Some([100, 100, 158, 200])
        );
        // the texture still has the old (bigger) size for a frame: the client area cuts
        assert_eq!(crop(&p, &g, (816, 639)), Some([100, 100, 158, 200]));
        // shrunk away, or minimised (nothing to read)
        let g = at(100, 50, 80, 60, 96);
        assert_eq!(crop(&p, &g, (g.frame.2, g.frame.3)), None);
        assert_eq!(crop(&p, &at(0, 0, 800, 600, 96), (0, 0)), None);
    }

    #[test]
    fn a_dpi_change_scales_the_crop_with_the_app() {
        let p = pick((200, 150, 300, 200), &at(100, 50, 800, 600, 96)).unwrap();
        // dragged to a 150 % monitor: the app draws 1.5x, so does the crop
        let g = at(2000, 0, 1200, 900, 144);
        assert_eq!(
            crop(&p, &g, (g.frame.2, g.frame.3)),
            Some([146, 135, 450, 300])
        );
        // and back
        let g = at(100, 50, 800, 600, 96);
        assert_eq!(
            crop(&p, &g, (g.frame.2, g.frame.3)),
            Some([100, 100, 300, 200])
        );
    }

    #[test]
    fn a_region_off_the_client_area_is_cut_to_it_or_refused() {
        let g = at(100, 50, 800, 600, 96);
        // over the title bar: only the client part
        let p = pick((150, 50, 200, 100), &g).unwrap();
        assert_eq!((p.x, p.y, p.w, p.h), (42, 0, 200, 69));
        assert_eq!(pick((0, 0, 50, 40), &g), None, "beside the window");
    }

    #[test]
    fn frames_are_read_at_most_every_min_gap() {
        let t = Instant::now();
        assert_eq!(wait(None, t), Duration::ZERO, "the first at once");
        assert_eq!(wait(Some(t), t), MIN_GAP);
        assert_eq!(
            wait(Some(t), t + Duration::from_millis(30)),
            Duration::from_millis(70)
        );
        assert_eq!(wait(Some(t), t + Duration::from_secs(5)), Duration::ZERO);
        // a clock that went back never waits longer than the gap
        assert_eq!(wait(Some(t + Duration::from_secs(1)), t), MIN_GAP);
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_a_source_says_it_is_unsupported() {
        let (tx, rx) = std::sync::mpsc::channel();
        let p = Pick {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            dpi: 96,
        };
        let (_source, run) = source(1, p, move |e| tx.send(e).is_ok());
        run();
        assert!(matches!(rx.recv(), Ok(Event::Failed(_))));
    }
}
