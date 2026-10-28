//! The overlay's view (moves to ui/screens with the UI plan).

use iced::widget::{canvas, container, image, opaque, pin, space, stack};
use iced::window::{self};
use iced::{ContentFit, Element, Fill, Point, Rectangle, Size, Vector};

use crate::actions::qr;
use crate::config::{self};

use crate::ui::app::message::{Message, Purpose};
use crate::ui::app::word;
use crate::ui::kit::panel_frame as card;
use crate::ui::kit::{motion, surface};
use crate::ui::screens::bar::geometry as bar;
use crate::ui::screens::overlay::edge as border;
use crate::ui::{overlay, widgets};

use super::{App, CHIP_SPACE, HINT, LINK_WIDTH, PROTECTED, QR_WIDTH, SLIDE, TextMode};

impl App {
    pub(in crate::ui) fn view(&self, window: window::Id) -> Element<'_, Message> {
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
        // the pinned chat in its own window (the overlay is closed)
        if let Some(chat) = self.chat.as_ref().filter(|c| c.window == Some(window)) {
            let (id, size) = (chat.id, chat.size);
            return (chat.panel.view(Rectangle::with_size(size), size, false))
                .map(move |msg| Message::Panel(id, msg));
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
        let dim = self.o.translate.is_none() || self.o.selection.is_some();
        let dim_frame = (self.view_moving() && dim)
            .then_some(())
            .filter(|_| self.o.selection.is_none() && self.o.others.is_empty())
            .map(|_| crate::ui::theme::dims(&config::look()).0.a);
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
            hover: snip.is_some_and(|s| s.kind.picks()) && self.o.edit.is_none(),
            dim: dim && dim_frame.is_none(),
            live_drag: snip.is_some(),
            look: config::look(),
            morph: (self.o.morph.as_ref()).map(|(from, at)| (from.as_slice(), *at)),
            words: (self.o.text_mode.as_ref()).map_or(&[], |m| m.boxes.as_slice()),
            picked,
            text: match self.o.translate {
                None if matches!(self.o.purpose, Purpose::Search) => {
                    self.o.screen_text.iter().map(|l| l.bounds).collect()
                }
                _ => Vec::new(),
            },
        };
        let translation = self.o.translate.as_ref().and_then(|(_, s)| s.image());
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
                // the glow breathes once the opening is over, and rests while
                // anything else moves
                match self.o.appear {
                    Some((_, t)) if t >= 1.0 => {
                        if self.animating() {
                            border::Breathe::Paused
                        } else {
                            border::Breathe::On
                        }
                    }
                    _ => border::Breathe::Off,
                },
            ),
        ]);
        if let Some(edit) = &self.o.edit {
            if edit.is_open() {
                // the toolbar once the bar has turned into it, in its place
                let at = self.bar_at();
                let toolbar = (self.swapped() >= 1.0).then_some(at);
                layers = layers.push(edit.view(
                    self.o.screen,
                    toolbar,
                    Message::Editor,
                    Message::BarGrab,
                ));
                if toolbar.is_none() {
                    layers = layers.push(self.morphing_bar(at));
                }
                return layers.into();
            }
            layers = layers.extend(edit.flat_view());
        }
        // Snip: the loupe beside the pointer (under the hint).
        if self.o.edit.is_none()
            && let Some(loupe) = snip.and_then(|s| s.guides(frame, self.o.scale, self.o.drawing))
        {
            layers = layers.push(loupe.map(Message::Snip));
        }
        let top = if let Some(pill) = snip.and_then(|s| s.pill_view()) {
            pill.map(Message::Snip) // Wayland only (above)
        } else if let Some(toast) = self.o.toast {
            container(widgets::pill(toast))
                .center_x(Fill)
                .padding(24)
                .into()
        } else if let Some((id, song)) = &self.o.song {
            let id = *id;
            song.view(self.o.screen)
                .map(move |msg| Message::Song(id, msg))
        } else if let Some((id, panel)) = &self.o.panel {
            let id = *id;
            let at = self.card_at();
            let card = panel
                .view(at, self.o.screen, self.o.card_hover)
                .map(move |msg| Message::Panel(id, msg));
            grow(card, at, self.card_grown())
        } else if let Some((id, session)) = &self.o.translate {
            let id = *id;
            session
                .view(self.o.screen, self.card_at(), self.o.card_hover)
                .map(move |msg| Message::Translate(id, msg))
        } else if let Some(snip) = snip {
            snip.view().map(Message::Snip)
        } else if let Some(selection) = self.o.selection {
            match self.o.purpose {
                Purpose::Search if !self.o.drawing && !self.settling() => {
                    self.selection_ui(selection)
                }
                _ => space().into(),
            }
        } else {
            // slides down into place as the overlay opens
            let hint = container(surface::hint(HINT)).center_x(Fill).padding(24);
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
            layers = layers.push(match self.swapped() > 0.0 {
                true => self.morphing_bar(at), // the toolbar turning back into it
                false => iced::widget::pin(self.bottom_bar()).position(at).into(),
            });
            if let Some(note) = self.reader_note() {
                layers = layers.push(self.beside_bar(note, at));
            }
        }
        // Beside a panel (an auto-search's): the chips.
        if let (Some(_), Some(selection), Purpose::Search, false) = (
            &self.o.panel,
            self.o.selection,
            &self.o.purpose,
            self.o.drawing || self.settling(),
        ) {
            layers = layers.push(self.selection_ui(selection));
        }
        // The pinned chat over everything on its side.
        if let Some(chat) = &self.chat {
            let id = chat.id;
            let card = chat.panel.view(self.chat_rect(), self.o.screen, false);
            layers = layers.push(card.map(move |msg| Message::Panel(id, msg)));
        }
        // A black capture (DRM / protected window): say so, but keep Copy, Save and Text.
        if self.o.selection.is_some() && self.o.scan.as_ref().is_some_and(|s| s.protected) {
            let room = self.room();
            let pill = container(surface::hint(PROTECTED))
                .center_x(room.width)
                .padding(24);
            layers = layers.push(iced::widget::pin(pill).position(room.position()));
        }
        layers.into()
    }

    /// The part of the overlay a result card leaves free (all of it without
    /// one): the bar, chips, popup and pills go there.
    pub(in crate::ui) fn room(&self) -> Rectangle {
        let card = match (&self.o.panel, &self.chat) {
            (Some(_), _) => Some(self.card_at()),
            (None, Some(_)) => Some(self.chat_rect()),
            (None, None) => None,
        };
        card::free(self.o.screen, card)
    }

    /// What floats around `selection`: only small chips ("Select text" under a
    /// snapped box, a QR code's); every action is in the bottom bar.
    pub(in crate::ui) fn selection_ui(
        &self,
        selection: overlay::Selection,
    ) -> Element<'_, Message> {
        let area = self.o.view.rect_to_screen(selection.area);
        let room = self.room();
        let mut layers = stack![];
        let mut below = area;
        if selection.kind == overlay::Kind::Snap {
            let label = match &self.o.text_mode {
                None => "Select text",
                Some(TextMode { lines: None, .. }) => "Reading text…",
                Some(_) => "Done",
            };
            let size = Size::new(widgets::TEXT_BUTTON_WIDTH, widgets::CONTROL);
            let at = chips_at(area, size, room);
            below = area.union(&Rectangle::new(at, size));
            let chip = widgets::float_chip(label, Message::SelectText);
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        if let Some(text) = self.o.scan.as_ref().and_then(|s| s.qr.as_deref()) {
            let open = qr::web_url(text).map(|_| Message::Qr(qr::Act::Open));
            let chip =
                crate::ui::screens::bar::view::qr_chip(text, Message::Qr(qr::Act::Copy), open);
            let size = Size::new(QR_WIDTH, bar::HEIGHT);
            let at = chips_at(below, size, room);
            let chip = container(chip).center_x(QR_WIDTH);
            below = below.union(&Rectangle::new(at, size));
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        // A link or email in the selected words: a click-only chip to open it.
        if let Some(entity) = self.chip_entity() {
            let open = Message::OpenEntity(entity.clone());
            let chip = crate::ui::screens::bar::view::link_chip(&entity, open);
            let size = Size::new(LINK_WIDTH, bar::HEIGHT);
            let at = chips_at(below, size, room);
            let chip = container(chip).center_x(LINK_WIDTH);
            layers = layers.push(iced::widget::pin(opaque(chip)).position(at));
        }
        layers.into()
    }
}

/// The card at `at`, `grown` of the way open: from `motion::OPEN_SCALE` to its
/// size, about its own middle (the design's open, without its fade: the CPU
/// renderer has no alpha for a group of widgets). Plain once open.
pub(in crate::ui) fn grow(
    card: Element<'_, Message>,
    at: Rectangle,
    grown: f32,
) -> Element<'_, Message> {
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

/// Where the chips (`size`) go: centred under the box `area`, `CHIP_SPACE`
/// below it, else above it; inside `room` (logical px), never on the box
/// unless it fills the screen.
pub(in crate::ui) fn chips_at(area: Rectangle, size: Size, room: Rectangle) -> Point {
    let x = (area.center_x() - size.width / 2.0)
        .min(room.x + room.width - bar::GAP - size.width)
        .max(room.x + bar::GAP);
    let (top, bottom) = (room.y + bar::GAP, room.y + room.height - bar::GAP);
    let below = area.y + area.height + CHIP_SPACE;
    let above = area.y - CHIP_SPACE - size.height;
    let y = if below + size.height <= bottom {
        below
    } else if above >= top {
        above
    } else {
        bottom - size.height // the box fills the screen: on it, at the bottom
    };
    Point::new(x, y)
}
