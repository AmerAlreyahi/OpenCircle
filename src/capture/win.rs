//! Windows capture via xcap (GDI/DXGI). Positions and sizes are physical desktop
//! px (the process is per-monitor DPI aware, winit's default).
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
    let (x, y, _, _) = m.rect;
    let monitor = Monitor::from_point(x, y).map_err(|e| e.to_string())?;
    let image = monitor.capture_image().map_err(|e| e.to_string())?;
    Ok(Frame {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        x,
        y,
        scale: m.scale,
    })
}

pub fn pointer() -> Option<(i32, i32)> {
    let mut p = windows::Win32::Foundation::POINT::default();
    // SAFETY: GetCursorPos only writes into the POINT we own.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}
