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
use crate::features::translate::{Line, Matched, Tweak};
use crate::shared::kit::area;

/// The session: lines are `None` while they are being read.
pub struct Session {
    /// Which read this session waits for: an answer for another is dropped.
    pub token: u32,
    pub lines: Option<Vec<Line>>,
    pub active: Option<usize>,
    pub draft: String,
    /// The picked line's look as the erase reads it; what the user set over it.
    pub matched: Option<Matched>,
    em: Option<f32>,
    colour: Option<[u8; 3]>,
    bold: Option<bool>,
}

impl Session {
    pub fn new(token: u32) -> Self {
        Session {
            token,
            lines: None,
            active: None,
            draft: String::new(),
            matched: None,
            em: None,
            colour: None,
            bold: None,
        }
    }

    /// Line `i` is picked, with its matched look; the user's settings start over.
    pub fn pick(&mut self, i: usize, matched: Matched) {
        if let Some(l) = self.lines.as_ref().and_then(|l| l.get(i)) {
            self.draft = l.text.clone();
            self.active = Some(i);
            self.matched = Some(matched);
            (self.em, self.colour, self.bold) = (None, None, None);
        }
    }

    /// A setting from the bar's controls (the editor's own messages).
    pub fn set(&mut self, m: crate::features::snip::annotate::Message) {
        use crate::features::snip::annotate::Message as M;
        match m {
            M::TextSize(v) => self.em = Some(v),
            // (back to what was read: a semibold line stays semibold)
            M::TextStyle(s) => {
                self.bold = Some(s.bold).filter(|b| Some(*b) != self.matched.map(|m| m.bold))
            }
            M::Colour(c) => self.colour = Some([c[0], c[1], c[2]]),
            _ => {}
        }
    }

    pub fn size(&self) -> f32 {
        self.em.or(self.matched.map(|m| m.em)).unwrap_or(16.0)
    }

    pub fn bold(&self) -> bool {
        self.bold.or(self.matched.map(|m| m.bold)).unwrap_or(false)
    }

    pub fn colour(&self) -> [u8; 4] {
        let [r, g, b] = self
            .colour
            .or(self.matched.map(|m| m.colour))
            .unwrap_or([0; 3]);
        [r, g, b, 255]
    }

    /// Only what the user changed; the rest stays matched.
    pub fn tweak(&self) -> Tweak {
        Tweak {
            em: self.em,
            colour: self.colour,
            bold: self.bold,
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

/// The rows round `line` (full width, a few line heights above and below):
/// all that matching and redrawing need, so a 4K picture is not copied whole.
/// Returns the first row, the rows, and the line moved into them.
pub fn band(rgba: &[u8], width: u32, height: u32, line: &Line) -> (usize, Vec<u8>, Line) {
    let [_, y, _, h] = line.bounds;
    let margin = 2.0 * h + 32.0;
    let top = ((y - margin).max(0.0) as usize).min(height as usize);
    let end = (((y + h + margin).ceil().max(0.0)) as usize).clamp(top, height as usize);
    let stride = width as usize * 4;
    let mut moved = line.clone();
    moved.bounds[1] -= top as f32;
    (top, rgba[top * stride..end * stride].to_vec(), moved)
}
