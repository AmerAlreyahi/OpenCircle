//! Snip mode (DESIGN.md "Snip mode"): pick a region, a window, the whole screen or
//! a scrolling area on the frozen frame. Then (`snip.after`) the capture is copied,
//! with a toast that offers Edit for a few seconds, and/or the editor opens on it.
//! While picking, edges snap to windows and (Windows) to the controls of the app
//! the user was in (Shift = no snapping), and a loupe shows the pixels under the
//! pointer. The timer hides the overlay, counts down (menus can be opened
//! meanwhile), then captures again. Recent (`snip.history`, off by default): the
//! last captures, kept only in the user's data dir.
//! Scrolling auto-scrolls and stitches on a thread (`scroll_capture`) while the
//! overlay shrinks to a progress pill beside the area, so it isn't in the frames.
use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::mouse::Cursor;
use iced::widget::canvas::{Geometry, Stroke};
use iced::widget::{canvas, column, container, image, row};
use iced::{Center, Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme};

use super::theme;
use super::widgets::{self, CLOSE, STOP};
use crate::actions::Crop;
use crate::capture::scroll::{self, Params};
use crate::capture::stitch::Stitcher;
use crate::capture::window_rect::WinRect;
use crate::config::{SnipAfter, Target};

/// Test hook: a tall PNG stands in for the scrolled page (WSLg/CI can't scroll).
const TEST_SCROLL: &str = "OPENCIRCLE_TEST_SCROLL";

/// An edge snaps to a window / control edge this near (logical px).
const SNAP: f32 = 8.0;
/// The toast offers Edit this long, then the overlay closes.
pub const TOAST_SECS: u64 = 4;
pub const COPIED: &str = "Copied: paste it into your AI chat";
/// The timer's choices, seconds (0 = off).
const DELAYS: [(u8, &str, &str); 4] = [
    (0, "Off", "No timer"),
    (3, "3 s", "Hide, wait 3 seconds, then capture"),
    (5, "5 s", "Hide, wait 5 seconds, then capture"),
    (10, "10 s", "Hide, wait 10 seconds, then capture"),
];
/// Recent: captures kept on disk, and shown in the strip.
const HISTORY: usize = 20;
const STRIP: usize = 8;
/// The loupe: physical px each side of the one under the pointer, and the
/// logical size each is drawn at (8x zoom).
const LOUPE_RADIUS: i32 = 7;
const LOUPE_CELL: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Region,
    Window,
    Full,
    Scrolling,
}

impl Kind {
    /// The mode a snip quick action starts in: circle (or last region) = Region,
    /// window = Window, full screen = Full.
    pub fn for_target(target: Target) -> Kind {
        match target {
            Target::Circle | Target::LastRegion => Kind::Region,
            Target::Window => Kind::Window,
            Target::FullScreen => Kind::Full,
        }
    }

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
            "Drag or circle an area · Shift = no snapping · Esc to close",
            "Click a window · Esc to close",
            "",
            "Drag over the scrolling area · Esc to close",
        ][self as usize]
    }

    /// Picking an area by hand: the loupe and snapping help.
    pub fn picks(self) -> bool {
        self != Kind::Full
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
    /// The timer: hide, wait this many seconds, capture again (0 = off).
    Delay(u8),
    /// Countdown `.0` has `.1` seconds left (0 = time to hide).
    Tick(u32, u8),
    /// Countdown `.0` is over and the overlay hidden: capture now.
    Shoot(u32),
    /// The countdown's Cancel.
    Cancel,
    /// The toast's Edit: the editor on the capture.
    Edit,
    /// Toast `.0` was shown long enough.
    ToastDone(u32),
    /// The controls of the app the user was in (logical px), for opening `.0`.
    Controls(u32, Vec<Rectangle>),
    /// Shift is held: no snapping.
    Shift(bool),
    /// The Recent strip's pictures, newest first.
    Recent(Vec<(PathBuf, image::Handle)>),
    OpenRecent(PathBuf),
    /// A recent capture, read back.
    Loaded(Result<Crop, String>),
    ClearRecent,
}

/// What a pick leads to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Next {
    /// The capture is this area (logical px): copy and / or edit it.
    Edit(Rectangle),
    /// Auto-scroll and stitch this area.
    Scroll(Rectangle),
}

/// What a finished capture does (config `snip.after`): (copy it, open the editor).
pub fn route(after: SnipAfter) -> (bool, bool) {
    match after {
        SnipAfter::Copy => (true, false),
        SnipAfter::Editor => (false, true),
        SnipAfter::Both => (true, true),
    }
}

/// The "Copied" toast: the capture, for its Edit button.
pub struct Toast {
    id: u32,
    pub crop: Crop,
    /// Still on the frozen frame at the selection (else: a scrolling or recent
    /// capture, edited fitted to the screen).
    pub in_place: bool,
}

pub struct Snip {
    pub kind: Kind,
    /// Windows on this monitor (logical px, clipped to it), topmost first.
    windows: Vec<Rectangle>,
    /// The controls of the app the user was in (logical px), arriving later.
    controls: Vec<Rectangle>,
    /// Auto-scroll works here (not on Wayland: no synthetic input).
    scrolls: bool,
    /// The running scrolling capture.
    pub scroll: Option<Scroll>,
    /// The window the user was in (Windows): focus goes back to it while the
    /// overlay is only a pill.
    pub foreground: Option<isize>,
    shift: bool,
    /// The pointer (logical px), for the loupe.
    pub pointer: Option<Point>,
    /// The timer pill: countdown number, seconds left.
    countdown: Option<(u32, u8)>,
    countdowns: u32,
    toast: Option<Toast>,
    toasts: u32,
    recent: Vec<(PathBuf, image::Handle)>,
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

/// The pill window's size (logical px): scrolling progress, countdown, toast.
pub const PILL: Size = Size::new(500.0, 68.0);

impl Snip {
    /// `windows`: logical px of this monitor, topmost first (empty = Window mode hidden).
    pub fn new(
        windows: Vec<Rectangle>,
        screen: Size,
        scrolls: bool,
        foreground: Option<isize>,
    ) -> Self {
        let whole = Rectangle::with_size(screen);
        Snip {
            kind: Kind::Region,
            windows: windows
                .iter()
                .filter_map(|w| w.intersection(&whole))
                .collect(),
            controls: Vec::new(),
            scrolls,
            scroll: None,
            foreground,
            shift: false,
            pointer: None,
            countdown: None,
            countdowns: 0,
            toast: None,
            toasts: 0,
            recent: Vec::new(),
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

    /// `p` (logical px) on the nearest window or control edge, per axis, while
    /// an area is dragged (Region, Scrolling) and Shift isn't held. Not in
    /// Window mode: a click near a window's edge would land on the edge.
    pub fn snap(&self, p: Point) -> Point {
        if self.shift || !matches!(self.kind, Kind::Region | Kind::Scrolling) {
            return p;
        }
        snap(p, self.windows.iter().chain(&self.controls), SNAP)
    }

    pub fn set_shift(&mut self, held: bool) {
        self.shift = held;
    }

    /// The controls arrived (clipped to this screen, like the windows).
    pub fn set_controls(&mut self, controls: Vec<Rectangle>, screen: Size) {
        let whole = Rectangle::with_size(screen);
        self.controls = (controls.iter())
            .filter_map(|c| c.intersection(&whole))
            .collect();
    }

    /// Starts in `kind` if this screen offers it (else Region, no Window list on
    /// Wayland). Full picks the whole screen at once.
    pub fn start(&mut self, kind: Kind, screen: Size) -> Option<Next> {
        let kind = if self.kinds().contains(&kind) {
            kind
        } else {
            Kind::Region
        };
        self.switch(kind, screen)
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

    /// The timer was set to `secs`: a new countdown (its number), `None` for off.
    pub fn delay(&mut self, secs: u8) -> Option<u32> {
        if secs == 0 {
            return None;
        }
        self.countdowns = self.countdowns.wrapping_add(1);
        self.countdown = Some((self.countdowns, secs));
        Some(self.countdowns)
    }

    /// Countdown `id` has `left` seconds to go: `Some(true)` = time to hide and
    /// capture; `None` = a cancelled or older countdown.
    pub fn tick(&mut self, id: u32, left: u8) -> Option<bool> {
        let (_, shown) = self.countdown.as_mut().filter(|(n, _)| *n == id)?;
        *shown = left;
        Some(left == 0)
    }

    /// The capture after countdown `id` may go ahead (once).
    pub fn shoot(&mut self, id: u32) -> bool {
        let due = self.countdown == Some((id, 0));
        if due {
            self.countdown = None;
        }
        due
    }

    /// The window is only a pill now (scrolling, the timer, the toast).
    pub fn is_pill(&self) -> bool {
        self.scroll.is_some() || self.countdown.is_some() || self.toast.is_some()
    }

    /// Shows the "Copied" toast for `crop`; its number.
    pub fn toast(&mut self, crop: Crop, in_place: bool) -> u32 {
        self.toasts = self.toasts.wrapping_add(1);
        self.toast = Some(Toast {
            id: self.toasts,
            crop,
            in_place,
        });
        self.toasts
    }

    pub fn has_toast(&self) -> bool {
        self.toast.is_some()
    }

    /// The toast's Edit: it goes, the capture comes out.
    pub fn take_toast(&mut self) -> Option<Toast> {
        self.toast.take()
    }

    /// Toast `id` is still the one showing.
    pub fn toast_showing(&self, id: u32) -> bool {
        self.toast.as_ref().is_some_and(|t| t.id == id)
    }

    pub fn set_recent(&mut self, recent: Vec<(PathBuf, image::Handle)>) {
        self.recent = recent;
    }

    /// The mode switch, the timer and Recent, at the top of the screen.
    pub fn view(&self) -> Element<'_, Msg> {
        let options: Vec<_> = (self.kinds().into_iter())
            .map(|k| (k, k.label(), k.tip()))
            .collect();
        let switch = row![
            widgets::segmented(&options, self.kind, Msg::Kind),
            widgets::segmented(&DELAYS, 0, Msg::Delay),
        ]
        .spacing(8);
        let mut top = column![widgets::surface(switch)]
            .spacing(12)
            .align_x(Center);
        if !self.recent.is_empty() {
            let shots = (self.recent.iter()).map(|(path, picture)| {
                let open = widgets::thumbnail(picture.clone(), Msg::OpenRecent(path.clone()));
                widgets::tip(
                    open,
                    "A recent capture",
                    iced::widget::tooltip::Position::Bottom,
                )
            });
            let clear = widgets::icon_button(CLOSE, false, Msg::ClearRecent);
            let strip = row![
                widgets::caption("Recent"),
                row(shots).spacing(4),
                widgets::tip(
                    clear,
                    "Clear recent captures",
                    iced::widget::tooltip::Position::Bottom
                ),
            ]
            .spacing(8)
            .padding([0, 8])
            .align_y(Center);
            top = top.push(widgets::surface(strip));
        }
        let hint = self.kind.hint();
        if !hint.is_empty() {
            top = top.push(widgets::pill(hint));
        }
        container(top).center_x(Fill).padding(24).into()
    }

    /// When the window is only a pill (scrolling, the timer, the toast): its view.
    pub fn pill_view(&self) -> Option<Element<'_, Msg>> {
        let bar = if let Some(s) = &self.scroll {
            row![
                widgets::body(&s.label),
                widgets::text_button(STOP, "Stop", "Esc".into(), true, Msg::Stop),
            ]
        } else if let Some((_, left)) = self.countdown {
            let label = match left {
                0 => "Capturing…".to_string(),
                n => format!("Capturing in {n} s · open a menu now"),
            };
            row![
                widgets::body(label),
                widgets::text_button(CLOSE, "Cancel", String::new(), false, Msg::Cancel),
            ]
        } else if self.toast.is_some() {
            row![
                widgets::body(COPIED),
                widgets::text_button(EDIT, "Edit", String::new(), true, Msg::Edit),
            ]
        } else {
            return None;
        };
        let bar = bar.spacing(16).padding([0, 12]).align_y(Center);
        Some(container(widgets::surface(bar)).center(Fill).into())
    }

    /// The loupe beside the pointer while an area is picked by hand: `frame` is
    /// the frozen frame, `scale` physical px per logical px.
    pub fn loupe<'a>(&self, frame: &'a image::Handle, scale: f32) -> Option<Element<'a, Msg>> {
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = frame
        else {
            return None;
        };
        let at = self.pointer.filter(|_| self.kind.picks())?;
        let loupe = Loupe {
            pixels,
            width: *width,
            height: *height,
            scale,
            at,
        };
        Some(canvas(loupe).width(Fill).height(Fill).into())
    }
}

/// The editor's glyph (lucide "pencil", as the action bar's Edit).
const EDIT: char = '\u{e1f8}';

/// `p` moved onto the nearest edge of the rectangles it is in or near, per axis,
/// if one is within `reach`.
fn snap<'a>(p: Point, rects: impl IntoIterator<Item = &'a Rectangle>, reach: f32) -> Point {
    let (mut x, mut y) = ((reach, p.x), (reach, p.y));
    for r in rects {
        let near = p.x >= r.x - reach
            && p.x <= r.x + r.width + reach
            && p.y >= r.y - reach
            && p.y <= r.y + r.height + reach;
        if !near {
            continue;
        }
        for edge in [r.x, r.x + r.width] {
            let d = (p.x - edge).abs();
            if d < x.0 {
                x = (d, edge);
            }
        }
        for edge in [r.y, r.y + r.height] {
            let d = (p.y - edge).abs();
            if d < y.0 {
                y = (d, edge);
            }
        }
    }
    Point::new(x.1, y.1)
}

/// A window or control rectangle (physical desktop px) in the logical px of the
/// monitor at `origin` (physical px) with `scale` physical px per logical px.
pub fn to_logical((x, y, w, h): WinRect, origin: (i32, i32), scale: f32) -> Rectangle {
    let (x, y) = ((x - origin.0) as f32, (y - origin.1) as f32);
    Rectangle::new(Point::new(x, y), Size::new(w as f32, h as f32)) * (1.0 / scale)
}

/// The `2 * radius + 1` square of pixels around `centre` (frame px), row by row;
/// outside the frame = transparent.
fn sample(rgba: &[u8], width: u32, height: u32, centre: (i32, i32), radius: i32) -> Vec<[u8; 4]> {
    let mut out = Vec::with_capacity(((2 * radius + 1) * (2 * radius + 1)) as usize);
    for y in centre.1 - radius..=centre.1 + radius {
        for x in centre.0 - radius..=centre.0 + radius {
            let inside = (0..width as i32).contains(&x) && (0..height as i32).contains(&y);
            let i = (y.max(0) as usize * width as usize + x.max(0) as usize) * 4;
            out.push(match rgba.get(i..i + 4) {
                Some(&[r, g, b, a]) if inside => [r, g, b, a],
                _ => [0; 4],
            });
        }
    }
    out
}

/// The magnifier: the pixels around the pointer at 8x, the one under it framed,
/// its coordinates (frame px) and colour.
struct Loupe<'a> {
    pixels: &'a [u8],
    width: u32,
    height: u32,
    scale: f32,
    /// The pointer, logical px.
    at: Point,
}

impl canvas::Program<Msg> for Loupe<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let centre = (
            (self.at.x * self.scale).floor() as i32,
            (self.at.y * self.scale).floor() as i32,
        );
        let cells = sample(self.pixels, self.width, self.height, centre, LOUPE_RADIUS);
        let n = 2 * LOUPE_RADIUS + 1;
        let side = n as f32 * LOUPE_CELL;
        let label = 24.0;
        // Below-right of the pointer; flipped where it would leave the screen.
        let gap = 24.0;
        let x = if self.at.x + gap + side < bounds.width {
            self.at.x + gap
        } else {
            self.at.x - gap - side
        };
        let y = if self.at.y + gap + side + label < bounds.height {
            self.at.y + gap
        } else {
            self.at.y - gap - side - label
        };
        let palette = theme::current();
        let body = canvas::Path::rectangle(
            Point::new(x - 2.0, y - 2.0),
            Size::new(side + 4.0, side + label + 4.0),
        );
        frame.fill(&body, palette.panel_solid);
        frame.stroke(
            &body,
            Stroke::default().with_color(palette.border).with_width(1.0),
        );
        for (i, &[r, g, b, a]) in cells.iter().enumerate() {
            let (col, row) = (i as i32 % n, i as i32 / n);
            let at = Point::new(x + col as f32 * LOUPE_CELL, y + row as f32 * LOUPE_CELL);
            let colour = Color::from_rgba8(r, g, b, f32::from(a) / 255.0);
            frame.fill_rectangle(at, Size::new(LOUPE_CELL, LOUPE_CELL), colour);
        }
        // The crosshair: a thin line through the middle row and column, the
        // pixel itself framed.
        let mid = LOUPE_RADIUS as f32 * LOUPE_CELL;
        let line = Stroke::default()
            .with_color(Color {
                a: 0.5,
                ..theme::ACCENT_START
            })
            .with_width(1.0);
        let half = LOUPE_CELL / 2.0;
        frame.stroke(
            &canvas::Path::line(
                Point::new(x, y + mid + half),
                Point::new(x + side, y + mid + half),
            ),
            line,
        );
        frame.stroke(
            &canvas::Path::line(
                Point::new(x + mid + half, y),
                Point::new(x + mid + half, y + side),
            ),
            line,
        );
        let pixel = canvas::Path::rectangle(
            Point::new(x + mid, y + mid),
            Size::new(LOUPE_CELL, LOUPE_CELL),
        );
        frame.stroke(
            &pixel,
            Stroke::default().with_color(Color::WHITE).with_width(2.0),
        );
        let [r, g, b, _] = cells[cells.len() / 2];
        frame.fill_text(canvas::Text {
            content: format!("{}, {}   #{r:02X}{g:02X}{b:02X}", centre.0, centre.1),
            position: Point::new(x + 6.0, y + side + 5.0),
            color: palette.text,
            size: 12.0.into(),
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}

// ---- Recent captures: only with `snip.history` on, only in the user's data dir ----

/// `<user data dir>/opencircle/recent` (Windows: %LOCALAPPDATA%).
fn history_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("opencircle").join("recent"))
}

/// Keeps `crop` in Recent (on a thread) if the user turned history on.
pub fn remember(crop: &Crop) {
    if !crate::config::current().snip.history {
        return;
    }
    let (Some(dir), crop) = (history_dir(), crop.clone()) else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("recent".into())
        .spawn(move || {
            if let Err(e) = keep(&dir, &crop, HISTORY) {
                eprintln!("[opencircle] recent capture not kept: {e}");
            }
        });
    if let Err(e) = spawned {
        eprintln!("[opencircle] recent capture not kept: {e}");
    }
}

/// Writes `crop` into `dir` as the newest PNG, then drops all but the newest `limit`.
fn keep(dir: &Path, crop: &Crop, limit: usize) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    // Fixed-width names sort by time; a second capture in the same ms takes the next one.
    let mut stamp = now;
    let path = loop {
        let path = dir.join(format!("{stamp:016}.png"));
        if !path.exists() {
            break path;
        }
        stamp += 1;
    };
    crate::actions::write_png(&path, crop.width, crop.height, &crop.rgba)?;
    for old in list(dir).into_iter().skip(limit) {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

/// The kept captures, newest first.
fn list(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = (entries.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    out.sort();
    out.reverse();
    out
}

/// Blocking: the Recent strip (newest `STRIP`, as thumbnails). With history off,
/// whatever an earlier "on" left is deleted instead: off = nothing on disk.
pub fn load_recent() -> Vec<(PathBuf, image::Handle)> {
    let Some(dir) = history_dir() else {
        return Vec::new();
    };
    if !crate::config::current().snip.history {
        clear_dir(&dir);
        return Vec::new();
    }
    (list(&dir).into_iter().take(STRIP))
        .filter_map(|path| {
            let frame = crate::capture::from_png(path.as_os_str()).ok()?;
            let crop = Crop {
                width: frame.width,
                height: frame.height,
                rgba: frame.rgba,
            };
            let small = crate::actions::fit_long_edge(crop, 96);
            let picture = image::Handle::from_rgba(small.width, small.height, small.rgba);
            Some((path, picture))
        })
        .collect()
}

/// Blocking: a recent capture, full size.
pub fn open_recent(path: &Path) -> Result<Crop, String> {
    let frame = crate::capture::from_png(path.as_os_str())?;
    Ok(Crop {
        width: frame.width,
        height: frame.height,
        rgba: frame.rgba,
    })
}

/// The Clear button: every kept capture goes.
pub fn clear_recent() {
    if let Some(dir) = history_dir() {
        clear_dir(&dir);
    }
}

fn clear_dir(dir: &Path) {
    if dir.exists()
        && let Err(e) = std::fs::remove_dir_all(dir)
    {
        eprintln!("[opencircle] recent captures not cleared: {e}");
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
        let mut s = Snip::new(vec![front, back, off], SCREEN, true, None);
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
    fn snip_quick_actions_start_in_the_mode_of_their_target() {
        let windows = vec![rect(0.0, 0.0, 400.0, 300.0)];
        for (target, kind) in [
            (Target::Circle, Kind::Region),
            (Target::LastRegion, Kind::Region),
            (Target::Window, Kind::Window),
            (Target::FullScreen, Kind::Full),
        ] {
            assert_eq!(Kind::for_target(target), kind, "{target:?}");
        }
        // Print Screen / Alt+Print Screen / Ctrl+Print Screen from the default config.
        let by_key = |key: &str| {
            let q = (crate::config::Config::default().quick_actions.into_iter())
                .find(|q| q.shortcut == key)
                .unwrap();
            assert_eq!(q.action, "snip");
            Kind::for_target(q.target)
        };
        assert_eq!(by_key("PrintScreen"), Kind::Region);
        assert_eq!(by_key("Alt+PrintScreen"), Kind::Window);
        assert_eq!(by_key("Ctrl+PrintScreen"), Kind::Full);

        let mut s = Snip::new(windows, SCREEN, true, None);
        assert_eq!(s.start(Kind::Window, SCREEN), None, "waits for a click");
        assert_eq!(s.kind, Kind::Window);
        // Full opens the editor on the whole screen at once.
        let whole = Rectangle::with_size(SCREEN);
        assert_eq!(s.start(Kind::Full, SCREEN), Some(Next::Edit(whole)));
        assert_eq!(s.kind, Kind::Full);
        // No window list (Wayland): the window shortcut falls back to Region.
        let mut wayland = Snip::new(Vec::new(), SCREEN, false, None);
        assert_eq!(wayland.start(Kind::Window, SCREEN), None);
        assert_eq!(wayland.kind, Kind::Region);
    }

    #[test]
    fn modes_without_support_are_hidden() {
        let all = Snip::new(vec![rect(0.0, 0.0, 10.0, 10.0)], SCREEN, true, None);
        assert_eq!(
            all.kinds(),
            [Kind::Region, Kind::Window, Kind::Full, Kind::Scrolling]
        );
        // Wayland: no window list, no synthetic scrolling.
        let wayland = Snip::new(Vec::new(), SCREEN, false, None);
        assert_eq!(wayland.kinds(), [Kind::Region, Kind::Full]);
        // Windows only on other monitors count as none.
        let elsewhere = Snip::new(vec![rect(3000.0, 0.0, 10.0, 10.0)], SCREEN, true, None);
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

    #[test]
    fn after_capture_routes_to_copy_editor_or_both() {
        assert_eq!(route(SnipAfter::Copy), (true, false));
        assert_eq!(route(SnipAfter::Editor), (false, true));
        assert_eq!(route(SnipAfter::Both), (true, true));
        // Out of the box a snip goes to the clipboard for the AI chat.
        assert_eq!(
            route(crate::config::Config::default().snip.after),
            (true, false)
        );
    }

    #[test]
    fn the_toast_offers_edit_until_it_is_taken_or_replaced() {
        let mut s = Snip::new(Vec::new(), SCREEN, true, None);
        let crop = Crop {
            width: 2,
            height: 1,
            rgba: vec![0; 8],
        };
        assert!(s.pill_view().is_none());
        let first = s.toast(crop.clone(), true);
        assert!(s.toast_showing(first) && s.pill_view().is_some());
        let texts = widgets::shot::texts(s.pill_view().unwrap());
        assert!(texts.iter().any(|t| t == COPIED), "{texts:?}");
        widgets::shot::save("snip-toast", s.pill_view().unwrap(), PILL);
        let second = s.toast(crop, false);
        assert!(!s.toast_showing(first), "an older toast's timeout is stale");
        let t = s.take_toast().unwrap();
        assert!(!t.in_place && (t.crop.width, t.crop.height) == (2, 1));
        assert!(!s.toast_showing(second) && !s.has_toast());
        assert!(widgets::shot::texts(s.view()).iter().all(|t| t != COPIED));
    }

    #[test]
    fn the_timer_counts_down_then_shoots_once() {
        let mut s = Snip::new(Vec::new(), SCREEN, true, None);
        assert_eq!(s.delay(0), None, "Off starts nothing");
        assert!(!s.is_pill());
        let id = s.delay(3).unwrap();
        assert!(s.is_pill());
        let texts = widgets::shot::texts(s.pill_view().unwrap());
        assert!(
            texts.iter().any(|t| t.contains("Capturing in 3 s")),
            "{texts:?}"
        );
        widgets::shot::save("snip-countdown", s.pill_view().unwrap(), PILL);
        assert_eq!(s.tick(id, 2), Some(false));
        assert_eq!(s.tick(id, 1), Some(false));
        assert!(!s.shoot(id), "not before zero");
        assert_eq!(s.tick(id, 0), Some(true), "hide now");
        assert!(s.shoot(id));
        assert!(!s.shoot(id), "only once");
        assert!(!s.is_pill());
        // A restarted timer makes the old one's ticks stale.
        let old = s.delay(5).unwrap();
        let new = s.delay(10).unwrap();
        assert_eq!(s.tick(old, 0), None);
        assert!(!s.shoot(old));
        assert_eq!(s.tick(new, 9), Some(false));
    }

    #[test]
    fn edges_snap_to_nearby_window_and_control_edges() {
        let window = rect(100.0, 100.0, 400.0, 300.0);
        let button = rect(200.0, 150.0, 80.0, 30.0);
        let rects = [window, button];
        // Near the button's top-left corner: both axes snap to it.
        assert_eq!(
            snap(Point::new(204.0, 147.0), &rects, SNAP),
            Point::new(200.0, 150.0)
        );
        // Inside the button near its right edge only: x snaps, y stays.
        assert_eq!(
            snap(Point::new(277.0, 165.0), &rects, SNAP),
            Point::new(280.0, 165.0)
        );
        // Just outside the window's left edge (within reach) snaps onto it.
        assert_eq!(
            snap(Point::new(95.0, 300.0), &rects, SNAP),
            Point::new(100.0, 300.0)
        );
        // Far from every edge: unchanged.
        let free = Point::new(350.0, 300.0);
        assert_eq!(snap(free, &rects, SNAP), free);
        // An edge of a rectangle the point is far from never pulls it.
        let far = Point::new(600.0, 155.0); // level with the button's top, outside
        assert_eq!(snap(far, &rects, SNAP), far);

        // Shift turns it off; only dragged areas snap (Window mode clicks don't).
        let mut s = Snip::new(vec![window], SCREEN, true, None);
        s.set_controls(vec![button], SCREEN);
        let p = Point::new(204.0, 147.0);
        assert_eq!(s.snap(p), Point::new(200.0, 150.0));
        s.set_shift(true);
        assert_eq!(s.snap(p), p);
        s.set_shift(false);
        let _ = s.switch(Kind::Scrolling, SCREEN);
        assert_eq!(s.snap(p), Point::new(200.0, 150.0));
        for kind in [Kind::Window, Kind::Full] {
            let _ = s.switch(kind, SCREEN);
            assert_eq!(s.snap(p), p, "{kind:?}");
        }
    }

    #[test]
    fn window_and_control_rects_become_logical_px_at_any_scale() {
        // A button at physical (2340, 300) 120x45 on a 150 % monitor whose origin
        // is at physical (1920, 0): logical (280, 200) 80x30.
        let r = to_logical((2340, 300, 120, 45), (1920, 0), 1.5);
        assert_eq!(r, rect(280.0, 200.0, 80.0, 30.0));
        // So a drag drawn in logical px snaps to it, and the crop (physical px)
        // lands on its real pixels.
        let p = snap(Point::new(283.0, 197.0), &[r], SNAP);
        assert_eq!(p, Point::new(280.0, 200.0));
        let crop = crate::actions::crop_bounds(3840, 2160, 1.5, Rectangle::new(p, r.size()));
        assert_eq!(crop, Some([420, 300, 540, 345]));
        // 100 %: unchanged.
        assert_eq!(
            to_logical((10, 20, 30, 40), (0, 0), 1.0),
            rect(10.0, 20.0, 30.0, 40.0)
        );
    }

    #[test]
    fn the_loupe_samples_around_the_pointer_and_pads_the_edges() {
        // 4x3 frame, pixel (x, y) = [x, y, 7, 255].
        let frame: Vec<u8> = (0..3u8)
            .flat_map(|y| (0..4u8).flat_map(move |x| [x, y, 7, 255]))
            .collect();
        let cells = sample(&frame, 4, 3, (1, 1), 1);
        assert_eq!(cells.len(), 9);
        assert_eq!(
            cells[4],
            [1, 1, 7, 255],
            "the centre is the pixel under the pointer"
        );
        assert_eq!(cells[0], [0, 0, 7, 255]);
        assert_eq!(cells[8], [2, 2, 7, 255]);
        // At the corner: outside the frame is transparent.
        let cells = sample(&frame, 4, 3, (0, 0), 1);
        assert_eq!(cells[0], [0; 4]);
        assert_eq!(cells[4], [0, 0, 7, 255]);
        assert_eq!(cells[8], [1, 1, 7, 255]);
        // Full size: 15x15 cells of 8 px = a 120 px loupe.
        let cells = sample(&frame, 4, 3, (3, 2), LOUPE_RADIUS);
        assert_eq!(cells.len(), 15 * 15);
        assert_eq!(cells[cells.len() / 2], [3, 2, 7, 255]);
        // It shows while picking an area by hand, not in Full.
        let handle = image::Handle::from_rgba(4, 3, frame);
        let mut s = Snip::new(Vec::new(), SCREEN, true, None);
        assert!(s.loupe(&handle, 1.0).is_none(), "no pointer yet");
        s.pointer = Some(Point::new(2.0, 1.0));
        assert!(s.loupe(&handle, 1.0).is_some());
        let _ = s.switch(Kind::Full, SCREEN);
        assert!(s.loupe(&handle, 1.0).is_none());
    }

    #[test]
    fn recent_keeps_the_newest_twenty_and_clears() {
        let dir = std::env::temp_dir().join(format!("oc-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let shot = |v: u8| Crop {
            width: 2,
            height: 2,
            rgba: vec![v; 16],
        };
        let mut kept = Vec::new();
        for i in 0..23 {
            kept.push(keep(&dir, &shot(i), HISTORY).unwrap());
        }
        let now = list(&dir);
        assert_eq!(now.len(), HISTORY);
        // Newest first; the three oldest were dropped.
        assert_eq!(now[0], kept[22]);
        assert_eq!(now[HISTORY - 1], kept[3]);
        assert!(!kept[2].exists());
        assert_eq!(open_recent(&now[0]).unwrap().rgba, vec![22; 16]);
        clear_dir(&dir);
        assert!(!dir.exists() && list(&dir).is_empty());
    }

    #[test]
    fn recent_is_off_by_default_and_never_written_then() {
        assert!(!crate::config::Config::default().snip.history);
        // Off: `remember` writes nothing (the running config has the default).
        let before = history_dir().map(|d| list(&d).len());
        remember(&Crop {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(history_dir().map(|d| list(&d).len()), before);
        // The strip is empty without history; with pictures it shows them.
        let mut s = Snip::new(vec![rect(0.0, 0.0, 10.0, 10.0)], SCREEN, true, None);
        let texts = widgets::shot::texts(s.view());
        assert!(!texts.iter().any(|t| t == "Recent"), "{texts:?}");
        let picture = image::Handle::from_rgba(2, 1, vec![200; 8]);
        s.set_recent(vec![(PathBuf::from("a.png"), picture)]);
        let texts = widgets::shot::texts(s.view());
        assert!(texts.iter().any(|t| t == "Recent"), "{texts:?}");
        widgets::shot::save("snip-switch", s.view(), Size::new(1280.0, 260.0));
    }
}
