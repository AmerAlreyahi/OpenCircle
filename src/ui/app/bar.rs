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
use crate::ui::app::screens;
use crate::ui::kit::panel_frame as card;
use crate::ui::screens::bar::geometry as bar;
use crate::ui::screens::snip_toolbar::SnipToolbar;

use super::{ASK_INPUT, App};

impl App {
    /// The bottom bar shows in main mode from the first frame, through drawing
    /// and selecting; a translation, the song card or a toast take its place.
    /// The AI chat sits beside it (`bar_room`): Translate and Copy text stay
    /// reachable while it is open (#170).
    pub(in crate::ui) fn bottom_bar_shown(&self) -> bool {
        matches!(self.o.purpose, Purpose::Search)
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
        if let Some((_, _, size)) = self.snip_dock() {
            return size;
        }
        let bar = match self.bar_other() {
            Some((work, k)) => self.bar_size_on(work.width * k).1 * (1.0 / k),
            None => self.bar_size().1,
        };
        let tool =
            (self.o.edit.as_ref()).map_or_else(SnipToolbar::toolbar_size, SnipToolbar::bar_size);
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
            // held whole on one screen (`bar_on`, set as it moves): never
            // across a bezel, where one window would cut it
            let area = match self.o.edit.as_ref().map(|e| e.annotator.movable) {
                Some(false) => Rectangle::with_size(self.o.screen),
                Some(true) => self.o.desk(), // (Snip's toolbar: its own rules)
                None => self.screen_area(self.o.bar_on),
            };
            let size = self.dock_size();
            let fit =
                |at: f32, lo: f32, whole: f32, side: f32| at.clamp(lo, lo.max(lo + whole - side));
            return Point::new(
                fit(at.x, area.x, area.width, size.width),
                fit(at.y, area.y, area.height, size.height),
            );
        }
        // Snip's live area: the toolbar docks to it (under, above, inside),
        // clear of the handle being dragged, on the screen holding most of it.
        if let Some(edit) = self.o.edit.as_ref().filter(|e| e.annotator.movable)
            && let Some((_, work, size)) = self.snip_dock()
        {
            if let Some(at) = self.o.bar_left {
                return at;
            }
            let area = edit.canvas_rect();
            let avoid =
                (edit.annotator.area_handle()).map(|p| p + (area.position() - Point::ORIGIN));
            // a translation of the area (More > Translate): its pill stays at top centre
            let pill = (self.o.translate.as_ref())
                .map(|_| crate::ui::screens::translate::view::pill_rect(self.o.screen));
            return bar::dock(area, size, &[work], avoid, pill);
        }
        let canvas = self.o.edit.as_ref().map(SnipToolbar::canvas_rect);
        let (room, size, inset, _) = self.bar_spot();
        let off = Vector::new(room.x, room.y);
        let blocked: Vec<Rectangle> = (self.o.selection.iter())
            .chain(self.o.others.iter().map(|(s, _)| s))
            .map(|s| self.o.view.rect_to_screen(s.area))
            .chain(canvas)
            .map(|r| r - off)
            .collect();
        let spot = bar::free(self.o.bar_anchor, &blocked, room.size(), size, inset);
        bar::anchor_at(spot, room.size(), size, inset) + off
    }

    /// Every screen's work area (the starting screen's px, the taskbar left
    /// out), the starting one first, with its scale against it (`Lens::k`).
    pub(in crate::ui) fn work_areas(&self) -> Vec<(Rectangle, f32)> {
        let start = self.o.place();
        let here = Size::new(
            self.o.screen.width,
            self.o.screen.height - self.o.work_inset,
        );
        let others = (self.o.screens.iter()).map(|s| {
            let k = start.scale / s.place.scale;
            let mut work = screens::area_of(start, s.place);
            work.height -= s.inset / k;
            (work, k)
        });
        std::iter::once((Rectangle::with_size(here), 1.0))
            .chain(others)
            .collect()
    }

    /// Snip's toolbar (`None` without its live area): the screen it docks on
    /// (`None` = the starting one, else that index in `screens`), its work
    /// area and the toolbar's size there, in the starting screen's px. The
    /// screen holding the larger part of the area (design state 8): it fits
    /// there whole, never across a bezel.
    pub(in crate::ui) fn snip_dock(&self) -> Option<(Option<usize>, Rectangle, Size)> {
        let edit = self.o.edit.as_ref().filter(|e| e.annotator.movable)?;
        let works = self.work_areas();
        let rects: Vec<Rectangle> = works.iter().map(|(r, _)| *r).collect();
        let i = bar::dock_screen(edit.canvas_rect(), &rects);
        let (work, k) = works[i];
        // its own logical width there, back in the starting screen's px
        let size = edit.bar_size() * (1.0 / k);
        Some((i.checked_sub(1), work, size))
    }

    /// The window Snip's toolbar is drawn in: the screen its spot is on now
    /// (where it docked, or where it was left), else the starting one.
    pub(in crate::ui) fn toolbar_on(&self) -> Option<iced::window::Id> {
        self.o.edit.as_ref().filter(|e| e.annotator.movable)?;
        let at = self.bar_at();
        let start = self.o.place();
        let here = (self.o.screens.iter())
            .find(|s| screens::area_of(start, s.place).contains(at))
            .map(|s| s.window);
        Some(here.unwrap_or(self.overlay))
    }

    /// Where the bar's spots are now, in the starting screen's px: the room
    /// (`bar_room`, or the work area of the other screen it was dropped on,
    /// `bar_on`), the bar's size there, the taskbar's share of the room's
    /// bottom, and the room's width (logical px, the screen's own: the bar's
    /// field and pills are laid out for it, `bar_size_on`).
    pub(in crate::ui) fn bar_spot(&self) -> (Rectangle, Size, f32, f32) {
        match self.bar_other() {
            Some((work, k)) => (work, self.dock_size(), 0.0, work.width * k),
            None => {
                let room = self.bar_room();
                (room, self.dock_size(), self.o.work_inset, room.width)
            }
        }
    }

    /// The other screen the bar is on (`bar_on`; Snip keeps it on the
    /// starting one): its work area (the starting screen's px) and its scale
    /// against the starting one (`k`).
    fn bar_other(&self) -> Option<(Rectangle, f32)> {
        (self.o.bar_on.filter(|_| self.o.edit.is_none()))
            .and_then(|i| self.work_areas().get(i + 1).copied())
    }

    /// The window the bar is drawn in: the screen it is on now.
    pub(in crate::ui) fn bar_window(&self) -> iced::window::Id {
        let held = Rectangle::new(self.bar_at(), self.dock_size());
        let on = self.screen_holding(held).filter(|_| self.o.edit.is_none());
        self.window_of(on)
    }

    /// Which other screen (index in `screens`; `None`: the starting one) holds
    /// the larger part of `r` (the starting screen's px). THE way anything the
    /// user drags across screens (the bar, the AI chat) finds the screen it is
    /// on, whole at its drop and drawn by that screen's window.
    /// Off every screen (held past the desktop's edge): the nearest one.
    pub(in crate::ui) fn screen_holding(&self, r: Rectangle) -> Option<usize> {
        let rects: Vec<Rectangle> = self.work_areas().iter().map(|(w, _)| *w).collect();
        let off = |w: &Rectangle| {
            let c = r.center();
            let d = Vector::new(
                (w.x - c.x).max(c.x - w.x - w.width).max(0.0),
                (w.y - c.y).max(c.y - w.y - w.height).max(0.0),
            );
            d.x * d.x + d.y * d.y
        };
        match rects.iter().any(|w| w.intersects(&r)) {
            true => bar::dock_screen(r, &rects),
            false => (0..rects.len())
                .min_by(|a, b| off(&rects[*a]).total_cmp(&off(&rects[*b])))
                .unwrap_or(0),
        }
        .checked_sub(1)
    }

    /// The screen `on` (`screen_holding`) whole, in the starting screen's px:
    /// where anything dragged across screens (the bar, the AI chat) is held
    /// while dragged. Only that screen's window draws it, so across a bezel it
    /// would be cut (or squeezed at the window's edge); it moves over whole
    /// once most of it is over.
    pub(in crate::ui) fn screen_area(&self, on: Option<usize>) -> Rectangle {
        let all = self.screens_on();
        self.monitor_of(&all[on.map_or(0, |i| i + 1).min(all.len() - 1)])
    }

    /// The window of the screen `on` (`screen_holding`).
    pub(in crate::ui) fn window_of(&self, on: Option<usize>) -> iced::window::Id {
        (on.and_then(|i| self.o.screens.get(i))).map_or(self.overlay, |s| s.window)
    }

    /// A pointer position in `window`'s own px as the starting screen's px.
    pub(in crate::ui) fn start_px(&self, window: iced::window::Id, p: Point) -> Point {
        // the chat's own window, moved under the pointer as it drags it: the
        // pointer on the desktop, never its px in a window still moving
        if let Some(placed) = (self.chat_win.as_ref())
            .filter(|w| w.id == window)
            .and_then(|w| w.placed)
        {
            #[cfg(not(test))]
            let pointer = crate::capture::pointer();
            #[cfg(test)]
            let pointer = None::<(i32, i32)>; // (not the test machine's)
            let desk = pointer.map_or_else(
                || {
                    let (x, y) = placed.origin;
                    Point::new(x as f32 + p.x * placed.scale, y as f32 + p.y * placed.scale)
                },
                |(x, y)| Point::new(x as f32, y as f32),
            );
            return (self.o.view).to_screen(super::screens::from_desktop(self.o.place(), desk));
        }
        match self.o.screens.iter().find(|s| s.window == window) {
            Some(s) => self.o.view.to_screen(s.view.to_image(p)),
            None => p,
        }
    }

    /// The bar was let go at `at`: it docks to the nearest spot of the screen
    /// holding most of it, however many screens there are.
    pub(in crate::ui) fn dock_bar(&mut self, at: Point) {
        self.o.bar_on = self.screen_holding(Rectangle::new(at, self.dock_size()));
        let (room, size, inset, _) = self.bar_spot();
        let at = at - Vector::new(room.x, room.y);
        self.o.bar_anchor = bar::nearest(at, room.size(), size, inset);
    }

    /// The part of the screen the bar's spots are in: beside the AI chat (open
    /// on the overlay or pinned) (it never covers the bar; centre = the middle of what it leaves),
    /// else the whole screen.
    // ponytail: a chat leaving no room for the shortest bar: the whole screen.
    pub(in crate::ui) fn bar_room(&self) -> Rectangle {
        let all = Rectangle::with_size(self.o.screen);
        let card = match (&self.o.ai_chat, &self.chat) {
            (Some(_), _) => self.chat_card_at(),
            (None, Some(_)) => self.chat_rect(),
            (None, None) => return all,
        };
        let room = card::free(self.o.screen, Some(card));
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
            BarButton::Edit if selected => Some(Message::Run(&actions::snip::ACTION)),
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
