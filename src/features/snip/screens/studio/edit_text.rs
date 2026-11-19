//! The Studio's Edit text (v0.4.5, STUDIO_V2_SPEC S8): the picture's lines are
//! found (dashed in the Active Look), one is picked and retyped, and Done
//! replaces it in the pixels with the erase-and-redraw Translate uses
//! (`translate::replace_line`): matched size, colour and alignment, never a
//! box over the old text. One undoable edit of the picture; Esc / Cancel
//! leave it as it was.
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action};
use iced::{Color, Length, Point, Rectangle, Renderer, Size, Theme};

use super::Msg;
use crate::features::translate::Line;
use crate::shared::kit::area;

/// The session: lines are `None` while they are being read.
pub struct Session {
    pub lines: Option<Vec<Line>>,
    pub active: Option<usize>,
    pub draft: String,
}

impl Session {
    pub fn new() -> Self {
        Session {
            lines: None,
            active: None,
            draft: String::new(),
        }
    }

    pub fn pick(&mut self, i: usize) {
        if let Some(l) = self.lines.as_ref().and_then(|l| l.get(i)) {
            self.draft = l.text.clone();
            self.active = Some(i);
        }
    }

    /// The picked line, and what it is to say.
    pub fn target(&self) -> Option<(&Line, &str)> {
        let line = self.lines.as_ref()?.get(self.active?)?;
        Some((line, self.draft.trim()))
    }
}

/// The lines over the picture: dashed, the picked one solid; a press on one
/// picks it, a press elsewhere is taken (the editor's tool must not draw).
pub struct Overlay<'a> {
    pub lines: &'a [Line],
    pub active: Option<usize>,
    /// The view's top-left (image px) and display px per image px.
    pub origin: Point,
    pub zoom: f32,
    pub look: (Color, Color),
}

impl Overlay<'_> {
    fn rect(&self, l: &Line) -> Rectangle {
        let [x, y, w, h] = l.bounds;
        Rectangle::new(
            Point::new(
                (x - self.origin.x) * self.zoom,
                (y - self.origin.y) * self.zoom,
            ),
            Size::new(w * self.zoom, h * self.zoom),
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
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let at = cursor.position_in(bounds)?;
        let hit = (self.lines.iter().enumerate())
            .filter(|(_, l)| self.rect(l).contains(at))
            .min_by(|a, b| area_of(self.rect(a.1)).total_cmp(&area_of(self.rect(b.1))));
        Some(match hit {
            Some((i, _)) => Action::publish(Msg::PickLine(i)).and_capture(),
            None => Action::capture(),
        })
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
        for (i, l) in self.lines.iter().enumerate() {
            let r = self.rect(l);
            match Some(i) == self.active {
                true => area::outline(&mut frame, r, self.look),
                false => area::dashed(&mut frame, r, self.look),
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &(), bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(at) if self.lines.iter().any(|l| self.rect(l).contains(at)) => {
                mouse::Interaction::Text
            }
            _ => mouse::Interaction::default(),
        }
    }
}

fn area_of(r: Rectangle) -> f32 {
    r.width * r.height
}

/// The overlay as an element the size of the shown picture.
pub fn overlay<'a>(o: Overlay<'a>, shown: Size) -> iced::Element<'a, Msg> {
    canvas::Canvas::new(o)
        .width(Length::Fixed(shown.width))
        .height(Length::Fixed(shown.height))
        .into()
}
