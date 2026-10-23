//! Windows capture via xcap (GDI/DXGI), monitor picked by the cursor.
use xcap::Monitor;

use super::Frame;

pub fn capture() -> Result<Frame, String> {
    let monitor = cursor_position()
        .and_then(|(x, y)| Monitor::from_point(x, y).ok())
        .map_or_else(primary_monitor, Ok)?;
    let image = monitor.capture_image().map_err(|e| e.to_string())?;
    Ok(Frame {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        x: monitor.x().map_err(|e| e.to_string())?,
        y: monitor.y().map_err(|e| e.to_string())?,
        scale: monitor.scale_factor().unwrap_or(1.0),
    })
}

fn primary_monitor() -> Result<Monitor, String> {
    let monitors = Monitor::all().map_err(|e| e.to_string())?;
    let primary = monitors
        .iter()
        .position(|m| m.is_primary().unwrap_or(false))
        .unwrap_or(0);
    monitors
        .into_iter()
        .nth(primary)
        .ok_or_else(|| "no monitor found".into())
}

fn cursor_position() -> Option<(i32, i32)> {
    let mut p = windows::Win32::Foundation::POINT::default();
    // SAFETY: GetCursorPos only writes into the POINT we own.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}
