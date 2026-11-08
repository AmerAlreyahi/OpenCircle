//! The Steps strip (STEPS_BOARD_SPEC, simplified, owner approved 2026-11-02):
//! a slim column of numbered thumbnails docked at a screen edge, in its own
//! floating window (`Windows::add_step`). Snip's More "Add to steps" puts the
//! capture here and the overlay closes; each next snip's "Add step N" adds
//! the next one. Click a step = copy it, double-click = edit it, hover x =
//! delete, drag = reorder; x on the strip clears it ("Steps cleared · Undo");
//! Copy all = ONE image of every step stacked and numbered (`stack_numbered`).
//!
//! Steps live in memory only and go when the strip is cleared or closed:
//! each keeps its capture as PNG bytes (a screenshot packs to a few % of its
//! raw pixels) and a thumbnail of at most `THUMB` px (92 KB), so 12 steps of
//! 1080p screens hold a few MB instead of 100.
use std::sync::Arc;

use iced::widget::tooltip::Position as Side;
use iced::widget::{Space, column, container, image, mouse_area};
use iced::{Center, Element, Fill, Padding, Point, Size};

use crate::actions::{Crop, encode_png, fit_long_edge};
use crate::ui::kit::button::{self, Kind};
use crate::ui::kit::surface::{Material, surface};
use crate::ui::kit::thumb::{self, Look, PAD, TOP, WELL};
use crate::ui::kit::tokens::radius;
use crate::ui::kit::tooltip::tip;
use crate::ui::kit::{grip, scroll, toast};
use crate::ui::platform::monitor::{self, Place};

/// Steps at most (open point 3: 12).
pub const MAX: usize = crate::snip::annotate::STEPS_MAX;
/// The strip's distance from the screen's edges.
const MARGIN: f32 = 12.0;
/// A press moves this far (logical px) before it drags a step.
const DRAG: f32 = 4.0;
/// A thumbnail's pixels at most: twice the well, sharp at 200 %.
pub const THUMB: (u32, u32) = (192, 120);
/// The window's corners (`floating`).
pub const RADIUS: f32 = radius::TRACK;
/// lucide "copy"
const COPY: char = '\u{e0a2}';
/// "Steps cleared · Undo" stays this long, then the steps are gone.
pub const UNDO_MS: u64 = 5000;

/// One step: its capture (annotated, flattened) as PNG, and its thumbnail.
pub struct Step {
    png: Vec<u8>,
    thumb: image::Handle,
}

impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Step({} B)", self.bytes())
    }
}

impl Step {
    /// Packs `crop` (slow for a big one: off the UI thread).
    pub fn new(crop: &Crop) -> Result<Step, String> {
        let png = encode_png(crop.width, crop.height, &crop.rgba)?;
        Ok(Step {
            png,
            thumb: thumbnail(crop),
        })
    }

    /// The capture again.
    pub fn crop(&self) -> Option<Crop> {
        let frame = crate::capture::decode_png(std::io::Cursor::new(&self.png)).ok()?;
        Some(Crop {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
        })
    }

    /// What it holds in memory (PNG + thumbnail pixels).
    pub fn bytes(&self) -> usize {
        let thumb = match &self.thumb {
            image::Handle::Rgba { pixels, .. } => pixels.len(),
            _ => 0,
        };
        self.png.len() + thumb
    }
}

/// `crop` scaled down (area average) to fit `THUMB`, never up.
fn thumbnail(crop: &Crop) -> image::Handle {
    let k = (THUMB.0 as f32 / crop.width as f32)
        .min(THUMB.1 as f32 / crop.height as f32)
        .min(1.0);
    let (w, h) = (
        ((crop.width as f32 * k).round() as u32).max(1),
        ((crop.height as f32 * k).round() as u32).max(1),
    );
    let src = ::image::ImageBuffer::<::image::Rgba<u8>, &[u8]>::from_raw(
        crop.width,
        crop.height,
        &crop.rgba[..],
    );
    match src {
        Some(src) => {
            let small = ::image::imageops::thumbnail(&src, w, h);
            image::Handle::from_rgba(w, h, small.into_raw())
        }
        None => image::Handle::from_rgba(1, 1, vec![0; 4]),
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// A press on the grip or a bare part: the OS moves the window.
    Grab,
    /// The strip's x: the strip turns into "Steps cleared · Undo".
    Clear,
    /// The pill's Undo.
    Undo,
    CopyAll,
    Hover(usize, bool),
    Press(usize),
    /// The pointer over the strip (window px).
    Move(Point),
    Release,
    /// A double-click: edit the step in Snip.
    Edit(usize),
    Delete(usize),
    Key(Key),
    /// A step is packed and joins the strip, or takes the `i`th's place
    /// (`Windows::add_step`).
    Ready(Arc<Step>, Option<usize>),
    /// The window moved (logical desktop px): it docks once it stops.
    Moved(Point),
    /// No move since the `n`th: dock now.
    Settle(u32),
    /// The same, with the scale its window has by now (it changes on a
    /// monitor of another DPI): dock on the monitor it was left on.
    Docked(u32, f32),
    /// The `n`th clear's undo time is over.
    Expire(u32),
    /// Copy all is on the clipboard (`false`: it failed).
    Copied(bool),
}

/// The strip's keys (while it has the focus, after a click).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    /// Ctrl+Up / Ctrl+Down: move the selected step.
    MoveUp,
    MoveDown,
    Enter,
    Delete,
    Copy,
    CopyAll,
    Undo,
}

/// What the strip asks of its window's owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Let the OS move the window.
    Drag,
    /// Copy this step.
    Copy(usize),
    CopyAll,
    /// Edit this step in Snip.
    Edit(usize),
    /// The strip changed size (resize and place the window).
    Resize,
    /// Cleared: wait `UNDO_MS`, then close.
    Cleared,
    /// It is being moved: settle (`Msg::Settle`) after a pause unless it
    /// moves again.
    Wait(u32),
}

pub struct Strip {
    steps: Vec<Arc<Step>>,
    /// The steps before the last remove or move (Ctrl+Z), or before Clear
    /// (the pill's Undo).
    undo: Option<Vec<Arc<Step>>>,
    hover: Option<usize>,
    /// The keyboard's step.
    focus: Option<usize>,
    /// A press on a step: which, and where.
    press: Option<(usize, Point)>,
    /// A step being dragged: from, to.
    drag: Option<(usize, usize)>,
    cursor: Point,
    /// "Steps cleared · Undo" shows instead of the strip.
    pub cleared: bool,
    /// Clears so far (an old clear's expiry does nothing).
    pub clears: u32,
    /// Moves so far (only the last one's settle docks), and where the last went.
    moves: u32,
    moved_to: Point,
    /// Right to left (Arabic): docks left, mirrored.
    pub rtl: bool,
    /// Docked at the left edge.
    left: bool,
    /// Its top (logical px from the work area's top) once dragged; else centred.
    top: Option<f32>,
    /// Its monitor and the taskbar's share of its bottom (logical px).
    monitor: Place,
    inset: f32,
}

impl Strip {
    /// An empty strip on `monitor` (`inset`: the taskbar's logical px).
    pub fn new(monitor: Place, inset: f32, rtl: bool) -> Strip {
        Strip {
            steps: Vec::new(),
            undo: None,
            hover: None,
            focus: None,
            press: None,
            drag: None,
            cursor: Point::ORIGIN,
            cleared: false,
            clears: 0,
            moves: 0,
            moved_to: Point::ORIGIN,
            rtl,
            left: rtl,
            top: None,
            monitor,
            inset,
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_full(&self) -> bool {
        self.steps.len() >= MAX
    }

    /// Adds `step` at the end (none past `MAX`).
    pub fn push(&mut self, step: Arc<Step>) -> bool {
        if self.is_full() {
            return false;
        }
        self.steps.push(step);
        true
    }

    /// Puts `step` in place of the `i`th (Update step N).
    pub fn replace(&mut self, i: usize, step: Arc<Step>) {
        if let Some(s) = self.steps.get_mut(i) {
            *s = step;
        }
    }

    /// The `i`th step's capture.
    pub fn crop(&self, i: usize) -> Option<Crop> {
        self.steps.get(i)?.crop()
    }

    /// Every step's capture, in order.
    pub fn crops(&self) -> Vec<Crop> {
        self.steps.iter().filter_map(|s| s.crop()).collect()
    }

    /// What the steps hold in memory (with an undo's).
    pub fn bytes(&self) -> usize {
        let held = self.steps.iter().chain(self.undo.iter().flatten());
        let mut seen: Vec<*const Step> = Vec::new();
        held.filter(|s| {
            let p = Arc::as_ptr(s);
            let new = !seen.contains(&p);
            seen.push(p);
            new
        })
        .map(|s| s.bytes())
        .sum()
    }

    fn remember(&mut self) {
        self.undo = Some(self.steps.clone());
    }

    fn remove(&mut self, i: usize) -> Option<Do> {
        if i >= self.steps.len() {
            return None;
        }
        self.remember();
        self.steps.remove(i);
        self.hover = None;
        self.focus = self.focus.filter(|_| !self.steps.is_empty());
        self.focus = self
            .focus
            .map(|f| f.min(self.steps.len().saturating_sub(1)));
        if self.steps.is_empty() {
            return self.clear(); // no empty state: the strip goes (with Undo)
        }
        Some(Do::Resize)
    }

    fn move_step(&mut self, from: usize, to: usize) {
        if from == to || from >= self.steps.len() || to >= self.steps.len() {
            return;
        }
        self.remember();
        let step = self.steps.remove(from);
        self.steps.insert(to, step);
    }

    fn clear(&mut self) -> Option<Do> {
        if !self.cleared {
            if !self.steps.is_empty() {
                self.remember();
            }
            self.steps.clear();
        }
        self.cleared = true;
        self.clears += 1;
        (self.hover, self.focus, self.press, self.drag) = (None, None, None, None);
        Some(Do::Cleared)
    }

    fn undo(&mut self) -> Option<Do> {
        let before = self.undo.take()?;
        self.steps = before;
        self.cleared = false;
        Some(Do::Resize)
    }

    /// The step under `y` (window px) while dragging.
    fn slot_at(&self, y: f32) -> usize {
        let i = ((y - TOP) / (WELL.height + PAD)).floor().max(0.0) as usize;
        i.min(self.steps.len().saturating_sub(1))
    }

    pub fn update(&mut self, msg: Msg) -> Option<Do> {
        match msg {
            Msg::Grab => return Some(Do::Drag),
            Msg::Clear => return self.clear(),
            Msg::Undo => return self.undo(),
            Msg::CopyAll => return (!self.steps.is_empty()).then_some(Do::CopyAll),
            Msg::Hover(i, on) => {
                self.hover = match on {
                    true => Some(i),
                    false => self.hover.filter(|&h| h != i),
                }
            }
            Msg::Press(i) => {
                self.press = Some((i, self.cursor));
                self.focus = Some(i);
            }
            Msg::Move(at) => {
                self.cursor = at;
                if let Some((i, from)) = self.press {
                    let far = (at.x - from.x).hypot(at.y - from.y) >= DRAG;
                    if self.drag.is_some() || far {
                        self.drag = Some((i, self.slot_at(at.y)));
                    }
                }
            }
            Msg::Release => {
                let press = self.press.take();
                if let Some((from, to)) = self.drag.take() {
                    self.move_step(from, to);
                    self.focus = Some(to);
                    return None;
                }
                return press.map(|(i, _)| Do::Copy(i));
            }
            Msg::Edit(i) => {
                // the press before the double-click copied it already; no second copy
                (self.press, self.drag) = (None, None);
                return (i < self.steps.len()).then_some(Do::Edit(i));
            }
            Msg::Delete(i) => {
                self.press = None;
                return self.remove(i);
            }
            Msg::Key(key) => return self.key(key),
            Msg::Moved(at) => {
                self.moves = self.moves.wrapping_add(1);
                self.moved_to = at;
                return Some(Do::Wait(self.moves));
            }
            Msg::Docked(n, k) if n == self.moves && !self.cleared => {
                return self
                    .dock_on(self.moved_to, k, &monitor::places())
                    .then_some(Do::Resize);
            }
            Msg::Settle(_) | Msg::Docked(..) | Msg::Ready(..) | Msg::Expire(_) | Msg::Copied(_) => {
            }
        }
        None
    }

    fn key(&mut self, key: Key) -> Option<Do> {
        let n = self.steps.len();
        if n == 0 {
            return None;
        }
        let at = self.focus.unwrap_or(0).min(n - 1);
        match key {
            Key::Up => self.focus = Some(at.saturating_sub(1)),
            Key::Down => self.focus = Some((at + 1).min(n - 1)),
            Key::MoveUp | Key::MoveDown => {
                let to = match key {
                    Key::MoveUp => at.saturating_sub(1),
                    _ => (at + 1).min(n - 1),
                };
                self.move_step(at, to);
                self.focus = Some(to);
            }
            Key::Enter => return Some(Do::Edit(at)),
            Key::Delete => return self.remove(at),
            // the step picked (a click, the arrows); none: the strip's
            // own Copy button, Copy all
            Key::Copy if self.focus.is_some() => return Some(Do::Copy(at)),
            Key::Copy | Key::CopyAll => return Some(Do::CopyAll),
            Key::Undo => return self.undo(),
        }
        None
    }

    /// The steps in the order shown (a dragged one at its new place).
    fn shown(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.steps.len()).collect();
        if let Some((from, to)) = self.drag {
            let i = order.remove(from);
            order.insert(to, i);
        }
        order
    }

    /// The window's size (logical px): the strip, or the cleared pill.
    pub fn size(&self) -> Size {
        if self.cleared {
            let (label, action) = self.cleared_words();
            return toast::size_with_action(label, action);
        }
        let n = self.steps.len().max(1) as f32;
        let thumbs = n * WELL.height + (n - 1.0) * PAD;
        let room = self.work().height - 2.0 * MARGIN;
        let h = TOP + thumbs + PAD + crate::ui::kit::tokens::size::ICON_BUTTON + PAD;
        Size::new(WELL.width + 2.0 * PAD, h.min(room))
    }

    /// The thumbnails' room (they scroll when the strip would not fit).
    fn thumbs_height(&self) -> f32 {
        self.size().height - TOP - 2.0 * PAD - crate::ui::kit::tokens::size::ICON_BUTTON
    }

    /// Its monitor's work area, logical px from the monitor's origin.
    fn work(&self) -> Size {
        let s = self.monitor.size();
        Size::new(s.width, s.height - self.inset)
    }

    /// Where the window goes: at its edge, 12 px in, centred (or where it was
    /// dragged to), kept inside the work area. Physical px.
    pub fn place(&self) -> Place {
        place(self.monitor, self.work(), self.left, self.top, self.size())
    }

    /// The window stopped at `at` (logical desktop px): dragged there, it
    /// docks to the nearer side and keeps its height from then on (`true`:
    /// place it). Where it was placed already (its own placement settling,
    /// 1 px rounding): nothing changes, it stays centred until dragged.
    ///
    /// First moving to the monitor of `monitors` holding the strip
    /// (`at` is logical px at its window's `scale`): dragged to another
    /// screen it docks to that screen's edge (its work area, its DPI).
    pub fn dock_on(&mut self, at: Point, scale: f32, monitors: &[Place]) -> bool {
        if let Some(m) = monitor::holding(monitors, at, self.size(), scale)
            && m != self.monitor
        {
            self.inset = monitor::taskbar_inset(m.logical_origin(), m.size(), m.scale);
            self.monitor = m;
        }
        let now = self.place().origin;
        let k = scale;
        let here = ((at.x * k).round() as i32, (at.y * k).round() as i32);
        if (here.0 - now.0).abs() <= 1 && (here.1 - now.1).abs() <= 1 {
            return false;
        }
        let origin = Point::new(
            self.monitor.origin.0 as f32 / k,
            self.monitor.origin.1 as f32 / k,
        );
        let mid = at.x - origin.x + self.size().width / 2.0;
        self.left = mid < self.work().width / 2.0;
        self.top = Some(at.y - origin.y);
        true
    }

    /// Where a pill under the strip goes (`size` logical): centred under it,
    /// or over it when there is no room under it. Physical px.
    pub fn under(&self, size: Size) -> Place {
        let strip = self.place();
        let k = self.monitor.scale;
        let (w, h) = (
            (size.width * k).round() as i32,
            (size.height * k).round() as i32,
        );
        let gap = (8.0 * k).round() as i32;
        let x = strip.origin.0 + strip.px.0 as i32 / 2 - w / 2;
        let bottom = self.monitor.origin.1 + ((self.work().height) * k) as i32;
        let below = strip.origin.1 + strip.px.1 as i32 + gap;
        let y = match below + h <= bottom {
            true => below,
            false => strip.origin.1 - gap - h,
        };
        let left = self.monitor.origin.0;
        let right = left + self.monitor.px.0 as i32 - w;
        Place {
            origin: (x.clamp(left, right.max(left)), y),
            px: (w as u32, h as u32),
            scale: k,
        }
    }

    fn cleared_words(&self) -> (&'static str, &'static str) {
        match self.rtl {
            true => ("مُسحت الخطوات", "تراجع"),
            false => ("Steps cleared", "Undo"),
        }
    }

    /// The strip (or the cleared pill), drawn in its window.
    pub fn view(&self, corners: f32) -> Element<'_, Msg> {
        if self.cleared {
            let (label, action) = self.cleared_words();
            let tone = toast::Tone::Neutral;
            return toast::with_action(label, tone, corners, action, Msg::Undo);
        }
        let mut top = vec![
            grip::small(Msg::Grab),
            Space::new().width(Fill).into(),
            tip(thumb::close(Msg::Clear), self.words().clear, Side::Bottom),
        ];
        if self.rtl {
            top.reverse();
        }
        let top = container(iced::widget::Row::with_children(top).align_y(Center)).height(TOP);
        let thumbs = self.shown().into_iter().enumerate().map(|(place, i)| {
            let look = Look {
                hover: self.hover == Some(i) && self.drag.is_none(),
                lifted: self.drag.is_some_and(|(from, _)| from == i),
                focused: self.focus == Some(i) && self.drag.is_none(),
                rtl: self.rtl,
            };
            let picture = thumb::step(
                self.steps[i].thumb.clone(),
                place + 1,
                look,
                Some(Msg::Delete(i)),
            );
            let step = mouse_area(picture)
                .on_press(Msg::Press(i))
                .on_double_click(Msg::Edit(i))
                .on_enter(Msg::Hover(i, true))
                .on_exit(Msg::Hover(i, false));
            tip(step, self.words().step, Side::Bottom)
        });
        let thumbs = scroll::quiet(column(thumbs).spacing(PAD)).height(self.thumbs_height());
        let words = self.words();
        let copy = button::labelled(
            COPY,
            words.copy_all,
            WELL.width,
            Kind::Accent,
            false,
            Some(Msg::CopyAll),
        );
        let strip = column![top, column![thumbs, copy].spacing(PAD)];
        let strip = surface(Material::Widget, corners, strip)
            .padding(Padding {
                top: 0.0,
                ..Padding::new(PAD)
            })
            .width(Fill)
            .height(Fill);
        mouse_area(strip)
            .on_press(Msg::Grab)
            .on_move(Msg::Move)
            .on_release(Msg::Release)
            .into()
    }

    fn words(&self) -> Words {
        match self.rtl {
            true => Words {
                copy_all: "نسخ الكل",
                clear: "مسح الخطوات",
                step: "خطوة · انقر للنسخ · انقر مرتين للتعديل",
            },
            false => Words {
                copy_all: "Copy all",
                clear: "Clear steps",
                step: "Step · click to copy · double-click to edit",
            },
        }
    }
}

struct Words {
    copy_all: &'static str,
    clear: &'static str,
    step: &'static str,
}

/// The strip's window on `monitor` (work area `work`, logical): `size`
/// (logical) at the left or right edge, `MARGIN` in, at `top` or centred,
/// inside the work area. Physical px.
fn place(monitor: Place, work: Size, left: bool, top: Option<f32>, size: Size) -> Place {
    let x = match left {
        true => MARGIN,
        false => work.width - MARGIN - size.width,
    };
    let centred = (work.height - size.height) / 2.0;
    let y = top
        .unwrap_or(centred)
        .min(work.height - MARGIN - size.height)
        .max(MARGIN.min(centred));
    let k = monitor.scale;
    Place {
        origin: (
            monitor.origin.0 + (x * k).round() as i32,
            monitor.origin.1 + (y * k).round() as i32,
        ),
        px: (
            (size.width * k).round() as u32,
            (size.height * k).round() as u32,
        ),
        scale: k,
    }
}

/// Copy all's image (s5): one column on `ground`, 40 px round it; each step
/// = a 44 px `accent` number disc at the start side, 16 px, then the capture
/// (radius 8, a 1 px `rim` edge), 32 px between steps; narrower captures keep
/// to the start side (right to left: the numbers at the right). Natural px,
/// never scaled up; scaled down to 8,000 px on the long side.
pub fn stack_numbered(crops: &[Crop], colours: [[u8; 4]; 3], rtl: bool) -> Option<Crop> {
    use tiny_skia::{FillRule, Paint, Pattern, Pixmap, PixmapPaint, SpreadMode, Transform};
    const SIDE: u32 = 40;
    const DISC: u32 = 44;
    const GAP: u32 = 16;
    const BETWEEN: u32 = 32;
    const CORNER: f32 = 8.0;
    const LONG: u32 = 8000;
    let [ground, accent, rim] = colours;
    let widest = crops.iter().map(|c| c.width).max()?;
    let rows: u32 = crops.iter().map(|c| c.height.max(DISC)).sum();
    let width = 2 * SIDE + DISC + GAP + widest;
    let height = 2 * SIDE + rows + BETWEEN * (crops.len() as u32 - 1);
    let mut pm = Pixmap::new(width, height)?;
    for px in pm.data_mut().as_chunks_mut::<4>().0 {
        *px = [ground[0], ground[1], ground[2], 255];
    }
    let mut y = SIDE;
    for (i, crop) in crops.iter().enumerate() {
        let (disc_x, x) = match rtl {
            false => (SIDE, SIDE + DISC + GAP),
            true => (width - SIDE - DISC, width - SIDE - DISC - GAP - crop.width),
        };
        // opaque captures: their pixels are already premultiplied
        let size = tiny_skia::IntSize::from_wh(crop.width, crop.height)?;
        let picture = Pixmap::from_vec(crop.rgba.clone(), size)?;
        let frame = iced::Rectangle::new(
            Point::new(x as f32, y as f32),
            Size::new(crop.width as f32, crop.height as f32),
        );
        if let Some(path) = crate::snip::annotate::rounded_path(frame, CORNER) {
            let paint = Paint {
                shader: Pattern::new(
                    picture.as_ref(),
                    SpreadMode::Pad,
                    tiny_skia::FilterQuality::Nearest,
                    1.0,
                    Transform::from_translate(x as f32, y as f32),
                ),
                ..Paint::default()
            };
            pm.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
            let mut edge = Paint::default();
            edge.set_color_rgba8(rim[0], rim[1], rim[2], rim[3]);
            edge.anti_alias = true;
            let stroke = tiny_skia::Stroke {
                width: 1.0,
                ..Default::default()
            };
            pm.stroke_path(&path, &edge, &stroke, Transform::identity(), None);
        } else {
            pm.draw_pixmap(
                x as i32,
                y as i32,
                picture.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        let r = DISC as f32 / 2.0;
        let at = Point::new(disc_x as f32 + r, y as f32 + r);
        pm = crate::snip::annotate::badge(pm, at, r, i as u32 + 1, accent);
        y += crop.height.max(DISC) + BETWEEN;
    }
    let rgba = pm.data().to_vec(); // opaque: premultiplied = straight
    Some(fit_long_edge(
        Crop {
            width,
            height,
            rgba,
        },
        LONG,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::testing::golden;
    use crate::ui::kit::themes::{self, DAYLIGHT, SAPPHIRE};

    /// A 1080p monitor at 100 %, 40 px taskbar.
    const MONITOR: Place = Place {
        origin: (0, 0),
        px: (1920, 1080),
        scale: 1.0,
    };

    /// A capture of `w` x `h` in one colour, a darker band across it.
    fn capture(w: u32, h: u32, shade: u8) -> Crop {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for _ in 0..w {
                let v = if y % 20 < 4 { shade / 2 } else { shade };
                rgba.extend([v, v.saturating_add(30), 255 - v, 255]);
            }
        }
        Crop {
            width: w,
            height: h,
            rgba,
        }
    }

    fn step(w: u32, h: u32, shade: u8) -> Arc<Step> {
        Arc::new(Step::new(&capture(w, h, shade)).unwrap())
    }

    fn strip(n: usize) -> Strip {
        let mut s = Strip::new(MONITOR, 40.0, false);
        for i in 0..n {
            assert!(s.push(step(400 + 40 * i as u32, 250, (40 + 15 * i) as u8)));
        }
        s
    }

    /// Which capture each place holds (by its width).
    fn order(s: &Strip) -> Vec<u32> {
        s.crops().iter().map(|c| c.width).collect()
    }

    /// Where windows can be placed (X11, Windows) the strip sits at its edge, clear of
    /// a pin opened at the screen centre. Wayland (GNOME) places every new toplevel
    /// itself, centred, and gives an app no say: there both overlap (known, no fix).
    #[test]
    fn the_strip_at_its_edge_is_clear_of_a_centred_pin() {
        let p = strip(1).place();
        let cx = MONITOR.px.0 as i32 / 2;
        assert!(
            p.origin.0 > cx + 300,
            "right of a 600 px wide pin at the centre"
        );
    }

    #[test]
    fn sizes_and_place_follow_the_spec() {
        // 1 step 104 x 124, 4 steps 104 x 316
        assert_eq!(strip(1).size(), Size::new(104.0, 124.0));
        assert_eq!(strip(4).size(), Size::new(104.0, 316.0));
        // at the right edge, 12 px in, centred in the work area (1040 high)
        let p = strip(1).place();
        assert_eq!(p.origin, (1920 - 12 - 104, (1040 - 124) / 2));
        // right to left: the left edge
        let mut rtl = Strip::new(MONITOR, 40.0, true);
        rtl.push(step(10, 10, 1));
        assert_eq!(rtl.place().origin.0, 12);
        // 12 steps on a 720 px screen: the strip fits, its thumbnails scroll
        let small = Place {
            px: (1280, 720),
            ..MONITOR
        };
        let mut s = Strip::new(small, 40.0, false);
        for _ in 0..MAX {
            s.push(step(8, 8, 1));
        }
        assert_eq!(s.size().height, 680.0 - 24.0);
        assert_eq!(s.place().origin.1, 12);
    }

    /// Copy all sits inside the window at every scale and step count, and the
    /// physical window is the logical size times the scale.
    #[test]
    fn copy_all_always_fits_the_window() {
        let icon = crate::ui::kit::tokens::size::ICON_BUTTON;
        for k in [1.0, 1.25, 1.5, 2.0] {
            for n in 1..=MAX {
                let mut s = strip(n);
                s.monitor.scale = k;
                s.monitor.px = ((1280.0 * k) as u32, (720.0 * k) as u32);
                let h = s.size().height;
                assert!(
                    TOP + s.thumbs_height() + PAD + icon + PAD <= h + 0.001,
                    "k {k} n {n}"
                );
                let p = s.place();
                assert_eq!(p.px.1, (h * k).round() as u32, "k {k} n {n}");
            }
        }
    }

    /// Dragged near the other edge it docks there and keeps its height.
    #[test]
    fn a_dragged_strip_docks_to_the_nearer_edge() {
        let mut s = strip(2);
        // its own placement settling: it stays centred as it grows (VM: it
        // grew down from a pinned top before)
        let p = s.place().origin;
        assert!(!s.dock_on(Point::new(p.0 as f32, p.1 as f32), 1.0, &[]));
        s.push(step(8, 8, 1));
        assert_eq!(s.place().origin.1, (1040 - 252) / 2);
        s.update(Msg::Delete(2));
        assert!(s.dock_on(Point::new(300.0, 200.0), 1.0, &[]));
        let p = s.place();
        assert_eq!(p.origin, (12, 200));
        // its own placement settling: no second move
        assert!(!s.dock_on(Point::new(12.0, 200.0), 1.0, &[]));
        // dragged off the bottom: kept inside
        s.dock_on(Point::new(1500.0, 5000.0), 1.0, &[]);
        let p = s.place();
        assert_eq!(p.origin, (1920 - 12 - 104, 1040 - 12 - 188));
    }

    /// Dragged onto another monitor (another DPI) it docks to THAT monitor's
    /// edge and work area, not back to the one it opened on (#125).
    #[test]
    fn a_strip_dragged_to_another_screen_docks_there() {
        let mut s = strip(2);
        let right = Place {
            origin: (1920, 0),
            px: (3840, 2160),
            scale: 2.0,
        };
        let monitors = [s.monitor, right];
        // logical px at 200 %: its middle (x 1100 -> 2200 + 52 px) is on `right`
        assert!(s.dock_on(Point::new(1100.0, 300.0), 2.0, &monitors));
        assert_eq!(s.monitor, right);
        let p = s.place();
        // the near (left) edge of the right monitor, 12 logical px in, at 200 %
        assert_eq!(p.origin, (1920 + 24, 600));
        assert_eq!(p.scale, 2.0);
        // and back
        assert!(s.dock_on(Point::new(500.0, 300.0), 1.0, &monitors));
        assert_eq!(s.place().origin, (12, 300));
    }

    #[test]
    fn add_stops_at_the_limit() {
        let mut s = strip(MAX - 1);
        assert!(!s.is_full());
        assert!(s.push(step(8, 8, 1)));
        assert!(s.is_full());
        assert!(!s.push(step(8, 8, 1)), "13th refused");
        assert_eq!(s.len(), MAX);
    }

    #[test]
    fn click_copies_double_click_edits_drag_reorders() {
        let mut s = strip(3);
        // a click
        s.update(Msg::Move(Point::new(50.0, 40.0)));
        s.update(Msg::Press(0));
        assert_eq!(s.update(Msg::Release), Some(Do::Copy(0)));
        // a double-click: the second press edits, its release copies nothing
        s.update(Msg::Press(1));
        assert_eq!(s.update(Msg::Edit(1)), Some(Do::Edit(1)));
        assert_eq!(s.update(Msg::Release), None);
        // a drag of step 3 to the top (past the 4 px threshold)
        let y3 = TOP + 2.0 * (WELL.height + PAD) + 30.0;
        s.update(Msg::Move(Point::new(50.0, y3)));
        s.update(Msg::Press(2));
        s.update(Msg::Move(Point::new(50.0, y3 - 2.0)));
        assert!(s.drag.is_none(), "under the threshold: still a click");
        s.update(Msg::Move(Point::new(50.0, TOP + 10.0)));
        assert_eq!(s.drag, Some((2, 0)));
        assert_eq!(s.shown(), vec![2, 0, 1], "renumbered live");
        assert_eq!(s.update(Msg::Release), None, "a drag copies nothing");
        assert_eq!(order(&s), vec![480, 400, 440]);
        // Ctrl+Z puts it back
        s.update(Msg::Key(Key::Undo));
        assert_eq!(order(&s), vec![400, 440, 480]);
    }

    #[test]
    fn delete_and_clear_undo() {
        let mut s = strip(3);
        assert_eq!(s.update(Msg::Delete(1)), Some(Do::Resize));
        assert_eq!(order(&s), vec![400, 480]);
        s.update(Msg::Key(Key::Undo));
        assert_eq!(order(&s), vec![400, 440, 480]);
        // clear: the pill, then Undo brings every step back
        assert_eq!(s.update(Msg::Clear), Some(Do::Cleared));
        assert!(s.cleared && s.len() == 0);
        assert_eq!(s.update(Msg::Undo), Some(Do::Resize));
        assert!(!s.cleared);
        assert_eq!(order(&s), vec![400, 440, 480]);
        // deleting the last step clears the strip (no empty state)
        let mut one = strip(1);
        assert_eq!(one.update(Msg::Delete(0)), Some(Do::Cleared));
        assert!(one.cleared);
    }

    #[test]
    fn keys_walk_move_and_act() {
        let mut s = strip(3);
        // nothing picked: Ctrl+C is the strip's Copy button (Copy all)
        assert_eq!(s.update(Msg::Key(Key::Copy)), Some(Do::CopyAll));
        s.update(Msg::Key(Key::Down));
        s.update(Msg::Key(Key::MoveDown));
        assert_eq!(order(&s), vec![400, 480, 440]);
        assert_eq!(s.update(Msg::Key(Key::Enter)), Some(Do::Edit(2)));
        assert_eq!(s.update(Msg::Key(Key::Copy)), Some(Do::Copy(2)));
        assert_eq!(s.update(Msg::Key(Key::CopyAll)), Some(Do::CopyAll));
        assert_eq!(s.update(Msg::Key(Key::Delete)), Some(Do::Resize));
        assert_eq!(s.len(), 2);
    }

    /// A step keeps its capture exactly, in far less memory than its pixels;
    /// cleared steps go once the undo is gone.
    #[test]
    fn steps_are_packed_and_freed() {
        let crop = capture(1920, 1080, 120);
        let raw = crop.rgba.len();
        let s = Arc::new(Step::new(&crop).unwrap());
        assert_eq!(s.crop().unwrap().rgba, crop.rgba, "lossless");
        assert!(s.bytes() < raw / 20, "{} of {raw} B", s.bytes());
        assert!(
            matches!(
                &s.thumb,
                image::Handle::Rgba {
                    width: 192,
                    height: 108,
                    ..
                }
            ),
            "thumbnail fits 192 x 120"
        );
        let weak = Arc::downgrade(&s);
        let mut strip = Strip::new(MONITOR, 40.0, false);
        strip.push(s);
        strip.update(Msg::Clear);
        assert!(weak.upgrade().is_some(), "kept for Undo");
        drop(strip); // the pill's time is over: the window and its strip go
        assert!(weak.upgrade().is_none(), "freed");
    }

    /// Copy all: one column, numbers at the start side, 40 px round, 32
    /// between, natural size; right to left mirrored.
    #[test]
    fn copy_all_stacks_numbered_steps() {
        let crops = [capture(300, 100, 200), capture(200, 30, 60)];
        let colours = [[11, 18, 32, 255], [56, 189, 248, 255], [40, 50, 70, 255]];
        let img = stack_numbered(&crops, colours, false).unwrap();
        let (w, h) = (40 + 44 + 16 + 300 + 40, 40 + 100 + 32 + 44 + 40);
        assert_eq!((img.width, img.height), (w, h));
        let px = |x: u32, y: u32| {
            let i = ((y * img.width + x) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        };
        assert_eq!(px(5, 5), [11, 18, 32], "ground");
        assert_eq!(px(40 + 8, 40 + 22), [56, 189, 248], "disc 1 at the start");
        assert_eq!(px(100 + 150, 40 + 50), [200, 230, 55], "step 1");
        assert_eq!(px(100 + 100, 172 + 15), [60, 90, 195], "step 2 under it");
        assert_eq!(
            px(100 + 250, 172 + 15),
            [11, 18, 32],
            "narrower: start side"
        );
        let rtl = stack_numbered(&crops, colours, true).unwrap();
        let i = ((40 + 22) * w + (w - 40 - 8)) as usize * 4;
        assert_eq!(rtl.rgba[i..i + 3], [56, 189, 248], "disc at the right");
        // huge: scaled to 8,000 px on the long side
        let tall: Vec<Crop> = (0..3).map(|_| capture(10, 3000, 90)).collect();
        let img = stack_numbered(&tall, colours, false).unwrap();
        assert_eq!(img.height, 8000);
    }

    /// The strip with 1 and 4 steps, a step under the cursor, one lifted,
    /// the keyboard's, the cleared pill: Sapphire and Daylight, Latin and
    /// Arabic.
    #[test]
    fn the_strip_matches_its_golden_images() {
        let mut failed = Vec::new();
        for theme in [&SAPPHIRE, &DAYLIGHT] {
            themes::set(theme);
            for rtl in [false, true] {
                let make = |n: usize| {
                    let mut s = strip(n);
                    s.rtl = rtl;
                    s
                };
                let mut hover = make(4);
                hover.update(Msg::Hover(1, true));
                let mut lifted = make(4);
                lifted.update(Msg::Press(3));
                lifted.update(Msg::Move(Point::new(50.0, TOP + 70.0)));
                let mut focused = make(4);
                focused.update(Msg::Key(Key::Down));
                let mut cleared = make(2);
                cleared.update(Msg::Clear);
                let cases = [
                    ("one", make(1)),
                    ("four", make(4)),
                    ("hover", hover),
                    ("drag", lifted),
                    ("focus", focused),
                    ("cleared", cleared),
                ];
                let dir = if rtl { "-ar" } else { "" };
                for (name, s) in cases {
                    let file = format!("steps-{name}{dir}-{}", theme.id);
                    failed.extend(golden::check("screens", &file, s.size(), &[1.0], || {
                        s.view(RADIUS)
                    }));
                }
            }
        }
        themes::set(&SAPPHIRE);
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
