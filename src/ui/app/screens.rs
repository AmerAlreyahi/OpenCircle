//! Every monitor at once (UI plan step 41): the starting screen is the overlay
//! (`Opening`'s frame and view); each other monitor gets a `Screen`, its own
//! window over its own frozen frame.
//!
//! One unit for every screen: whatever the user draws (stroke, selection,
//! words, lines, OCR bands) is kept in the starting screen's image px, which
//! are desktop physical px moved to the starting screen's origin and divided by
//! its scale once (`to_desktop` / `from_desktop`). It is one affine map of the
//! desktop for all screens, so a stroke crossing screens of mixed scaling stays
//! one straight line, and on one screen nothing changes. Each screen converts
//! only to draw (`Screen::view`).

use std::cell::{Cell, RefCell};
use std::time::Instant;

use iced::widget::{canvas, image, stack};
use iced::window::{self, Mode};
use iced::{Element, Fill, Point, Rectangle, Size, Task, Vector};

use crate::actions::{self, Crop};
use crate::config;
use crate::ocr;
use crate::ui::app::background::background;
use crate::ui::app::capture::overlay_window;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::app::opening::Opening;
use crate::ui::app::word;
use crate::ui::kit::motion;
use crate::ui::platform::monitor::{Place, place_window};
use crate::ui::screens::overlay::edge as border;
use crate::ui::{overlay, windows};

use super::App;

/// Another monitor's overlay while the overlay is open.
pub(in crate::ui) struct Screen {
    /// Its overlay window.
    pub(in crate::ui) window: window::Id,
    /// Where the monitor sits (physical desktop px) and its scale.
    pub(in crate::ui) place: Place,
    /// Its frozen frame: what it shows (one image, `overlay::Frozen`) and what crops take.
    pub(in crate::ui) frame: image::Handle,
    /// The starting screen's image px <-> this window's logical px.
    pub(in crate::ui) view: overlay::View,
    /// Its own draw log, so the starting screen's frame budget stays its own.
    pub(in crate::ui) first_draw: Cell<Option<(Instant, Size)>>,
    pub(in crate::ui) frame_times: RefCell<Vec<Instant>>,
    /// Its text (Windows OCR, one read of its frame), in the starting screen's
    /// px, once `read`; `reading` while it is out.
    pub(in crate::ui) lines: Vec<word::Line>,
    pub(in crate::ui) read: bool,
    pub(in crate::ui) reading: bool,
}

impl Screen {
    /// The monitor at `place` with its `frame`, beside the starting screen `start`.
    pub(in crate::ui) fn new(
        window: window::Id,
        start: Place,
        place: Place,
        frame: image::Handle,
    ) -> Screen {
        Screen {
            window,
            place,
            frame,
            view: view_of(start, place),
            first_draw: Cell::new(None),
            frame_times: RefCell::default(),
            lines: Vec::new(),
            read: false,
            reading: false,
        }
    }
}

/// Where the edge glow (the Active Look) is (owner, 2026-10-28): on the
/// screen the mouse is on, the starting one (`None`) at first. Moving to
/// another screen fades it out of the one left and into the new one at the
/// widget pace (150 ms ease-in out, 210 ms expo-out in), from wherever each
/// was; the frame clock runs only while a fade is on (`moving`).
#[derive(Default)]
pub(in crate::ui) struct Glow {
    /// Each screen that faded (the starting one: `None`), and how.
    fades: Vec<(Option<window::Id>, motion::Shown)>,
    /// The latest frame's time (`tick`); `None` until the first move.
    now: Option<Instant>,
}

impl Glow {
    /// The screen it is on (or going to).
    pub(in crate::ui) fn here(&self) -> Option<window::Id> {
        (self.fades.iter().find(|(_, s)| s.is_open())).and_then(|(w, _)| *w)
    }

    /// The pointer is on the screen `to` at `now`: the glow goes there.
    pub(in crate::ui) fn move_to(&mut self, to: Option<window::Id>, now: Instant) {
        let from = self.here();
        if from == to {
            return;
        }
        match self.fades.iter_mut().find(|(w, _)| *w == from) {
            Some((_, s)) => s.close(now),
            None => self
                .fades
                .push((from, motion::Shown::closing(motion::WIDGET, now))),
        }
        match self.fades.iter_mut().find(|(w, _)| *w == to) {
            Some((_, s)) => s.open(now),
            None => self
                .fades
                .push((to, motion::Shown::opening(motion::WIDGET, now))),
        }
        self.now = Some(now);
    }

    /// A frame at `now`.
    pub(in crate::ui) fn tick(&mut self, now: Instant) {
        if self.now.is_some() {
            self.now = Some(now);
        }
    }

    /// A fade is on: frames wanted.
    pub(in crate::ui) fn moving(&self) -> bool {
        self.now
            .is_some_and(|now| self.fades.iter().any(|(_, s)| s.moving(now)))
    }

    /// How strong the glow is on the screen `on` (0..1) at the latest frame.
    pub(in crate::ui) fn at(&self, on: Option<window::Id>) -> f32 {
        let fade = self.fades.iter().find(|(w, _)| *w == on);
        match (fade, self.now) {
            (Some((_, s)), Some(now)) => s.at(now),
            _ if on.is_none() => 1.0,
            _ => 0.0,
        }
    }
}

/// A point in the starting screen's image px (`start`) in desktop physical px.
pub(in crate::ui) fn to_desktop(start: Place, u: Point) -> Point {
    Point::new(
        start.origin.0 as f32 + u.x * start.scale,
        start.origin.1 as f32 + u.y * start.scale,
    )
}

/// A desktop physical px point in the starting screen's image px.
pub(in crate::ui) fn from_desktop(start: Place, d: Point) -> Point {
    Point::new(
        (d.x - start.origin.0 as f32) / start.scale,
        (d.y - start.origin.1 as f32) / start.scale,
    )
}

/// The monitor at `place` in the starting screen's image px.
pub(in crate::ui) fn area_of(start: Place, place: Place) -> Rectangle {
    let at = from_desktop(
        start,
        Point::new(place.origin.0 as f32, place.origin.1 as f32),
    );
    Rectangle::new(
        at,
        Size::new(place.px.0 as f32, place.px.1 as f32) * (1.0 / start.scale),
    )
}

/// How the window over the monitor at `place` shows the starting screen's
/// image px: `u` at `u * start.scale / place.scale + (start - place origin) /
/// place.scale` of its logical px, i.e. through the desktop px both share.
pub(in crate::ui) fn view_of(start: Place, place: Place) -> overlay::View {
    let k = start.scale / place.scale;
    let at = Vector::new(
        (start.origin.0 - place.origin.0) as f32 / place.scale,
        (start.origin.1 - place.origin.1) as f32 / place.scale,
    );
    overlay::View::mapped(place.size(), k, at)
}

/// How the lines of the monitor at `place` (its own logical px) go into the
/// starting screen's px (`Line::mapped`): through the desktop px both share.
pub(in crate::ui) fn into_model(start: Place, place: Place) -> (f32, Vector) {
    let k = place.scale / start.scale;
    let at = Vector::new(
        (place.origin.0 - start.origin.0) as f32 / start.scale,
        (place.origin.1 - start.origin.1) as f32 / start.scale,
    );
    (k, at)
}

impl Opening {
    /// The starting screen in physical desktop px.
    pub(in crate::ui) fn place(&self) -> Place {
        let px = |v: f32| (v * self.scale).round();
        Place {
            origin: (px(self.origin.x) as i32, px(self.origin.y) as i32),
            px: (px(self.screen.width) as u32, px(self.screen.height) as u32),
            scale: self.scale,
        }
    }

    /// Where a selection may go (image px): the starting screen, or the box
    /// round every screen (a stroke, box or text drag crosses between them).
    pub(in crate::ui) fn desk(&self) -> Rectangle {
        let start = self.place();
        (self.screens.iter()).fold(Rectangle::with_size(self.screen), |all, s| {
            all.union(&area_of(start, s.place))
        })
    }
}

/// `area` slid back inside `desk` (`overlay::keep_inside`, which keeps to a box at 0, 0).
pub(in crate::ui) fn keep_on(desk: Rectangle, area: Rectangle) -> Rectangle {
    let at = Vector::new(desk.x, desk.y);
    overlay::keep_inside(area - at, desk.size()) + at
}

/// Closing: every other monitor's window goes at once (cleared now, so no
/// frame of it shows while it closes). Their frames go with the opening.
// ponytail: opened and closed per opening, no pool of hidden windows; add one
// if opening the second screen's window ever shows as late.
pub(in crate::ui) fn close_all<T: Send + 'static>(screens: &[Screen]) -> Task<T> {
    Task::batch(screens.iter().map(|s| {
        windows::clear_now(s.window);
        windows::forget(s.window);
        window::close(s.window)
    }))
}

impl App {
    /// `area` (image px) cut out of the frozen frames: the starting screen's
    /// alone, as with one screen, unless it reaches another screen; then
    /// stitched from every frame it meets in desktop px (`actions::stitch`), at
    /// each screen's own pixels: one image whose text reads in order across.
    pub(in crate::ui) fn crop_of(&self, area: Rectangle) -> Option<Crop> {
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.o.frame
        else {
            return None;
        };
        let start = self.o.place();
        let meets = |s: &&Screen| {
            (area_of(start, s.place).intersection(&area)).is_some_and(|r| r.width * r.height > 0.0)
        };
        if !self.o.screens.iter().any(|s| meets(&s)) {
            return actions::crop(pixels, *width, *height, self.o.scale, area);
        }
        let bounds = self.desk_bounds(area)?;
        fn part(at: (i32, i32), frame: &image::Handle) -> Option<actions::crop::Part<'_>> {
            match frame {
                image::Handle::Rgba {
                    width,
                    height,
                    pixels,
                    ..
                } => Some((at, *width, *height, &pixels[..])),
                _ => None,
            }
        }
        let others = (self.o.screens.iter().filter(meets)).map(|s| part(s.place.origin, &s.frame));
        let parts: Vec<_> = (std::iter::once(Some((start.origin, *width, *height, &pixels[..]))))
            .chain(others)
            .flatten()
            .collect();
        actions::stitch(&parts, bounds)
    }

    /// `area` (image px, on a stitched crop) in desktop px: `[x0, y0, x1, y1]`, the
    /// edges rounded outwards, as `crop` does (past a mapping's float error), kept
    /// on the screens.
    fn desk_bounds(&self, area: Rectangle) -> Option<[i32; 4]> {
        let start = self.o.place();
        let desk = self.o.desk().intersection(&area)?;
        let a = to_desktop(start, desk.position());
        let b = to_desktop(start, desk.position() + Vector::from(desk.size()));
        let (down, up) = (
            |v: f32| (v + 0.01).floor() as i32,
            |v: f32| (v - 0.01).ceil() as i32,
        );
        Some([down(a.x), down(a.y), up(b.x), up(b.y)])
    }

    /// The desktop px (physical) of `crop_of(area)`'s top-left corner.
    pub(in crate::ui) fn crop_origin(&self, area: Rectangle) -> Option<(i32, i32)> {
        let image::Handle::Rgba { width, height, .. } = self.o.frame.as_ref()? else {
            return None;
        };
        let start = self.o.place();
        let meets = |s: &Screen| {
            (area_of(start, s.place).intersection(&area)).is_some_and(|r| r.width * r.height > 0.0)
        };
        if self.o.screens.iter().any(meets) {
            let [x, y, ..] = self.desk_bounds(area)?;
            return Some((x, y));
        }
        let [x, y, ..] = actions::crop_bounds(*width, *height, self.o.scale, area)?;
        Some((start.origin.0 + x as i32, start.origin.1 + y as i32))
    }

    /// The other screens' text is read too: Windows OCR reads this opening
    /// (one read of each frame). The helper reads the starting screen only.
    // ponytail: the helper's bands are the starting screen's rows; add a Rows
    // per screen if the helper ever reads other screens.
    fn screens_text(&self) -> bool {
        self.o.text_by == Some(ocr::Source::System) && !self.o.screens.is_empty()
    }

    /// Row or screen under `p` (image px) was read: its lines are known (or
    /// there are none).
    pub(in crate::ui) fn text_known(&self, p: Point) -> bool {
        let start = self.o.place();
        let other = (self.o.screens.iter())
            .filter(|_| self.screens_text())
            .find(|s| area_of(start, s.place).contains(p));
        other.map_or_else(|| self.o.screen_text.knows(p.y), |s| s.read)
    }

    /// Every screen's text was read.
    pub(in crate::ui) fn text_whole(&self) -> bool {
        self.o.screen_text.is_whole(self.o.screen.height)
            && (!self.screens_text() || self.o.screens.iter().all(|s| s.read))
    }

    /// Every line read so far, whichever screen.
    pub(in crate::ui) fn lines_iter(&self) -> impl Iterator<Item = &word::Line> {
        let others = (self.o.screens.iter()).flat_map(|s| s.lines.iter());
        self.o.screen_text.iter().chain(others)
    }

    /// Each monitor's area (image px) while text is read on several, else none.
    pub(in crate::ui) fn screen_areas(&self) -> Vec<Rectangle> {
        if !self.screens_text() {
            return Vec::new();
        }
        let start = self.o.place();
        let others = (self.o.screens.iter()).map(|s| area_of(start, s.place));
        std::iter::once(Rectangle::with_size(self.o.screen))
            .chain(others)
            .collect()
    }

    /// Every line read so far in reading order: screen by screen, left to
    /// right (then top to bottom), so a selection across a seam runs from the
    /// end of one screen's text into the start of the next's.
    pub(in crate::ui) fn all_lines(&self) -> Vec<word::Line> {
        if !self.screens_text() {
            return self.o.screen_text.lines();
        }
        let mut parts: Vec<((i32, i32), Vec<word::Line>)> = (self.o.screens.iter())
            .map(|s| (s.place.origin, s.lines.clone()))
            .collect();
        parts.push((self.o.place().origin, self.o.screen_text.lines()));
        parts.sort_by_key(|(origin, _)| *origin);
        parts.into_iter().flat_map(|(_, lines)| lines).collect()
    }

    /// Windows OCR reads the next other screen not read yet, when the starting
    /// screen's own read is done and none is out: one read of its frame, in
    /// the starting screen's px.
    pub(in crate::ui) fn read_screens(&mut self) -> Task<Message> {
        if !self.screens_text() || self.o.screens.iter().any(|s| s.reading) {
            return Task::none();
        }
        let start = self.o.place();
        let opening = self.opening;
        let held = (self.o.helper.as_ref())
            .map(std::sync::Arc::downgrade)
            .unwrap_or_default();
        let setting = self.o.behaviour.text_reader;
        let Some(screen) = self.o.screens.iter_mut().find(|s| !s.read) else {
            return Task::none();
        };
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = &screen.frame
        else {
            screen.read = true;
            return Task::none();
        };
        screen.reading = true;
        let (width, height, pixels) = (*width, *height, pixels.clone());
        let (place, window) = (screen.place, screen.window);
        background(move |tx| {
            let Some(_held) = held.upgrade() else {
                return; // closed meanwhile
            };
            let read = || -> Result<Vec<word::Line>, String> {
                if ocr::route(setting, Some(ocr::system::language())) == ocr::Source::Helper {
                    return Err("Windows OCR: a right-to-left language".into());
                }
                // the closer read only: no quick one first (gently, nobody waits)
                let k = *ocr::system::passes(place.scale, width, height)
                    .last()
                    .unwrap_or(&1);
                let blocks =
                    ocr::system::recognize(&pixels, width, height, k).map_err(|e| e.to_string())?;
                let mut lines = word::lines(blocks, [0, 0], place.scale);
                word::measure(&mut lines, &pixels, width, height, place.scale);
                let (k, at) = into_model(start, place);
                Ok(lines.into_iter().map(|l| l.mapped(k, at)).collect())
            };
            let _ = tx.unbounded_send(read());
        })
        .map(move |read| Message::ScreenLines(opening, window, read))
    }

    /// Another screen's text is back (or failed: it is then read, with none):
    /// a text selection takes it in, and the next screen is read.
    pub(in crate::ui) fn on_screen_lines(
        &mut self,
        window: window::Id,
        read: Result<Vec<word::Line>, String>,
    ) -> Task<Message> {
        let Some(screen) = self.o.screens.iter_mut().find(|s| s.window == window) else {
            return Task::none();
        };
        (screen.read, screen.reading) = (true, false);
        match read {
            Ok(lines) => screen.lines = lines,
            Err(e) => crate::log!("[opencircle] Windows OCR didn't read another screen: {e}"),
        }
        let done = self.more_text();
        Task::batch([done, self.read_more()])
    }

    /// `overlay::settle` on the desk (`Opening::desk`): a scribble's hull is
    /// kept to the screens, not to the starting one.
    pub(in crate::ui) fn settle_on_desk(
        &self,
        zoom: f32,
    ) -> Option<(overlay::Selection, Vec<Point>)> {
        let desk = self.o.desk();
        let at = Vector::new(desk.x, desk.y);
        let stroke: Vec<Point> = self.o.stroke.iter().map(|&p| p - at).collect();
        let (sel, kept) = overlay::settle(&stroke, zoom, desk.size())?;
        let sel = overlay::Selection {
            area: sel.area + at,
            ..sel
        };
        Some((sel, kept.into_iter().map(|p| p + at).collect()))
    }

    /// Another monitor's frame of capture `n` landed: its window opens over it
    /// (no focus: the keys stay with the starting screen). Dropped when it is
    /// late (closed, another capture's) or this opening isn't Circle to Search.
    pub(in crate::ui) fn add_screen(
        &mut self,
        n: u32,
        place: Place,
        frame: image::Handle,
    ) -> Task<Message> {
        let start = self.place();
        let known = (self.o.screens.iter().map(|s| s.place))
            .chain(self.o.waiting.iter().map(|(p, _)| *p))
            .any(|p| p == place);
        if self.o.capture != n
            || !matches!(self.o.purpose, Purpose::Search)
            || place.origin == start.origin
            || known
        {
            return Task::none();
        }
        // The starting screen's opening first: a window opened and drawn
        // meanwhile slows its frames (and its first one) on a small PC.
        if self.appeared() < 1.0 {
            self.o.waiting.push((place, frame));
            return Task::none();
        }
        self.show_screen(place, frame)
    }

    /// A press with other screens open: every screen's frame log starts over.
    pub(in crate::ui) fn clear_screen_frames(&self) {
        if self.o.screens.is_empty() {
            return;
        }
        self.frame_times.borrow_mut().clear();
        for s in &self.o.screens {
            s.frame_times.borrow_mut().clear();
        }
    }

    /// Its release: each screen's frames since the press.
    pub(in crate::ui) fn log_screen_frames(&self) {
        if self.o.screens.is_empty() {
            return;
        }
        super::log_gaps("drag, starting screen", &self.frame_times);
        for s in &self.o.screens {
            let what = format!("drag, screen at {:?}", s.place.origin);
            super::log_gaps(&what, &s.frame_times);
        }
    }

    /// The frames that waited for the opening (`add_screen`), shown now.
    pub(in crate::ui) fn show_waiting(&mut self) -> Task<Message> {
        let waiting = std::mem::take(&mut self.o.waiting);
        Task::batch(
            waiting
                .into_iter()
                .map(|(place, frame)| self.show_screen(place, frame)),
        )
    }

    /// The monitor at `place`'s window over its `frame`.
    fn show_screen(&mut self, place: Place, frame: image::Handle) -> Task<Message> {
        let start = self.place();
        let (id, open) = windows::screen(overlay_window(None));
        self.o.screens.push(Screen::new(id, start, place, frame));
        crate::log!(
            "[opencircle] screen {}x{} at {:?} frozen too",
            place.px.0,
            place.px.1,
            place.origin
        );
        let read = self.read_more(); // its text, once the starting screen's is read
        open.discard()
            .chain(place_window(id, place))
            .chain(window::set_mode(id, Mode::Fullscreen))
            .chain(window::gain_focus(self.overlay))
            .chain(read)
    }

    /// An event from another monitor's window: its points go on in the
    /// starting screen's px (where `on_overlay` takes them), so a stroke, box
    /// or text drag carries on across screens. Zoom and pan are the starting
    /// screen's own.
    pub(in crate::ui) fn on_screen_event(
        &mut self,
        window: window::Id,
        event: overlay::Event,
    ) -> Task<Message> {
        let Some(view) =
            (self.o.screens.iter()).find_map(|s| (s.window == window).then_some(s.view))
        else {
            return Task::none();
        };
        let start = self.o.view;
        let on_start = |p: Point| start.to_screen(view.to_image(p));
        let event = match event {
            overlay::Event::Press { at, shift } => overlay::Event::Press {
                at: on_start(at),
                shift,
            },
            overlay::Event::Move(p) => overlay::Event::Move(on_start(p)),
            overlay::Event::Hover(p) => overlay::Event::Hover(on_start(p)),
            overlay::Event::Zoom { .. }
            | overlay::Event::PanStart(_)
            | overlay::Event::PanMove(_)
            | overlay::Event::PanEnd
            | overlay::Event::ZoomReset => return Task::none(),
            event => event,
        };
        let release = matches!(event, overlay::Event::Release);
        let done = self.on_overlay(event);
        match release {
            // the click gave that window the focus: back to the starting screen
            true if self.o.is_some() => done.chain(window::gain_focus(self.overlay)),
            _ => done,
        }
    }

    /// The pointer came into `window` at `now`: the edge glow follows it there
    /// when it is one of this opening's screens (the bar stays put).
    pub(in crate::ui) fn pointer_on(&mut self, window: window::Id, now: Instant) {
        if self.o.is_none() || !matches!(self.o.purpose, Purpose::Search) {
            return;
        }
        let to = match window {
            w if w == self.overlay => None,
            w if self.o.screens.iter().any(|s| s.window == w) => Some(w),
            _ => return, // the pinned chat, a popup
        };
        self.o.glow.move_to(to, now);
    }

    /// Another monitor's window: its frozen frame, dimmed, whatever of the
    /// selection is on it, and the edge glow while the mouse is there (still:
    /// the breath is the starting screen's). No bar, card, toast or hint:
    /// those stay on the starting screen.
    pub(in crate::ui) fn screen_view<'a>(&'a self, screen: &'a Screen) -> Element<'a, Message> {
        let picked: Vec<Rectangle> = (self.o.text_mode.as_ref())
            .and_then(|m| m.picked.as_ref())
            .map_or_else(Vec::new, word::Picked::rects);
        let overlay = overlay::Overlay {
            stroke: &self.o.stroke,
            selection: self.o.selection,
            others: &self.o.others,
            view: screen.view,
            zooming: false,
            handles: self.handles(),
            opening: self.opening,
            first_draw: &screen.first_draw,
            frame_times: &screen.frame_times,
            hover: false,
            dim: true,
            live_drag: false,
            look: config::look(),
            morph: (self.o.morph.as_ref()).map(|(from, at)| (from.as_slice(), *at)),
            words: (self.o.text_mode.as_ref()).map_or(&[], |m| m.boxes.as_slice()),
            picked,
            text: match self.o.translate {
                None if matches!(self.o.purpose, Purpose::Search) => {
                    self.lines_iter().map(|l| l.bounds).collect()
                }
                _ => Vec::new(),
            },
        };
        let frozen = overlay::Frozen {
            frame: &screen.frame,
            scale: screen.place.scale,
        };
        let window = screen.window;
        let glow = self.o.glow.at(Some(window));
        stack![
            canvas(frozen).width(Fill).height(Fill),
            Element::from(canvas(overlay).width(Fill).height(Fill))
                .map(move |event| Message::ScreenEvent(window, event)),
        ]
        // (last: its coming and going leaves the layers under it as they are)
        .push((glow > 0.0).then(|| {
            let (mode, scale) = (border::Mode::Overlay, screen.place.scale);
            border::view(mode, scale, false, border::Breathe::Off, glow)
        }))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::Trigger;
    use crate::ui::app::message::Shot;
    use crate::ui::kit::testing::shot;

    fn place(origin: (i32, i32), px: (u32, u32), scale: f32) -> Place {
        Place { origin, px, scale }
    }

    fn solid(place: Place, grey: u8) -> image::Handle {
        let (w, h) = place.px;
        image::Handle::from_rgba(w, h, vec![grey; (w * h * 4) as usize])
    }

    /// The starting screen: 640x400 at 100 % at the desktop's origin.
    const START: Place = Place {
        origin: (0, 0),
        px: (640, 400),
        scale: 1.0,
    };

    /// Circle to Search opened on `START`; its capture's number.
    fn opened() -> (App, u32) {
        let mut app = App::idle(window::Id::unique());
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let n = app.capturing.expect("pending");
        let shot = Shot {
            image: solid(START, 200),
            scale: START.scale,
            origin: START.logical_origin(),
            size: START.size(),
            snip: None,
        };
        let _ = app.update(Message::Captured(n, Instant::now(), Ok(shot)));
        assert!(app.o.is_some());
        app.o.appear = Some((Instant::now(), 1.0)); // its opening is over
        (app, n)
    }

    #[test]
    fn every_other_monitor_gets_its_own_window_without_the_glow_or_the_bar() {
        let (mut app, n) = opened();
        // right of it at 150 %, and left of it, higher, at 100 %
        let right = place((640, 0), (480, 300), 1.5);
        let left = place((-320, -100), (320, 200), 1.0);
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        let _ = app.update(Message::Screen(n, left, solid(left, 90)));
        let _ = app.update(Message::Screen(n, left, solid(left, 90))); // twice: one window
        let _ = app.update(Message::Screen(n, START, solid(START, 90))); // its own
        let ids: Vec<_> = app.o.screens.iter().map(|s| s.window).collect();
        assert_eq!(ids.len(), 2, "one window per other screen");
        assert!(ids[0] != ids[1] && !ids.contains(&app.overlay));
        // the other screens: the frozen frame under the dim, nothing else
        for s in &app.o.screens {
            let size = s.place.size();
            let rgba = shot::render(app.view(s.window), size.width as u32, size.height as u32);
            let (pixels, _) = rgba.as_chunks::<4>();
            assert!(pixels.iter().all(|p| *p == pixels[0]), "only the dim");
            assert!(pixels[0][0] < 90, "dimmed: {:?}", pixels[0]);
        }
        // the starting screen: its edge glow and bar
        let rgba = shot::render(app.view(app.overlay), 640, 400);
        let (pixels, _) = rgba.as_chunks::<4>();
        assert!(pixels.iter().any(|p| *p != pixels[320 * 640 + 200]));
    }

    /// The edge glow follows the mouse (owner, 2026-10-28): onto the screen it
    /// comes into, fading out of the one it left and in there, frames only
    /// while it fades; the bar stays where it opened; a popup's window and the
    /// screen it is on already change nothing.
    #[test]
    fn the_glow_follows_the_mouse_onto_another_screen() {
        use std::time::Duration;
        let (mut app, n) = opened();
        // (big enough for the glow: a tiny screen has none)
        let right = place((640, 0), (960, 600), 1.5);
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        let id = app.o.screens[0].window;
        (app.o.bar_now, app.o.reveal) = (Some(app.bar_target()), None);
        let bar = app.bar_at();
        assert!(!app.animating(), "at rest");
        assert_eq!((app.o.glow.at(None), app.o.glow.at(Some(id))), (1.0, 0.0));
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        app.pointer_on(app.overlay, t0);
        assert!(!app.animating(), "where it is already: nothing");
        app.pointer_on(id, t0);
        assert_eq!(app.o.glow.here(), Some(id));
        assert!(app.animating(), "frames while it fades");
        let _ = app.update(Message::Frame(ms(100)));
        let (start, there) = (app.o.glow.at(None), app.o.glow.at(Some(id)));
        assert!(start > 0.0 && start < 1.0, "fading out: {start}");
        assert!(there > 0.0 && there < 1.0, "fading in: {there}");
        let _ = app.update(Message::Frame(ms(300)));
        assert_eq!((app.o.glow.at(None), app.o.glow.at(Some(id))), (0.0, 1.0));
        assert!(!app.animating(), "then at rest: no frames");
        assert_eq!(app.bar_at(), bar, "the bar stays where it opened");
        // drawn there now: more than the dim (a frame as the window draws it)
        type Both = (App, window::Id);
        let view: for<'a> fn(&'a Both) -> Element<'a, Message> = |(app, id)| app.view(*id);
        let no_input = |_| (iced::mouse::Cursor::Unavailable, Vec::new());
        let mut both = (app, id);
        let drawn = crate::ui::frame_times::drawn(
            &mut both,
            right.size(),
            1.5,
            view,
            1,
            true,
            |_, _, _| {},
            no_input,
        );
        let (mut app, id) = both;
        let (pixels, _) = drawn[0].rgba.as_chunks::<4>();
        let (w, h) = right.px;
        let (middle, edge) = (
            pixels[(h / 2 * w + w / 2) as usize],
            pixels[(h / 2 * w + 2) as usize],
        );
        assert!(edge != middle, "its edge glows: {edge:?} {middle:?}");
        // a popup's window (the pinned chat) doesn't move it
        app.pointer_on(window::Id::unique(), ms(400));
        assert_eq!(app.o.glow.here(), Some(id));
        // back: it returns, from where it is
        app.pointer_on(app.overlay, ms(400));
        assert_eq!(app.o.glow.here(), None);
        let _ = app.update(Message::Frame(ms(800)));
        assert_eq!((app.o.glow.at(None), app.o.glow.at(Some(id))), (1.0, 0.0));
    }

    /// The crop's pixels as runs of one grey: (grey, how many) per row.
    fn runs(c: &Crop) -> Vec<Vec<(u8, u32)>> {
        let (px, _) = c.rgba.as_chunks::<4>();
        (px.chunks(c.width as usize))
            .map(|row| {
                let mut out: Vec<(u8, u32)> = Vec::new();
                for p in row {
                    match out.last_mut() {
                        Some((g, n)) if *g == p[0] => *n += 1,
                        _ => out.push((p[0], 1)),
                    }
                }
                out
            })
            .collect()
    }

    #[test]
    fn a_box_drawn_across_screens_is_one_crop_of_both() {
        use overlay::Event::{Move, Press, Release};
        let (mut app, n) = opened();
        // right of it, a screen at 150 % (320 x 200 logical px)
        let right = place((640, 0), (480, 300), 1.5);
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        let id = app.o.screens[0].window;
        // From the starting screen onto the other (its window keeps the
        // pointer: the points go on past its edge).
        let press = |at: Point| Press { at, shift: false };
        for event in [
            press(Point::new(600.0, 100.0)),
            Move(Point::new(700.0, 150.0)),
        ] {
            let _ = app.update(Message::Overlay(event));
        }
        let _ = app.update(Message::Overlay(Release));
        let crop = app.crop().expect("a crop");
        assert_eq!((crop.width, crop.height), (100, 50));
        assert!(runs(&crop).iter().all(|r| *r == [(200, 40), (90, 60)]));
        // From the other screen (its logical px) back onto the starting one:
        // desktop px (700, 30) to (600, 90).
        let local = |x: f32, y: f32| Point::new((x - 640.0) / 1.5, y / 1.5);
        for event in [press(local(700.0, 30.0)), Move(local(600.0, 90.0)), Release] {
            let _ = app.update(Message::ScreenEvent(id, event));
        }
        let area = app.o.selection.expect("selected").area;
        assert!(
            (area.x - 600.0).abs() < 0.01 && (area.y - 30.0).abs() < 0.01,
            "{area:?}"
        );
        let crop = app.crop().expect("a crop");
        assert_eq!((crop.width, crop.height), (100, 60));
        assert!(runs(&crop).iter().all(|r| *r == [(200, 40), (90, 60)]));
        // a box moved onto the other screen stays there (the desk, not the start)
        let desk = app.o.desk();
        assert_eq!((desk.width, desk.height), (1120.0, 400.0)); // 640 + 480 px
        let moved = keep_on(desk, Rectangle::new(Point::new(1100.0, 10.0), area.size()));
        assert!((moved.x + area.width - 1120.0).abs() < 0.01, "{moved:?}");
    }

    #[test]
    fn the_other_screens_open_once_the_starting_ones_opening_is_over() {
        let (mut app, n) = opened();
        app.o.appear = Some((Instant::now(), 0.5)); // still opening
        let right = place((640, 0), (480, 300), 1.5);
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        assert!(app.o.screens.is_empty(), "no window during the opening");
        app.o.appear = Some((Instant::now(), 1.0));
        // the frame clock keeps going for them once the opening is over
        (app.o.bar_now, app.o.reveal) = (Some(app.bar_target()), None);
        assert!(app.animating(), "a frame still comes");
        let _ = app.update(Message::Frame(Instant::now()));
        assert_eq!(app.o.screens.len(), 1, "shown after it");
        assert!(app.o.waiting.is_empty());
    }

    #[test]
    fn esc_closes_every_screen_at_once_and_a_late_frame_is_dropped() {
        let (mut app, n) = opened();
        let right = place((640, 0), (480, 300), 1.5);
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        let other = app.o.screens[0].window;
        windows::CLEARED.with(|c| c.borrow_mut().clear());
        let _ = app.update(Message::Escape);
        assert!(app.o.is_none(), "closed, every frame freed with it");
        let cleared = windows::CLEARED.with(|c| std::mem::take(&mut *c.borrow_mut()));
        assert!(cleared.contains(&other), "its window went with it");
        // a frame landing after Esc, then one of the previous capture: dropped
        let _ = app.update(Message::Screen(n, right, solid(right, 90)));
        assert!(app.o.is_none());
        let (mut app, m) = opened();
        let _ = app.update(Message::Screen(m.wrapping_sub(1), right, solid(right, 90)));
        assert!(app.o.screens.is_empty(), "another capture's frame");
        let _ = app.update(Message::Screen(m, right, solid(right, 90)));
        assert_eq!(app.o.screens.len(), 1);
    }

    fn near(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
    }

    #[test]
    fn every_scale_maps_desktop_px_both_ways_through_one_unit() {
        // (scale, origin): every scale the owner's screens use, origins left of
        // and above the primary (negative) and right of it.
        let scales = [1.0, 1.25, 1.5, 2.0, 2.5];
        let origins = [(0, 0), (-1920, -150), (3840, 200), (-2560, 1080)];
        for &s1 in &scales {
            for &o1 in &origins {
                let start = place(o1, (3840, 2160), s1);
                for &s2 in &scales {
                    for &o2 in &origins {
                        let other = place(o2, (1920, 1080), s2);
                        let view = view_of(start, other);
                        for d in [
                            Point::new(o2.0 as f32, o2.1 as f32),
                            Point::new(o2.0 as f32 + 1919.0, o2.1 as f32 + 1079.0),
                            Point::new(o2.0 as f32 + 333.5, o2.1 as f32 + 77.25),
                        ] {
                            let u = from_desktop(start, d);
                            assert!(near(to_desktop(start, u), d), "{s1} {o1:?} {d:?}");
                            // in the other window: (desktop - its origin) / its scale
                            let local =
                                Point::new((d.x - o2.0 as f32) / s2, (d.y - o2.1 as f32) / s2);
                            let shown = view.to_screen(u);
                            assert!(near(shown, local), "{s1}->{s2} {o1:?}->{o2:?}: {shown:?}");
                            assert!(near(view.to_image(local), u), "back: {s1}->{s2}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn one_screen_is_the_identity_and_screens_meet_at_their_seam() {
        // the laptop at 250 % and, right of it, a 1080p screen at 100 %
        let laptop = place((0, 0), (3840, 2400), 2.5);
        let monitor = place((3840, 300), (1920, 1080), 1.0);
        let own = view_of(laptop, laptop);
        let p = Point::new(123.4, 567.8);
        assert!(near(own.to_screen(p), p) && near(own.to_image(p), p));
        // the monitor's left edge is the laptop's right edge, in one unit
        let area = area_of(laptop, monitor);
        assert!(near(area.position(), Point::new(1536.0, 120.0)), "{area:?}");
        assert!((area.width - 768.0).abs() < 0.01 && (area.height - 432.0).abs() < 0.01);
        assert_eq!(laptop.size().width, area.x);
        // a laptop image px is 2.5 monitor px
        let v = view_of(laptop, monitor);
        let a = v.to_screen(Point::new(1536.0, 120.0));
        let b = v.to_screen(Point::new(1537.0, 120.0));
        assert!(near(a, Point::ORIGIN) && (b.x - a.x - 2.5).abs() < 0.01);
    }
}

#[cfg(test)]
mod text_tests {
    use std::time::Instant;

    use iced::window;

    use super::*;
    use crate::Trigger;
    use crate::ocr::Source;
    use crate::ui::app::message::Shot;

    const START: Place = Place {
        origin: (0, 0),
        px: (640, 400),
        scale: 1.0,
    };
    /// Right of it at 150 %: 320 x 200 logical px, 480 px of the starting unit.
    const RIGHT: Place = Place {
        origin: (640, 0),
        px: (480, 300),
        scale: 1.5,
    };

    fn line(text: &str, x: f32, y: f32, w: f32) -> word::Line {
        word::Line {
            text: text.into(),
            bounds: Rectangle::new(Point::new(x, y), Size::new(w, 20.0)),
            carets: Vec::new(),
        }
    }

    fn solid(place: Place) -> image::Handle {
        let (w, h) = place.px;
        image::Handle::from_rgba(w, h, vec![200; (w * h * 4) as usize])
    }

    /// Two screens open, the starting one's text ("alpha beta", at 500..600)
    /// read by Windows OCR, the right one's not yet.
    fn two_screens() -> (App, window::Id) {
        let mut app = App::idle(window::Id::unique());
        let _ = app.update(Message::Trigger(Trigger::Capture(Instant::now())));
        let n = app.capturing.expect("pending");
        let shot = Shot {
            image: solid(START),
            scale: 1.0,
            origin: START.logical_origin(),
            size: START.size(),
            snip: None,
        };
        let _ = app.update(Message::Captured(n, Instant::now(), Ok(shot)));
        app.o.appear = Some((Instant::now(), 1.0));
        let _ = app.update(Message::Screen(n, RIGHT, solid(RIGHT)));
        app.o.text_by = Some(Source::System);
        let all = Rectangle::with_size(app.o.screen);
        let lines = vec![line("alpha beta", 500.0, 100.0, 100.0)];
        let screen = app.o.screen;
        app.o
            .screen_text
            .add_read(Source::System, all, lines, screen);
        let id = app.o.screens[0].window;
        (app, id)
    }

    /// "gamma delta" as the right screen's OCR gives it: its own logical px.
    fn right_text(app: &mut App, window: window::Id) {
        let (k, at) = into_model(START, RIGHT);
        let own = line("gamma delta", 10.0, 10.0, 100.0);
        let _ = app.update(Message::ScreenLines(
            app.opening,
            window,
            Ok(vec![own.mapped(k, at)]),
        ));
    }

    #[test]
    fn another_screens_lines_land_in_the_starting_screens_px() {
        let (k, at) = into_model(START, RIGHT);
        let l = line("x", 10.0, 10.0, 100.0).mapped(k, at);
        // (10, 10) of the 150 % screen: 15 px in and 640 px over, in the unit
        assert_eq!(
            (l.bounds.x, l.bounds.y, l.bounds.width, l.bounds.height),
            (655.0, 15.0, 150.0, 30.0)
        );
        // the laptop at 250 % starting, a 100 % monitor right of it
        let laptop = Place {
            origin: (0, 0),
            px: (3840, 2400),
            scale: 2.5,
        };
        let monitor = Place {
            origin: (3840, 300),
            px: (1920, 1080),
            scale: 1.0,
        };
        let (k, at) = into_model(laptop, monitor);
        let l = line("x", 0.0, 0.0, 10.0).mapped(k, at);
        assert_eq!(
            (l.bounds.x, l.bounds.y, l.bounds.width),
            (1536.0, 120.0, 4.0)
        );
    }

    #[test]
    fn text_on_every_screen_is_picked_from_its_own_window() {
        let (mut app, id) = two_screens();
        let on_right = Point::new(660.0 + 80.0, 25.0); // in "delta"
        assert!(app.text_known(Point::new(550.0, 110.0)));
        assert!(!app.text_known(on_right), "its text is not read yet");
        assert!(app.text_at(on_right, word::Unit::Word).is_none());
        right_text(&mut app, id);
        assert!(app.text_known(on_right) && app.screens_text());
        // the I-beam is over both screens' text
        assert_eq!(app.lines_iter().count(), 2);
        // a double click in the right window picks the word there
        let local = Point::new((on_right.x - 640.0) / 1.5, on_right.y / 1.5);
        let press = overlay::Event::Press {
            at: local,
            shift: false,
        };
        for event in [press, overlay::Event::Release, press] {
            let _ = app.update(Message::ScreenEvent(id, event));
        }
        let picked = app.o.word.as_ref().expect("a word is picked");
        assert_eq!(picked.text(), "delta");
        let _ = app.update(Message::ScreenEvent(id, overlay::Event::Release));
    }

    #[test]
    fn a_text_drag_crosses_the_seam_in_reading_order() {
        use overlay::Event::{Move, Press, Release};
        let (mut app, id) = two_screens();
        right_text(&mut app, id);
        // from "beta" (the start's) into "gamma" (the right one's)
        let _ = app.update(Message::Overlay(Press {
            at: Point::new(561.0, 110.0),
            shift: false,
        }));
        let _ = app.update(Message::Overlay(Move(Point::new(724.0, 30.0))));
        let _ = app.update(Message::Overlay(Release));
        assert_eq!(app.o.word.as_ref().unwrap().text(), "beta\ngamma");
        // and the other way: from the right screen's window back onto the start
        let local = |x: f32, y: f32| Point::new((x - 640.0) / 1.5, y / 1.5);
        let _ = app.update(Message::ScreenEvent(
            id,
            Press {
                at: local(724.0, 30.0),
                shift: false,
            },
        ));
        let _ = app.update(Message::ScreenEvent(id, Move(local(561.0, 110.0))));
        let _ = app.update(Message::ScreenEvent(id, Release));
        assert_eq!(app.o.word.as_ref().unwrap().text(), "beta\ngamma");
    }

    #[test]
    fn ctrl_a_waits_for_every_screen() {
        let (mut app, id) = two_screens();
        app.o.want_all = true;
        let _ = app.more_text();
        assert!(app.o.want_all, "the right screen is not read yet");
        right_text(&mut app, id);
        assert!(!app.o.want_all);
        assert_eq!(
            app.o.word.as_ref().unwrap().text(),
            "alpha beta\ngamma delta"
        );
    }

    #[test]
    fn the_next_screen_is_read_once_the_starting_screens_text_is() {
        let (mut app, _) = two_screens();
        let _ = app.read_screens();
        assert!(app.o.screens[0].reading, "its read is out");
        // not twice at once
        let _ = app.read_screens();
        let id = app.o.screens[0].window;
        let _ = app.update(Message::ScreenLines(app.opening, id, Err("none".into())));
        let screen = &app.o.screens[0];
        assert!(screen.read && !screen.reading, "failed: read, with no text");
        assert!(app.text_known(Point::new(700.0, 30.0)));
    }

    #[test]
    fn the_chat_hole_and_the_protected_windows_are_judged_per_screen() {
        let (mut app, _) = two_screens();
        // the pinned chat on the right screen: desktop px 700..800 x 50..150
        app.o.chat_hole = Some(Rectangle::new(
            Point::new(700.0, 50.0),
            Size::new(100.0, 100.0),
        ));
        let select = |app: &mut App, area: Rectangle| {
            app.o.selection = Some(overlay::Selection {
                area,
                kind: overlay::Kind::Drag,
            });
        };
        // a box across the seam: one image from desktop x 600, y 20
        select(
            &mut app,
            Rectangle::new(Point::new(600.0, 20.0), Size::new(200.0, 200.0)),
        );
        assert_eq!(app.chat_hole(), Some([100, 30, 200, 130]));
        // wholly on the right screen: its crop starts at desktop (700, 50)
        select(
            &mut app,
            Rectangle::new(Point::new(700.0, 50.0), Size::new(50.0, 50.0)),
        );
        assert_eq!(app.chat_hole(), Some([0, 0, 50, 50]));
        let origin = app.crop_origin(app.crop_area().unwrap()).unwrap();
        assert_eq!(origin, (700, 50));
        // a protected window of the right screen (desktop px), in that crop
        let windows = actions::qr::crop_rects(origin, (50, 50), &[(690, 60, 100, 100)]);
        assert_eq!(windows, vec![[0, 10, 50, 50]]);
        // the starting screen's crop: its own origin, the chat not in it
        select(
            &mut app,
            Rectangle::new(Point::new(10.0, 10.0), Size::new(50.0, 50.0)),
        );
        assert_eq!(app.chat_hole(), None);
        assert_eq!(app.crop_origin(app.crop_area().unwrap()), Some((10, 10)));
    }
}
