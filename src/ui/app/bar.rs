//! The bottom bar's state: size, dock, glide, drag and keys (its view:
//! `screens::bar::view`).

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::operation;
use iced::{Point, Rectangle, Size, Task, Vector};

use crate::actions;
use crate::config::{self};
use crate::labs::{self, BarButton};

use crate::ui::app::background::background;
use crate::ui::app::message::{Message, Purpose};
use crate::ui::kit::panel_frame as card;
use crate::ui::screens::bar::geometry as bar;
use crate::ui::screens::editor::Editing;

use super::{ASK_INPUT, App};

impl App {
    /// The bottom bar shows in main mode from the first frame, through drawing
    /// and selecting; a result card, a translation, the song card or a toast
    /// take its place.
    pub(in crate::ui) fn bottom_bar_shown(&self) -> bool {
        matches!(self.o.purpose, Purpose::Search)
            && self.o.panel.is_none()
            && self.o.translate.is_none()
            && self.o.song.is_none()
            && self.o.toast.is_none()
    }

    /// The bottom bar's field width and size: the same in every state (no
    /// selection, a selection, after a card), so it never jumps.
    /// Beside a pinned chat, it fits what the chat leaves (a shorter field).
    pub(in crate::ui) fn bar_size(&self) -> (f32, Size) {
        self.bar_size_on(self.bar_room().width)
    }

    /// The bar's field width and size on a screen `width` wide.
    pub(in crate::ui) fn bar_size_on(&self, width: f32) -> (f32, Size) {
        let icons = labs::bar(&self.o.labs)
            .iter()
            .filter(|b| !b.is_pill())
            .count();
        bar::bottom_size(width, icons as f32)
    }

    /// The size of what is docked at the bar's spot now: the bar, the editing
    /// toolbar, or the shape between them while one turns into the other.
    pub(in crate::ui) fn dock_size(&self) -> Size {
        let (_, bar) = self.bar_size();
        let tool = Editing::toolbar_size(self.o.screen.width);
        let t = self.swapped();
        Size::new(
            bar.width + (tool.width - bar.width) * t,
            bar.height + (tool.height - bar.height) * t,
        )
    }

    /// Where the bar is drawn now (top-left): on its way to `bar_target`.
    pub(in crate::ui) fn bar_at(&self) -> Point {
        self.o.bar_now.unwrap_or_else(|| self.bar_target())
    }

    /// Where the bar (or the toolbar in its place) goes: following the pointer
    /// while held, else its spot, or the nearest free one while the selection
    /// (the editor's canvas) is where it would be.
    pub(in crate::ui) fn bar_target(&self) -> Point {
        if let Some((_, at)) = self.o.bar_drag {
            return at;
        }
        let size = self.dock_size();
        // Snip's live area: the toolbar docks to it (under, above, inside),
        // clear of the handle being dragged.
        if let Some(edit) = self.o.edit.as_ref().filter(|e| e.editor.movable) {
            let area = edit.canvas_rect();
            let work = Size::new(
                self.o.screen.width,
                self.o.screen.height - self.o.work_inset,
            );
            let avoid = (edit.editor.area_handle()).map(|p| p + (area.position() - Point::ORIGIN));
            return bar::dock(area, size, &[Rectangle::with_size(work)], avoid);
        }
        let canvas = self.o.edit.as_ref().map(Editing::canvas_rect);
        let room = self.bar_room();
        let off = Vector::new(room.x, room.y);
        let blocked: Vec<Rectangle> = (self.o.selection.iter())
            .chain(self.o.others.iter().map(|(s, _)| s))
            .map(|s| self.o.view.rect_to_screen(s.area))
            .chain(canvas)
            .map(|r| r - off)
            .collect();
        let inset = self.o.work_inset;
        let spot = bar::free(self.o.bar_anchor, &blocked, room.size(), size, inset);
        bar::anchor_at(spot, room.size(), size, inset) + off
    }

    /// The part of the screen the bar's spots are in: beside the pinned AI
    /// chat (it never covers the bar; centre = the middle of what it leaves),
    /// else the whole screen.
    // ponytail: a chat leaving no room for the shortest bar: the whole screen.
    pub(in crate::ui) fn bar_room(&self) -> Rectangle {
        let all = Rectangle::with_size(self.o.screen);
        let Some(_) = &self.chat else { return all };
        let room = card::free(self.o.screen, Some(self.chat_rect()));
        let (_, size) = self.bar_size_on(room.width);
        match room.width >= size.width + 2.0 * bar::GAP {
            true => room,
            false => all,
        }
    }

    /// The bar is on its way somewhere.
    pub(in crate::ui) fn bar_gliding(&self) -> bool {
        self.o.frame.is_some() && self.o.bar_now.is_some_and(|at| at != self.bar_target())
    }

    /// The bar was dropped: its place is remembered (config `[bar]`).
    pub(in crate::ui) fn remember_bar(&self) -> Task<Message> {
        let anchor = self.o.bar_anchor;
        if cfg!(test) {
            let mut cfg = config::current();
            cfg.bar.anchor = anchor;
            config::set_current(cfg);
            return Task::none(); // never the user's file
        }
        background(move |_: &UnboundedSender<()>| {
            if let Err(e) = config::update(|c| c.bar.anchor = anchor) {
                crate::log!("[opencircle] the bar's place was not saved: {e}");
            }
        })
        .discard()
    }

    /// Something is selected for the bar's pills to act on.
    pub(in crate::ui) fn selected_for_bar(&self) -> bool {
        self.o.selection.is_some() && !self.settling()
    }

    /// What a bar button does (also what Enter / Space do on it once Tab
    /// focused it); none = dimmed: Search in browser and Copy text need a
    /// selection, Translate takes the whole screen without one.
    pub(in crate::ui) fn bar_message(&self, which: BarButton) -> Option<Message> {
        let selected = self.selected_for_bar();
        match which {
            BarButton::Search | BarButton::CopyText => selected.then_some(Message::Pill(which)),
            BarButton::Translate => Some(Message::Pill(which)),
            BarButton::Live => Some(Message::LiveToggle),
            BarButton::Song => Some(Message::SongStart),
            BarButton::Edit if selected => Some(Message::Run(&actions::edit::ACTION)),
            BarButton::Edit => Some(Message::EditScreen),
        }
    }

    /// Tab / Shift+Tab: the focus moves on through the bar (`bar::stops`).
    /// The AI field takes the text input's focus, any other stop lets go of it.
    pub(in crate::ui) fn tab_bar(&mut self, back: bool) -> Task<Message> {
        let stops = bar::stops(self.selected_for_bar(), &labs::bar(&self.o.labs));
        self.o.bar_focus = bar::tab(&stops, self.o.bar_focus, back);
        match self.o.bar_focus {
            Some(bar::Stop::Field) => operation::focus(ASK_INPUT),
            _ => operation::focus("overlay-none"), // focusing nothing unfocuses the field
        }
    }

    /// Enter / Space on the stop Tab focused.
    pub(in crate::ui) fn activate(&mut self, stop: bar::Stop) -> Task<Message> {
        match stop {
            bar::Stop::Logo => self.update(Message::AskButton),
            bar::Stop::Include => self.update(Message::IncludeScreen),
            bar::Stop::Badge => self.update(Message::AskSubmit),
            bar::Stop::Button(which) => match self.bar_message(which) {
                Some(message) => self.update(message),
                None => Task::none(),
            },
            bar::Stop::Field => Task::none(), // the field's own Enter submits
        }
    }
}
