//! The overlay's view (moves to ui/screens with the UI plan).

use crate::t;
use iced::widget::{canvas, container, image, opaque, pin, space, stack};
use iced::window::{self};
use iced::{ContentFit, Element, Fill, Point, Rectangle, Size, Vector};

use crate::actions::qr;
use crate::shared::config;

use crate::features::text_reading::app::word;
use crate::shared::kit::panel_frame as card;
use crate::shared::kit::{motion, surface};
use crate::ui::app::message::{Message, Purpose};
use crate::ui::overlay;
use crate::ui::screens::bar::geometry as bar;
use crate::ui::screens::overlay::edge as border;
use crate::ui::screens::snip_toolbar::canvas::ScreenMap;
use crate::ui::screens::toast;
use crate::ui::snip;

use super::{App, CHIP_SPACE, LINK_WIDTH, QR_WIDTH, SELECT_WIDTH, SLIDE, TextMode};

impl App {
    /// How much of the screen edge's glow shows on `window` (`None`: the
    /// starting screen's): the mouse's screen (`screens::Glow`), and in Snip
    /// only while nothing is picked: it says "the screen is frozen"; over a
    /// picked area (the live area, long capture) it would read as "the whole
    /// screen was captured", so the area's outline alone shows (owner, 2026-10-31).
    pub(crate) fn edge_glow(&self, window: Option<window::Id>) -> f32 {
        match &self.o.purpose {
            Purpose::Snip(s) if s.scroll.is_some() || self.o.edit.is_some() => 0.0,
            _ => self.o.glow.at(window),
        }
    }

    /// The running long capture's outline, picture, hint and note, drawn by
    /// the window of the screen it runs on (`window` is that one).
    pub(crate) fn long_view(&self, window: window::Id) -> Option<Element<'_, Message>> {
        let long = self.scroll()?;
        let on = self.long_on();
        (long.window.unwrap_or(self.overlay) == window).then(|| {
            let look = crate::shared::theme::accent_of(&config::look());
            let bar = self.long_bar();
            long.view(on.work, bar, look).map(Message::Snip)
        })
    }

    pub(crate) fn view(&self, window: window::Id) -> Element<'_, Message> {
        if let Some(page) = self.windows.view(window) {
            return page.map(Message::Windows);
        }
        if let Some(live) = &self.live {
            if live.window() == Some(window) {
                return live.view().map(Message::Live);
            }
            if live.controller() == Some(window) {
                return live.controller_view().map(Message::Live);
            }
        }
        // the AI chat in its own window (`ai_chat::ChatWindow`)
        if let Some(size) = (self.chat_win.as_ref())
            .filter(|w| w.id == window)
            .map(|w| w.size)
        {
            let hover = self.o.is_some() && self.o.chat_hover;
            return match self.chat_shown() {
                Some((id, panel)) => (panel.view(Rectangle::with_size(size), size, hover))
                    .map(move |msg| Message::AiChat(id, msg)),
                None => space().into(), // (closing)
            };
        }
        if let Some(screen) = self.o.screens.iter().find(|s| s.window == window) {
            return self.screen_view(screen);
        }
        let Some(frame) = &self.o.frame else {
            return space().into();
        };
        let snip = match &self.o.purpose {
            Purpose::Snip(snip) => Some(snip),
            _ => None,
        };
        // Scrolling, the timer, the toast: the window is only a pill (Wayland:
        // the pill over the frame, see `pill_window`).
        if self.can_hide
            && let Some(pill) = snip.and_then(|s| s.pill_view())
        {
            return pill.map(Message::Snip);
        }
        let picked: Vec<Rectangle> = (self.o.text_mode.as_ref())
            .and_then(|m| m.picked.as_ref())
            .map_or_else(Vec::new, word::Picked::rects);
        // The plain dim (nothing selected) while the view moves: folded into
        // the zoomed frame's drawing (`overlay::Zoomed`), one pass.
        let dim =
            (self.o.translate.is_none() || self.o.selection.is_some()) && self.tool().is_none();
        let dim_frame = (self.view_moving() && dim)
            .then_some(())
            .filter(|_| self.o.selection.is_none() && self.o.others.is_empty())
            .map(|_| crate::shared::theme::dims(&config::look()).0.a);
        let overlay = overlay::Overlay {
            stroke: &self.o.stroke,
            selection: self.o.selection,
            others: &self.o.others,
            view: self.o.view,
            zooming: self.view_moving(),
            handles: self.handles(),
            opening: self.opening,
            first_draw: &self.first_draw,
            frame_times: &self.frame_times,
            hover: snip.is_some_and(|s| s.kind.picks() && s.scroll.is_none())
                && self.o.edit.is_none(),
            dim: dim && dim_frame.is_none(),
            live_drag: snip.is_some(),
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
            waiting: self.text_waiting(),
        };
        // (Snip's More pastes its translation into the picture: no frame of the session's)
        let own = matches!(self.o.purpose, Purpose::Snip(_));
        let translation = (self.o.translate.as_ref())
            .filter(|_| !own)
            .and_then(|(_, s)| s.image());
        // The frame is its own layer: iced draws a canvas's images above its shapes.
        let mut shown = image(translation.unwrap_or(frame).clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Fill);
        match self.o.view.crop() {
            Some(part) => shown = shown.crop(part), // zoomed in
            // pixel for pixel: no filtering (a full redraw, e.g. while the dim
            // fades in, costs a copy, not a resample)
            None => shown = shown.filter_method(image::FilterMethod::Nearest),
        }
        // At rest (not zoomed, no translation): pixel for pixel (`overlay::Frozen`).
        let shown: Element<'_, Message> = match translation {
            // zooming or panning: Nearest (fractions of a frame px)
            _ if self.view_moving() => {
                let picture = translation.unwrap_or(frame);
                let zoomed = overlay::Zoomed {
                    picture,
                    view: self.o.view,
                    dim: dim_frame.unwrap_or(0.0),
                };
                canvas(zoomed).width(Fill).height(Fill).into()
            }
            None if self.o.view.crop().is_none() => {
                let scale = self.o.scale;
                let frozen = overlay::Frozen { frame, scale };
                canvas(frozen).width(Fill).height(Fill).into()
            }
            _ => shown.into(),
        };
        // What is translated so far: only its area, over the frame (no copy of the
        // whole frame per partial).
        let part = (self.o.translate.as_ref())
            .filter(|_| !own)
            .and_then(|(_, s)| s.part())
            .and_then(|(handle, area)| {
                let (crop, at) = self.o.view.patch_in_view(area)?;
                let patch = image(handle.clone())
                    .width(at.width)
                    .height(at.height)
                    .content_fit(ContentFit::Fill)
                    .crop(crop)
                    .filter_method(image::FilterMethod::Nearest);
                Some(Element::from(pin(patch).position(at.position())))
            });
        let mut layers = stack![shown];
        if let Some(part) = part {
            layers = layers.push(part);
        }
        layers = layers.extend([
            Element::from(canvas(overlay).width(Fill).height(Fill)).map(Message::Overlay),
            // static: drawn once, with the window's fade-in
            border::view(
                if snip.is_some() {
                    border::Mode::Snip
                } else {
                    border::Mode::Overlay
                },
                self.o.scale,
                false,
                self.edge_glow(None),
            ),
        ]);
        if let Some(edit) = &self.o.edit {
            if edit.is_open() {
                // the toolbar once the bar has turned into it, in its place
                // (Snip's: unless it docks on another screen)
                let at = self.bar_at();
                let here = self.toolbar_on().is_none_or(|w| w == self.overlay);
                let toolbar = (self.swapped() >= 1.0 && here).then_some(at);
                // long capture: its outline, picture and hint under the toolbar
                // (on its own screen's window)
                if let Some(shown) = self.long_view(self.overlay) {
                    layers = layers.push(shown);
                }
                layers = layers.push(edit.view(
                    self.o.screen,
                    toolbar,
                    ScreenMap::ONE,
                    Message::Annotate,
                    Message::BarGrab,
                ));
                if toolbar.is_none() {
                    layers = layers.push(self.morphing_bar(at));
                }
                // More's results over the picture: the AI chat, the translation's
                // pill and cards, the text reader's download card by the toolbar
                if let Some(cards) = self.cards() {
                    layers = layers.push(cards);
                }
                if let Some(note) = self.reader_note() {
                    let bar = Rectangle::new(at, edit.bar_size());
                    layers = layers.push(self.beside_bar(note, bar, self.o.screen.height));
                }
                return layers.extend(self.pill_view()).into();
            }
            layers = layers.extend(edit.flat_view());
        }
        // Snip: the loupe beside the pointer (under the hint), while it is
        // on this screen (another screen's window draws its own).
        if self.o.edit.is_none()
            && let Some(loupe) = snip
                .filter(|s| s.scroll.is_none() && s.tool.is_none())
                .filter(|s| {
                    let p = s.pointer.unwrap_or(Point::ORIGIN);
                    s.monitor_at(p) == Rectangle::with_size(self.o.screen)
                })
                .and_then(|s| {
                    let look = crate::shared::theme::accent_of(&config::look());
                    let (view, screen) = (self.o.view, self.o.screen);
                    s.guides(frame, self.o.scale, view, screen, self.snip_drag(), look)
                })
        {
            layers = layers.push(loupe.map(Message::Snip));
        }
        let top = if let Some(pill) = snip.and_then(|s| s.pill_view()) {
            pill.map(Message::Snip) // Wayland only (above)
        } else if let Some(toast) = self.o.toast {
            container(surface::hint(crate::shared::app_language::tr(toast.0)))
                .center_x(Fill)
                .padding(24)
                .into()
        } else if let Some((id, song)) = &self.o.song {
            let id = *id;
            song.view(self.o.screen)
                .map(move |msg| Message::Song(id, msg))
        } else if let Some(cards) = self.cards() {
            cards
        } else if let Some(snip) = snip {
            let look = crate::shared::theme::accent_of(&config::look());
            match (&snip.scroll, &snip.tool) {
                // (drawn with the toolbar, above)
                (Some(_), _) => space().into(),
                // the colour picker or the ruler, with its toolbar here
                (None, Some(tool)) => {
                    let (scale, view, screen) = (self.o.scale, self.o.view, self.o.screen);
                    let hinting = snip.hinting;
                    (tool.view(
                        frame,
                        scale,
                        view,
                        screen,
                        true,
                        hinting,
                        &self.colours,
                        look,
                    ))
                    .map(snip::Msg::Tool)
                }
                (None, None) => snip.view(),
            }
            .map(Message::Snip)
        } else if let Some(selection) = self.o.selection {
            match self.o.purpose {
                Purpose::Search if !self.o.drawing && !self.settling() => {
                    self.selection_ui(selection, self.overlay)
                }
                _ => space().into(),
            }
        } else {
            // slides down into place as the overlay opens
            let hint = container(surface::hint(super::hint()))
                .center_x(Fill)
                .padding(24);
            let rise = (1.0 - self.appeared()) * SLIDE;
            iced::widget::float(hint)
                .translate(move |_, _| Vector::new(0.0, -rise))
                .into()
        };
        layers = layers.push(top);
        if self.bottom_bar_shown() {
            // slides up into place as the overlay opens
            let rise = (1.0 - self.appeared()) * SLIDE;
            let at = self.bar_at() + Vector::new(0.0, rise);
            // (on another screen: that screen's window draws it)
            if self.bar_window() == self.overlay {
                layers = layers.push(match self.swapped() > 0.0 {
                    true => self.morphing_bar(at), // the toolbar turning back into it
                    false => iced::widget::pin(self.bottom_bar()).position(at).into(),
                });
                if let Some(note) = self.reader_note() {
                    let bar = Rectangle::new(at, self.bar_size().1);
                    layers = layers.push(self.beside_bar(note, bar, self.o.screen.height));
                }
            }
        }
        // Beside a panel (an auto-search's): the chips.
        if let (Some(_), Some(selection), Purpose::Search, false) = (
            &self.o.ai_chat,
            self.o.selection,
            &self.o.purpose,
            self.o.drawing || self.settling(),
        ) {
            layers = layers.push(self.selection_ui(selection, self.overlay));
        }
        // The pinned chat over everything on its side (Wayland: else its own window).
        if let Some(chat) = self
            .pinned_drawn()
            .filter(|_| self.draws_chat() && self.chat_window() == self.overlay)
        {
            let id = chat.id;
            let at = self.chat_rect();
            let card = chat.panel.view(at, self.o.screen, self.o.chat_hover);
            let card = card.map(move |msg| Message::AiChat(id, msg));
            layers = layers.push(lifted(card, at, chat.panel.held));
        }
        // A black capture (DRM / protected window): say so, but keep Copy, Save and Text.
        if self.o.selection.is_some() && self.o.scan.as_ref().is_some_and(|s| s.protected) {
            let room = self.room();
            let pill = container(surface::hint(super::protected()))
                .center_x(room.width)
                .padding(24);
            layers = layers.push(iced::widget::pin(pill).position(room.position()));
        }
        layers.extend(self.pill_view()).into()
    }

    /// The AI chat card, or the translation's pill and cards (main mode's
    /// result cards; Snip's More shows them over the picked area too).
    fn cards(&self) -> Option<Element<'_, Message>> {
        // (Snip's More: Ask AI's quick question has no window of its own while
        // Snip is up, `sync_chat_window`: the overlay draws it, as it did before)
        let snip_asks = matches!(self.o.purpose, Purpose::Snip(_));
        let draws = self.draws_chat() || snip_asks;
        if self.o.ai_chat.is_some() && (!draws || self.chat_window() != self.overlay) {
            return Some(space().into()); // (its own window, or another screen's, draws the card)
        }
        if let Some((id, panel)) = &self.o.ai_chat {
            let id = *id;
            let at = self.chat_card_at();
            let card = panel
                .view(at, self.o.screen, self.o.chat_hover)
                .map(move |msg| Message::AiChat(id, msg));
            return Some(lifted(grow(card, at, self.chat_grown()), at, panel.held));
        }
        let (id, session) = self.o.translate.as_ref()?;
        let id = *id;
        Some(
            session
                .view(self.o.screen, self.chat_card_at(), self.o.chat_hover)
                .map(move |msg| Message::Translate(id, msg)),
        )
    }

    /// Wayland: the Copy / Save pill drawn over the frame at its place, with its
    /// rounded corners (no window there can be cut to it: `toast_for`).
    fn pill_view(&self) -> Option<Element<'_, Message>> {
        let (text, ok, at) = self.o.result.as_ref()?;
        let pill = toast::view(text, (*ok).into(), toast::RADIUS);
        Some(pin(pill).position(*at).into())
    }

    /// Snip: the box dragged so far (its stroke's span, image px), while drawing.
    pub(crate) fn snip_drag(&self) -> Option<Rectangle> {
        if !self.o.drawing || self.o.stroke.is_empty() {
            return None;
        }
        let (lo, hi) = (self.o.stroke.iter()).fold(
            (
                Point::new(f32::MAX, f32::MAX),
                Point::new(f32::MIN, f32::MIN),
            ),
            |(lo, hi), p| {
                (
                    Point::new(lo.x.min(p.x), lo.y.min(p.y)),
                    Point::new(hi.x.max(p.x), hi.y.max(p.y)),
                )
            },
        );
        Some(Rectangle::new(
            lo,
            Size::new((hi.x - lo.x).max(0.0), (hi.y - lo.y).max(0.0)),
        ))
    }

    /// The part of the overlay a result card leaves free (all of it without
    /// one): the bar, chips, popup and pills go there.
    pub(crate) fn room(&self) -> Rectangle {
        let card = match (&self.o.ai_chat, self.pinned_drawn()) {
            (Some(_), _) => Some(self.chat_card_at()),
            (None, Some(_)) => Some(self.chat_rect()),
            (None, None) => None,
        };
        let card = card.filter(|_| self.o.chat_on.is_none()); // (on another screen: all is free here)
        card::free(self.o.screen, card)
    }

    /// Where the bar goes, for the chips to keep off it (it is drawn over them).
    pub(crate) fn chip_bar(&self) -> Option<Rectangle> {
        (self.bottom_bar_shown()).then(|| Rectangle::new(self.bar_target(), self.dock_size()))
    }

    /// What floats around `selection`: only small chips ("Select text" under a
    /// snapped box, a QR code's); every action is in the bottom bar.
    /// Drawn by `window` only if the selection is on that screen (the rule of
    /// everything dragged across screens, `screen_holding`), in its own px (#147).
    pub(crate) fn selection_ui(
        &self,
        selection: overlay::Selection,
        window: iced::window::Id,
    ) -> Element<'_, Message> {
        let held = self.o.view.rect_to_screen(selection.area);
        if self.window_of(self.screen_holding(held)) != window {
            return space().into();
        }
        let (view, room, bar) = match self.o.screens.iter().find(|s| s.window == window) {
            None => (self.o.view, self.room(), self.chip_bar()),
            Some(s) => {
                let card = (self.o.ai_chat.is_some() || self.chat.is_some())
                    .then(|| s.view.rect_to_screen(self.chat_rect()))
                    .filter(|_| self.chat_window() == window);
                let bar = (self.bottom_bar_shown() && self.bar_window() == window)
                    .then(|| Rectangle::new(self.bar_target(), self.dock_size()))
                    .map(|r| s.view.rect_to_screen(r));
                (s.view, card::free(s.place.size(), card), bar)
            }
        };
        let area = view.rect_to_screen(selection.area);
        let mut layers = stack![];
        let mut below = area;
        if selection.kind == overlay::Kind::Snap {
            let label = match &self.o.text_mode {
                None => t!(bar_select_text),
                Some(TextMode { lines: None, .. }) => t!(bar_reading),
                Some(_) => t!(wn_done),
            };
            let size = Size::new(SELECT_WIDTH, bar::HEIGHT);
            let at = chips_at(area, size, room, bar);
            below = area.union(&Rectangle::new(at, size));
            let chip =
                crate::ui::screens::bar::view::action_chip(label.into(), Message::SelectText);
            let chip = container(chip).center_x(SELECT_WIDTH);
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        if let Some(text) = self.o.scan.as_ref().and_then(|s| s.qr.as_deref()) {
            let open = qr::web_url(text).map(|_| Message::Qr(qr::Act::Open));
            let chip =
                crate::ui::screens::bar::view::qr_chip(text, Message::Qr(qr::Act::Copy), open);
            let size = Size::new(QR_WIDTH, bar::HEIGHT);
            let at = chips_at(below, size, room, bar);
            let chip = container(chip).center_x(QR_WIDTH);
            below = below.union(&Rectangle::new(at, size));
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        // A link or email in the selected words: a click-only chip to open it.
        if let Some(entity) = self.chip_entity() {
            let open = Message::OpenEntity(entity.clone());
            let chip = crate::ui::screens::bar::view::link_chip(&entity, open);
            let size = Size::new(LINK_WIDTH, bar::HEIGHT);
            let at = chips_at(below, size, room, bar);
            let chip = container(chip).center_x(LINK_WIDTH);
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        layers.into()
    }
}

/// The card at `at`, `grown` of the way open: from `motion::OPEN_SCALE` to its
/// size, about its own middle (the design's open, without its fade: the CPU
/// renderer has no alpha for a group of widgets). Plain once open.
pub(crate) fn grow(card: Element<'_, Message>, at: Rectangle, grown: f32) -> Element<'_, Message> {
    if grown >= 1.0 {
        return card;
    }
    let s = motion::OPEN_SCALE + (1.0 - motion::OPEN_SCALE) * grown;
    let middle = at.center();
    iced::widget::float(card)
        .scale(s)
        .translate(move |bounds, _| (middle - bounds.center()) * (1.0 - s))
        .into()
}

/// A card at `at` picked up (`held`): `motion::DRAG_SCALE` about its middle
/// while dragged, 1 once let go (or with reduced motion).
pub(crate) fn lifted<'a>(
    card: Element<'a, Message>,
    at: Rectangle,
    held: bool,
) -> Element<'a, Message> {
    if !held || motion::reduced() {
        return card;
    }
    let (s, middle) = (motion::DRAG_SCALE, at.center());
    iced::widget::float(card)
        .scale(s)
        .translate(move |bounds, _| (middle - bounds.center()) * (1.0 - s))
        .into()
}

/// Where the chips (`size`) go: centred under the box `area`, `CHIP_SPACE`
/// below it, else above it, else under the bar (`dock`) when the bar is on
/// the box's other side; inside `room` (logical px), never on the box or the
/// bar unless the box fills the screen.
pub(crate) fn chips_at(
    area: Rectangle,
    size: Size,
    room: Rectangle,
    dock: Option<Rectangle>,
) -> Point {
    let x = (area.center_x() - size.width / 2.0)
        .min(room.x + room.width - bar::GAP - size.width)
        .max(room.x + bar::GAP);
    let (top, bottom) = (room.y + bar::GAP, room.y + room.height - bar::GAP);
    let below = area.y + area.height + CHIP_SPACE;
    let above = area.y - CHIP_SPACE - size.height;
    let past_bar = dock.map(|d| d.y + d.height + CHIP_SPACE);
    let free = |y: &f32| {
        *y >= top
            && *y + size.height <= bottom
            && dock.is_none_or(|d| {
                d.intersection(&Rectangle::new(Point::new(x, *y), size))
                    .is_none()
            })
            && Rectangle::new(Point::new(x, *y), size)
                .intersection(&area)
                .is_none()
    };
    let y = [below, above]
        .into_iter()
        .chain(past_bar)
        .find(free)
        .unwrap_or(bottom - size.height); // the box fills the screen: on it, at the bottom
    Point::new(x, y)
}
