//! What the pointer and the keys mean to the editor: which item shows its
//! selection frame, the cursor, canvas <-> image coordinates, key -> message.
//! (The canvas that draws and feeds these is `ui::screens::editor::canvas`.)
use iced::keyboard::{Key, Modifiers, key::Named};
use iced::mouse;
use iced::{Point, Rectangle};

use super::{
    Drag, Editor, Menu, Message, PRESETS, Shape, TOOLS, Tool, area, callout, label, select, text,
};

impl Editor {
    /// The item that shows the selection frame (and handles): the one drawn,
    /// moved or typed, else the selected one. Pen and highlighter strokes never
    /// do: they are moved by dragging the stroke itself.
    pub(crate) fn framed(&self) -> Option<&super::Item> {
        let item = (self.pending.as_ref().map(|p| &p.item))
            .or_else(|| self.doc.selected.and_then(|i| self.doc.items.get(i)))?;
        (!matches!(item.shape, Shape::Pen(_) | Shape::Highlighter(_))).then_some(item)
    }

    /// The pointer over `p` (image px): grabbing while something moves; a resize
    /// cursor on the selection's handles; a move cursor on what a press moves.
    pub(crate) fn cursor(&self, p: Point) -> mouse::Interaction {
        use mouse::Interaction as I;
        match self.pending.as_ref().map(|p| &p.drag) {
            Some(Drag::Move { .. }) => return I::Grabbing,
            Some(Drag::Draw | Drag::Resize(..) | Drag::Bend(_)) => return I::Crosshair,
            Some(Drag::Type) | None => {}
        }
        // Snip's live area: its handles and edges, and a move inside.
        if self.movable {
            let grip = match &self.area_drag {
                Some(drag) => Some(drag.grip),
                None => area::grip_at(self.doc.view(), p, area::REACH / self.zoom),
            };
            match grip {
                Some(area::Grip::Move) => return I::Grabbing,
                Some(area::Grip::Edge(_, 0)) => return I::ResizingHorizontally,
                Some(area::Grip::Edge(0, _)) => return I::ResizingVertically,
                Some(area::Grip::Edge(x, y)) if x == y => return I::ResizingDiagonallyDown,
                Some(area::Grip::Edge(..)) => return I::ResizingDiagonallyUp,
                None => {}
            }
        }
        let reach = super::REACH / self.zoom;
        let handle = (self.doc.selected.and_then(|i| self.doc.items.get(i)))
            .and_then(|item| select::handle_at(item, p, reach));
        match (handle, self.tool) {
            (Some(select::Handle::Corner(c)), _) if c % 2 == 0 => I::ResizingDiagonallyDown,
            (Some(select::Handle::Corner(_)), _) => I::ResizingDiagonallyUp,
            (Some(select::Handle::Bend), _) => I::Grab,
            (None, Tool::Select) if select::hit(&self.doc.items, p, reach).is_some() => I::Grab,
            (None, Tool::Select) if self.movable && self.doc.view().contains(p) => I::Grab,
            (None, Tool::Select) => I::default(),
            (None, _) if self.grabbed(p).is_some() => I::Move,
            (None, Tool::Text | Tool::Label | Tool::Callout) => I::Text,
            (None, _) => I::Crosshair,
        }
    }

    /// While a step's note is typed: where the note's text starts (image px)
    /// and its size; its pill is drawn beside the badge.
    pub(crate) fn note_origin(&self, at: Point, radius: f32) -> (Point, f32) {
        let size = (radius * 1.1).max(12.0 / self.zoom);
        (
            at + iced::Vector::new(radius * 1.6, -text::line_height(size) / 2.0),
            size,
        )
    }

    /// Where the caret is while text is typed (image px, its top) and the font
    /// size.
    pub(crate) fn caret(&self) -> Option<(Point, f32)> {
        if !self.typing() {
            return None;
        }
        let item = self.framed()?;
        let at_end = |origin: Point, s: &str, size: f32| (origin + text::caret(s, size), size);
        match &item.shape {
            Shape::Text {
                at,
                text,
                size,
                style,
            } => Some((*at + text::caret_styled(text, *size, *style), *size)),
            Shape::Callout { at, text, size, .. } => {
                Some(at_end(callout::text_origin(*at, text, *size), text, *size))
            }
            Shape::Label { at, text, size } => {
                Some(at_end(label::text_origin(*at, *size), text, *size))
            }
            Shape::Arrow { a, text, .. } => {
                let size = label::arrow_size(item.width);
                let at = label::centred(*a, text, size);
                Some(at_end(label::text_origin(at, size), text, size))
            }
            Shape::Step { at, radius, note } => {
                let (origin, size) = self.note_origin(*at, *radius);
                Some(at_end(origin, note, size))
            }
            _ => None,
        }
    }

    /// Window px → image px for Snip's live area (the canvas starts at the
    /// monitor's origin there, wherever the area is now).
    pub(crate) fn window_to_image(&self, p: Point) -> Point {
        Point::new(p.x / self.zoom, p.y / self.zoom)
    }

    /// Canvas px → image px.
    pub(crate) fn to_image(&self, p: Point) -> Point {
        let v = self.doc.view();
        Point::new(v.x + p.x / self.zoom, v.y + p.y / self.zoom)
    }

    pub(crate) fn to_display_point(&self, p: Point) -> Point {
        let v = self.doc.view();
        Point::new((p.x - v.x) * self.zoom, (p.y - v.y) * self.zoom)
    }

    pub(crate) fn to_display(&self, r: Rectangle) -> Rectangle {
        Rectangle::new(self.to_display_point(r.position()), r.size() * self.zoom)
    }

    /// What a key does: typing while a text is edited; Esc closes the picker
    /// (its hex field has the keys); otherwise tool letters and shortcuts.
    pub(crate) fn key(
        &self,
        key: &Key,
        latin: Option<char>,
        modifiers: Modifiers,
        text: Option<&str>,
    ) -> Option<Message> {
        if self.typing() {
            return match key {
                Key::Named(Named::Escape) => Some(Message::Finish),
                Key::Named(Named::Backspace) => Some(Message::Backspace),
                Key::Named(Named::Enter) => Some(Message::Type("\n".into())),
                _ if modifiers.command() => None,
                _ => text
                    .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                    .map(|t| Message::Type(t.to_string())),
            };
        }
        if self.menu == Some(Menu::Colour) {
            return matches!(key, Key::Named(Named::Escape)).then_some(Message::Picker);
        }
        let arrow = match key {
            Key::Named(Named::ArrowLeft) => Some((-1, 0)),
            Key::Named(Named::ArrowRight) => Some((1, 0)),
            Key::Named(Named::ArrowUp) => Some((0, -1)),
            Key::Named(Named::ArrowDown) => Some((0, 1)),
            _ => None,
        };
        if let Some(arrow) = arrow.filter(|_| self.movable) {
            let keys = (modifiers.command(), modifiers.shift(), modifiers.alt());
            return Some(Message::Nudge(arrow, keys));
        }
        // Snip: Tab walks the toolbar, Enter / Space press what it is on;
        // Enter with nothing focused copies.
        if self.movable {
            match key {
                Key::Named(Named::Tab) => return Some(Message::Focus(modifiers.shift())),
                Key::Named(Named::Enter | Named::Space) if self.focus.is_some() => {
                    return Some(Message::Activate);
                }
                Key::Named(Named::Enter) => return Some(Message::Copy),
                _ => {}
            }
        }
        match key {
            Key::Named(Named::Escape) if self.picking => Some(Message::Eyedropper),
            Key::Named(Named::Escape) => match self.menu {
                Some(menu) => Some(Message::Menu(menu)),
                None => Some(Message::Back),
            },
            Key::Named(Named::Delete | Named::Backspace) => Some(Message::Delete),
            _ => match (
                modifiers.command(),
                modifiers.shift(),
                latin?.to_ascii_lowercase(),
            ) {
                (true, false, 'z') => Some(Message::Undo),
                (true, true, 'z') | (true, _, 'y') => Some(Message::Redo),
                (true, _, 'c') => Some(Message::Copy),
                (true, _, 's') => Some(Message::Save),
                // Ctrl: plain P is the Pen tool.
                (true, _, 'p') => Some(Message::Pin),
                (false, _, 'd') => Some(Message::Eyedropper),
                (false, _, digit @ '1'..='9') => {
                    let i = digit as usize - '1' as usize;
                    (i < PRESETS.len()).then_some(Message::Preset(i))
                }
                (false, _, letter) => TOOLS
                    .iter()
                    .find(|t| t.key == letter)
                    .map(|t| Message::Tool(t.tool)),
                _ => None,
            },
        }
    }
}
