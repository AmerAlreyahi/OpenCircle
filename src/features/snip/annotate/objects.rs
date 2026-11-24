//! Object editing (Studio v3, S1/S5/S7): a selection of several items, the
//! object clipboard, duplicate / nudge / order / merge, and options that
//! follow the selection. Every change is ONE undo step. Only the Studio
//! (`Annotator::studio`) uses it; Snip's quick widget keeps one selected item.
use std::sync::Mutex;

use iced::keyboard::{Key, Modifiers, key::Named};
use iced::{Point, Rectangle, Size, Vector};

use super::document::{Image, Item, Pic, Shape, span};
use super::{Annotator, Document, Edit, Message, Output, TextStyle, Tool, Width, select, style};
use crate::shared::picture::Crop;

/// Where a pasted copy lands, per paste (image px).
const OFFSET: f32 = 10.0;

/// What the Studio's key table says about a key.
pub(crate) enum StudioKey {
    /// This editor message.
    Do(Message),
    /// Not the editor's: leave the key for the window.
    Ignore,
    /// Not a Studio editing key: the editor's own table goes on.
    Pass,
}

/// The canvas never grows past this side (px), area (64 MP = 256 MB of
/// RGBA) or this many times the picture's own area.
pub(super) const GROW_SIDE: f64 = 16384.0;
pub(super) const GROW_PIXELS: f64 = 64.0 * 1024.0 * 1024.0;
pub(super) const GROW_TIMES: f64 = 4.0;
/// The history keeps the old picture of every growth: all of them together may hold
/// this many bytes; a growth past it is refused (the picture is never cut).
pub(super) const GROW_HISTORY: usize = 512 * 1024 * 1024;

/// Why "Grow the canvas past the edge" said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrowRefusal {
    /// A side would pass the limit (or the position is not a number).
    TooLarge,
    /// The area would pass the memory budget or four times the picture.
    TooManyPixels,
}

/// A z-order move (Ctrl+] / Ctrl+[, with Shift = to the front / back).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Forward,
    Backward,
    Front,
    Back,
}

/// The options bar's values (the next drawing's, or the selection's).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawStyle {
    pub(super) colour: [u8; 4],
    pub(super) width: Width,
    pub(super) stroke: f32,
    pub(super) outline: bool,
    pub(super) fill: bool,
    pub(super) text_size: f32,
    pub(super) text_style: TextStyle,
    pub(super) blur_size: Option<Width>,
    pub(super) blur_cell: Width,
}

/// What the options bar shows for the selection; `None` = mixed.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionStyle {
    pub count: usize,
    pub colour: Option<[u8; 4]>,
    /// Line width, display px.
    pub stroke: Option<f32>,
    pub outline: Option<bool>,
    pub fill: Option<bool>,
    /// Opacity (0-255) when every selected item is a pasted picture.
    pub opacity: Option<u8>,
}

/// The object clipboard: the items copied last and the PNG that went to the
/// system clipboard with them (a paste whose picture looks like it = ours).
struct Copied {
    items: Vec<Item>,
    png: Crop,
    pastes: u32,
    /// Copied by Ctrl+X: the first paste goes back in place.
    cut: bool,
}

/// The pixels the process holds for pasting (the last Studio window closed).
pub fn clear_clipboard() {
    *CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

static CLIPBOARD: Mutex<Option<Copied>> = Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    v: u32,
    items: Vec<Item>,
}

/// What an options message changes (see `Annotator::touch`).
pub(super) mod touch {
    pub const COLOUR: u8 = 1;
    pub const STROKE: u8 = 2;
    pub const TEXT: u8 = 4;
    pub const BLUR: u8 = 8;
    pub const OUTLINE: u8 = 16;
    pub const FILL: u8 = 32;

    use super::super::Message as M;

    pub fn of(m: &M) -> u8 {
        match m {
            M::Colour(_) | M::Hex(_) => COLOUR,
            M::Width(_) => STROKE | TEXT,
            M::Stroke(_) => STROKE,
            M::TextSize(_) | M::TextStyle(_) => TEXT,
            M::BlurSize(_) | M::BlurCell(_) => BLUR,
            M::Outline(_) => OUTLINE,
            M::Fill(_) => FILL,
            M::Preset(_) => COLOUR | STROKE | TEXT,
            _ => 0,
        }
    }
}

/// A group drag: the items as they were, and which move.
pub(super) struct GroupDrag {
    pub from: Point,
    pub before: Vec<Item>,
    pub idx: Vec<usize>,
}

impl Annotator {
    /// The editor message of a Studio key, if it has one.
    #[cfg(test)]
    pub(crate) fn studio_key(
        &self,
        key: &Key,
        latin: Option<char>,
        m: Modifiers,
    ) -> Option<Message> {
        match self.studio_keys(key, latin, m) {
            StudioKey::Do(message) => Some(message),
            _ => None,
        }
    }

    /// What a Studio key does in the editor (typing and open menus are
    /// handled before this). Esc ladder here: selection, then the tool back
    /// to Select, then the key is left to the window (its menus, sheets).
    pub(crate) fn studio_keys(&self, key: &Key, latin: Option<char>, m: Modifiers) -> StudioKey {
        let ch = latin.or(match key {
            Key::Character(s) => s.chars().next(),
            _ => None,
        });
        let ch = ch.map(|c| c.to_ascii_lowercase());
        let sel = !self.selection().is_empty();
        let go = |m: Message| StudioKey::Do(m);
        if let Key::Named(named) = key {
            return match named {
                Named::Escape if self.picking || self.menu.is_some() => StudioKey::Pass,
                Named::Escape if sel => go(Message::Deselect),
                Named::Escape if self.tool != Tool::Select => go(Message::Tool(Tool::Select)),
                Named::Escape => StudioKey::Ignore,
                // the window's Save and Add to steps
                Named::Enter if m.command() => go(Message::AddToSteps),
                Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown
                    if sel && !m.command() =>
                {
                    let arrow = match named {
                        Named::ArrowLeft => (-1, 0),
                        Named::ArrowRight => (1, 0),
                        Named::ArrowUp => (0, -1),
                        _ => (0, 1),
                    };
                    go(Message::Shove(arrow, m.shift()))
                }
                _ => StudioKey::Pass,
            };
        }
        if !m.command() {
            return StudioKey::Pass;
        }
        let order =
            |shift: bool, fwd: Order, to: Order| go(Message::Order(if shift { to } else { fwd }));
        match (ch, m.shift()) {
            (Some('c'), false) => go(Message::CopySelection),
            (Some('c'), true) => go(Message::Copy),
            (Some('x'), false) => go(Message::CutSelection),
            (Some('d'), false) => go(Message::Duplicate),
            (Some('a'), false) => go(Message::SelectAll),
            // Save as is the window's (it opens the Picture sheet)
            (Some('s'), true) => StudioKey::Ignore,
            (Some(']' | '}'), shift) => order(shift, Order::Forward, Order::Front),
            (Some('[' | '{'), shift) => order(shift, Order::Backward, Order::Back),
            _ => StudioKey::Pass,
        }
    }

    /// Selected items' indices, ascending.
    pub fn selection(&self) -> Vec<usize> {
        self.doc.selection()
    }

    /// The box round the selected items (image px).
    pub fn selection_bounds(&self) -> Option<Rectangle> {
        let sel = self.doc.selection();
        let mut all = sel.iter().map(|&i| self.doc.items[i].frame());
        let first = all.next()?;
        Some(all.fold(first, |a, b| a.union(&b)))
    }

    /// The rubber band being dragged (display px), for the canvas.
    pub fn band_rect(&self) -> Option<Rectangle> {
        let (a, b) = self.band?;
        Some(self.to_display(span(&[a, b])))
    }

    fn select_set(&mut self, all: &[usize]) {
        self.commit();
        self.doc.select(all);
        self.refresh();
    }

    pub(super) fn select_all(&mut self) {
        let all: Vec<usize> = (0..self.doc.items.len()).collect();
        self.select_set(&all);
    }

    /// Shift+click: adds `i` to the selection or takes it out.
    pub(super) fn toggle_selected(&mut self, i: usize) {
        let mut sel = self.doc.selection();
        match sel.iter().position(|&s| s == i) {
            Some(at) => {
                sel.remove(at);
            }
            None => sel.push(i),
        }
        self.doc.select(&sel);
    }

    /// Replaces the items with `after` as one undo step and selects `sel`.
    // ponytail: the whole list is kept per step (items are small); a diff edit
    // if documents with thousands of objects ever matter.
    fn set_items(&mut self, after: Vec<Item>, sel: &[usize]) {
        self.commit();
        if after != self.doc.items {
            let before = self.doc.items.clone();
            let frames: Vec<Rectangle> = (after.iter().filter(|i| !i.hidden))
                .map(Item::frame)
                .collect();
            let Ok(grow) = self.grow_for(&frames) else {
                self.refresh(); // refused: nothing changed
                return;
            };
            let edit = Edit::Items(before, after);
            self.doc.exec(match grow {
                Some(grow) => Edit::Seq(vec![edit, grow]),
                None => edit,
            });
        }
        self.doc.select(sel);
        self.refresh();
    }

    fn picked(&self) -> Vec<Item> {
        (self.doc.selection().iter())
            .map(|&i| self.doc.items[i].clone())
            .filter(|i| !i.hidden)
            .collect()
    }

    /// Ctrl+C / Ctrl+X: the selected objects (our format + a PNG of just them
    /// for other apps); nothing selected: the whole picture when the setting
    /// allows. The host puts the `Output::Copy` on the system clipboard.
    pub(super) fn copy_selection(&mut self, cut: bool) -> Option<Output> {
        self.commit();
        let items = self.picked();
        if items.is_empty() {
            return (self.copy_picture_when_empty && !cut)
                .then(|| Output::Copy(super::render(&self.doc)));
        }
        let png = objects_png(&items)?;
        *CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(Copied {
            items,
            png: png.clone(),
            pastes: 0,
            cut,
        });
        if cut {
            self.delete_selection();
        }
        Some(Output::Copy(png))
    }

    /// Ctrl+V with a picture `clip` from the system clipboard: our own copied
    /// objects when it is the PNG they went out with, else a movable pasted
    /// picture. `false`: no picture (the host tries text).
    pub fn paste_clipboard(&mut self, clip: Option<Crop>) -> bool {
        let Some(clip) = clip else {
            return false;
        };
        self.commit();
        let ours = {
            let mut held = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
            held.as_mut().filter(|c| similar(&c.png, &clip)).map(|c| {
                c.pastes += 1;
                // after Ctrl+X the first paste lands where they were
                (c.items.clone(), c.pastes - u32::from(c.cut))
            })
        };
        self.pick_tool(Tool::Select);
        match ours {
            Some((items, n)) => {
                let by = Vector::new(OFFSET, OFFSET) * n as f32;
                let moved: Vec<Item> = items.iter().map(|i| select::moved(i, by)).collect();
                self.append(moved);
            }
            None => {
                let view = self.doc.view();
                let mut size = Size::new(clip.width as f32, clip.height as f32);
                let k = (0.8 * view.width / size.width)
                    .min(0.8 * view.height / size.height)
                    .min(1.0);
                size = size * k;
                let a = view.center() - Vector::new(size.width, size.height) * 0.5;
                let item = Item {
                    shape: Shape::Picture {
                        a,
                        b: a + Vector::new(size.width, size.height),
                        pic: Pic::new(clip.width, clip.height, &clip.rgba),
                        lock: true,
                    },
                    colour: [255, 255, 255, 255],
                    width: 0.0,
                    outline: false,
                    fill: false,
                    hidden: false,
                };
                if self.paste_movable {
                    self.append(vec![item]);
                } else {
                    // "Pasted pictures stay movable" off: into the picture at once
                    // (centred in the view, so it never needs the canvas to grow)
                    let edits: Vec<Edit> = (self.bake(&[&item]))
                        .map(|p| Edit::Pixels(Box::new(p)))
                        .into_iter()
                        .collect();
                    if !edits.is_empty() {
                        self.doc.exec(Edit::Seq(edits));
                        self.refresh();
                    }
                }
            }
        }
        true
    }

    /// `new` on top of the others, selected.
    fn append(&mut self, new: Vec<Item>) {
        let mut after = self.doc.items.clone();
        let first = after.len();
        after.extend(new);
        let sel: Vec<usize> = (first..after.len()).collect();
        self.set_items(after, &sel);
    }

    /// A right-click or an Objects row: select what is at `p` (`add`: Shift,
    /// toggles it); an empty spot deselects unless `add`. Right-clicking one
    /// of several selected keeps them all.
    pub(super) fn select_at(&mut self, p: Point, add: bool) {
        self.commit();
        let hit = select::hit(&self.doc.items, p, super::REACH / self.zoom);
        match hit {
            Some(i) => self.select_index(i, add),
            None if !add => self.doc.deselect(),
            None => {}
        }
    }

    pub(super) fn select_index(&mut self, i: usize, add: bool) {
        if i >= self.doc.items.len() {
            return;
        }
        self.commit();
        self.pick_tool(Tool::Select);
        if add {
            self.toggle_selected(i);
        } else if !self.doc.selection().contains(&i) {
            self.doc.select_one(Some(i));
        }
    }

    /// The Objects list's eye: shows / hides item `i` (one undo step).
    pub(super) fn toggle_hidden(&mut self, i: usize) {
        self.commit();
        let Some(before) = self.doc.items.get(i).cloned() else {
            return;
        };
        let after = Item {
            hidden: !before.hidden,
            ..before.clone()
        };
        self.doc.exec(Edit::Change(i, before, after));
        self.refresh();
    }

    /// Dragging an Objects row: item `from` goes to place `to` (one undo step;
    /// it stays selected).
    pub(super) fn reorder_to(&mut self, from: usize, to: usize) {
        let n = self.doc.items.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let mut after = self.doc.items.clone();
        let moved = after.remove(from);
        after.insert(to, moved);
        self.set_items(after, &[to]);
    }

    /// The drawings as JSON (crash recovery keeps these, not a flat picture).
    pub fn annotations_json(&self) -> String {
        serde_json::to_string(&Saved {
            v: 1,
            items: self.doc.items.clone(),
        })
        .unwrap_or_default()
    }

    /// Puts the saved drawings on the picture (one undo step). `false`: the
    /// text is not ours (nothing changes).
    pub fn restore_annotations(&mut self, json: &str) -> bool {
        let Ok(saved) = serde_json::from_str::<Saved>(json) else {
            return false;
        };
        if saved.v != 1 {
            return false;
        }
        self.set_items(saved.items, &[]);
        true
    }

    /// "Grow the canvas past the edge" (on): the edit that makes the picture
    /// big enough for `frames` (new pixels see-through), `Ok(None)` when they
    /// fit, the setting is off or the picture is cropped. To the left or top
    /// it grows the other way and the drawings move with the picture. A
    /// growth over the budget (`GROW_SIDE`, `GROW_PIXELS`, `GROW_TIMES`) or
    /// from a non-finite position is refused, and the reason is kept for the
    /// window (`take_grow_refusal`): the picture is never cut.
    pub(super) fn grow_for(&mut self, frames: &[Rectangle]) -> Result<Option<Edit>, ()> {
        if !self.grow_canvas || self.doc.crop.is_some() {
            return Ok(None);
        }
        let img = &self.doc.image;
        let (w, h) = (f64::from(img.width), f64::from(img.height));
        let (mut x0, mut y0, mut x1, mut y1) = (0.0f64, 0.0f64, w, h);
        for f in frames {
            let edge = [f.x, f.y, f.x + f.width, f.y + f.height];
            if edge.iter().any(|v| !v.is_finite()) {
                self.grow_refused = Some(GrowRefusal::TooLarge);
                return Err(());
            }
            (x0, y0) = (
                x0.min(f64::from(f.x).floor()),
                y0.min(f64::from(f.y).floor()),
            );
            (x1, y1) = (
                x1.max(f64::from(f.x + f.width).ceil()),
                y1.max(f64::from(f.y + f.height).ceil()),
            );
        }
        if (x0, y0, x1, y1) == (0.0, 0.0, w, h) {
            return Ok(None);
        }
        // x0 <= 0 and x1 >= w here: the new canvas always holds the old picture
        let (nw, nh) = (x1 - x0, y1 - y0);
        if nw > GROW_SIDE || nh > GROW_SIDE {
            self.grow_refused = Some(GrowRefusal::TooLarge);
            return Err(());
        }
        let kept = self.doc.canvas_bytes() + img.rgba.len();
        if nw * nh > self.grow_pixels || nw * nh > GROW_TIMES * w * h || kept > self.grow_history {
            self.grow_refused = Some(GrowRefusal::TooManyPixels);
            return Err(());
        }
        Ok(Some(Edit::Canvas {
            old: Box::new(img.clone()),
            size: (nw as u32, nh as u32),
            by: Vector::new((-x0) as f32, (-y0) as f32),
            undo: false,
        }))
    }

    /// A drag began on a shape with a drawing tool: true once per such press, and
    /// only the first time ever (`general.ctrl_hint_seen`, written here); the host
    /// then shows the "Ctrl+drag moves" pill.
    pub fn take_ctrl_hint(&mut self) -> bool {
        if !std::mem::take(&mut self.ctrl_hint)
            || crate::shared::config::current().general.ctrl_hint_seen
        {
            return false;
        }
        crate::shared::config::update_in_background(|c| c.general.ctrl_hint_seen = true);
        true
    }

    /// Why the last growth was refused (once): the window says so in a pill.
    pub fn take_grow_refusal(&mut self) -> Option<GrowRefusal> {
        self.grow_refused.take()
    }

    /// The patch that bakes `layers` (in order) into the picture.
    fn bake(&self, layers: &[&Item]) -> Option<super::document::Patch> {
        let img = &self.doc.image;
        let layers: Vec<_> = layers.iter().map(|i| (*i, true)).collect();
        let whole = tiny_skia::IntRect::from_xywh(0, 0, img.width, img.height)?;
        let rgba: Vec<u8> = super::document::paint(img, &layers, whole, self.doc.seed)
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        self.doc.rows_patch(0, &rgba)
    }

    /// Ctrl+D.
    pub(super) fn duplicate(&mut self) {
        let by = Vector::new(OFFSET, OFFSET);
        let copies: Vec<Item> = self.picked().iter().map(|i| select::moved(i, by)).collect();
        if !copies.is_empty() {
            self.append(copies);
        }
    }

    /// Delete.
    pub(super) fn delete_selection(&mut self) {
        let sel = self.doc.selection();
        if sel.is_empty() {
            return;
        }
        let after: Vec<Item> = (self.doc.items.iter().enumerate())
            .filter(|(i, _)| !sel.contains(i))
            .map(|(_, item)| item.clone())
            .collect();
        self.set_items(after, &[]);
    }

    /// Arrow keys: the selection moves 1 px (`far`: 10 px) of the picture.
    pub(super) fn shove(&mut self, (dx, dy): (i8, i8), far: bool) {
        let sel = self.doc.selection();
        if sel.is_empty() {
            return;
        }
        let by = Vector::new(f32::from(dx), f32::from(dy)) * if far { 10.0 } else { 1.0 };
        let mut after = self.doc.items.clone();
        for &i in &sel {
            after[i] = select::moved(&after[i], by);
        }
        self.set_items(after, &sel);
    }

    /// Ctrl+] / Ctrl+[ (with Shift: to the front / back).
    pub(super) fn reorder(&mut self, how: Order) {
        let sel = self.doc.selection();
        let mut all: Vec<(Item, bool)> = (self.doc.items.iter().cloned().enumerate())
            .map(|(i, item)| (item, sel.contains(&i)))
            .collect();
        match how {
            Order::Front => all.sort_by_key(|(_, s)| *s),
            Order::Back => all.sort_by_key(|(_, s)| !*s),
            Order::Forward => {
                for i in (0..all.len().saturating_sub(1)).rev() {
                    if all[i].1 && !all[i + 1].1 {
                        all.swap(i, i + 1);
                    }
                }
            }
            Order::Backward => {
                for i in 1..all.len() {
                    if all[i].1 && !all[i - 1].1 {
                        all.swap(i, i - 1);
                    }
                }
            }
        }
        let now: Vec<usize> = (all.iter().enumerate())
            .filter(|(_, (_, s))| *s)
            .map(|(i, _)| i)
            .collect();
        self.set_items(all.into_iter().map(|(item, _)| item).collect(), &now);
    }

    /// "Merge into picture": the selected pasted pictures become part of the
    /// picture's own pixels (one undo step).
    pub(super) fn merge(&mut self) {
        self.commit();
        // what the user saw: everything under the topmost merged picture goes
        // into the picture with it (drawings above it stay objects)
        let Some(top) = (self.doc.selection().into_iter())
            .filter(|&i| matches!(self.doc.items[i].shape, Shape::Picture { .. }))
            .max()
        else {
            return;
        };
        let layers: Vec<&Item> = self.doc.items[..=top].iter().collect();
        let patch = self.bake(&layers);
        let mut edits = Vec::new();
        if let Some(patch) = patch {
            edits.push(Edit::Pixels(Box::new(patch)));
        }
        for i in (0..=top).rev() {
            edits.push(Edit::Delete(i, self.doc.items[i].clone()));
        }
        self.doc.exec(Edit::Seq(edits));
        self.refresh();
    }

    /// The Properties panel's X / Y / W / H: the selection's box becomes
    /// `to` (image px); a pasted picture with its ratio locked keeps it.
    pub fn set_selection_bounds(&mut self, to: Rectangle) {
        let (Some(from), sel) = (self.selection_bounds(), self.doc.selection()) else {
            return;
        };
        let (sx, sy) = (
            if from.width > 0.0 {
                to.width / from.width
            } else {
                1.0
            },
            if from.height > 0.0 {
                to.height / from.height
            } else {
                1.0
            },
        );
        let mut after = self.doc.items.clone();
        for &i in &sel {
            let locked = matches!(after[i].shape, Shape::Picture { lock: true, .. });
            let k = sx.abs().max(sy.abs());
            let (kx, ky) = if locked { (k, k) } else { (sx, sy) };
            after[i].shape.transform(
                |p| Point::new(to.x + (p.x - from.x) * kx, to.y + (p.y - from.y) * ky),
                ky.abs().max(0.05),
            );
        }
        self.set_items(after, &sel);
    }

    /// A pasted picture's opacity (0-255) or ratio lock, for the selection.
    pub fn set_picture(&mut self, opacity: Option<u8>, lock: Option<bool>) {
        let sel = self.doc.selection();
        let mut after = self.doc.items.clone();
        for &i in &sel {
            let item = &mut after[i];
            if let Shape::Picture { lock: l, .. } = &mut item.shape {
                if let Some(v) = lock {
                    *l = v;
                }
                if let Some(o) = opacity {
                    item.colour[3] = o;
                }
            }
        }
        self.set_items(after, &sel);
    }

    /// Alt+drag: a copy of the item under the press is what moves.
    pub(super) fn press_copy(&mut self, p: Point) {
        self.commit();
        let hit = select::hit(&self.doc.items, p, super::REACH / self.zoom);
        match hit {
            Some(i) => {
                let copy = self.doc.items[i].clone();
                self.doc.deselect();
                self.begin(
                    None,
                    copy.clone(),
                    super::Drag::Move {
                        from: copy,
                        edits: false,
                    },
                    p,
                );
            }
            None => self.press(p),
        }
    }

    /// A press on a member of a several-item selection starts moving them all.
    pub(super) fn start_group(&mut self, p: Point) {
        self.group = Some(GroupDrag {
            from: p,
            before: self.doc.items.clone(),
            idx: self.doc.selection(),
        });
    }

    pub(super) fn drag_group(&mut self, p: Point) {
        let Some(g) = &self.group else {
            return;
        };
        let by = if p.distance(g.from) * self.zoom < super::SLOP {
            Vector::ZERO
        } else {
            p - g.from
        };
        let mut items = g.before.clone();
        for &i in &g.idx {
            items[i] = select::moved(&g.before[i], by);
        }
        if items != self.doc.items {
            self.doc.items = items;
            // ponytail: the whole picture is repainted per move; paint only the
            // union of the old and new boxes if a big document drags slowly.
            self.refresh();
        }
    }

    pub(super) fn end_group(&mut self) {
        let Some(g) = self.group.take() else {
            return;
        };
        let after = std::mem::replace(&mut self.doc.items, g.before.clone());
        let sel = g.idx;
        self.set_items(after, &sel);
    }

    pub(super) fn cancel_group(&mut self) {
        if let Some(g) = self.group.take() {
            self.doc.items = g.before;
            self.refresh();
        }
    }

    /// The rubber band ends: what its box touches is selected.
    pub(super) fn end_band(&mut self) {
        let Some((a, b)) = self.band.take() else {
            return;
        };
        let r = span(&[a, b]);
        let hit: Vec<usize> = (self.doc.items.iter().enumerate())
            .filter(|(_, item)| r.intersects(&item.frame()))
            .map(|(i, _)| i)
            .collect();
        self.doc.select(&hit);
    }

    // ---- options follow the selection ----

    pub(crate) fn current_style(&self) -> DrawStyle {
        DrawStyle {
            colour: self.colour,
            width: self.width,
            stroke: self.stroke,
            outline: self.outline,
            fill: self.fill,
            text_size: self.text_size,
            text_style: self.text_style,
            blur_size: self.blur_size,
            blur_cell: self.blur_cell,
        }
    }

    /// The next drawing's style (what is remembered): the stash while a
    /// selection shows its own values.
    pub(crate) fn drawing_style(&self) -> DrawStyle {
        self.stash.clone().unwrap_or_else(|| self.current_style())
    }

    fn apply_style(&mut self, d: &DrawStyle) {
        self.colour = d.colour;
        self.hex = super::hex(d.colour);
        self.width = d.width;
        self.stroke = d.stroke;
        self.outline = d.outline;
        self.fill = d.fill;
        self.text_size = d.text_size;
        self.text_style = d.text_style;
        self.blur_size = d.blur_size;
        self.blur_cell = d.blur_cell;
    }

    /// The selection changed: the options show the picked item's values (the
    /// next drawing's are kept aside); nothing picked: they come back.
    pub(super) fn sync_style(&mut self) {
        let sel = self.doc.selection();
        let primary = self.doc.selected.filter(|&i| i < self.doc.items.len());
        match (sel.is_empty(), primary) {
            (false, Some(i)) => {
                if self.stash.is_none() {
                    self.stash = Some(self.current_style());
                }
                let item = self.doc.items[i].clone();
                let z = self.size_zoom();
                self.colour = item.colour;
                self.hex = super::hex(item.colour);
                self.width = style::level(&item, z);
                self.stroke = match item.shape {
                    Shape::Highlighter(_) => item.width * z / 4.0,
                    _ => item.width * z,
                };
                self.outline = item.outline;
                self.fill = item.fill;
                match &item.shape {
                    Shape::Text { size, style, .. } => {
                        (self.text_size, self.text_style) = (size * z, *style)
                    }
                    Shape::Callout { size, .. } | Shape::Label { size, .. } => {
                        self.text_size = size * z / 0.75
                    }
                    _ => {}
                }
            }
            _ => {
                if let Some(d) = self.stash.take() {
                    self.apply_style(&d);
                }
            }
        }
    }

    /// The options bar's values for the selection (`None`: nothing selected).
    pub fn selection_style(&self) -> Option<SelectionStyle> {
        let sel = self.doc.selection();
        let items: Vec<&Item> = sel.iter().map(|&i| &self.doc.items[i]).collect();
        let first = items.first()?;
        let z = self.size_zoom();
        let same = |f: &dyn Fn(&Item) -> bool| items.iter().all(|i| f(i) == f(first));
        let pics = items
            .iter()
            .all(|i| matches!(i.shape, Shape::Picture { .. }));
        Some(SelectionStyle {
            count: items.len(),
            colour: same(&|i| i.colour == first.colour).then_some(first.colour),
            stroke: same(&|i| i.width == first.width).then_some(first.width * z),
            outline: same(&|i| i.outline == first.outline).then_some(first.outline),
            fill: same(&|i| i.fill == first.fill).then_some(first.fill),
            opacity: (pics && same(&|i| i.colour[3] == first.colour[3])).then_some(first.colour[3]),
        })
    }

    /// An options change with a selection: only what the user touched
    /// (`touch`) goes to every selected item, in one undo step; everything
    /// else, mixed values included, stays as it is.
    pub(super) fn restyle_selection(&mut self, styled: &dyn Fn(&Item, &Annotator) -> Item) {
        let sel = self.doc.selection();
        let t = std::mem::take(&mut self.touch);
        if sel.is_empty() || t == 0 {
            return;
        }
        let now = self.current_style();
        let mut after = self.doc.items.clone();
        for &i in &sel {
            let before = &self.doc.items[i];
            if matches!(before.shape, Shape::Picture { .. }) {
                continue; // its colour is its opacity (Properties)
            }
            let full = styled(before, self);
            let mut item = before.clone();
            if t & touch::COLOUR != 0 {
                item.colour = now.colour;
            }
            if t & touch::OUTLINE != 0 {
                item.outline = now.outline;
            }
            if t & touch::FILL != 0 {
                item.fill = now.fill;
            }
            // sizes: each shape takes only the size its own option sets
            match &before.shape {
                Shape::Text { .. } | Shape::Callout { .. } | Shape::Label { .. }
                    if t & touch::TEXT != 0 =>
                {
                    item.shape = full.shape.clone();
                }
                Shape::Blur { .. } if t & touch::BLUR != 0 => item.shape = full.shape.clone(),
                Shape::Step { .. } | Shape::Pointer { .. } if t & touch::STROKE != 0 => {
                    item.shape = full.shape.clone();
                    if matches!(before.shape, Shape::Pointer { .. }) {
                        item.width = full.width;
                    }
                }
                _ => {}
            }
            let has_line = !matches!(
                before.shape,
                Shape::Text { .. }
                    | Shape::Label { .. }
                    | Shape::Step { .. }
                    | Shape::Pointer { .. }
                    | Shape::Blur { .. }
            );
            if t & touch::STROKE != 0 && has_line {
                item.width = full.width;
            }
            after[i] = item;
        }
        self.set_items(after, &sel);
    }
}

/// A PNG's worth (RGBA) of just `items`, on see-through, cut to their box.
// ponytail: a blur or spotlight has no picture under it here; they copy as
// drawn on nothing.
fn objects_png(items: &[Item]) -> Option<Crop> {
    let first = items.first()?.bounds();
    let bounds = items.iter().fold(first, |a, i| a.union(&i.bounds()));
    let (x, y) = (bounds.x.floor(), bounds.y.floor());
    let w = ((bounds.x + bounds.width).ceil() - x).clamp(1.0, 8192.0) as u32;
    let h = ((bounds.y + bounds.height).ceil() - y).clamp(1.0, 8192.0) as u32;
    let mut doc = Document::new(Image {
        width: w,
        height: h,
        rgba: vec![0; w as usize * h as usize * 4],
    });
    doc.legend = false;
    let by = Vector::new(-x, -y);
    doc.items = items.iter().map(|i| select::moved(i, by)).collect();
    Some(super::render(&doc))
}

/// Is `got` (read back from the system clipboard) the PNG `ours` that went
/// out? Same size, and either the pixels agree (sampled, alpha too) or the
/// clipboard dropped the alpha and `ours` on white or black gives `got`.
/// Needs enough opaque samples in `ours` to tell pictures apart: an all
/// see-through picture proves nothing.
pub(super) fn similar(ours: &Crop, got: &Crop) -> bool {
    if (ours.width, ours.height) != (got.width, got.height) || ours.rgba.len() != got.rgba.len() {
        return false;
    }
    const SAMPLES: usize = 256;
    let n = ours.rgba.len() / 4;
    let at = |k: usize| (k * n / SAMPLES.min(n)) * 4;
    let ks = 0..SAMPLES.min(n);
    let (solid, to) = (|p: &[u8]| p[3] >= 200, |c: u8, d: u8| c.abs_diff(d));
    if ks
        .clone()
        .filter(|&k| solid(&ours.rgba[at(k)..at(k) + 4]))
        .count()
        < 4
    {
        return false;
    }
    let same = ks.clone().all(|k| {
        let (p, q) = (&ours.rgba[at(k)..at(k) + 4], &got.rgba[at(k)..at(k) + 4]);
        to(p[3], q[3]) <= 2 && (!solid(p) || (0..3).all(|c| to(p[c], q[c]) <= 4))
    });
    // alpha dropped: ours over white / black
    let flat = |bg: u32| {
        ks.clone().all(|k| {
            let (p, q) = (&ours.rgba[at(k)..at(k) + 4], &got.rgba[at(k)..at(k) + 4]);
            let a = u32::from(p[3]);
            q[3] >= 253
                && (0..3).all(|c| {
                    let v = (u32::from(p[c]) * a + bg * (255 - a)) / 255;
                    v.abs_diff(u32::from(q[c])) <= 6
                })
        })
    };
    same || flat(255) || flat(0)
}
