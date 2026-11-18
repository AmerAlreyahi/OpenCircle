//! Windows: floating widgets with per-pixel alpha AND out of screen captures. A window without a
//! redirection bitmap (`iced_winit::NO_REDIRECTION`) shows what is put into a DirectComposition
//! surface: the CPU renderer's premultiplied frame, copied in on every present (only the redrawn
//! rectangles). Unlike a layered window it accepts `WDA_EXCLUDEFROMCAPTURE` (lab, W74: GDI and
//! DXGI captures leave it out). The D3D / DirectComposition devices live only while such a window
//! does (a few MB), made when the first one is about to open. If they can't be made (no D3D at all)
//! widgets stay cut windows (`shape`), as before.
//! Part of the UI (a window; `app::arch_tests`).

#[cfg(windows)]
pub use win::{forget, is_dcomp, prepare};

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;

    use iced::window::raw_window_handle::RawWindowHandle;
    use iced_tiny_skia::window::compositor::DamageRect;
    use windows::Win32::Foundation::{HMODULE, HWND, POINT};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
        ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::DirectComposition::{
        DCompositionCreateDevice, IDCompositionDevice, IDCompositionSurface, IDCompositionTarget,
        IDCompositionVisual,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_NOREDIRECTIONBITMAP,
    };
    use windows::core::Interface;

    struct Devices {
        _d3d: ID3D11Device,
        ctx: ID3D11DeviceContext,
        dc: IDCompositionDevice,
    }

    struct Win {
        _target: IDCompositionTarget,
        _visual: IDCompositionVisual,
        surface: IDCompositionSurface,
        size: (u32, u32),
    }

    #[derive(Default)]
    struct State {
        devices: Option<Devices>,
        windows: HashMap<isize, Win>,
    }

    thread_local! {
        /// The UI thread's (every present and every window open happens there).
        static STATE: RefCell<State> = RefCell::new(State::default());
    }

    fn devices() -> Result<Devices, String> {
        let make = |kind: D3D_DRIVER_TYPE| {
            let mut dev = None;
            // SAFETY: plain device creation; `dev` outlives the call.
            unsafe {
                D3D11CreateDevice(
                    None,
                    kind,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut dev),
                    None,
                    None,
                )
            }
            .map(|()| dev)
            .map_err(|e| e.to_string())
        };
        let d3d = make(D3D_DRIVER_TYPE_HARDWARE)
            .or_else(|_| make(D3D_DRIVER_TYPE_WARP))?
            .ok_or("no D3D device")?;
        let dxgi: IDXGIDevice = d3d.cast().map_err(|e| e.to_string())?;
        // SAFETY: a DirectComposition device over the DXGI device just made.
        let dc: IDCompositionDevice =
            unsafe { DCompositionCreateDevice(&dxgi) }.map_err(|e| e.to_string())?;
        // SAFETY: reading the immediate context of the device just made.
        let ctx = unsafe { d3d.GetImmediateContext() }.map_err(|e| e.to_string())?;
        Ok(Devices { _d3d: d3d, ctx, dc })
    }

    /// Called right before a floating window opens: makes the devices when none exist, and
    /// tells the window shell whether the next floating window gets no redirection bitmap.
    pub fn prepare() {
        if crate::shared::platform::sys::test_hook(crate::shared::platform::sys::NO_DCOMP).is_some()
        {
            return;
        }
        let _ = iced_tiny_skia::window::compositor::ALPHA_PRESENT.set(present);
        let ok = STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.devices.is_none() {
                match devices() {
                    Ok(d) => s.devices = Some(d),
                    Err(e) => crate::log!("[opencircle] DirectComposition unavailable: {e}"),
                }
            }
            s.devices.is_some()
        });
        iced_winit::NO_REDIRECTION.store(ok, Ordering::Relaxed);
    }

    /// A window closed: its surface goes; the devices go with the last one.
    pub fn forget(hwnd: isize) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.windows.remove(&hwnd);
            if s.windows.is_empty() {
                s.devices = None;
                iced_winit::NO_REDIRECTION.store(false, Ordering::Relaxed);
            }
        });
    }

    /// Whether `hwnd` is drawn through DirectComposition (made without a redirection bitmap).
    pub fn is_dcomp(hwnd: isize) -> bool {
        // SAFETY: reading a live window's extended style.
        let ex = unsafe { GetWindowLongPtrW(HWND(hwnd as *mut _), GWL_EXSTYLE) };
        ex & WS_EX_NOREDIRECTIONBITMAP.0 as isize != 0
    }

    /// `iced_tiny_skia`'s `ALPHA_PRESENT`: the frame of a DirectComposition window goes into its
    /// surface (`rects`: the redrawn parts, `None` all). `false`: not such a window, or it failed.
    pub fn present(
        handle: RawWindowHandle,
        px: &[u32],
        (w, h): (u32, u32),
        rects: Option<&[DamageRect]>,
    ) -> bool {
        let RawWindowHandle::Win32(handle) = handle else {
            return false;
        };
        let hwnd = handle.hwnd.get();
        if w == 0 || h == 0 || px.len() < (w * h) as usize || !is_dcomp(hwnd) {
            return false;
        }
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.devices.is_none() {
                match devices() {
                    Ok(d) => s.devices = Some(d),
                    Err(e) => {
                        crate::log!("[opencircle] DirectComposition: {e}");
                        return false;
                    }
                }
            }
            match draw(&mut s, hwnd, px, (w, h), rects) {
                Ok(()) => true,
                Err(e) => {
                    crate::log!("[opencircle] DirectComposition present: {e}");
                    s.windows.remove(&hwnd);
                    false
                }
            }
        })
    }

    fn draw(
        s: &mut State,
        hwnd: isize,
        px: &[u32],
        (w, h): (u32, u32),
        rects: Option<&[DamageRect]>,
    ) -> Result<(), String> {
        let e = |e: windows::core::Error| e.to_string();
        let State { devices, windows } = s;
        let d = devices.as_ref().ok_or("no devices")?;
        // SAFETY: COM calls on devices made by this thread; `px` holds `w * h` pixels.
        unsafe {
            let fresh = windows.get(&hwnd).is_none_or(|win| win.size != (w, h));
            if fresh {
                let target =
                    d.dc.CreateTargetForHwnd(HWND(hwnd as *mut _), true)
                        .map_err(e)?;
                let visual = d.dc.CreateVisual().map_err(e)?;
                let surface =
                    d.dc.CreateSurface(
                        w,
                        h,
                        DXGI_FORMAT_B8G8R8A8_UNORM,
                        DXGI_ALPHA_MODE_PREMULTIPLIED,
                    )
                    .map_err(e)?;
                visual.SetContent(&surface).map_err(e)?;
                target.SetRoot(&visual).map_err(e)?;
                windows.insert(
                    hwnd,
                    Win {
                        _target: target,
                        _visual: visual,
                        surface,
                        size: (w, h),
                    },
                );
            }
            let win = windows.get(&hwnd).ok_or("no window")?;
            let all = [DamageRect {
                x: 0,
                y: 0,
                width: w.try_into().unwrap_or(std::num::NonZeroU32::MIN),
                height: h.try_into().unwrap_or(std::num::NonZeroU32::MIN),
            }];
            // a new surface has nothing yet: all of it, whatever was redrawn
            let rects = if fresh {
                &all[..]
            } else {
                rects.unwrap_or(&all[..])
            };
            for r in rects {
                let (x, y) = (r.x, r.y);
                let (rw, rh) = (r.width.get().min(w - x), r.height.get().min(h - y));
                let rect = windows::Win32::Foundation::RECT {
                    left: x as i32,
                    top: y as i32,
                    right: (x + rw) as i32,
                    bottom: (y + rh) as i32,
                };
                let mut at = POINT::default();
                let tex: ID3D11Texture2D =
                    win.surface.BeginDraw(Some(&rect), &mut at).map_err(e)?;
                let b = D3D11_BOX {
                    left: at.x as u32,
                    top: at.y as u32,
                    right: at.x as u32 + rw,
                    bottom: at.y as u32 + rh,
                    front: 0,
                    back: 1,
                };
                d.ctx.UpdateSubresource(
                    &tex,
                    0,
                    Some(&b),
                    px.as_ptr().add((y * w + x) as usize).cast(),
                    w * 4,
                    0,
                );
                win.surface.EndDraw().map_err(e)?;
            }
            d.dc.Commit().map_err(e)?;
        }
        Ok(())
    }
}
