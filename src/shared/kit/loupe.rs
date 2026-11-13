//! The loupe (snip's magnifier): the pixels around the pointer at 9x, the one
//! under it framed, its colour (`#RRGGBB`) and coordinates (frame px); and,
//! while picking, the outline of what a click would capture (`area::outline`,
//! in the active look's pair; its info chip is a widget, `chip::info`). Drawn
//! on a canvas in the theme's widget, rim, text and accent. While an area is
//! dragged it keeps off it: diagonally away from it, outward from the corner
//! being dragged.
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Geometry, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use super::themes::c;

/// Physical px each side of the one under the pointer, and the logical size
/// each is drawn at (9x zoom).
pub const RADIUS: i32 = 7;
const CELL: f32 = 9.0;

/// The `2 * radius + 1` square of pixels around `centre` (frame px), row by row;
/// outside the frame = transparent.
pub fn sample(
    rgba: &[u8],
    width: u32,
    height: u32,
    centre: (i32, i32),
    radius: i32,
) -> Vec<[u8; 4]> {
    let mut out = Vec::with_capacity(((2 * radius + 1) * (2 * radius + 1)) as usize);
    for y in centre.1 - radius..=centre.1 + radius {
        for x in centre.0 - radius..=centre.0 + radius {
            let inside = (0..width as i32).contains(&x) && (0..height as i32).contains(&y);
            let i = (y.max(0) as usize * width as usize + x.max(0) as usize) * 4;
            out.push(match rgba.get(i..i + 4) {
                Some(&[r, g, b, a]) if inside => [r, g, b, a],
                _ => [0; 4],
            });
        }
    }
    out
}

/// The magnifier: the pixels around the pointer at 9x, the one under it framed,
/// its coordinates (frame px) and colour.
pub struct Loupe<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    /// The pointer, logical px.
    pub at: Point,
    /// What a click would capture.
    pub outline: Option<Rectangle>,
    /// The active look's pair (the outline's colours).
    pub look: (Color, Color),
    /// The area being dragged (its corner is at the pointer): the loupe keeps off it.
    pub away: Option<Rectangle>,
}

/// The loupe's distance from the pointer.
const GAP: f32 = 22.0;

impl<Message> canvas::Program<Message> for Loupe<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // ponytail: square corners; Windows 11 rounds its windows by 8 px (DWM
        // corner preference) and the outline could follow.
        if let Some(r) = self.outline {
            // just inside: no outline pixel off the screen
            let inside = Rectangle::new(
                Point::new(r.x + 1.0, r.y + 1.0),
                Size::new((r.width - 2.0).max(0.0), (r.height - 2.0).max(0.0)),
            );
            super::area::outline(&mut frame, inside, self.look);
        }
        let centre = (
            (self.at.x * self.scale).floor() as i32,
            (self.at.y * self.scale).floor() as i32,
        );
        let cells = sample(self.pixels, self.width, self.height, centre, RADIUS);
        let n = 2 * RADIUS + 1;
        let side = n as f32 * CELL;
        let label = 24.0;
        // Below-right of the pointer, or away from the dragged area (outward
        // from its corner); flipped where it would leave the screen.
        let (right, down) = match self.away {
            Some(r) => (self.at.x >= r.center_x(), self.at.y >= r.center_y()),
            None => (true, true),
        };
        let (w, h) = (side, side + label);
        let fits_x = |right: bool| match right {
            true => self.at.x + GAP + w < bounds.width,
            false => self.at.x - GAP - w > 0.0,
        };
        let fits_y = |down: bool| match down {
            true => self.at.y + GAP + h < bounds.height,
            false => self.at.y - GAP - h > 0.0,
        };
        let right = if fits_x(right) { right } else { !right };
        let down = if fits_y(down) { down } else { !down };
        let x = if right {
            self.at.x + GAP
        } else {
            self.at.x - GAP - w
        };
        let y = if down {
            self.at.y + GAP
        } else {
            self.at.y - GAP - h
        };
        let body = canvas::Path::rounded_rectangle(
            Point::new(x - 6.0, y - 6.0),
            Size::new(side + 12.0, side + label + 10.0),
            12.0.into(),
        );
        frame.fill(&body, c().widget);
        frame.stroke(&body, Stroke::default().with_color(c().rim).with_width(1.0));
        for (i, &[r, g, b, a]) in cells.iter().enumerate() {
            let (col, row) = (i as i32 % n, i as i32 / n);
            let at = Point::new(x + col as f32 * CELL, y + row as f32 * CELL);
            let colour = Color::from_rgba8(r, g, b, f32::from(a) / 255.0);
            frame.fill_rectangle(at, Size::new(CELL, CELL), colour);
        }
        // The crosshair: a thin line through the middle row and column, the
        // pixel itself framed.
        let mid = RADIUS as f32 * CELL;
        let line = Stroke::default()
            .with_color(c().accent.scale_alpha(0.5))
            .with_width(1.0);
        let half = CELL / 2.0;
        frame.stroke(
            &canvas::Path::line(
                Point::new(x, y + mid + half),
                Point::new(x + side, y + mid + half),
            ),
            line,
        );
        frame.stroke(
            &canvas::Path::line(
                Point::new(x + mid + half, y),
                Point::new(x + mid + half, y + side),
            ),
            line,
        );
        let pixel = canvas::Path::rectangle(Point::new(x + mid, y + mid), Size::new(CELL, CELL));
        frame.stroke(
            &pixel,
            Stroke::default().with_color(Color::WHITE).with_width(2.0),
        );
        // a swatch, the true colour's hex, the position (muted)
        let [r, g, b, _] = cells[cells.len() / 2];
        let swatch = canvas::Path::rounded_rectangle(
            Point::new(x + 4.0, y + side + 6.0),
            Size::new(12.0, 12.0),
            3.0.into(),
        );
        frame.fill(&swatch, Color::from_rgb8(r, g, b));
        frame.stroke(
            &swatch,
            Stroke::default().with_color(c().rim).with_width(1.0),
        );
        frame.fill_text(canvas::Text {
            content: format!("#{r:02X}{g:02X}{b:02X}"),
            position: Point::new(x + 21.0, y + side + 5.0),
            color: c().text,
            size: 11.5.into(),
            font: super::text::MONO,
            ..canvas::Text::default()
        });
        frame.fill_text(canvas::Text {
            content: format!("{}, {}", centre.0, centre.1),
            position: Point::new(x + side - 2.0, y + side + 5.5),
            color: c().text_muted,
            size: 10.5.into(),
            font: super::text::MONO,
            align_x: iced::widget::text::Alignment::Right,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}
