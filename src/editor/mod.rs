//! The annotation editor: ONE component behind both entry points, Edit in place
//! (the overlay's action bar turns into its toolbar) and Snip mode.
//!
//! Model: `Document` (image, items, selection, crop, undo stack), flattened by the
//! pure `render`. UI: `Editor` = the image with a canvas over it (input + selection
//! chrome) and the floating toolbar. One file per tool.
//!
//! Smooth on big crops: the flattened document (`base`) is cached and repainted
//! once per change; while something is drawn, moved or typed, only its own
//! region is repainted (`live`) and shown over the base.
mod arrow;
mod blur;
mod callout;
mod canvas;
mod crop;
mod document;
mod highlighter;
mod magnifier;
mod pen;
mod select;
mod shape;
mod spotlight;
mod step;
#[cfg(test)]
mod tests;
mod text;
mod toolbar;

pub use document::{Document, Edit, Image, Item, Shape, render};

use iced::widget::{column, image, pin, stack};
use iced::{ContentFit, Element, Point, Rectangle, Size};

use crate::actions::Crop;
use document::int_rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Crop,
    Rect,
    Ellipse,
    Arrow,
    Pen,
    Highlighter,
    Text,
    Callout,
    Step,
    Blur,
    Spotlight,
    Magnifier,
}

pub struct ToolInfo {
    pub tool: Tool,
    /// Lucide glyph (listed in `assets/lucide/subset.sh`).
    pub icon: char,
    /// Tooltip = the button's description, with its key.
    pub tip: &'static str,
    /// Its key (no Ctrl) while the editor is open.
    pub key: char,
    /// Toolbar group: a divider goes between groups.
    pub group: u8,
}

/// Every tool, in toolbar order.
pub const TOOLS: &[ToolInfo] = &[
    tool(Tool::Select, '\u{e1c2}', "Select, move, resize (V)", 'v', 0),
    tool(Tool::Crop, '\u{e0af}', "Crop (X)", 'x', 0),
    tool(Tool::Rect, '\u{e16a}', "Rectangle (R)", 'r', 1),
    tool(Tool::Ellipse, '\u{e07a}', "Ellipse (O)", 'o', 1),
    tool(
        Tool::Arrow,
        '\u{e497}',
        "Arrow: drag the middle to curve it (A)",
        'a',
        1,
    ),
    tool(
        Tool::Pen,
        '\u{e133}',
        "Pen: rough boxes, circles, lines and arrows snap clean (P)",
        'p',
        1,
    ),
    tool(Tool::Highlighter, '\u{e0f7}', "Highlighter (H)", 'h', 1),
    tool(Tool::Text, '\u{e197}', "Text (T)", 't', 2),
    tool(Tool::Callout, '\u{e11a}', "Callout (C)", 'c', 2),
    tool(Tool::Step, '\u{e1d0}', "Numbered step (N)", 'n', 2),
    tool(Tool::Blur, '\u{e503}', "Blur (B)", 'b', 3),
    tool(Tool::Spotlight, '\u{e29d}', "Spotlight (S)", 's', 3),
    tool(Tool::Magnifier, '\u{e1b5}', "Magnifier (M)", 'm', 3),
];

const fn tool(tool: Tool, icon: char, tip: &'static str, key: char, group: u8) -> ToolInfo {
    ToolInfo {
        tool,
        icon,
        tip,
        key,
        group,
    }
}

/// Stroke weight, in display px (as seen on screen, whatever the zoom).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Thin,
    Medium,
    Thick,
}

impl Width {
    pub const ALL: [Width; 3] = [Width::Thin, Width::Medium, Width::Thick];
    fn stroke(self) -> f32 {
        [3.0, 5.0, 9.0][self as usize]
    }
    fn font(self) -> f32 {
        [18.0, 26.0, 38.0][self as usize]
    }
    fn badge(self) -> f32 {
        [12.0, 16.0, 22.0][self as usize]
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Pointer, image px (from the canvas).
    Press(Point),
    Move(Point),
    Release,
    /// Typing into a text or callout.
    Type(String),
    Backspace,
    /// Ends typing.
    Finish,
    Tool(Tool),
    /// Opens/closes the colour picker.
    Picker,
    Colour([u8; 4]),
    Width(Width),
    /// The picker's custom colour field.
    Hex(String),
    Undo,
    Redo,
    Delete,
    Copy,
    Save,
    Back,
}

/// What the editor asks of its host.
pub enum Output {
    /// Back to the action bar (edits are kept in the editor).
    Back,
    /// The flattened image, for the Copy / Save actions.
    Copy(Crop),
    Save(Crop),
}

/// An item being drawn, moved, resized or typed: shown live, committed as one edit.
struct Pending {
    /// The document item it replaces; `None` = new.
    index: Option<usize>,
    item: Item,
    drag: Drag,
    start: Point,
    /// The base is painted without the item (it changed since it was picked up).
    hidden: bool,
}

enum Drag {
    Draw,
    /// Last pointer position.
    Move(Point),
    Resize(usize, Item),
    Bend(Item),
    Type,
}

/// Handles react this close to the pointer (display px).
const REACH: f32 = 10.0;

pub struct Editor {
    pub doc: Document,
    pub tool: Tool,
    colour: [u8; 4],
    width: Width,
    picker: bool,
    hex: String,
    /// Display px per image px.
    zoom: f32,
    base: image::Handle,
    /// The pending item's region (image px) painted with it.
    live: Option<(Rectangle, image::Handle)>,
    pending: Option<Pending>,
    /// Crop drag: from, to.
    cropping: Option<(Point, Point)>,
}

impl Editor {
    /// Opens `image`, shown at `zoom` display px per image px (e.g. 1 / the
    /// monitor's scale factor, so the crop shows at its on-screen size).
    pub fn new(image: Image, zoom: f32) -> Self {
        let colour = crate::ui::theme::ANNOTATION[0].into_rgba8();
        let mut editor = Editor {
            doc: Document::new(image),
            tool: Tool::Arrow,
            colour,
            width: Width::Medium,
            picker: false,
            hex: hex(colour),
            zoom,
            base: image::Handle::from_rgba(1, 1, vec![0; 4]),
            live: None,
            pending: None,
            cropping: None,
        };
        editor.refresh();
        editor
    }

    pub fn update(&mut self, message: Message) -> Option<Output> {
        match message {
            Message::Press(p) => self.press(p),
            Message::Move(p) => self.drag_to(p),
            Message::Release => self.release(),
            Message::Type(s) => self.edit_text(|t| t.push_str(&s)),
            Message::Backspace => self.edit_text(|t| {
                t.pop();
            }),
            Message::Finish => self.commit(),
            Message::Tool(tool) => {
                self.commit();
                self.tool = tool;
                self.picker = false;
            }
            Message::Picker => self.picker = !self.picker,
            Message::Colour(c) => {
                self.colour = c;
                self.hex = hex(c);
                self.restyle();
            }
            Message::Width(w) => {
                self.width = w;
                self.restyle();
            }
            Message::Hex(s) => {
                if let Some(c) = parse_hex(&s) {
                    self.colour = c;
                    self.restyle();
                }
                self.hex = s;
            }
            Message::Undo | Message::Redo => {
                self.commit();
                let undo = matches!(message, Message::Undo);
                if if undo {
                    self.doc.undo()
                } else {
                    self.doc.redo()
                } {
                    self.refresh();
                }
            }
            Message::Delete => {
                self.commit();
                if let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) {
                    let item = self.doc.items[i].clone();
                    self.doc.exec(Edit::Delete(i, item));
                    self.refresh();
                }
            }
            Message::Copy | Message::Save => {
                self.commit();
                let flat = render(&self.doc);
                return Some(if matches!(message, Message::Copy) {
                    Output::Copy(flat)
                } else {
                    Output::Save(flat)
                });
            }
            Message::Back => {
                self.commit();
                return Some(Output::Back);
            }
        }
        None
    }

    /// The canvas (image + annotations, takes the pointer) above the toolbar.
    pub fn view(&self) -> Element<'_, Message> {
        column![self.canvas(), self.toolbar()]
            .spacing(12)
            .align_x(iced::Center)
            .into()
    }

    /// The editing surface alone, `size()` big: for Edit in place, over the selection.
    pub fn canvas(&self) -> Element<'_, Message> {
        let size = self.size();
        let view = self.doc.view();
        let mut layers: Vec<Element<'_, Message>> = vec![
            image(self.base.clone())
                .width(size.width)
                .height(size.height)
                .content_fit(ContentFit::Fill)
                .into(),
        ];
        if let Some((r, handle)) = &self.live {
            let at = Point::new((r.x - view.x) * self.zoom, (r.y - view.y) * self.zoom);
            layers.push(
                pin(image(handle.clone())
                    .width(r.width * self.zoom)
                    .height(r.height * self.zoom)
                    .content_fit(ContentFit::Fill))
                .position(at)
                .into(),
            );
        }
        layers.push(
            iced::widget::canvas(canvas::Input { editor: self })
                .width(size.width)
                .height(size.height)
                .into(),
        );
        stack(layers).width(size.width).height(size.height).into()
    }

    /// The floating editing toolbar (with the colour picker under it when open).
    pub fn toolbar(&self) -> Element<'_, Message> {
        toolbar::view(self)
    }

    /// The canvas size, display px (changes with the crop).
    pub fn size(&self) -> Size {
        self.doc.view().size() * self.zoom
    }

    /// Text or a callout is being typed into.
    fn typing(&self) -> bool {
        matches!(
            self.pending,
            Some(Pending {
                drag: Drag::Type,
                ..
            })
        )
    }

    fn press(&mut self, p: Point) {
        let reach = REACH / self.zoom;
        self.commit();
        // The selected item's handles work in every tool.
        if let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) {
            let original = self.doc.items[i].clone();
            if let Some(handle) = select::handle_at(&original, p, reach) {
                let drag = match handle {
                    select::Handle::Corner(c) => Drag::Resize(c, original.clone()),
                    select::Handle::Bend => Drag::Bend(original.clone()),
                };
                self.begin(Some(i), original, drag, p);
                return;
            }
        }
        let hit = select::hit(&self.doc.items, p, reach);
        match self.tool {
            Tool::Select => {
                self.doc.selected = hit;
                if let Some(i) = hit {
                    self.begin(Some(i), self.doc.items[i].clone(), Drag::Move(p), p);
                }
            }
            Tool::Crop => self.cropping = Some((p, p)),
            Tool::Text | Tool::Callout => {
                // Clicking a text (callout) edits it.
                let editable = hit.filter(|&i| {
                    matches!(
                        (&self.doc.items[i].shape, self.tool),
                        (Shape::Text { .. }, Tool::Text) | (Shape::Callout { .. }, Tool::Callout)
                    )
                });
                if let Some(i) = editable {
                    self.doc.selected = Some(i);
                    self.begin(Some(i), self.doc.items[i].clone(), Drag::Type, p);
                    self.hide();
                } else {
                    let item = self.create(p);
                    self.begin(None, item, Drag::Draw, p);
                }
            }
            _ => {
                let item = self.create(p);
                self.begin(None, item, Drag::Draw, p);
            }
        }
    }

    fn begin(&mut self, index: Option<usize>, item: Item, drag: Drag, start: Point) {
        self.pending = Some(Pending {
            index,
            item,
            drag,
            start,
            hidden: false,
        });
        self.update_live();
    }

    /// Repaints the base without the pending item (once, when it first changes).
    fn hide(&mut self) {
        if let Some(pending) = &mut self.pending
            && pending.index.is_some()
            && !pending.hidden
        {
            pending.hidden = true;
            self.refresh();
        }
    }

    fn drag_to(&mut self, p: Point) {
        if let Some((from, _)) = self.cropping {
            self.cropping = Some((from, p));
            return;
        }
        let Some(pending) = &mut self.pending else {
            return;
        };
        pending.item = match &pending.drag {
            Drag::Draw => grow(&pending.item, pending.start, p, self.zoom),
            Drag::Move(last) => {
                let item = select::moved(&pending.item, p - *last);
                pending.drag = Drag::Move(p);
                item
            }
            Drag::Resize(corner, original) => select::resized(original, *corner, p),
            Drag::Bend(original) => select::bent(original, p),
            Drag::Type => return,
        };
        if pending.hidden || pending.index.is_none() {
            self.update_live();
        } else {
            self.hide();
        }
    }

    fn release(&mut self) {
        if let Some((from, to)) = self.cropping.take() {
            if let Some(area) = crop::area(from, to, self.doc.view()) {
                self.doc.exec(Edit::Crop(self.doc.crop, Some(area)));
                self.refresh();
            }
            return;
        }
        let Some(pending) = &mut self.pending else {
            return;
        };
        match (&pending.drag, &mut pending.item.shape) {
            (Drag::Type, _) => {}
            (Drag::Draw, Shape::Text { .. } | Shape::Callout { .. }) => {
                pending.drag = Drag::Type;
                self.update_live();
            }
            (Drag::Draw, Shape::Pen(points)) => {
                if let Some(snapped) = shape::snap(points) {
                    pending.item.shape = snapped;
                }
                self.commit();
            }
            _ => self.commit(),
        }
    }

    /// Ends the pending change: one undoable edit (or none if nothing changed).
    fn commit(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let item = pending.item;
        let empty = matches!(&item.shape,
            Shape::Text { text, .. } | Shape::Callout { text, .. } if text.trim().is_empty());
        let edit = match pending.index {
            None if empty || self.too_small(&item) => None,
            None => Some(Edit::Insert(self.doc.items.len(), item)),
            Some(i) if empty => Some(Edit::Delete(i, self.doc.items[i].clone())),
            Some(i) if item != self.doc.items[i] => {
                Some(Edit::Change(i, self.doc.items[i].clone(), item))
            }
            Some(_) => None,
        };
        let repaint = edit.is_some() || pending.hidden;
        if let Some(edit) = edit {
            self.doc.exec(edit);
        }
        if repaint {
            self.refresh();
        } else {
            self.live = None;
        }
    }

    /// A drag that is really a click (shapes that need a size).
    fn too_small(&self, item: &Item) -> bool {
        let f = item.frame();
        let tiny = f.width.hypot(f.height) * self.zoom < 4.0;
        tiny && matches!(
            item.shape,
            Shape::Rect { .. }
                | Shape::Ellipse { .. }
                | Shape::Arrow { .. }
                | Shape::Blur { .. }
                | Shape::Spotlight { .. }
        )
    }

    fn edit_text(&mut self, f: impl FnOnce(&mut String)) {
        if let Some(Pending {
            item,
            drag: Drag::Type,
            ..
        }) = &mut self.pending
            && let Shape::Text { text, .. } | Shape::Callout { text, .. } = &mut item.shape
        {
            f(text);
            self.update_live();
        }
    }

    /// A new item of the current tool at `p`, in the current colour and width.
    fn create(&self, p: Point) -> Item {
        let z = self.zoom;
        let shape = match self.tool {
            Tool::Rect => Shape::Rect { a: p, b: p },
            Tool::Ellipse => Shape::Ellipse { a: p, b: p },
            Tool::Arrow => Shape::Arrow {
                a: p,
                b: p,
                bend: p,
                head: true,
            },
            Tool::Pen | Tool::Select | Tool::Crop => Shape::Pen(vec![p]),
            Tool::Highlighter => Shape::Highlighter(vec![p]),
            Tool::Text => Shape::Text {
                at: p,
                text: String::new(),
                size: self.width.font() / z,
            },
            Tool::Callout => Shape::Callout {
                tip: p,
                at: p + iced::Vector::new(90.0, -60.0) * (1.0 / z),
                text: String::new(),
                size: self.width.font() * 0.75 / z,
            },
            Tool::Step => Shape::Step {
                at: p,
                radius: self.width.badge() / z,
            },
            Tool::Blur => Shape::Blur {
                a: p,
                b: p,
                seed: blur::seed(),
            },
            Tool::Spotlight => Shape::Spotlight { a: p, b: p },
            Tool::Magnifier => Shape::Magnifier {
                at: p,
                radius: 60.0 / z,
            },
        };
        let width = match self.tool {
            Tool::Highlighter => self.width.stroke() * 4.0,
            _ => self.width.stroke(),
        } / z;
        Item {
            shape,
            colour: self.colour,
            width,
        }
    }

    /// The colour/width picked while an item is selected (or typed) applies to it.
    fn restyle(&mut self) {
        let styled = |item: &Item, editor: &Editor| {
            let mut new = editor.create(Point::ORIGIN);
            new.shape = item.shape.clone();
            let z = editor.zoom;
            match &mut new.shape {
                Shape::Text { size, .. } => *size = editor.width.font() / z,
                Shape::Callout { size, .. } => *size = editor.width.font() * 0.75 / z,
                Shape::Step { radius, .. } => *radius = editor.width.badge() / z,
                Shape::Highlighter(_) => new.width = editor.width.stroke() * 4.0 / z,
                _ => new.width = editor.width.stroke() / z,
            }
            new
        };
        if let Some(pending) = &self.pending {
            let item = styled(&pending.item, self);
            if let Some(pending) = &mut self.pending {
                pending.item = item;
            }
            self.update_live();
        } else if let Some(i) = self.doc.selected.filter(|&i| i < self.doc.items.len()) {
            let before = self.doc.items[i].clone();
            let after = styled(&before, self);
            if after != before {
                self.doc.exec(Edit::Change(i, before, after));
                self.refresh();
            }
        }
    }

    /// Repaints the base (all items but a hidden pending one), then the live layer.
    fn refresh(&mut self) {
        let hide = self
            .pending
            .as_ref()
            .and_then(|p| p.index.filter(|_| p.hidden));
        let layers: Vec<_> = (self.doc.items.iter().enumerate())
            .map(|(i, item)| (item, Some(i) != hide))
            .collect();
        let view = self.doc.view();
        let pm = document::paint(&self.doc.image, &layers, int_rect(view, view));
        self.base = handle(pm);
        self.update_live();
    }

    /// Repaints the pending item's region with it in place.
    fn update_live(&mut self) {
        self.live = self.pending.as_ref().map(|pending| {
            let mut layers: Vec<_> = self.doc.items.iter().map(|item| (item, true)).collect();
            match pending.index {
                Some(i) => layers[i] = (&pending.item, true),
                None => layers.push((&pending.item, true)),
            }
            let view = self.doc.view();
            // A spotlight changes the dim everywhere.
            // ponytail: repaints the whole view per pointer move while a spotlight
            // is dragged; dim only its old/new boxes if that is slow on 4K+.
            let area = match pending.item.shape {
                Shape::Spotlight { .. } => view,
                _ => pending.item.bounds(),
            };
            let r = int_rect(area, view);
            let rect = Rectangle::new(
                Point::new(r.x() as f32, r.y() as f32),
                Size::new(r.width() as f32, r.height() as f32),
            );
            (rect, handle(document::paint(&self.doc.image, &layers, r)))
        });
    }
}

/// The new item `item` (started at `start`) dragged to `p`.
fn grow(item: &Item, start: Point, p: Point, zoom: f32) -> Item {
    let mut item = item.clone();
    match &mut item.shape {
        Shape::Rect { b, .. }
        | Shape::Ellipse { b, .. }
        | Shape::Blur { b, .. }
        | Shape::Spotlight { b, .. } => *b = p,
        Shape::Arrow { a, b, bend, .. } => {
            *b = p;
            *bend = Point::new((a.x + p.x) / 2.0, (a.y + p.y) / 2.0);
        }
        Shape::Pen(points) | Shape::Highlighter(points) => {
            if points
                .last()
                .is_none_or(|last| last.distance(p) * zoom >= 1.0)
            {
                points.push(p);
            }
        }
        Shape::Text { .. } => {}
        Shape::Step { at, .. } | Shape::Callout { at, .. } => *at = p,
        Shape::Magnifier { radius, .. } => *radius = start.distance(p).max(20.0 / zoom),
    }
    item
}

/// A painted pixmap as an iced image (premultiplied = straight: the image is opaque).
fn handle(pm: tiny_skia::Pixmap) -> image::Handle {
    image::Handle::from_rgba(pm.width(), pm.height(), pm.take())
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// `#RRGGBB` (the `#` optional) as an opaque colour.
fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, 255])
}
