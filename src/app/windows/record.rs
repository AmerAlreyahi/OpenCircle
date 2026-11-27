//! The recording bar's window and the corner thumbnail's (R-02): two floating
//! windows (`floating`: no box behind them, out of captures like every widget of
//! ours). The state machine and the fake recorder live in the App
//! (`features::snip::app::record`); this only shows what it is told to.
use iced::window::{self, Position};
use iced::{Point, Rectangle, Size, Task};

use super::{Msg, POPUP, Windows, appear, close, floating, platform, shape};
use crate::features::snip::screens::record_bar::{self, Bar};
use crate::features::snip::screens::record_result::{self, Thumb};

/// The thumbnail's distance from the screen's bottom-end corner.
const CORNER: f32 = 16.0;

/// Where the thumbnail goes: the bottom-end corner of `screen` (the end the text ends on).
pub fn thumb_spot(size: Size, screen: Rectangle, rtl: bool) -> Point {
    let x = match rtl {
        true => screen.x + CORNER,
        false => screen.x + screen.width - size.width - CORNER,
    };
    Point::new(x, screen.y + screen.height - size.height - CORNER - 48.0)
}

impl Windows {
    pub fn record_bar(&self) -> Option<&Bar> {
        self.record_bar.as_ref().map(|(_, bar)| bar)
    }

    pub fn record_bar_window(&self) -> Option<window::Id> {
        self.record_bar.as_ref().map(|(id, _)| *id)
    }

    #[cfg(test)]
    pub fn record_thumb(&self) -> Option<&Thumb> {
        self.record_thumb.as_ref().map(|(_, t)| t)
    }

    pub fn record_thumb_window(&self) -> Option<window::Id> {
        self.record_thumb.as_ref().map(|(id, _)| *id)
    }

    /// Shows `bar`: opens its window (at the remembered spot, else the top centre
    /// of `screen`), or updates it and resizes the window to what it shows now.
    pub fn show_record_bar(&mut self, bar: Bar, screen: Rectangle) -> Task<Msg> {
        let size = bar.size();
        let spot = self.record_spot;
        match &mut self.record_bar {
            Some((id, shown)) => {
                let resized = shown.size() != size;
                *shown = bar;
                let id = *id;
                match resized {
                    true => {
                        let at =
                            spot.unwrap_or_else(|| self.record_bar().unwrap().default_spot(screen));
                        window::move_to(id, at).chain(platform::monitor::resize(id, size))
                    }
                    false => Task::none(),
                }
            }
            None => {
                let at = spot.unwrap_or_else(|| bar.default_spot(screen));
                let (id, open) = floating(
                    window::Settings {
                        size,
                        position: Position::Specific(at),
                        visible: false,
                        resizable: false,
                        ..window::Settings::default()
                    },
                    record_bar::RADIUS,
                    POPUP,
                );
                self.record_bar = Some((id, bar));
                open.then(move |id| appear::<Msg>(id, POPUP))
            }
        }
    }

    pub fn close_record_bar(&mut self) -> Task<Msg> {
        match self.record_bar.take() {
            Some((id, _)) => close(id),
            None => Task::none(),
        }
    }

    /// The bar's window is dragged by its body.
    pub fn grab_record_bar(&mut self) -> Task<Msg> {
        self.record_grabbed = true;
        match self.record_bar_window() {
            Some(id) => window::drag(id),
            None => Task::none(),
        }
    }

    /// The window moved: after a drag of the bar began, its spot is kept for the session (the
    /// moves of opening and resizing are ours).
    pub(super) fn bar_moved(&mut self, at: Point) {
        if self.record_grabbed {
            self.record_spot = Some(at);
        }
    }

    /// Shows the thumbnail in the bottom-end corner of `screen`.
    pub fn show_record_thumb(&mut self, thumb: Thumb, screen: Rectangle) -> Task<Msg> {
        let size = thumb.size();
        let at = thumb_spot(size, screen, crate::shared::kit::dir::rtl());
        let radius = thumb.radius();
        let gone = self.close_record_thumb();
        let (id, open) = floating(
            window::Settings {
                size,
                position: Position::Specific(at),
                visible: false,
                resizable: false,
                ..window::Settings::default()
            },
            radius,
            POPUP,
        );
        self.record_thumb = Some((id, thumb));
        Task::batch([gone, open.then(move |id| appear::<Msg>(id, POPUP))])
    }

    pub fn close_record_thumb(&mut self) -> Task<Msg> {
        match self.record_thumb.take() {
            Some((id, _)) => close(id),
            None => Task::none(),
        }
    }

    /// The thumbnail's own messages (hover): the timer waits while it is on it.
    pub fn on_record_result(&mut self, msg: record_result::Msg) {
        if let Some((_, thumb)) = &mut self.record_thumb {
            thumb.update(msg);
        }
    }

    /// `dt` passed: the thumbnail's timer runs. `true` once it has slid away (closed here).
    pub fn tick_record_thumb(&mut self, dt: std::time::Duration) -> (bool, Task<Msg>) {
        let gone = self.record_thumb.as_mut().is_some_and(|(_, t)| t.tick(dt));
        match gone {
            true => (true, self.close_record_thumb()),
            false => (false, Task::none()),
        }
    }

    pub(super) fn record_bar_view(&self, window: window::Id) -> Option<iced::Element<'_, Msg>> {
        if let Some((_, bar)) = self.record_bar.as_ref().filter(|(w, _)| *w == window) {
            return Some(bar.view(shape::radius(record_bar::RADIUS)).map(Msg::Record));
        }
        let (_, thumb) = self.record_thumb.as_ref().filter(|(w, _)| *w == window)?;
        Some(
            thumb
                .view(shape::radius(thumb.radius()))
                .map(Msg::RecordResult),
        )
    }
}
