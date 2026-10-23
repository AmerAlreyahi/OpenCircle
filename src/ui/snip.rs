//! Snip mode (DESIGN.md "Snip mode"): pick a region, a window, the whole screen or
//! a scrolling area on the frozen frame; the editor then opens on it (no action bar).
//! Scrolling auto-scrolls and stitches on a thread (`scroll_capture`) while the
//! overlay shrinks to a progress pill beside the area, so it isn't in the frames.
use std::cell::Cell;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::widget::{column, container, row};
use iced::{Center, Element, Fill, Point, Rectangle, Size};

use super::widgets::{self, STOP};
use crate::actions::Crop;
use crate::capture::scroll::{self, Params};
use crate::capture::stitch::Stitcher;

/// Test hook: a tall PNG stands in for the scrolled page (WSLg/CI can't scroll).
const TEST_SCROLL: &str = "OPENCIRCLE_TEST_SCROLL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Region,
    Window,
    Full,
    Scrolling,
}

impl Kind {
    fn label(self) -> &'static str {
        ["Region", "Window", "Full", "Scrolling"][self as usize]
    }

    fn tip(self) -> &'static str {
        [
            "Drag or circle an area",
            "Click a window",
            "The whole screen",
            "Select the scrolling area; it scrolls and stitches by itself",
        ][self as usize]
    }

    fn hint(self) -> &'static str {
        [
            "Drag or circle an area · Esc to close",
            "Click a window · Esc to close",
            "",
            "Drag over the scrolling area · Esc to close",
        ][self as usize]
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Kind(Kind),
    /// Stop button or Esc while scrolling: keep what was gathered.
    Stop,
    /// The stitched height so far.
    Progress(u32),
    Done(Result<Crop, String>),
}

/// What a pick leads to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Next {
    /// Open the editor on this area (logical px).
    Edit(Rectangle),
    /// Auto-scroll and stitch this area.
    Scroll(Rectangle),
}

pub struct Snip {
    pub kind: Kind,
    /// Windows on this monitor (logical px, clipped to it), topmost first.
    windows: Vec<Rectangle>,
    /// Auto-scroll works here (not on Wayland: no synthetic input).
    scrolls: bool,
    /// The running scrolling capture.
    pub scroll: Option<Scroll>,
}

pub struct Scroll {
    stop: Arc<AtomicBool>,
    width: u32,
    label: String,
}

impl Scroll {
    /// `width` px wide; `stop` is shared with the capture thread.
    pub fn new(width: u32, stop: Arc<AtomicBool>) -> Self {
        Scroll {
            stop,
            width,
            label: "Scrolling…".into(),
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn progress(&mut self, height: u32) {
        self.label = format!("Scrolling · {} × {} px", self.width, height);
    }
}

/// The progress pill's window size (logical px).
pub const PILL: Size = Size::new(420.0, 68.0);

impl Snip {
    /// `windows`: logical px of this monitor, topmost first (empty = Window mode hidden).
    pub fn new(windows: Vec<Rectangle>, screen: Size, scrolls: bool) -> Self {
        let whole = Rectangle::with_size(screen);
        Snip {
            kind: Kind::Region,
            windows: windows
                .iter()
                .filter_map(|w| w.intersection(&whole))
                .collect(),
            scrolls,
            scroll: None,
        }
    }

    /// The modes offered, in switch order.
    pub fn kinds(&self) -> Vec<Kind> {
        [Kind::Region, Kind::Window, Kind::Full, Kind::Scrolling]
            .into_iter()
            .filter(|k| match k {
                Kind::Window => !self.windows.is_empty(),
                Kind::Scrolling => self.scrolls,
                _ => true,
            })
            .collect()
    }

    /// The topmost window under `p`.
    pub fn window_at(&self, p: Point) -> Option<Rectangle> {
        self.windows.iter().copied().find(|w| w.contains(p))
    }

    /// Switches mode; Full picks the whole screen at once.
    pub fn switch(&mut self, kind: Kind, screen: Size) -> Option<Next> {
        self.kind = kind;
        (kind == Kind::Full).then(|| Next::Edit(Rectangle::with_size(screen)))
    }

    /// What a finished stroke does: `selection` = the area it selected (`None` for a
    /// click), `at` = where it ended. A drag in Window mode still takes its area.
    pub fn pick(&self, selection: Option<Rectangle>, at: Point, screen: Size) -> Option<Next> {
        match self.kind {
            Kind::Region => selection.map(Next::Edit),
            Kind::Window => selection.or_else(|| self.window_at(at)).map(Next::Edit),
            Kind::Full => Some(Next::Edit(Rectangle::with_size(screen))),
            Kind::Scrolling => selection.map(Next::Scroll),
        }
    }

    /// The mode switch and its hint, at the top of the screen.
    pub fn view(&self) -> Element<'_, Msg> {
        let options: Vec<_> = (self.kinds().into_iter())
            .map(|k| (k, k.label(), k.tip()))
            .collect();
        let switch = widgets::surface(widgets::segmented(&options, self.kind, Msg::Kind));
        let hint = self.kind.hint();
        let mut top = column![switch].spacing(12).align_x(Center);
        if !hint.is_empty() {
            top = top.push(widgets::pill(hint));
        }
        container(top).center_x(Fill).padding(24).into()
    }

    /// While scrolling: the whole (pill-sized) window.
    pub fn progress_view(&self) -> Option<Element<'_, Msg>> {
        let s = self.scroll.as_ref()?;
        let bar = row![
            widgets::body(&s.label),
            widgets::text_button(STOP, "Stop", "Esc".into(), true, Msg::Stop),
        ]
        .spacing(16)
        .padding([0, 12])
        .align_y(Center);
        Some(container(widgets::surface(bar)).center(Fill).into())
    }
}

/// Auto-scroll can run: synthetic wheel input works (not on native Wayland).
pub fn can_scroll() -> bool {
    #[cfg(target_os = "linux")]
    if crate::capture::wayland_session() {
        return false;
    }
    true
}

/// Auto-scrolls `region` (physical desktop px: x, y, width, height) and stitches
/// it until the end of the page, the height cap or `stop`. Blocking: run it on a
/// thread. `progress` gets the stitched height after each new frame.
pub fn scroll_capture(
    region: (i32, i32, u32, u32),
    stop: &AtomicBool,
    progress: impl FnMut(u32),
) -> Result<Crop, String> {
    let (x, y, w, h) = region;
    if let Some(path) = std::env::var_os(TEST_SCROLL) {
        let page = crate::capture::from_png(&path)?;
        let fake = FakePage::new(page.rgba, page.width, page.height);
        let params = Params {
            poll: std::time::Duration::from_millis(30),
            ..Params::default()
        };
        return stitch(
            w,
            h,
            || fake.frame(w, h),
            |n| fake.scroll(n, h),
            stop,
            &params,
            progress,
        );
    }
    let centre = (x + w as i32 / 2, y + h as i32 / 2);
    let grab = || {
        let frame = crate::capture::monitor_under_cursor().map_err(io::Error::other)?;
        let crop = cut(&frame.rgba, frame.width, (x - frame.x, y - frame.y), (w, h));
        crop.ok_or_else(|| io::Error::other("the area left the monitor"))
    };
    let scroll = |n| scroll::scroll_once(centre, n);
    stitch(w, h, grab, scroll, stop, &Params::default(), progress)
}

/// The capture loop over any source; a failed or stopped run keeps what it gathered.
fn stitch(
    w: u32,
    h: u32,
    grab: impl FnMut() -> io::Result<Vec<u8>>,
    scroll: impl FnMut(i32) -> io::Result<()>,
    stop: &AtomicBool,
    params: &Params,
    progress: impl FnMut(u32),
) -> Result<Crop, String> {
    let mut st = Stitcher::new(w, h);
    match scroll::run(&mut st, grab, scroll, stop, params, progress) {
        Ok(reason) => eprintln!("[opencircle] scrolling capture: {reason:?}"),
        Err(e) if st.height() == 0 => return Err(e.to_string()),
        Err(e) => eprintln!("[opencircle] scrolling capture stopped early: {e}"),
    }
    let (rgba, width, height) = st.finish();
    Ok(Crop {
        width,
        height,
        rgba,
    })
}

/// `size` pixels at `at` out of a `width`-wide RGBA frame; `None` if not inside it.
fn cut(rgba: &[u8], width: u32, at: (i32, i32), size: (u32, u32)) -> Option<Vec<u8>> {
    let (x, y) = (u32::try_from(at.0).ok()?, u32::try_from(at.1).ok()?);
    let height = rgba.len() as u32 / 4 / width.max(1);
    if x + size.0 > width || y + size.1 > height {
        return None;
    }
    let row = |r: u32| {
        let start = (((y + r) * width + x) * 4) as usize;
        &rgba[start..start + size.0 as usize * 4]
    };
    Some((0..size.1).flat_map(row).copied().collect())
}

/// A tall page seen through a viewport that scrolls 100 px per notch.
struct FakePage {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    top: Cell<u32>,
}

impl FakePage {
    fn new(rgba: Vec<u8>, width: u32, height: u32) -> Self {
        FakePage {
            rgba,
            width,
            height,
            top: Cell::new(0),
        }
    }

    fn frame(&self, w: u32, h: u32) -> io::Result<Vec<u8>> {
        cut(&self.rgba, self.width, (0, self.top.get() as i32), (w, h))
            .ok_or_else(|| io::Error::other("the test page is smaller than the area"))
    }

    fn scroll(&self, notches: i32, h: u32) -> io::Result<()> {
        let to = self.top.get() as i64 + i64::from(notches) * 100;
        self.top
            .set(to.clamp(0, i64::from(self.height.saturating_sub(h))) as u32);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size::new(1920.0, 1080.0);

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn each_mode_picks_its_area() {
        let front = rect(100.0, 100.0, 400.0, 300.0);
        let back = rect(0.0, 0.0, 1200.0, 900.0);
        let off = rect(2000.0, 0.0, 500.0, 500.0); // another monitor
        let mut s = Snip::new(vec![front, back, off], SCREEN, true);
        let area = rect(10.0, 10.0, 50.0, 50.0);
        let drag = Some(area);
        let click = Point::new(150.0, 150.0);

        assert_eq!(s.kind, Kind::Region);
        assert_eq!(s.pick(drag, click, SCREEN), Some(Next::Edit(area)));
        assert_eq!(s.pick(None, click, SCREEN), None, "a click selects nothing");

        assert_eq!(s.switch(Kind::Window, SCREEN), None);
        // The topmost window under the click; a click on the desktop picks nothing.
        assert_eq!(s.pick(None, click, SCREEN), Some(Next::Edit(front)));
        assert_eq!(
            s.pick(None, Point::new(1000.0, 800.0), SCREEN),
            Some(Next::Edit(back))
        );
        assert_eq!(s.pick(None, Point::new(1500.0, 1000.0), SCREEN), None);
        assert_eq!(s.pick(drag, click, SCREEN), Some(Next::Edit(area)));

        let whole = Rectangle::with_size(SCREEN);
        assert_eq!(s.switch(Kind::Full, SCREEN), Some(Next::Edit(whole)));

        assert_eq!(s.switch(Kind::Scrolling, SCREEN), None);
        assert_eq!(s.pick(drag, click, SCREEN), Some(Next::Scroll(area)));
        assert_eq!(s.pick(None, click, SCREEN), None);
    }

    #[test]
    fn modes_without_support_are_hidden() {
        let all = Snip::new(vec![rect(0.0, 0.0, 10.0, 10.0)], SCREEN, true);
        assert_eq!(
            all.kinds(),
            [Kind::Region, Kind::Window, Kind::Full, Kind::Scrolling]
        );
        // Wayland: no window list, no synthetic scrolling.
        let wayland = Snip::new(Vec::new(), SCREEN, false);
        assert_eq!(wayland.kinds(), [Kind::Region, Kind::Full]);
        // Windows only on other monitors count as none.
        let elsewhere = Snip::new(vec![rect(3000.0, 0.0, 10.0, 10.0)], SCREEN, true);
        assert!(!elsewhere.kinds().contains(&Kind::Window));
    }

    /// A page with a distinct block pattern on every row (the stitcher matches rows).
    fn page(w: u32, h: u32) -> Vec<u8> {
        let mut seed = 0x9e37_79b9_u32;
        let blocks: Vec<u8> = (0..(w / 8 + 1) * (h / 4 + 1))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let v = blocks[((y / 4) * (w / 8 + 1) + x / 8) as usize];
                [v, v.wrapping_mul(3), 255 - v, 255]
            })
            .collect()
    }

    fn fast() -> Params {
        Params {
            poll: std::time::Duration::ZERO,
            settle_tries: 2,
            ..Params::default()
        }
    }

    #[test]
    fn scrolling_runs_to_the_end_of_the_page_with_progress() {
        let (w, h, vh) = (320, 3000, 600);
        let fake = FakePage::new(page(w, h), w, h);
        let mut seen = Vec::new();
        let stop = AtomicBool::new(false);
        let out = stitch(
            w,
            vh,
            || fake.frame(w, vh),
            |n| fake.scroll(n, vh),
            &stop,
            &fast(),
            |height| seen.push(height),
        )
        .unwrap();
        assert_eq!((out.width, out.height), (w, h));
        assert!(out.rgba == fake.rgba, "stitched page differs");
        assert!(seen.windows(2).all(|p| p[0] < p[1]), "{seen:?}");
        assert_eq!(seen.last(), Some(&h));
    }

    #[test]
    fn stop_keeps_what_was_gathered() {
        let (w, h, vh) = (320, 6000, 600);
        let fake = FakePage::new(page(w, h), w, h);
        let stop = AtomicBool::new(false);
        let mut seen = Vec::new();
        let out = stitch(
            w,
            vh,
            || fake.frame(w, vh),
            |n| fake.scroll(n, vh),
            &stop,
            &fast(),
            |height| {
                seen.push(height);
                if seen.len() == 3 {
                    stop.store(true, Ordering::Relaxed); // the Stop button
                }
            },
        )
        .unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(out.height, seen[2]);
        assert!(vh < out.height && out.height < h);
        // The part gathered is the top of the page, exactly.
        let n = (w * out.height * 4) as usize;
        assert!(out.rgba[..] == fake.rgba[..n]);
    }

    #[test]
    fn a_source_that_fails_at_once_is_an_error() {
        let err = stitch(
            4,
            4,
            || Err(io::Error::other("no screen")),
            |_| Ok(()),
            &AtomicBool::new(false),
            &fast(),
            |_| {},
        );
        assert_eq!(err.unwrap_err(), "no screen");
    }

    #[test]
    fn cut_takes_an_area_inside_the_frame_only() {
        // 3x2 frame, pixel (x, y) = [x, y, 0, 255].
        let frame: Vec<u8> = (0..2u8)
            .flat_map(|y| (0..3u8).flat_map(move |x| [x, y, 0, 255]))
            .collect();
        assert_eq!(
            cut(&frame, 3, (1, 1), (2, 1)),
            Some(vec![1, 1, 0, 255, 2, 1, 0, 255])
        );
        assert_eq!(cut(&frame, 3, (2, 0), (2, 1)), None);
        assert_eq!(cut(&frame, 3, (-1, 0), (1, 1)), None);
    }
}
