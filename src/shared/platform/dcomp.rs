//! Windows: floating widgets with per-pixel alpha AND out of screen captures. A window without a
//! redirection bitmap (`iced_winit::NO_REDIRECTION`) shows what is put into a DirectComposition
//! surface: the CPU renderer's premultiplied frame, copied in on every present (only the redrawn
//! rectangles). Unlike a layered window it accepts `WDA_EXCLUDEFROMCAPTURE` (lab, W74: GDI and
//! DXGI captures leave it out). The D3D / DirectComposition devices live only while such a window
//! does (a few MB), made when the first one is about to open. If they can't be made (no D3D at all)
//! widgets stay cut windows (`shape`), as before.
//! Part of the UI (a window; `app::arch_tests`).

/// What one present of a DirectComposition window must do (pure: the decision tests pin down).
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Plan {
    /// First frame: a target, a visual, a surface.
    Fresh,
    /// The window was resized: keep the target and visual (a second target on the same
    /// window fails, `WINDOW_ALREADY_COMPOSED`: the widget went blank), swap the surface.
    NewSurface,
    /// Same size: only the redrawn rectangles.
    Update,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn plan(have: Option<(u32, u32)>, want: (u32, u32)) -> Plan {
    match have {
        None => Plan::Fresh,
        Some(size) if size != want => Plan::NewSurface,
        Some(_) => Plan::Update,
    }
}

/// What to do after a present failed (a lost device: driver update, TDR).
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Failure {
    /// Rebuild the devices and surfaces, try the frame once more.
    Reset,
    /// It failed again: no more DirectComposition this run; new widgets are cut windows.
    GiveUp,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn on_failure(retried: bool) -> Failure {
    if retried {
        Failure::GiveUp
    } else {
        Failure::Reset
    }
}

/// Set once DirectComposition failed for good (`Failure::GiveUp`): widgets opened from now on
/// are cut windows (`shape`), never windows that can't be drawn.
#[cfg_attr(not(windows), allow(dead_code))]
static GAVE_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg_attr(not(windows), allow(dead_code))]
pub fn gave_up() -> bool {
    GAVE_UP.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(windows)]
pub use win::{forget, is_dcomp, prepare};

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;

    use super::{Failure, GAVE_UP, Plan, gave_up, on_failure, plan};
    use iced::window::raw_window_handle::RawWindowHandle;
    use iced_tiny_skia::window::compositor::DamageRect;
    use windows::Win32::Foundation::{HMODULE, HWND, POINT, RECT};
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
        GWL_EXSTYLE, GetWindowLongPtrW, IsWindow, WS_EX_NOREDIRECTIONBITMAP,
    };
    use windows::core::Interface;

    struct Devices {
        _d3d: ID3D11Device,
        ctx: ID3D11DeviceContext,
        dc: IDCompositionDevice,
    }

    struct Win {
        _target: IDCompositionTarget,
        visual: IDCompositionVisual,
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
    /// tells the window shell whether the next floating window gets no redirection bitmap
    /// (not after DirectComposition gave up: then it is a cut window, as before).
    pub fn prepare() {
        if crate::shared::platform::sys::test_hook(crate::shared::platform::sys::NO_DCOMP).is_some()
            || gave_up()
        {
            iced_winit::NO_REDIRECTION.store(false, Ordering::Relaxed);
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

    /// A window closed: its surface goes (and any other closed window's); the devices go with
    /// the last one, so nothing stays resident while no widget shows.
    pub fn forget(hwnd: isize) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.windows.remove(&hwnd);
            // SAFETY: asking whether a handle is still a window.
            s.windows
                .retain(|h, _| unsafe { IsWindow(Some(HWND(*h as *mut _))) }.as_bool());
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
    /// A failed frame resets the devices and surfaces and is tried once more; a second failure
    /// ends DirectComposition for this run (logged once).
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
        // (a window already closing is no failure of ours: nothing to show, nothing to reset)
        // SAFETY: asking whether a handle is still a window.
        let alive = unsafe { IsWindow(Some(HWND(hwnd as *mut _))) }.as_bool();
        if w == 0 || h == 0 || px.len() < (w * h) as usize || !alive || !is_dcomp(hwnd) {
            return false;
        }
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let mut retried = false;
            loop {
                let tried = match s.devices {
                    Some(_) => draw(&mut s, hwnd, px, (w, h), rects),
                    None => devices().map(|d| s.devices = Some(d)).and_then(|()| {
                        draw(&mut s, hwnd, px, (w, h), rects)
                    }),
                };
                let Err(e) = tried else { return true };
                // lost device or a broken surface: everything is made again (the old
                // targets first: a window takes one target)
                s.windows.clear();
                s.devices = None;
                match on_failure(retried) {
                    Failure::Reset => {
                        crate::log!("[opencircle] DirectComposition: {e}; resetting");
                        retried = true;
                    }
                    Failure::GiveUp => {
                        if !GAVE_UP.swap(true, Ordering::Relaxed) {
                            crate::log!(
                                "[opencircle] DirectComposition: {e}; off for this run, widgets are cut windows"
                            );
                        }
                        iced_winit::NO_REDIRECTION.store(false, Ordering::Relaxed);
                        return false;
                    }
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
        let step = plan(windows.get(&hwnd).map(|win| win.size), (w, h));
        // SAFETY: COM calls on devices made by this thread; `px` holds `w * h` pixels.
        unsafe {
            if step != Plan::Update {
                let surface =
                    d.dc.CreateSurface(
                        w,
                        h,
                        DXGI_FORMAT_B8G8R8A8_UNORM,
                        DXGI_ALPHA_MODE_PREMULTIPLIED,
                    )
                    .map_err(e)?;
                if step == Plan::Fresh {
                    // windows closed meanwhile: their entries go (every close, not only some)
                    windows.retain(|h, _| IsWindow(Some(HWND(*h as *mut _))).as_bool());
                    let target =
                        d.dc.CreateTargetForHwnd(HWND(hwnd as *mut _), true)
                            .map_err(e)?;
                    let visual = d.dc.CreateVisual().map_err(e)?;
                    visual.SetContent(&surface).map_err(e)?;
                    target.SetRoot(&visual).map_err(e)?;
                    windows.insert(
                        hwnd,
                        Win {
                            _target: target,
                            visual,
                            surface,
                            size: (w, h),
                        },
                    );
                } else if let Some(win) = windows.get_mut(&hwnd) {
                    win.visual.SetContent(&surface).map_err(e)?;
                    win.surface = surface;
                    win.size = (w, h);
                }
            }
            let win = windows.get(&hwnd).ok_or("no window")?;
            let all = [DamageRect {
                x: 0,
                y: 0,
                width: w.try_into().unwrap_or(std::num::NonZeroU32::MIN),
                height: h.try_into().unwrap_or(std::num::NonZeroU32::MIN),
            }];
            // a new surface has nothing yet: all of it, whatever was redrawn
            let rects = if step == Plan::Update {
                rects.unwrap_or(&all[..])
            } else {
                &all[..]
            };
            for r in rects {
                let (x, y) = (r.x, r.y);
                let (rw, rh) = (r.width.get().min(w - x), r.height.get().min(h - y));
                let rect = RECT {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resized_window_keeps_its_target_and_swaps_its_surface() {
        assert_eq!(plan(None, (100, 40)), Plan::Fresh);
        assert_eq!(plan(Some((100, 40)), (100, 40)), Plan::Update);
        // a second target on the same window fails (the widget went blank)
        assert_eq!(plan(Some((100, 40)), (140, 40)), Plan::NewSurface);
    }

    #[test]
    fn a_failed_frame_resets_once_then_gives_up() {
        assert_eq!(on_failure(false), Failure::Reset);
        assert_eq!(on_failure(true), Failure::GiveUp);
        // (after giving up, `prepare` makes new widgets cut windows; the flag starts clear)
        assert!(!gave_up());
    }
}
