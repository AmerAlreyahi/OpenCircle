//! Object editing (Studio v3, S1/S5/S7): a selection of several items, the
//! object clipboard, duplicate / nudge / order / merge, and options that
//! follow the selection. Every change is ONE undo step. Only the Studio
//! (`Annotator::studio`) uses it; Snip's quick widget keeps one selected item.
use std::sync::Mutex;

use iced::{Point, Rectangle, Size, Vector};

use super::document::{Image, Item, Pic, Shape, span};
use super::{Annotator, Document, Edit, Output, Tool, TextStyle, Width, select, style};
use crate::shared::picture::Crop;

/// Where a pasted copy lands, per paste (image px).
const OFFSET: f32 = 10.0;

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
#[allow(dead_code)] // read by the Studio's bar (W87)
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
}

static CLIPBOARD: Mutex<Option<Copied>> = Mutex::new(None);

/// A group drag: the items as they were, and which move.
pub(super) struct GroupDrag {
    pub from: Point,
    pub before: Vec<Item>,
    pub idx: Vec<usize>,
}

impl Annotator {
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
            self.doc.exec(Edit::Items(before, after));
        }
        self.doc.select(sel);
        self.refresh();
    }

    fn picked(&self) -> Vec<Item> {
        (self.doc.selection().iter())
            .map(|&i| self.doc.items[i].clone())
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
                (c.items.clone(), c.pastes)
            })
        };
        self.tool = Tool::Select;
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
                };
                self.append(vec![item]);
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
        let sel: Vec<usize> = (self.doc.selection().into_iter())
            .filter(|&i| matches!(self.doc.items[i].shape, Shape::Picture { .. }))
            .collect();
        if sel.is_empty() {
            return;
        }
        let img = &self.doc.image;
        let layers: Vec<_> = sel.iter().map(|&i| (&self.doc.items[i], true)).collect();
        let whole = tiny_skia::IntRect::from_xywh(0, 0, img.width, img.height);
        let rgba: Option<Vec<u8>> = whole.map(|r| {
            super::document::paint(img, &layers, r)
                .pixels()
                .iter()
                .flat_map(|p| {
                    let c = p.demultiply();
                    [c.red(), c.green(), c.blue(), c.alpha()]
                })
                .collect()
        });
        let mut edits = Vec::new();
        if let Some(patch) = rgba.and_then(|rows| self.doc.rows_patch(0, &rows)) {
            edits.push(Edit::Pixels(Box::new(patch)));
        }
        for &i in sel.iter().rev() {
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
            if from.width > 0.0 { to.width / from.width } else { 1.0 },
            if from.height > 0.0 { to.height / from.height } else { 1.0 },
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
                let z = self.zoom;
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
        self.shown = Some(self.current_style());
    }

    /// The options bar's values for the selection (`None`: nothing selected).
    #[allow(dead_code)] // read by the Studio's bar (W87)
    pub fn selection_style(&self) -> Option<SelectionStyle> {
        let sel = self.doc.selection();
        let items: Vec<&Item> = sel.iter().map(|&i| &self.doc.items[i]).collect();
        let first = items.first()?;
        let z = self.zoom;
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

    /// An options change with a selection: the values that CHANGED since the
    /// bar showed them go to every selected item (one undo step); mixed
    /// values the user did not touch stay as they are.
    pub(super) fn restyle_selection(&mut self, styled: &dyn Fn(&Item, &Annotator) -> Item) {
        let sel = self.doc.selection();
        let now = self.current_style();
        let old = self.shown.clone().unwrap_or_else(|| now.clone());
        if sel.is_empty() || old == now {
            return;
        }
        let sizes = (old.stroke, old.width, old.text_size, old.text_style, old.blur_size, old.blur_cell)
            != (now.stroke, now.width, now.text_size, now.text_style, now.blur_size, now.blur_cell);
        let mut after = self.doc.items.clone();
        for &i in &sel {
            let before = &self.doc.items[i];
            let mut item = if sizes && !matches!(before.shape, Shape::Picture { .. }) {
                styled(before, self)
            } else {
                before.clone()
            };
            item.colour = if old.colour != now.colour && !matches!(before.shape, Shape::Picture { .. }) {
                now.colour
            } else {
                before.colour
            };
            if old.outline == now.outline {
                item.outline = before.outline;
            }
            if old.fill == now.fill {
                item.fill = before.fill;
            }
            after[i] = item;
        }
        self.shown = Some(now);
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

/// `a` and `b` are the same picture, give or take what a trip through the
/// system clipboard changes (same size, sampled pixels within a few levels).
fn similar(a: &Crop, b: &Crop) -> bool {
    if (a.width, a.height) != (b.width, b.height) || a.rgba.len() != b.rgba.len() {
        return false;
    }
    let n = a.rgba.len() / 4;
    (0..64.min(n)).all(|k| {
        let i = (k * n / 64.min(n)) * 4;
        let (p, q) = (&a.rgba[i..i + 4], &b.rgba[i..i + 4]);
        p[3].abs_diff(q[3]) <= 2 && (p[3] < 200 || (0..3).all(|c| p[c].abs_diff(q[c]) <= 4))
    })
}
