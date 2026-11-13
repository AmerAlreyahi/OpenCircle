//! Windows capture: the screen as shown, through GDI (windows that keep out of
//! captures, ours among them, are not in it); monitors listed by xcap.
//! Positions and sizes are physical desktop px (the process is per-monitor DPI
//! aware, winit's default).
use xcap::Monitor;

use super::Frame;

pub fn monitors() -> Result<Vec<super::Monitor>, String> {
    let err = |e: xcap::XCapError| e.to_string();
    (Monitor::all().map_err(err)?.iter())
        .map(|m| {
            Ok(super::Monitor {
                rect: (
                    m.x().map_err(err)?,
                    m.y().map_err(err)?,
                    m.width().map_err(err)?,
                    m.height().map_err(err)?,
                ),
                scale: m.scale_factor().unwrap_or(1.0),
                primary: m.is_primary().unwrap_or(false),
            })
        })
        .collect()
}

pub fn grab(m: &super::Monitor) -> Result<Frame, String> {
    let (x, y, width, height) = m.rect;
    Ok(Frame {
        rgba: grab_rect(m.rect)?,
        width,
        height,
        x,
        y,
        scale: m.scale,
    })
}

/// `w` x `h` px of the desktop at (`x`, `y`), RGBA. The screen is copied
/// into a DIB section (its pixels are ours to read: no GetDIBits copy) and
/// turned into RGBA in one pass, on up to 4 cores for a screen. xcap's grab
/// (a zeroed buffer, GetDIBits into it, then a swap per pixel on one core,
/// after listing the monitors again) cost ~20 ms more at 3440x1440 on the VM.
/// The copy itself (DWM hands GDI the composed screen) is most of a freeze.
pub fn grab_rect((x, y, w, h): (i32, i32, u32, u32)) -> Result<Vec<u8>, String> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SRCCOPY, SelectObject,
    };
    if w == 0 || h == 0 {
        return Err("nothing to grab".into());
    }
    let (wi, hi) = (w as i32, h as i32);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: wi,
            biHeight: -hi, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: plain GDI calls on handles made here and released before
    // returning; the DIB section holds exactly `h` top-down rows of `w`
    // 32-bit pixels, read only while it is alive, after GdiFlush.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(dib) => {
                let old = SelectObject(mem, dib.into());
                let copied = BitBlt(mem, 0, 0, wi, hi, Some(screen), x, y, SRCCOPY);
                let _ = GdiFlush();
                let result = match copied {
                    Ok(()) if !bits.is_null() => {
                        let n = w as usize * h as usize * 4;
                        Ok(to_rgba(std::slice::from_raw_parts(bits as *const u8, n)))
                    }
                    Ok(()) => Err("no pixels for the grab".into()),
                    Err(e) => Err(e.to_string()),
                };
                SelectObject(mem, old);
                let _ = DeleteObject(dib.into());
                result
            }
            Err(e) => Err(e.to_string()),
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}

/// GDI's BGRx (the fourth byte undefined) as opaque RGBA, split between up
/// to 4 threads from a megapixel.
fn to_rgba(bgrx: &[u8]) -> Vec<u8> {
    let mut rgba = vec![0u8; bgrx.len()];
    let px = bgrx.len() / 4;
    let threads = match px >= 1 << 20 {
        true => std::thread::available_parallelism().map_or(1, |n| n.get().min(4)),
        false => 1,
    };
    let swap = |from: &[u8], to: &mut [u8]| {
        for (p, q) in (from.as_chunks::<4>().0.iter()).zip(to.as_chunks_mut::<4>().0) {
            *q = [p[2], p[1], p[0], 255];
        }
    };
    if threads == 1 {
        swap(bgrx, &mut rgba);
        return rgba;
    }
    let part = px.div_ceil(threads) * 4;
    std::thread::scope(|s| {
        for (from, to) in bgrx.chunks(part).zip(rgba.chunks_mut(part)) {
            s.spawn(move || swap(from, to));
        }
    });
    rgba
}

pub fn pointer() -> Option<(i32, i32)> {
    let mut p = windows::Win32::Foundation::POINT::default();
    // SAFETY: GetCursorPos only writes into the POINT we own.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

/// A kept-open Desktop Duplication of the monitor holding an area (long
/// capture, owner 2026-11-03): every frame the screen shows (60-144 a second)
/// instead of ~40 GDI copies at ~25 ms; `next` waits for one, so a still
/// screen costs a wake-up per wait. Like GDI's grab it leaves out windows that
/// keep out of captures and the pointer. Closed when dropped.
pub struct Duplication {
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    dup: windows::Win32::Graphics::Dxgi::IDXGIOutputDuplication,
    staging: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    area: windows::Win32::Graphics::Direct3D11::D3D11_BOX,
    size: (u32, u32),
    /// Frames read and the time copying them took (the log line).
    pub frames: u32,
    pub copy: std::time::Duration,
}

impl Duplication {
    /// For `region` (physical desktop px), wholly on one unrotated monitor.
    pub fn start((x, y, w, h): (i32, i32, u32, u32)) -> Result<Duplication, String> {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice,
        };
        use windows::Win32::Graphics::Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_IDENTITY,
            DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
        };
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1};
        use windows::core::Interface;
        let err = |e: windows::core::Error| e.message();
        if w == 0 || h == 0 {
            return Err("nothing to grab".into());
        }
        let (right, bottom) = (x + w as i32, y + h as i32);
        // SAFETY: COM calls on interfaces made here; every out pointer is a
        // local of the type the call writes.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(err)?;
            for a in 0.. {
                let Ok(adapter) = factory.EnumAdapters1(a) else {
                    break;
                };
                for o in 0.. {
                    let Ok(output) = adapter.EnumOutputs(o) else {
                        break;
                    };
                    let desc = output.GetDesc().map_err(err)?;
                    let r = desc.DesktopCoordinates;
                    if x < r.left || y < r.top || right > r.right || bottom > r.bottom {
                        continue;
                    }
                    if ![DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED]
                        .contains(&desc.Rotation)
                    {
                        return Err("a rotated monitor".into());
                    }
                    let (mut device, mut context) = (None, None);
                    D3D11CreateDevice(
                        &adapter,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )
                    .map_err(err)?;
                    let (Some(device), Some(context)) = (device, context) else {
                        return Err("no Direct3D device".into());
                    };
                    let dup = output
                        .cast::<IDXGIOutput1>()
                        .map_err(err)?
                        .DuplicateOutput(&device)
                        .map_err(err)?;
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
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..Default::default()
                    };
                    let mut staging = None;
                    device
                        .CreateTexture2D(&desc, None, Some(&mut staging))
                        .map_err(err)?;
                    let at = |v: i32, from: i32| (v - from) as u32;
                    return Ok(Duplication {
                        context,
                        dup,
                        staging: staging.ok_or("no staging texture")?,
                        area: D3D11_BOX {
                            left: at(x, r.left),
                            top: at(y, r.top),
                            front: 0,
                            right: at(right, r.left),
                            bottom: at(bottom, r.top),
                            back: 1,
                        },
                        size: (w, h),
                        frames: 0,
                        copy: Default::default(),
                    });
                }
            }
        }
        Err("the area is not on one monitor".into())
    }

    /// The area as of the next frame the screen shows, waiting at most `wait`
    /// ms for one; none when nothing new was shown (or only the pointer moved).
    /// An error ends it (a mode change, the secure desktop): grab another way.
    pub fn next(&mut self, wait: u32) -> Result<Option<Vec<u8>>, String> {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, ID3D11Texture2D,
        };
        use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO};
        use windows::core::Interface;
        let err = |e: windows::core::Error| e.message();
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // SAFETY: COM calls on interfaces we hold; the frame is released
        // before the staging copy is read; the mapped rows are read within
        // `RowPitch` x height, then unmapped.
        unsafe {
            match self.dup.AcquireNextFrame(wait, &mut info, &mut resource) {
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
                Err(e) => return Err(err(e)),
                Ok(()) => {}
            }
            let started = std::time::Instant::now();
            let copied = match (info.LastPresentTime, resource) {
                (0, _) | (_, None) => Ok(false),
                (_, Some(r)) => r.cast::<ID3D11Texture2D>().map(|t| {
                    let (to, area) = (&self.staging, Some(&raw const self.area));
                    self.context
                        .CopySubresourceRegion(to, 0, 0, 0, 0, &t, 0, area);
                    true
                }),
            };
            let _ = self.dup.ReleaseFrame();
            if !copied.map_err(err)? {
                return Ok(None);
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            (self.context)
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(err)?;
            let (w, h) = (self.size.0 as usize, self.size.1 as usize);
            let mut rgba = Vec::with_capacity(w * h * 4);
            for row in 0..h {
                let from = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
                let bgra = std::slice::from_raw_parts(from, w * 4);
                rgba.extend(
                    bgra.as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| [p[2], p[1], p[0], 255]),
                );
            }
            self.context.Unmap(&self.staging, 0);
            self.frames += 1;
            self.copy += started.elapsed();
            Ok(Some(rgba))
        }
    }
}
