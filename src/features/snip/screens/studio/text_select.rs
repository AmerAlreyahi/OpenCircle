//! Selectable text in the Studio (R-16, owner 2026-11-25): every word of the picture is selectable the
//! way a document's is (Windows Snipping Tool Text actions, macOS Live Text). "Select text" in the
//! Actions card reads the picture ONCE with the text reader (`studio_text` in the App; the lines are kept
//! for the picture), then an overlay takes the pointer: I-beam over the lines, a drag selects to the
//! character, a double click a word, a triple click a line, Ctrl+A everything, Ctrl+C copies, and a right
//! press ON the selection opens main mode's own menu (`circle::screens::context_menu`: Copy, Search the
//! web, Ask AI, Translate, ...). The selection itself is main mode's `words::Picked`; nothing of it is
//! copied here. A right press off the selection is the stage's own, as before.
//! Edit text from a selected line is not done (its own picker stays the way to edit a line).
use std::time::Instant;

use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use super::{Msg, Outcome, Studio};
use crate::app::DOUBLE_CLICK;
use crate::features::circle::entities;
use crate::features::circle::text_menu as menu;
use crate::features::text_reading::words::{self, Line, Picked, Unit};
use crate::shared::kit::option_list;

/// A click this near (image px) the last one, within `DOUBLE_CLICK`, counts as the next of a double or triple click.
const NEAR: f32 = 4.0;

/// What the Studio's text selection says or asks.
#[derive(Debug, Clone)]
pub enum Ev {
    /// The Actions card's row: start (read the picture once), or stop.
    Toggle,
    /// The overlay, in image px: a press, the pointer while pressed, the release, a right press.
    Press(Point),
    Move(Point),
    Release,
    Right(Point),
    /// The right-click menu's own message.
    Menu(menu::Msg),
}

/// What the host (the App) does for the selection.
#[derive(Debug, Clone)]
pub enum Work {
    /// Read the picture's words (once per picture).
    Read,
    /// A menu row (or Ctrl+C) on the selected text and the selected span of each line (image px).
    Row(menu::Row, String, Vec<(String, Rectangle)>),
}

/// The Studio's text selection: the mode, the words read for the picture, the selection, the menu.
#[derive(Default)]
pub struct State {
    /// The picture (`Studio::epoch`) the mode is on for.
    on: Option<u32>,
    /// The words of the picture `epoch`, in image px (read once; empty = none).
    read: Option<(u32, Vec<Line>, Size)>,
    /// The picture whose words are being read.
    reading: Option<u32>,
    /// The picture's state (`Studio::picture_stamp`) the read was asked for: its words are only good for that.
    stamp: u64,
    picked: Option<Picked>,
    pressed: bool,
    clicks: Option<(Instant, Point, u8)>,
    menu: Option<menu::Menu>,
}

impl State {
    pub fn active(&self, epoch: u32) -> bool {
        self.on == Some(epoch)
    }

    pub fn reading(&self, epoch: u32) -> bool {
        self.active(epoch) && self.reading == Some(epoch)
    }

    /// The mode ends and what was read is forgotten (the pixels change: Edit text, Cut out, Translate).
    pub fn reset(&mut self) {
        *self = State::default();
    }

    fn lines(&self, epoch: u32) -> &[Line] {
        match &self.read {
            Some((e, lines, _)) if *e == epoch => lines,
            _ => &[],
        }
    }

    fn covered(&self) -> Rectangle {
        let size = self.read.as_ref().map_or(Size::ZERO, |r| r.2);
        Rectangle::new(Point::ORIGIN, size)
    }

    /// The row the Actions card shows: on while the mode is.
    pub fn toggle(&mut self, epoch: u32, stamp: u64) -> Option<Work> {
        if self.active(epoch) {
            self.on = None;
            self.picked = None;
            self.menu = None;
            return None;
        }
        self.on = Some(epoch);
        match &self.read {
            Some((e, ..)) if *e == epoch => None, // read for this picture already
            _ if self.reading == Some(epoch) => None, // being read: one read
            _ => {
                self.stamp = stamp;
                self.reading = Some(epoch);
                Some(Work::Read)
            }
        }
    }

    /// The words read for picture `epoch` (image px).
    pub fn set(&mut self, epoch: u32, lines: Vec<Line>, size: Size) {
        if self.reading == Some(epoch) {
            self.reading = None;
            self.read = Some((epoch, lines, size));
        }
    }

    /// The read failed or found nothing: the mode ends.
    pub fn failed(&mut self, epoch: u32) {
        if self.reading == Some(epoch) {
            *self = State::default();
        }
    }

    /// A press at `p` (image px): the caret there, its word (double click) or line (triple click).
    pub fn press(&mut self, epoch: u32, p: Point, now: Instant) {
        let n = match self.clicks {
            Some((at, last, n)) if now - at <= DOUBLE_CLICK && last.distance(p) <= NEAR => n + 1,
            _ => 1,
        };
        self.clicks = Some((now, p, n));
        let unit = match n {
            1 => Unit::Caret,
            2 => Unit::Word,
            _ => Unit::Line,
        };
        self.menu = None;
        self.picked = Picked::new(self.lines(epoch).to_vec(), self.covered(), p, unit);
        self.pressed = self.picked.is_some();
    }

    /// The pointer moves while pressed: the selection stretches to the caret nearest it.
    pub fn drag(&mut self, p: Point) {
        if let (true, Some(picked)) = (self.pressed, &mut self.picked) {
            picked.extend(p);
        }
    }

    pub fn release(&mut self) {
        self.pressed = false;
        // a click with no drag leaves no empty selection
        if self.picked.as_ref().is_some_and(|p| p.is_empty()) {
            self.picked = None;
        }
    }

    /// Everything (Ctrl+A, the menu's Select all text).
    pub fn all(&mut self, epoch: u32) {
        self.picked = Picked::all(self.lines(epoch).to_vec(), self.covered());
    }

    /// The selected text (none while only a caret is placed).
    pub fn text(&self) -> Option<String> {
        let text = self.picked.as_ref().filter(|p| !p.is_empty())?.text();
        (!text.trim().is_empty()).then_some(text)
    }

    /// `p` (image px) is on the selection: a right press there opens the menu.
    pub fn on_selection(&self, p: Point) -> bool {
        (self.picked.as_ref().filter(|s| !s.is_empty()))
            .is_some_and(|s| s.rects().iter().any(|r| r.contains(p)))
    }

    /// Esc: the menu, else the selection, else the mode. `false` = it was not on.
    pub fn escape(&mut self, epoch: u32) -> bool {
        if !self.active(epoch) {
            return false;
        }
        if self.menu.take().is_none() && self.picked.take().is_none() {
            self.on = None;
        }
        true
    }

    /// The menu's rows (tests).
    #[cfg(test)]
    pub fn menu_rows(&self) -> Vec<menu::Row> {
        self.menu
            .iter()
            .flat_map(|m| m.rows().into_iter().cloned())
            .collect()
    }

    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    fn work(&self, row: menu::Row) -> Option<Work> {
        let text = self.text()?;
        let segments = self
            .picked
            .as_ref()
            .map(|p| p.segments())
            .unwrap_or_default();
        Some(Work::Row(row, text, segments))
    }
}

/// The lines over the picture while the mode is on: the selection as flat boxes; a press picks, a drag
/// selects, a right press on the selection opens the menu; every other press is taken (the editor's tool
/// must not draw), and a right press off the selection goes on to the stage.
struct Overlay<'a> {
    state: &'a State,
    epoch: u32,
    /// The view's top-left (image px) and display px per image px.
    origin: Point,
    zoom: f32,
    fill: Color,
}

impl Overlay<'_> {
    fn image(&self, at: Point) -> Point {
        Point::new(
            self.origin.x + at.x / self.zoom,
            self.origin.y + at.y / self.zoom,
        )
    }

    fn shown(&self, r: Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(
                (r.x - self.origin.x) * self.zoom,
                (r.y - self.origin.y) * self.zoom,
            ),
            Size::new(r.width * self.zoom, r.height * self.zoom),
        )
    }
}

impl canvas::Program<Msg> for Overlay<'_> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Msg>> {
        use mouse::{Button, Event};
        let canvas::Event::Mouse(event) = event else {
            return None;
        };
        let say = |ev| Action::publish(Msg::Text(ev));
        match event {
            Event::ButtonPressed(Button::Left) => {
                let at = cursor.position_in(bounds)?;
                Some(say(Ev::Press(self.image(at))).and_capture())
            }
            Event::ButtonPressed(Button::Right) => {
                let p = self.image(cursor.position_in(bounds)?);
                self.state
                    .on_selection(p)
                    .then(|| say(Ev::Right(p)).and_capture())
            }
            Event::CursorMoved { .. } if self.state.pressed => {
                let at = cursor.position()? - bounds.position();
                Some(say(Ev::Move(self.image(Point::new(at.x, at.y)))))
            }
            Event::ButtonReleased(Button::Left) if self.state.pressed => Some(say(Ev::Release)),
            _ => None,
        }
    }

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for r in (self.state.picked.iter()).flat_map(|p| p.rects()) {
            let r = self.shown(r);
            crate::shared::kit::area::wash(&mut frame, r, self.fill);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        let over = |at: Point| {
            let p = self.image(at);
            self.state
                .lines(self.epoch)
                .iter()
                .any(|l| l.bounds.contains(p))
        };
        match cursor.position_in(bounds) {
            Some(at) if over(at) => mouse::Interaction::Text,
            _ => mouse::Interaction::default(),
        }
    }
}

/// `drawn` with the text overlay over it while the mode is on.
pub fn over<'a>(
    s: &'a Studio,
    drawn: Element<'a, Msg>,
    shown: Size,
    origin: Point,
    zoom: f32,
) -> Element<'a, Msg> {
    if !s.text.active(s.epoch()) {
        return drawn;
    }
    let overlay = Overlay {
        state: &s.text,
        epoch: s.epoch(),
        origin,
        zoom,
        fill: crate::shared::kit::tokens::fixed::TEXT_SELECTION,
    };
    let overlay = canvas::Canvas::new(overlay)
        .width(Length::Fixed(shown.width))
        .height(Length::Fixed(shown.height));
    iced::widget::stack![drawn, overlay].into()
}

/// The right-click menu is open.
pub fn menu_open(s: &Studio) -> bool {
    s.text.menu.is_some()
}

/// `main` with the right-click menu over it while it is open (a press anywhere else closes it).
pub fn menu_layer<'a>(s: &'a Studio, main: Element<'a, Msg>) -> Element<'a, Msg> {
    let Some(m) = &s.text.menu else {
        return main;
    };
    iced::widget::stack![
        main,
        iced::widget::opaque(
            iced::widget::mouse_area(
                iced::widget::pin(menu::view(m).map(|m| Msg::Text(Ev::Menu(m))))
                    .x(m.at.x)
                    .y(m.at.y)
            )
            .on_press(Msg::Text(Ev::Menu(menu::Msg::Close)))
            .on_right_press(Msg::Text(Ev::Menu(menu::Msg::Close)))
        )
    ]
    .into()
}

/// The Actions card's row.
pub fn row(s: &Studio) -> Element<'_, Msg> {
    let epoch = s.epoch();
    let on = s.text.active(epoch);
    let can = s.editor.is_some() && s.card.is_none() && s.edit.is_none() && s.original.is_none();
    option_list::action(option_list::ActionRow {
        lead: option_list::glyph('\u{e1c2}', on), // lucide "mouse-pointer-2"
        label: crate::shared::app_language::tr(crate::shared::app_language::S::st_select_text),
        external: false,
        caps: Vec::new(),
        reading: s.text.reading(epoch),
        focused: false,
        press: (can || on).then_some(Msg::Text(Ev::Toggle)),
    })
}

impl Studio {
    pub(super) fn on_text(&mut self, ev: Ev) -> Outcome {
        let epoch = self.epoch;
        match ev {
            Ev::Toggle => {
                self.popover = None;
                let can = self.editor.is_some()
                    && self.card.is_none()
                    && self.edit.is_none()
                    && self.original.is_none();
                if !can && !self.text.active(epoch) {
                    return Outcome::None;
                }
                match self.text.toggle(epoch, self.picture_stamp()) {
                    Some(work) => Outcome::Text(work),
                    None => Outcome::None,
                }
            }
            _ if !self.text.active(epoch) => Outcome::None,
            Ev::Press(_) | Ev::Right(_) | Ev::Menu(_) if !self.text_fresh() => Outcome::None,
            Ev::Press(p) => {
                self.text.press(epoch, p, Instant::now());
                Outcome::None
            }
            Ev::Move(p) => {
                self.text.drag(p);
                Outcome::None
            }
            Ev::Release => {
                self.text.release();
                Outcome::None
            }
            Ev::Right(p) => {
                if self.text.on_selection(p) {
                    self.open_text_menu();
                }
                Outcome::None
            }
            Ev::Menu(msg) => {
                let Some(m) = self.text.menu.as_mut() else {
                    return Outcome::None;
                };
                match m.update(msg) {
                    Some(menu::Outcome::Run(row)) => {
                        self.text.menu = None;
                        self.text_row(row)
                    }
                    Some(menu::Outcome::Close) => {
                        self.text.menu = None;
                        Outcome::None
                    }
                    None => Outcome::None,
                }
            }
        }
    }

    /// A menu row: Select all stays here; the rest is the App's.
    fn text_row(&mut self, row: menu::Row) -> Outcome {
        if row == menu::Row::SelectAll {
            self.text.all(self.epoch);
            return Outcome::None;
        }
        self.text.work(row).map_or(Outcome::None, Outcome::Text)
    }

    /// The picture's Ctrl+C / Ctrl+A are the words' while the mode is on (Ctrl+C: only with words selected).
    pub(super) fn text_takes(&mut self, m: &crate::features::snip::annotate::Message) -> bool {
        use crate::features::snip::annotate::Message as M;
        use crate::features::snip::annotate::Message as Q;
        let epoch = self.epoch;
        if matches!(m, Q::Copy | Q::CopySelection | Q::SelectAll) && !self.text_fresh() {
            return false;
        }
        let live = self.text.active(epoch) && !self.text.reading(epoch);
        live && match m {
            M::Copy | M::CopySelection => self.text.text().is_some(),
            M::SelectAll => true,
            _ => false,
        }
    }

    /// Ctrl+C copies the selected words, Ctrl+A selects them all.
    pub(super) fn text_key(&mut self, m: &crate::features::snip::annotate::Message) -> Outcome {
        match m {
            crate::features::snip::annotate::Message::SelectAll => {
                self.text.all(self.epoch);
                Outcome::None
            }
            _ => self.text_row(menu::Row::Copy),
        }
    }

    /// The menu at the pointer (the stage's origin: as the picture's own right-click menu places it).
    fn open_text_menu(&mut self) {
        let Some(text) = self.text.text() else {
            return;
        };
        let sidebar_left = self.sidebar_at_start() != crate::shared::app_language::rtl();
        let left = match sidebar_left {
            true => self.side_room(),
            false => super::RAIL + 2.0,
        };
        let (p, at) = (
            self.pointer.get(),
            Point::new(left, super::HEADER + super::OPTIONS + 1.0),
        );
        let all = !self.text.picked.as_ref().is_some_and(|p| p.is_all());
        let entity = entities::find(&text)
            .into_iter()
            .find(|e| e.kind != entities::Kind::Phone);
        let mut m = menu::Menu::new(Point::new(at.x + p.x, at.y + p.y), all, entity, false);
        m.at = option_list::place(m.at, m.size(), self.window, super::EDGE);
        self.text.menu = Some(m);
    }

    /// The picture as the reader sees it, in one number: its pixels, size, view and how many edits it has.
    fn picture_stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let Some(e) = &self.editor else {
            return 0;
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let (d, v) = (&e.doc, e.doc.view());
        (d.image.width, d.image.height, d.depth()).hash(&mut h);
        [v.x, v.y, v.width, v.height].map(f32::to_bits).hash(&mut h);
        d.image.rgba.hash(&mut h);
        h.finish()
    }

    /// The words still belong to the picture; if not they (and the mode) are dropped. `false` = dropped.
    fn text_fresh(&mut self) -> bool {
        let epoch = self.epoch;
        if self.text.active(epoch)
            && !self.text.reading(epoch)
            && self.text.stamp != self.picture_stamp()
        {
            self.text.reset();
        }
        self.text.active(epoch)
    }

    /// The words read for picture `epoch` (the App's answer), or why none (a pill).
    pub fn set_words(&mut self, epoch: u32, read: Result<Vec<Line>, String>) -> Outcome {
        if self.editor.is_none() || epoch != self.epoch {
            return Outcome::None;
        }
        // the picture changed (an edit, an undo, a crop, the view) since the read began: its words are not its own
        if self.text.stamp != self.picture_stamp() {
            self.text.failed(epoch);
            return Outcome::None;
        }
        let Some(e) = self.editor.as_ref() else {
            return Outcome::None;
        };
        // read from the clean view: back to image px
        let view = e.doc.view();
        let at = iced::Vector::new(view.x.floor(), view.y.floor());
        let size = Size::new(e.doc.image.width as f32, e.doc.image.height as f32);
        match read {
            Ok(lines) if lines.iter().any(|l| !l.text.trim().is_empty()) => {
                let lines = lines.into_iter().map(|l| l.mapped(1.0, at)).collect();
                self.text.set(epoch, lines, size);
                Outcome::None
            }
            Ok(_) => {
                self.text.failed(epoch);
                Outcome::Say(crate::t!(sn_no_text).to_string())
            }
            Err(why) => {
                self.text.failed(epoch);
                Outcome::Say(why)
            }
        }
    }

    /// The reader's models are missing (the card asks): the mode stops, the row runs again once they are in.
    pub fn cancel_text(&mut self) {
        self.text.reset();
    }
}

/// The words as OCR lines in image px for `Studio::set_words`.
pub fn lines_of(
    blocks: Vec<crate::features::text_reading::TextBlock>,
    rgba: &[u8],
    w: u32,
    h: u32,
) -> Vec<Line> {
    let mut lines = words::lines(blocks, [0, 0], 1.0);
    words::measure(&mut lines, rgba, w, h, 1.0);
    lines
}
