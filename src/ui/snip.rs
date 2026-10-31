//! Snip mode (DESIGN.md "Snip mode"): pick a region, a window, the whole screen or
//! a scrolling area on the frozen frame. Then (`snip.after`) the capture is copied,
//! with a toast that offers Edit for a few seconds, and/or the editor opens on it.
//! While picking, edges snap to windows and (Windows) to the controls of the app
//! the user was in (Shift = no snapping), and a loupe shows the pixels under the
//! pointer. The timer hides the overlay, counts down (menus can be opened
//! meanwhile), then captures again. Recent (`snip.history`, off by default): the
//! last captures, kept only in the user's data dir.
//! Long capture: the user scrolls the page under the picked area by hand while
//! a thread grabs and stitches it (`screens::long_capture`, `scroll::by_hand`).
//! The colour picker and the ruler run on the frozen screen (`screens::tools`).
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use iced::widget::tooltip::Position;
use iced::widget::{canvas, column, container, image, row};
use iced::{Center, Element, Fill, Point, Rectangle, Size};

use super::kit::button;
use super::kit::chip;
use super::kit::icon::Icon;
use super::kit::icon_button::{self, State, icon_button};
use super::kit::key_hint::{self, key_hint};
use super::kit::loupe::Loupe;
use super::kit::surface::{self, Material, surface};
use super::kit::text::{sub, text};
use super::kit::tokens::{Role, radius, space};
use super::kit::tooltip::tip;
use super::overlay;
use crate::actions::Crop;
use crate::capture::window_rect::WinRect;
use crate::config::{SnipAfter, Target};

/// An edge snaps to a window / control edge this near (logical px).
const SNAP: f32 = 8.0;
/// The hover info chip's height, the drag size chip's width (logical px).
const INFO_H: f32 = 30.0;
const SIZE_CHIP_W: f32 = 96.0;
/// The toast offers Edit this long, then the overlay closes.
pub const TOAST_SECS: u64 = 4;
pub const COPIED: &str = "Copied: paste it into your AI chat";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Region,
    Window,
    Full,
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

    /// Picking an area by hand: the loupe and snapping help.
    pub fn picks(self) -> bool {
        self != Kind::Full
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// Long capture's Done (Enter): the stitched image opens in the editor.
    Stop,
    /// Long capture so far.
    Progress(crate::capture::scroll::Update),
    /// Long capture ended (Done or Cancel): the image gathered.
    Done(Result<Crop, String>),
    /// The timer: hide, wait this many seconds, capture again (0 = off).
    Delay(u8),
    /// Countdown `.0` has `.1` seconds left (0 = time to hide).
    Tick(u32, u8),
    /// Countdown `.0` is over and the overlay hidden: capture now.
    Shoot(Kind),
    /// The countdown's Cancel; long capture's (Esc): back to the picked area.
    Cancel,
    /// The toast's Edit: the editor on the capture.
    Edit,
    /// Toast `.0` was shown long enough.
    ToastDone(u32),
    /// Toast `.0`'s capture could not be copied: the editor opens on it.
    CopyFailed(u32),
    /// Controls of one window (logical px), for opening `.0`.
    Controls(u32, Vec<Pick>),
    /// Shift is held: no snapping.
    Shift(bool),
    /// The Recent strip's pictures, newest first.
    Recent(Vec<(PathBuf, image::Handle)>),
    OpenRecent(PathBuf),
    /// A recent capture, read back.
    Loaded(Result<Crop, String>),
    ClearRecent,
    /// The freeze hint has shown long enough (`HINT_SECS`).
    HintGone(u32),
    /// The colour picker's or the ruler's (`screens::tools`).
    Tool(crate::ui::screens::tools::Msg),
}

/// The freeze hint shows this long (or until a drag starts).
pub const HINT_SECS: u64 = 2;
/// The info chip's ladder (the wheel's steps).
const LADDER: [&str; 3] = ["Element", "Window", "Screen"];
/// What the freeze hint says (Region, Window).
const HINT_REGION: [(Option<&str>, &str); 4] = [
    (None, "Click a window"),
    (None, "Drag an area"),
    (Some("Enter"), "Full screen"),
    (Some("Esc"), "Cancel"),
];
/// With several screens, when it fits: Shift+Enter too.
const HINT_ALL: [(Option<&str>, &str); 5] = [
    (None, "Click a window"),
    (None, "Drag an area"),
    (Some("Enter"), "Full screen"),
    (Some("Shift+Enter"), "All screens"),
    (Some("Esc"), "Cancel"),
];
const HINT_WINDOW: [(Option<&str>, &str); 2] = [(None, "Click a window"), (Some("Esc"), "Cancel")];

/// What a pick leads to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Next {
    /// The capture is this area (logical px): copy and / or edit it.
    Edit(Rectangle),
}

/// What a finished capture does (config `snip.after`): (copy it, open the editor).
pub fn route(after: SnipAfter) -> (bool, bool) {
    match after {
        SnipAfter::Copy => (true, false),
        SnipAfter::Editor => (false, true),
        SnipAfter::Both => (true, true),
    }
}

/// Something a click can capture: a window, a control in one, or the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    /// Logical px.
    pub rect: Rectangle,
    /// What the outline's label says ("Window: Notepad", "Element: button: Save").
    pub label: String,
    /// The window it is, or belongs to (its handle / id).
    pub window: isize,
}

impl Pick {
    pub fn window(rect: Rectangle, title: &str, handle: isize) -> Self {
        let label = match title.trim() {
            "" => "Window".to_string(),
            t => format!("Window: {t}"),
        };
        Pick {
            rect,
            label,
            window: handle,
        }
    }

    pub fn control(rect: Rectangle, what: &str, window: isize) -> Self {
        Pick {
            rect,
            label: format!("Element: {what}"),
            window,
        }
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

pub use super::platform::monitor::Place;

/// A captured monitor as the overlay shows it: where, and its frame.
pub fn screen_of(frame: crate::capture::Frame) -> (Place, image::Handle) {
    let place = Place {
        origin: (frame.x, frame.y),
        px: (frame.width, frame.height),
        scale: frame.scale,
    };
    (
        place,
        image::Handle::from_rgba(frame.width, frame.height, frame.rgba),
    )
}

pub struct Snip {
    pub kind: Kind,
    /// Every window on the desktop (physical desktop px, `desktop_rect`),
    /// topmost first; their controls (Windows: UI Automation), arriving later.
    desktop: Vec<Pick>,
    desktop_controls: Vec<Pick>,
    /// The starting monitor (the overlay's), and every monitor in its px
    /// (`app::screens`' one unit: the others have their own windows), it first.
    place: Place,
    monitors: Vec<Rectangle>,
    /// The windows on the desktop in the starting monitor's px (clipped to the
    /// box round the monitors), topmost first, and their controls (cut to their
    /// window).
    windows: Vec<Pick>,
    controls: Vec<Pick>,
    /// The wheel's walk from the smallest target under the pointer (the key)
    /// up to its window and the screen: how many steps up.
    walk: (Option<Rectangle>, usize),
    /// Long capture works here (not on Wayland: no silent grab).
    scrolls: bool,
    /// The running long capture.
    pub scroll: Option<crate::ui::screens::long_capture::Long>,
    /// The colour picker or the ruler, while one runs (`screens::tools`).
    pub tool: Option<crate::ui::screens::tools::Tool>,
    /// The window the user was in (Windows): focus goes back to it while the
    /// overlay is only a pill.
    pub foreground: Option<isize>,
    shift: bool,
    /// Asks the reader thread for a window's controls (Windows; see
    /// `set_reader`), and the windows already asked for.
    reader: Option<Sender<isize>>,
    /// The reader and how many windows to read at once, until the first frame
    /// is drawn (`start_reading`): UI Automation asks every window, the
    /// overlay too, and held its first frame back ~130 ms on the VM.
    later: Option<(Sender<isize>, usize)>,
    asked: Vec<isize>,
    /// Where the button went down: as pressed, and as snapped.
    press: Option<(Point, Point)>,
    /// The pointer (logical px), for the loupe.
    pub pointer: Option<Point>,
    /// The timer pill: countdown number, seconds left.
    countdown: Option<(u32, u8)>,
    countdowns: u32,
    toast: Option<Toast>,
    toasts: u32,
    recent: Vec<(PathBuf, image::Handle)>,
    /// The freeze hint shows (`HintGone`, a drag hides it).
    pub hinting: bool,
    /// Started by the live pin quick action: a pick becomes a live pin, no editor.
    pub live_pin: bool,
}

/// The pill window's size (logical px): countdown, toast.
pub const PILL: Size = Size::new(500.0, 68.0);
/// Its corners (the window is cut to them).
pub const PILL_RADIUS: f32 = radius::WINDOW;

impl Snip {
    /// `windows`: every window, physical desktop px, topmost first (none = no
    /// window picking, Wayland); shown on the monitor at `place`.
    pub fn new(windows: Vec<Pick>, place: Place, scrolls: bool, foreground: Option<isize>) -> Self {
        let mut snip = Snip {
            kind: Kind::Region,
            desktop: windows,
            desktop_controls: Vec::new(),
            place,
            monitors: vec![Rectangle::with_size(place.size())],
            windows: Vec::new(),
            controls: Vec::new(),
            walk: (None, 0),
            scrolls,
            scroll: None,
            tool: None,
            foreground,
            shift: false,
            reader: None,
            later: None,
            asked: Vec::new(),
            press: None,
            pointer: None,
            countdown: None,
            countdowns: 0,
            toast: None,
            toasts: 0,
            recent: Vec::new(),
            hinting: true,
            live_pin: false,
        };
        snip.relocal();
        snip
    }

    /// The topmost window under `area`'s centre (logical px, this screen): the
    /// one a live pin of it follows.
    pub fn window_under(&self, area: Rectangle) -> Option<isize> {
        let centre = area.center();
        (self.windows.iter())
            .find(|w| w.rect.contains(centre))
            .map(|w| w.window)
    }

    /// Every monitor (the starting one first) in the starting monitor's px:
    /// hover, picks and drags work on all of them (each has its window).
    pub fn set_monitors(&mut self, monitors: Vec<Rectangle>) {
        if !monitors.is_empty() {
            self.monitors = monitors;
            self.relocal();
        }
    }

    /// The box round every monitor (starting monitor's px): where an area may go.
    pub fn desk(&self) -> Rectangle {
        (self.monitors.iter()).fold(self.monitors[0], |all, m| all.union(m))
    }

    /// The monitor under `p`, else the starting one.
    pub fn monitor_at(&self, p: Point) -> Rectangle {
        (self.monitors.iter().find(|m| m.contains(p))).map_or(self.monitors[0], |m| *m)
    }

    /// The windows and controls in the starting monitor's px, nothing hovered.
    fn relocal(&mut self) {
        self.windows = self.local(&self.desktop);
        self.controls = Vec::new();
        let controls = self.local(&self.desktop_controls);
        self.cut_in(controls);
        (self.walk, self.press, self.pointer) = ((None, 0), None, None);
    }

    /// `picks` (physical desktop px) in the starting monitor's logical px (on
    /// every monitor: one unit), cut to the box round the monitors; those off it go.
    fn local(&self, picks: &[Pick]) -> Vec<Pick> {
        let Place { origin, scale, .. } = self.place;
        let here = picks.iter().map(|p| Pick {
            rect: local(p.rect, origin, scale),
            ..p.clone()
        });
        clip(here.collect(), self.desk())
    }

    /// Adds `controls` (logical px) cut to their window: a page's content runs
    /// on below a window's bottom edge.
    fn cut_in(&mut self, controls: Vec<Pick>) {
        let within = controls.into_iter().filter_map(|c| {
            let window = self.windows.iter().find(|w| w.window == c.window)?;
            Some(Pick {
                rect: c.rect.intersection(&window.rect)?,
                ..c
            })
        });
        self.controls.extend(within);
    }

    /// The modes offered, in switch order.
    pub fn kinds(&self) -> Vec<Kind> {
        [Kind::Region, Kind::Window, Kind::Full]
            .into_iter()
            .filter(|k| match k {
                Kind::Window => !self.windows.is_empty(),
                _ => true,
            })
            .collect()
    }

    /// The topmost window under `p`.
    pub fn window_at(&self, p: Point) -> Option<Rectangle> {
        self.windows
            .iter()
            .find(|w| w.rect.contains(p))
            .map(|w| w.rect)
    }

    /// Controls are read through `reader` (a window's handle in; its controls
    /// come back as `Msg::Controls`): now for the topmost `first` windows, the
    /// rest when the pointer first goes over them.
    pub fn set_reader(&mut self, reader: Sender<isize>, first: usize) {
        self.reader = Some(reader);
        let top: Vec<isize> = self.windows.iter().take(first).map(|w| w.window).collect();
        top.into_iter().for_each(|w| self.ask(w));
    }

    /// `set_reader` once the overlay's first frame is drawn (`start_reading`).
    pub fn read_later(&mut self, reader: Sender<isize>, first: usize) {
        self.later = Some((reader, first));
    }

    /// The first frame is up: the controls are read from now on.
    pub fn start_reading(&mut self) {
        if let Some((reader, first)) = self.later.take() {
            self.set_reader(reader, first);
        }
    }

    /// Asks once for the controls of `window`.
    fn ask(&mut self, window: isize) {
        if let Some(reader) = &self.reader
            && !self.asked.contains(&window)
        {
            self.asked.push(window);
            let _ = reader.send(window); // the reader is gone: the snip is closing
        }
    }

    /// What a click at `p` can capture, smallest first: the controls of the
    /// topmost window there that contain `p`, that window, the screen. Empty
    /// without a window list (Wayland: free drags only).
    pub fn chain(&self, p: Point) -> Vec<Pick> {
        if self.desktop.is_empty() {
            return Vec::new();
        }
        let top = self.windows.iter().find(|w| w.rect.contains(p));
        let mut chain: Vec<Pick> = match top {
            Some(w) => (self.controls.iter())
                .filter(|c| c.window == w.window && c.rect.contains(p))
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        let area = |t: &Pick| t.rect.width * t.rect.height;
        chain.sort_by(|a, b| area(a).total_cmp(&area(b)));
        chain.extend(top.cloned());
        chain.push(Pick {
            rect: self.monitor_at(p),
            label: "Screen".into(),
            window: 0,
        });
        // A control as big as its parent (a window's root pane) is one step, not
        // two, named as the bigger one ("Window: …").
        let mut steps: Vec<Pick> = Vec::new();
        for t in chain {
            if steps.last().is_some_and(|l| l.rect == t.rect) {
                steps.pop();
            }
            steps.push(t);
        }
        steps
    }

    /// The pointer moved (logical px): the outline starts again from the
    /// smallest target there when that changed.
    /// A window not read yet has its controls asked for.
    pub fn hover(&mut self, p: Point) {
        self.pointer = Some(p);
        if let Some(w) = self.windows.iter().find(|w| w.rect.contains(p)) {
            self.ask(w.window);
        }
        let first = self.chain(p).first().map(|t| t.rect);
        if first != self.walk.0 {
            self.walk = (first, 0);
        }
    }

    /// The wheel: away from the user (`notches` > 0) = the next bigger target,
    /// towards = back to a smaller one.
    pub fn walk(&mut self, notches: f32) {
        let Some(p) = self.pointer else { return };
        let last = self.chain(p).len().saturating_sub(1);
        let level = self.walk.1;
        self.walk.1 = if notches > 0.0 {
            (level + 1).min(last)
        } else if notches < 0.0 {
            level.saturating_sub(1)
        } else {
            level
        };
    }

    /// What a click would capture now (Region mode), outlined while hovering.
    pub fn outlined(&self) -> Option<Pick> {
        if self.kind != Kind::Region {
            return None;
        }
        let chain = self.chain(self.pointer?);
        let at = self.walk.1.min(chain.len().checked_sub(1)?);
        chain.into_iter().nth(at)
    }

    /// `p` (logical px) on the nearest window or control edge, per axis, while
    /// an area is dragged (Region) and Shift isn't held. Not in
    /// Window mode: a click near a window's edge would land on the edge.
    /// Until the pointer leaves the press by more than a tap's jitter, it stays
    /// where the press was put: two nearby edges can't turn a click into a drag.
    pub fn snap(&self, p: Point) -> Point {
        if self.shift || self.kind != Kind::Region {
            return p;
        }
        if let Some((down, at)) = self.press
            && down.distance(p) <= overlay::TAP
        {
            return at;
        }
        let rects = self.windows.iter().chain(&self.controls).map(|t| &t.rect);
        snap(p, rects, SNAP)
    }

    pub fn set_shift(&mut self, held: bool) {
        self.shift = held;
    }

    /// Shift (or Alt) is held.
    pub fn shift(&self) -> bool {
        self.shift
    }

    /// The button went down at `p`: where the stroke starts (snapped).
    pub fn press(&mut self, p: Point) -> Point {
        self.hinting = false;
        self.press = None;
        let at = self.snap(p);
        self.press = Some((p, at));
        at
    }

    /// Controls of a window arrived (physical desktop px): kept for every
    /// monitor, those on this one cut to their window.
    pub fn add_controls(&mut self, controls: Vec<Pick>) {
        let here = self.local(&controls);
        self.desktop_controls.extend(controls);
        self.cut_in(here);
    }

    /// Starts in `kind` if this screen offers it (else Region, no Window list on
    /// Wayland). Full picks the whole screen at once.
    /// Snip is one flow (owner, 2026-10-31): every start is the pick step
    /// (hover + click = a window, drag = an area, Enter = the screen); a
    /// window snip (Alt+Print Screen, `--target window`) starts with the
    /// window the user was in already picked, a full-screen one with the screen.
    pub fn start(&mut self, kind: Kind, screen: Size) -> Option<Next> {
        let user = (self.windows.iter()).find(|w| Some(w.window) == self.foreground);
        match kind {
            Kind::Window if self.kinds().contains(&kind) => {
                let picked = user.map(|w| Next::Edit(w.rect));
                let _ = self.switch(Kind::Region, screen);
                picked
            }
            Kind::Full => self.switch(kind, screen),
            _ => self.switch(Kind::Region, screen),
        }
    }

    /// Enter: the whole screen the pointer is on, whatever is outlined.
    pub fn full(&self) -> Next {
        Next::Edit(self.monitor_at(self.pointer.unwrap_or(Point::ORIGIN)))
    }

    /// Shift+Enter: every screen as one picture (the box round them).
    pub fn all(&self) -> Next {
        Next::Edit(self.desk())
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
            // A click (no drag) takes what is outlined: a control, a window, the screen.
            Kind::Region => (selection.or_else(|| self.outlined().map(|t| t.rect))).map(Next::Edit),
            Kind::Window => selection.or_else(|| self.window_at(at)).map(Next::Edit),
            Kind::Full => Some(Next::Edit(Rectangle::with_size(screen))),
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

    /// Long capture works here (the toolbar's Long capture button).
    pub fn can_scroll(&self) -> bool {
        self.scrolls
    }

    /// The window is only a pill now (the timer, the toast).
    pub fn is_pill(&self) -> bool {
        self.countdown.is_some() || self.toast.is_some()
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

    /// The toast's capture (tests).
    #[cfg(test)]
    pub fn toast_crop(&self) -> Option<&Crop> {
        self.toast.as_ref().map(|t| &t.crop)
    }

    /// Toast `id` is still the one showing.
    pub fn toast_showing(&self, id: u32) -> bool {
        self.toast.as_ref().is_some_and(|t| t.id == id)
    }

    pub fn set_recent(&mut self, recent: Vec<(PathBuf, image::Handle)>) {
        self.recent = recent;
    }

    /// The hint and Recent, at the top of the screen.
    pub fn view(&self) -> Element<'_, Msg> {
        // No mode switch: hover + click picks a control or a window, a drag an
        // area, the wheel walks up to the screen (quick actions still start in
        // Window or Full mode).
        let mut top = column![].spacing(space::PAGE[0]).align_x(Center);
        if !self.recent.is_empty() {
            let shots = (self.recent.iter()).map(|(path, picture)| {
                let shown = image(picture.clone()).content_fit(iced::ContentFit::Contain);
                let open =
                    icon_button::option(shown, State::default(), Msg::OpenRecent(path.clone()));
                tip(open, "A recent capture", Position::Bottom)
            });
            let clear = icon_button(
                Icon::Close.glyph(),
                State::default(),
                Some(Msg::ClearRecent),
            );
            let strip = row![
                sub(Role::META, "Recent"),
                row(shots).spacing(space::GAP),
                tip(clear, "Clear recent captures", Position::Bottom),
            ]
            .spacing(space::GAP)
            .padding([0.0, space::GAP])
            .align_y(Center);
            top =
                top.push(surface(Material::Widget, radius::WINDOW, strip).padding(space::BAR_PAD));
        }
        // the freeze hint, 16 px from the top, for its 2 s
        match self.kind {
            _ if !self.hinting => {}
            Kind::Region if self.desktop.is_empty() => {
                let hint = "Drag or circle an area (no window picking here) · Esc to close";
                top = top.push(surface::hint(hint));
            }
            Kind::Region => {
                let all = self.monitors.len() > 1
                    && key_hint::width(&HINT_ALL) + 2.0 * space::PAGE[0] <= self.place.size().width;
                let groups: &[key_hint::Group<'_>] = match all {
                    true => &HINT_ALL,
                    false => &HINT_REGION,
                };
                top = top.push(key_hint(groups, false));
            }
            Kind::Window => top = top.push(key_hint(&HINT_WINDOW, false)),
            Kind::Full => {}
        }
        container(top).center_x(Fill).padding(space::PAGE[0]).into()
    }

    /// When the window is only a pill (the timer, the toast): its view.
    pub fn pill_view(&self) -> Option<Element<'_, Msg>> {
        let bar = if let Some((_, left)) = self.countdown {
            let label = match left {
                0 => "Capturing…".to_string(),
                n => format!("Capturing in {n} s · open a menu now"),
            };
            row![
                text(Role::NOTE, label),
                button::regular("Cancel", false, Some(Msg::Cancel)),
            ]
        } else if self.toast.is_some() {
            row![
                text(Role::NOTE, COPIED),
                button::regular("Snip", true, Some(Msg::Edit)),
            ]
        } else {
            return None;
        };
        let bar = bar
            .spacing(space::PAGE[0])
            .padding(space::PAGE)
            .align_y(Center);
        // Exactly the window's size when it is only this pill (`windows::cut` shapes the
        // window to it); over the frame (Wayland) it is the same flat pill, centred.
        let pill = surface(Material::Widget, PILL_RADIUS, bar)
            .width(PILL.width)
            .height(PILL.height)
            .align_x(Center)
            .align_y(Center);
        Some(container(pill).center(Fill).into())
    }

    /// While an area is picked by hand: the loupe beside the pointer and,
    /// unless a drag is under way, the outline of what a click would capture
    /// with its info chip (kind · title · size | the wheel's ladder); while
    /// dragging (`drag` = the box so far), its size chip by the dragged corner
    /// (`Snapped` when an edge snapped) and the loupe away from it. `frame` is
    /// the frozen frame, `scale` physical px per logical px, `look` the active
    /// look's pair. Drawn by the window of the monitor the pointer is on:
    /// `view` maps the starting monitor's px to that window's (`screen` big),
    /// `frame` / `scale` are that monitor's.
    pub fn guides<'a>(
        &self,
        frame: &'a image::Handle,
        scale: f32,
        view: overlay::View,
        screen: Size,
        drag: Option<Rectangle>,
        look: (iced::Color, iced::Color),
    ) -> Option<Element<'a, Msg>> {
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = frame
        else {
            return None;
        };
        let at = view.to_screen(self.pointer.filter(|_| self.kind.picks())?);
        let drag = drag.map(|r| view.rect_to_screen(r));
        let px = |v: f32| (v * scale).round() as u32;
        let size_of = |r: Rectangle| format!("{} × {}", px(r.width), px(r.height));
        // over bare wallpaper: no outline (the edge frames the screen)
        let picked = (self.outlined())
            .filter(|t| drag.is_none() && t.label != "Screen")
            .map(|t| Pick {
                rect: view.rect_to_screen(t.rect),
                ..t
            });
        let loupe = Loupe {
            pixels,
            width: *width,
            height: *height,
            scale,
            at,
            outline: picked.as_ref().map(|t| t.rect),
            look,
            away: drag,
        };
        let mut layers = iced::widget::stack![canvas(loupe).width(Fill).height(Fill)];
        if let Some(t) = picked {
            let (kind, title) = match t.label.split_once(": ") {
                Some((kind, title)) => (kind.to_string(), Some(title.to_string())),
                None => (t.label.clone(), None),
            };
            let step = LADDER.iter().position(|s| *s == kind).unwrap_or(1);
            let kind: &'static str = LADDER[step];
            let chip = chip::info(kind, title, size_of(t.rect), &LADDER, step);
            // 8 px above the outline's top-left, else inside it
            let r = t.rect;
            let spot = match r.y >= INFO_H + 8.0 {
                true => Point::new(r.x, r.y - 8.0 - INFO_H),
                false => Point::new(r.x + 8.0, r.y + 8.0),
            };
            let spot = Point::new(spot.x.clamp(0.0, (screen.width - 380.0).max(0.0)), spot.y);
            layers = layers.push(iced::widget::pin(chip).position(spot));
        }
        if let Some(r) = drag.filter(|r| r.width >= 1.0 && r.height >= 1.0) {
            let snapped = self.snap(at) != at;
            let note = snapped.then_some("Snapped");
            let chip = chip::readout(size_of(r), false, note);
            // under (or over) the edge the pointer drags, aligned to the
            // area's side it is on
            let down = at.y >= r.center_y();
            let right = at.x >= r.center_x();
            let width = if snapped {
                SIZE_CHIP_W + 70.0
            } else {
                SIZE_CHIP_W
            };
            let y = match down {
                true if r.y + r.height + 8.0 + chip::READOUT_H < screen.height => {
                    r.y + r.height + 8.0
                }
                true => r.y + r.height - 8.0 - chip::READOUT_H,
                false if r.y - 8.0 - chip::READOUT_H > 0.0 => r.y - 8.0 - chip::READOUT_H,
                false => r.y + 8.0,
            };
            let row = match right {
                true => iced::widget::row![iced::widget::space::horizontal(), chip],
                false => iced::widget::row![chip, iced::widget::space::horizontal()],
            };
            let x = if right { r.x + r.width - width } else { r.x };
            let at = Point::new(x.clamp(0.0, (screen.width - width).max(0.0)), y);
            layers = layers.push(iced::widget::pin(container(row).width(width)).position(at));
        }
        Some(layers.into())
    }
}

/// `targets` cut to `whole`; those off it go.
fn clip(targets: Vec<Pick>, whole: Rectangle) -> Vec<Pick> {
    (targets.into_iter())
        .filter_map(|t| {
            Some(Pick {
                rect: t.rect.intersection(&whole)?,
                ..t
            })
        })
        .collect()
}

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

/// A window or control rectangle, still in physical desktop px (what `Pick`s
/// given to `Snip` hold).
pub fn desktop_rect((x, y, w, h): WinRect) -> Rectangle {
    Rectangle::new(
        Point::new(x as f32, y as f32),
        Size::new(w as f32, h as f32),
    )
}

/// `r` (physical desktop px) in the logical px of the monitor at `origin`
/// (physical px) with `scale` physical px per logical px.
fn local(r: Rectangle, origin: (i32, i32), scale: f32) -> Rectangle {
    let at = Point::new(r.x - origin.0 as f32, r.y - origin.1 as f32);
    Rectangle::new(at, r.size()) * (1.0 / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size::new(1920.0, 1080.0);
    /// The starting screen's own view (its px as they are).
    fn ident() -> overlay::View {
        overlay::View::new(1920, 1080, SCREEN)
    }
    fn look() -> (iced::Color, iced::Color) {
        let c = crate::ui::kit::themes::c();
        (c.accent, c.text)
    }

    /// A monitor at the origin at 100 %: physical px = logical px.
    fn flat(size: Size) -> Place {
        Place {
            origin: (0, 0),
            px: (size.width as u32, size.height as u32),
            scale: 1.0,
        }
    }

    fn to_logical(r: WinRect, origin: (i32, i32), scale: f32) -> Rectangle {
        local(desktop_rect(r), origin, scale)
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    /// Windows `rects`, topmost first, with handles 1, 2, 3…
    fn wins(rects: &[Rectangle]) -> Vec<Pick> {
        (rects.iter().zip(1..))
            .map(|(r, h)| Pick::window(*r, &format!("App {h}"), h))
            .collect()
    }

    #[test]
    fn a_live_pin_follows_the_topmost_window_under_the_areas_centre() {
        let front = rect(100.0, 100.0, 400.0, 300.0);
        let back = rect(0.0, 0.0, 1200.0, 900.0);
        let s = Snip::new(wins(&[front, back]), flat(SCREEN), true, None);
        // over both: the one in front; reaching past it: the centre decides
        assert_eq!(s.window_under(rect(150.0, 150.0, 50.0, 50.0)), Some(1));
        assert_eq!(s.window_under(rect(20.0, 20.0, 100.0, 100.0)), Some(2));
        assert_eq!(
            s.window_under(rect(1300.0, 950.0, 50.0, 50.0)),
            None,
            "the desktop"
        );
    }

    #[test]
    fn each_mode_picks_its_area() {
        let front = rect(100.0, 100.0, 400.0, 300.0);
        let back = rect(0.0, 0.0, 1200.0, 900.0);
        let off = rect(2000.0, 0.0, 500.0, 500.0); // another monitor
        let mut s = Snip::new(wins(&[front, back, off]), flat(SCREEN), true, None);
        let area = rect(10.0, 10.0, 50.0, 50.0);
        let drag = Some(area);
        let click = Point::new(150.0, 150.0);

        assert_eq!(s.kind, Kind::Region);
        assert_eq!(s.pick(drag, click, SCREEN), Some(Next::Edit(area)));
        // A click takes the window under it (no Window mode needed).
        s.hover(click);
        assert_eq!(s.pick(None, click, SCREEN), Some(Next::Edit(front)));

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

        // One flow: a window snip is the pick step with the user's window
        // already picked; the user's window unknown: the pick step.
        let mut s = Snip::new(wins(&windows), flat(SCREEN), true, None);
        assert_eq!(s.start(Kind::Window, SCREEN), None, "the pick step");
        assert_eq!(s.kind, Kind::Region);
        let user = s.windows[0].window;
        let mut s = Snip::new(wins(&windows), flat(SCREEN), true, Some(user));
        assert_eq!(s.start(Kind::Window, SCREEN), Some(Next::Edit(windows[0])));
        assert_eq!(s.kind, Kind::Region, "the same flow once picked");
        // Full opens the editor on the whole screen at once.
        let whole = Rectangle::with_size(SCREEN);
        assert_eq!(s.start(Kind::Full, SCREEN), Some(Next::Edit(whole)));
        assert_eq!(s.kind, Kind::Full);
        // No window list (Wayland): the window shortcut falls back to Region.
        let mut wayland = Snip::new(Vec::new(), flat(SCREEN), false, None);
        assert_eq!(wayland.start(Kind::Window, SCREEN), None);
        assert_eq!(wayland.kind, Kind::Region);
    }

    #[test]
    fn the_overlay_offers_no_mode_switch_yet_every_target_picks_its_area() {
        let window = rect(100.0, 100.0, 400.0, 300.0);
        let inside = Point::new(200.0, 200.0);
        let whole = Rectangle::with_size(SCREEN);
        for target in [
            Target::Circle,
            Target::LastRegion,
            Target::Window,
            Target::FullScreen,
        ] {
            let mut s = Snip::new(wins(&[window]), flat(SCREEN), true, None);
            let first = s.start(Kind::for_target(target), SCREEN);
            // Hover picks a window or a control, a drag an area, the wheel
            // the screen: no Region / Window / Full switch.
            let texts = crate::ui::kit::testing::shot::texts(s.view());
            for mode in ["Region", "Window", "Full"] {
                assert!(!texts.iter().any(|t| t == mode), "{target:?}: {texts:?}");
            }
            s.hover(inside);
            let picked = first.or_else(|| s.pick(None, inside, SCREEN));
            let want = match target {
                Target::FullScreen => whole,
                _ => window, // a click on a window takes it, in any mode
            };
            assert_eq!(picked, Some(Next::Edit(want)), "{target:?}");
        }
    }

    #[test]
    fn modes_without_support_are_hidden() {
        let all = Snip::new(
            wins(&[rect(0.0, 0.0, 10.0, 10.0)]),
            flat(SCREEN),
            true,
            None,
        );
        assert_eq!(all.kinds(), [Kind::Region, Kind::Window, Kind::Full]);
        // Wayland: no window list, no synthetic scrolling.
        let wayland = Snip::new(Vec::new(), flat(SCREEN), false, None);
        assert_eq!(wayland.kinds(), [Kind::Region, Kind::Full]);
        // Windows only on other monitors count as none.
        let elsewhere = Snip::new(
            wins(&[rect(3000.0, 0.0, 10.0, 10.0)]),
            flat(SCREEN),
            true,
            None,
        );
        assert!(!elsewhere.kinds().contains(&Kind::Window));
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
        let mut s = Snip::new(Vec::new(), flat(SCREEN), true, None);
        let crop = Crop {
            width: 2,
            height: 1,
            rgba: vec![0; 8],
        };
        assert!(s.pill_view().is_none());
        let first = s.toast(crop.clone(), true);
        assert!(s.toast_showing(first) && s.pill_view().is_some());
        let texts = crate::ui::kit::testing::shot::texts(s.pill_view().unwrap());
        assert!(texts.iter().any(|t| t == COPIED), "{texts:?}");
        crate::ui::kit::testing::shot::save("snip-toast", s.pill_view().unwrap(), PILL);
        let second = s.toast(crop, false);
        assert!(!s.toast_showing(first), "an older toast's timeout is stale");
        let t = s.take_toast().unwrap();
        assert!(!t.in_place && (t.crop.width, t.crop.height) == (2, 1));
        assert!(!s.toast_showing(second) && !s.has_toast());
        assert!(
            crate::ui::kit::testing::shot::texts(s.view())
                .iter()
                .all(|t| t != COPIED)
        );
    }

    #[test]
    fn the_timer_counts_down_then_shoots_once() {
        let mut s = Snip::new(Vec::new(), flat(SCREEN), true, None);
        assert_eq!(s.delay(0), None, "Off starts nothing");
        assert!(!s.is_pill());
        let id = s.delay(3).unwrap();
        assert!(s.is_pill());
        let texts = crate::ui::kit::testing::shot::texts(s.pill_view().unwrap());
        assert!(
            texts.iter().any(|t| t.contains("Capturing in 3 s")),
            "{texts:?}"
        );
        crate::ui::kit::testing::shot::save("snip-countdown", s.pill_view().unwrap(), PILL);
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
        let mut s = Snip::new(wins(&[window]), flat(SCREEN), true, None);
        s.add_controls(vec![Pick::control(button, "button", 1)]);
        let p = Point::new(204.0, 147.0);
        assert_eq!(s.snap(p), Point::new(200.0, 150.0));
        s.set_shift(true);
        assert_eq!(s.snap(p), p);
        s.set_shift(false);
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
        let cells = crate::ui::kit::loupe::sample(&frame, 4, 3, (1, 1), 1);
        assert_eq!(cells.len(), 9);
        assert_eq!(
            cells[4],
            [1, 1, 7, 255],
            "the centre is the pixel under the pointer"
        );
        assert_eq!(cells[0], [0, 0, 7, 255]);
        assert_eq!(cells[8], [2, 2, 7, 255]);
        // At the corner: outside the frame is transparent.
        let cells = crate::ui::kit::loupe::sample(&frame, 4, 3, (0, 0), 1);
        assert_eq!(cells[0], [0; 4]);
        assert_eq!(cells[4], [0, 0, 7, 255]);
        assert_eq!(cells[8], [1, 1, 7, 255]);
        // Full size: 15x15 cells of 8 px = a 120 px loupe.
        let cells =
            crate::ui::kit::loupe::sample(&frame, 4, 3, (3, 2), crate::ui::kit::loupe::RADIUS);
        assert_eq!(cells.len(), 15 * 15);
        assert_eq!(cells[cells.len() / 2], [3, 2, 7, 255]);
        // It shows while picking an area by hand, not in Full.
        let handle = image::Handle::from_rgba(4, 3, frame);
        let mut s = Snip::new(Vec::new(), flat(SCREEN), true, None);
        assert!(
            s.guides(&handle, 1.0, ident(), SCREEN, None, look())
                .is_none(),
            "no pointer yet"
        );
        s.pointer = Some(Point::new(2.0, 1.0));
        assert!(
            s.guides(&handle, 1.0, ident(), SCREEN, None, look())
                .is_some()
        );
        let _ = s.switch(Kind::Full, SCREEN);
        assert!(
            s.guides(&handle, 1.0, ident(), SCREEN, None, look())
                .is_none()
        );
    }

    #[test]
    fn the_recent_strip_shows_only_with_pictures() {
        assert!(!crate::config::Config::default().snip.history);
        // The strip is empty without history; with pictures it shows them.
        let mut s = Snip::new(
            wins(&[rect(0.0, 0.0, 10.0, 10.0)]),
            flat(SCREEN),
            true,
            None,
        );
        let texts = crate::ui::kit::testing::shot::texts(s.view());
        assert!(!texts.iter().any(|t| t == "Recent"), "{texts:?}");
        let picture = image::Handle::from_rgba(2, 1, vec![200; 8]);
        s.set_recent(vec![(PathBuf::from("a.png"), picture)]);
        let texts = crate::ui::kit::testing::shot::texts(s.view());
        assert!(texts.iter().any(|t| t == "Recent"), "{texts:?}");
        crate::ui::kit::testing::shot::save("snip-switch", s.view(), Size::new(1280.0, 260.0));
    }

    /// A browser at the back, an editor in front with a toolbar and a button
    /// in the toolbar; the back window's controls hide behind the front one.
    fn desk() -> Snip {
        let editor = rect(100.0, 100.0, 800.0, 600.0);
        let browser = rect(0.0, 0.0, 1600.0, 1000.0);
        let mut s = Snip::new(wins(&[editor, browser]), flat(SCREEN), true, None);
        s.add_controls(vec![
            Pick::control(rect(100.0, 130.0, 800.0, 40.0), "tool bar", 1),
            Pick::control(rect(110.0, 135.0, 30.0, 30.0), "button: Save", 1),
            Pick::control(editor, "pane", 1), // the root pane: as big as the window
            Pick::control(rect(120.0, 140.0, 300.0, 300.0), "document", 2), // behind
        ]);
        s
    }

    #[test]
    fn hovering_outlines_the_smallest_thing_under_the_pointer() {
        let mut s = desk();
        let labels =
            |s: &Snip, p| -> Vec<String> { s.chain(p).into_iter().map(|t| t.label).collect() };
        // Over the button: button, toolbar, the editor window (its root pane is
        // the same rect: one step), the screen. Not the browser's document behind.
        assert_eq!(
            labels(&s, Point::new(120.0, 150.0)),
            [
                "Element: button: Save",
                "Element: tool bar",
                "Window: App 1",
                "Screen"
            ]
        );
        // On the browser where nothing covers it: the browser, then the screen.
        assert_eq!(
            labels(&s, Point::new(1200.0, 900.0)),
            ["Window: App 2", "Screen"]
        );
        // On the desktop: only the screen.
        assert_eq!(labels(&s, Point::new(1800.0, 1050.0)), ["Screen"]);
        s.hover(Point::new(120.0, 150.0));
        let outlined = s.outlined().unwrap();
        assert_eq!(outlined.rect, rect(110.0, 135.0, 30.0, 30.0));
        let frame = image::Handle::from_rgba(4, 4, vec![90; 64]);
        let guides = s
            .guides(&frame, 1.0, ident(), SCREEN, None, look())
            .unwrap();
        crate::ui::kit::testing::shot::save("snip-outline", guides, SCREEN);
        // A click there captures the button exactly.
        let at = Point::new(120.0, 150.0);
        assert_eq!(s.pick(None, at, SCREEN), Some(Next::Edit(outlined.rect)));
        // Only Region mode outlines.
        let _ = s.switch(Kind::Full, SCREEN);
        assert!(s.outlined().is_none());
        // Wayland (no window list): nothing to outline, a click takes nothing.
        let mut w = Snip::new(Vec::new(), flat(SCREEN), false, None);
        w.hover(at);
        assert!(w.outlined().is_none() && w.pick(None, at, SCREEN).is_none());
        let texts = crate::ui::kit::testing::shot::texts(w.view());
        assert!(
            texts.iter().any(|t| t.contains("no window picking")),
            "{texts:?}"
        );
    }

    #[test]
    fn the_wheel_walks_element_parent_window_screen_and_back() {
        let mut s = desk();
        let at = Point::new(120.0, 150.0);
        s.hover(at);
        let label = |s: &Snip| s.outlined().unwrap().label;
        assert_eq!(label(&s), "Element: button: Save");
        s.walk(1.0);
        assert_eq!(label(&s), "Element: tool bar");
        s.walk(1.0);
        assert_eq!(label(&s), "Window: App 1");
        s.walk(1.0);
        assert_eq!(label(&s), "Screen");
        s.walk(1.0);
        assert_eq!(label(&s), "Screen", "stops at the screen");
        assert_eq!(
            s.pick(None, at, SCREEN),
            Some(Next::Edit(Rectangle::with_size(SCREEN)))
        );
        s.walk(-1.0);
        s.walk(-1.0);
        assert_eq!(label(&s), "Element: tool bar");
        // Moving within the same button keeps the level; onto something else resets it.
        s.hover(Point::new(125.0, 152.0));
        assert_eq!(label(&s), "Element: tool bar");
        s.hover(Point::new(600.0, 500.0));
        assert_eq!(label(&s), "Window: App 1");
        s.walk(-5.0);
        assert_eq!(label(&s), "Window: App 1", "stops at the smallest");
    }

    /// N-73 / design state 8: every screen is frozen with its own window, and
    /// picks work on all of them in the starting screen's px (one unit, mixed
    /// scales): hover, the wheel, snapping, Enter = the pointer's screen,
    /// Shift+Enter = every screen.
    #[test]
    fn picks_work_on_every_screen_in_one_unit_at_any_scale() {
        use crate::ui::app::screens::{area_of, to_desktop, view_of};
        // The starting screen at 150 %; screen 2 LEFT of it, 100 %, its top 200 px higher.
        let primary = Place {
            origin: (0, 0),
            px: (2880, 1620),
            scale: 1.5,
        };
        let left = Place {
            origin: (-1920, -200),
            px: (1920, 1080),
            scale: 1.0,
        };
        let windows = vec![
            Pick::window(desktop_rect((-1620, 100, 1200, 600)), "Left", 1),
            Pick::window(desktop_rect((150, 150, 1200, 900)), "Primary", 2),
        ];
        let mut s = Snip::new(windows, primary, true, None);
        let two = area_of(primary, left);
        s.set_monitors(vec![Rectangle::with_size(primary.size()), two]);
        s.hover(Point::new(150.0, 150.0));
        assert_eq!(s.outlined().unwrap().label, "Window: Primary");
        // Screen 2's window, in the starting screen's px: its desktop px / 1.5.
        s.hover(Point::new(-700.0, 200.0));
        let window = s.outlined().unwrap();
        assert_eq!(window.label, "Window: Left");
        let near = |a: Point, b: Point| a.distance(b) < 0.01;
        assert!(near(
            to_desktop(primary, window.rect.position()),
            Point::new(-1620.0, 100.0)
        ));
        // drawn on screen 2's window at its own pixels
        let view = view_of(primary, left);
        let shown = view.rect_to_screen(window.rect);
        assert!(
            near(shown.position(), Point::new(300.0, 300.0)),
            "{shown:?}"
        );
        assert!((shown.width - 1200.0).abs() < 0.01 && (shown.height - 600.0).abs() < 0.01);
        let handle = image::Handle::from_rgba(4, 4, vec![0; 64]);
        assert!(
            s.guides(&handle, 1.0, view, left.size(), None, look())
                .is_some()
        );
        // Controls (physical px) read now land on screen 2 too.
        s.add_controls(vec![Pick::control(
            desktop_rect((-1500, 400, 300, 150)),
            "button: OK",
            1,
        )]);
        s.hover(Point::new(-950.0, 300.0));
        let button = s.outlined().unwrap();
        assert_eq!(button.label, "Element: button: OK");
        // A drag snaps to its edges.
        let snapped = s.snap(Point::new(button.rect.x - 3.0, button.rect.y - 2.0));
        assert_eq!(snapped, button.rect.position());
        // The wheel's top is screen 2: the monitor under the pointer.
        s.walk(1.0);
        s.walk(1.0);
        assert_eq!(s.outlined().unwrap().rect, two);
        // Enter takes that screen; Shift+Enter every screen as one picture.
        assert_eq!(s.full(), Next::Edit(two));
        let all = Rectangle::with_size(primary.size()).union(&two);
        assert_eq!(s.all(), Next::Edit(all));
        assert!((all.width - 3200.0).abs() < 0.01, "{all:?}");
        // Its bare wallpaper: a click takes screen 2.
        s.hover(Point::new(-100.0, 500.0));
        assert_eq!(
            s.pick(None, Point::new(-100.0, 500.0), SCREEN),
            Some(Next::Edit(two))
        );
        // Back on the starting screen.
        s.hover(Point::new(150.0, 150.0));
        assert_eq!(s.outlined().unwrap().label, "Window: Primary");
        assert_eq!(s.full(), Next::Edit(Rectangle::with_size(primary.size())));
    }

    /// The freeze hint says Shift+Enter only with several screens, and only
    /// when it fits the starting screen.
    #[test]
    fn the_freeze_hint_offers_all_screens_only_when_there_are_some_and_it_fits() {
        let says = |s: &Snip| {
            let texts = crate::ui::kit::testing::shot::texts(s.view());
            texts.iter().any(|t| t == "All screens")
        };
        let one = wins(&[rect(0.0, 0.0, 10.0, 10.0)]);
        let mut s = Snip::new(one.clone(), flat(SCREEN), true, None);
        assert!(!says(&s), "one screen");
        let other = rect(1920.0, 0.0, 1920.0, 1080.0);
        s.set_monitors(vec![Rectangle::with_size(SCREEN), other]);
        assert!(says(&s), "two screens");
        // a narrow starting screen: the hint without it
        let narrow = Size::new(560.0, 900.0);
        let mut s = Snip::new(one, flat(narrow), true, None);
        s.set_monitors(vec![Rectangle::with_size(narrow), other]);
        assert!(!says(&s), "doesn't fit");
    }

    #[test]
    fn outlines_and_their_size_label_hold_at_161_dpi() {
        // The Windows VM: 161 DPI = 1.677 physical px per logical px. A window
        // at physical (335, 168) 1342 x 1006 on a monitor at the origin.
        let scale = 161.0 / 96.0;
        let notepad = (335, 168, 1342, 1006);
        let r = to_logical(notepad, (0, 0), scale);
        let place = Place {
            origin: (0, 0),
            px: (3220, 1812),
            scale,
        };
        let mut s = Snip::new(
            vec![Pick::window(desktop_rect(notepad), "Notepad", 7)],
            place,
            true,
            None,
        );
        let inside = Point::new(r.x + 10.0, r.y + 10.0);
        s.hover(inside);
        assert_eq!(s.outlined().unwrap().label, "Window: Notepad");
        // The crop of the outline is the window's own pixels, give or take one.
        let picked = s.pick(None, inside, SCREEN);
        let Some(Next::Edit(area)) = picked else {
            panic!("{picked:?}")
        };
        let [x0, y0, x1, y1] = crate::actions::crop_bounds(3220, 1812, scale, area).unwrap();
        assert!(x0.abs_diff(335) <= 1 && y0.abs_diff(168) <= 1, "{x0} {y0}");
        assert!((x1 - x0).abs_diff(1342) <= 1 && (y1 - y0).abs_diff(1006) <= 1);
        // And the label says the physical size.
        let handle = image::Handle::from_rgba(4, 4, vec![0; 64]);
        assert!(
            s.guides(&handle, scale, ident(), SCREEN, None, look())
                .is_some()
        );
        let px = |v: f32| (v * scale).round() as u32;
        assert_eq!((px(area.width), px(area.height)), (1342, 1006));
    }

    #[test]
    fn controls_are_read_only_once_the_first_frame_is_up() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut s = Snip::new(
            wins(&[rect(0.0, 0.0, 400.0, 300.0)]),
            flat(SCREEN),
            true,
            None,
        );
        s.read_later(tx, 4);
        s.hover(Point::new(10.0, 10.0));
        assert!(rx.try_iter().next().is_none(), "nothing before the frame");
        s.start_reading();
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [1]);
        // Enter takes the whole screen, whatever is outlined.
        assert_eq!(s.full(), Next::Edit(Rectangle::with_size(SCREEN)));
    }

    #[test]
    fn any_window_under_the_pointer_gets_its_controls_read_once() {
        let (tx, rx) = std::sync::mpsc::channel();
        let front = rect(100.0, 100.0, 400.0, 300.0);
        let back = [1, 2, 3, 4, 5].map(|i| rect(0.0, 0.0, 1000.0 + 100.0 * i as f32, 1000.0));
        let mut all = vec![front];
        all.extend(back);
        let mut s = Snip::new(wins(&all), flat(SCREEN), true, None);
        s.set_reader(tx, 1);
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            [1],
            "the topmost at once"
        );
        // The 6th window down, where only it is: read when first hovered, once.
        s.hover(Point::new(1450.0, 500.0));
        s.hover(Point::new(1460.0, 510.0));
        s.hover(Point::new(200.0, 200.0)); // the front one: already read
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [6]);
        // Without a reader (X11, Wayland) nothing is asked and nothing breaks.
        let mut w = Snip::new(wins(&all), flat(SCREEN), true, None);
        w.hover(Point::new(1450.0, 500.0));
        assert!(w.asked.is_empty());
    }

    #[test]
    fn controls_are_cut_to_their_window() {
        let window = rect(100.0, 100.0, 400.0, 300.0);
        let mut s = Snip::new(wins(&[window]), flat(SCREEN), true, None);
        s.add_controls(vec![
            // a page running on below the window's bottom edge
            Pick::control(rect(100.0, 150.0, 400.0, 2000.0), "document", 1),
            // scrolled away entirely, and one of a window not on this screen
            Pick::control(rect(100.0, 900.0, 50.0, 20.0), "link", 1),
            Pick::control(rect(10.0, 10.0, 50.0, 20.0), "button", 9),
        ]);
        let rects: Vec<Rectangle> = s.controls.iter().map(|c| c.rect).collect();
        assert_eq!(rects, [rect(100.0, 150.0, 400.0, 250.0)]);
        // Below the window the outline is never the page's.
        s.hover(Point::new(300.0, 600.0));
        assert_eq!(s.outlined().unwrap().label, "Screen");
    }

    #[test]
    fn a_click_with_a_little_jitter_between_two_edges_stays_a_click() {
        // A 16 px icon in a toolbar; the user walked up to the window and clicks.
        let window = rect(0.0, 0.0, 800.0, 600.0);
        let icon = rect(100.0, 100.0, 16.0, 16.0);
        let mut s = Snip::new(wins(&[window]), flat(SCREEN), true, None);
        s.add_controls(vec![Pick::control(icon, "button", 1)]);
        s.hover(Point::new(107.0, 107.0));
        s.walk(1.0);
        assert_eq!(s.outlined().unwrap().rect, window);
        // Down near the icon's top-left, up 3 px further: nearer its other edges.
        let down = s.press(Point::new(107.0, 107.0));
        let up = s.snap(Point::new(110.0, 109.0));
        let stroke = [down, up];
        let selection = overlay::settle(&stroke, 1.0, SCREEN).map(|(sel, _)| sel.area);
        assert_eq!(selection, None, "a click, not a {selection:?} drag");
        assert_eq!(s.pick(selection, up, SCREEN), Some(Next::Edit(window)));
        // A real drag still snaps its end.
        assert_eq!(s.snap(Point::new(118.0, 113.0)), Point::new(116.0, 116.0));
    }
}
