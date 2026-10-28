//! The result panel and card, and the crops they take.

use iced::futures::channel::mpsc::UnboundedSender;
use iced::widget::image;
use iced::{Rectangle, Task};

use crate::actions::{self, Crop, lens, stack_crops};

use crate::ui::app::background::background;
use crate::ui::app::keys;
use crate::ui::app::message::Message;
use crate::ui::kit::motion;
use crate::ui::widgets::card;
use crate::ui::{panel, widgets};

use super::App;

impl App {
    pub(in crate::ui) fn open_panel(
        &mut self,
        kind: actions::Panel,
        crop: Crop,
        question: Option<String>,
    ) -> Task<Message> {
        self.panels = self.panels.wrapping_add(1);
        let id = self.panels;
        let (panel, task) = panel::Panel::open(kind, crop, question);
        self.o.panel = Some((id, panel));
        task.map(move |msg| Message::Panel(id, msg))
    }

    pub(in crate::ui) fn on_panel(&mut self, id: u32, msg: panel::Msg) -> Task<Message> {
        // Every opened page is held, whichever panel is showing now.
        if let panel::Msg::LensOpened(Ok(page)) = &msg {
            if self.o.frame.is_some() {
                self.o.lens_pages.push(page.clone());
                // The search is in the browser now: the overlay, its panel and
                // pills must not stay on screen behind or over it. Close.
                return self.close();
            } else {
                page.clone().release(); // the overlay closed meanwhile
            }
        }
        let Some((_, panel)) = self.o.panel.as_mut().filter(|(open, _)| *open == id) else {
            return Task::none();
        };
        match msg {
            panel::Msg::Card(card::Event::Grab(how)) => {
                self.o.card_drag = Some((how, None));
                self.frame_times.borrow_mut().clear();
            }
            panel::Msg::Card(card::Event::Hover(on)) => self.o.card_hover = on,
            panel::Msg::Card(card::Event::Reset) => self.card = None,
            panel::Msg::Card(card::Event::Close) => self.o.panel = None,
            panel::Msg::Cancel => self.o.panel = None,
            panel::Msg::Search => return self.run(&lens::ACTION),
            // A card's focused field gave Ctrl+C back (nothing selected in it).
            panel::Msg::Key(keys::Keyed::Copy) => {
                return self.update(Message::Field(keys::Keyed::Copy));
            }
            panel::Msg::Key(_) => {}
            panel::Msg::CopyText => {
                let text = panel.text().unwrap_or_default();
                let copy = background(move |_: &UnboundedSender<()>| {
                    if let Err(e) = actions::copy::text(text) {
                        crate::log!("copy text failed: {e}");
                    }
                });
                return Task::batch([self.close(), copy.discard()]);
            }
            // Lens follow-up: hand the question over to Ask AI.
            panel::Msg::Submit if panel.kind() == actions::Panel::Lens => {
                let question = panel.take_input();
                if question.trim().is_empty() {
                    return Task::none();
                }
                let Some(crop) = self.crop() else {
                    return Task::none();
                };
                return self.open_panel(actions::Panel::Ask, crop, Some(question));
            }
            msg => return panel.update(msg).map(move |msg| Message::Panel(id, msg)),
        }
        Task::none()
    }

    /// The selection cut out of the frozen frame, in physical pixels; with the
    /// edits once it was edited (Ask AI then sees the annotations).
    pub(in crate::ui) fn crop(&self) -> Option<Crop> {
        if let Some(edit) = &self.o.edit {
            return Some(edit.image());
        }
        let (
            Some(image::Handle::Rgba {
                width,
                height,
                pixels,
                ..
            }),
            Some(selection),
        ) = (&self.o.frame, self.o.selection)
        else {
            return None;
        };
        let all = (self.o.others.iter()).fold(selection.area, |all, (s, _)| all.union(&s.area));
        actions::crop(pixels, *width, *height, self.o.scale, all)
    }

    /// Every selection's crop, in the order made, stacked top to bottom on white
    /// (one image: the Text panel reads them in that order, Ask AI sees them all).
    /// One selection: just its crop.
    // ponytail: one stacked image, not one image per selection: the Ask panel
    // (panel.rs) sends one; give `ai::Message` several images when it can.
    pub(in crate::ui) fn stacked(&self) -> Option<Crop> {
        if self.o.others.is_empty() || self.o.edit.is_some() {
            return self.crop();
        }
        let Some(image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        }) = &self.o.frame
        else {
            return None;
        };
        let areas =
            (self.o.others.iter().map(|(s, _)| s.area)).chain(self.o.selection.map(|s| s.area));
        let crops: Vec<Crop> = areas
            .filter_map(|area| actions::crop(pixels, *width, *height, self.o.scale, area))
            .collect();
        stack_crops(&crops)
    }

    /// Where the result card shows: where the user left it, else away from
    /// the selection (`widgets::card::place`).
    pub(in crate::ui) fn card_at(&self) -> Rectangle {
        let selection = self.o.selection.map(|s| self.o.view.rect_to_screen(s.area));
        widgets::card::place(self.o.screen, selection, self.card)
    }

    /// How open the result card is (0..1; 0 before its first frame, 1 at once
    /// when the OS asks for no animations).
    pub(in crate::ui) fn card_grown(&self) -> f32 {
        match (&self.o.panel, self.o.card_open) {
            _ if motion::reduced() => 1.0,
            (Some((id, _)), Some((was, _, grown))) if *id == was => grown,
            _ => 0.0,
        }
    }

    /// The result card still opening (frames wanted).
    pub(in crate::ui) fn card_opening(&self) -> bool {
        self.o.frame.is_some() && self.o.panel.is_some() && self.card_grown() < 1.0
    }
}
