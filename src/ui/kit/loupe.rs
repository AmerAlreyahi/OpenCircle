//! The loupe (snip's magnifier): the pixels around the pointer at 8x, the one
//! under it framed, its coordinates (frame px) and colour; and, while picking,
//! the outline of what a click would capture with its label. Drawn on a canvas
//! in the theme's widget, rim, text and accent.
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Geometry, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use super::themes::c;

/// Physical px each side of the one under the pointer, and the logical size
/// each is drawn at (8x zoom).
pub const RADIUS: i32 = 7;
const CELL: f32 = 8.0;

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

/// The magnifier: the pixels around the pointer at 8x, the one under it framed,
/// its coordinates (frame px) and colour.
pub struct Loupe<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    /// The pointer, logical px.
    pub at: Point,
    /// What a click would capture, and its label.
    pub outline: Option<(Rectangle, String)>,
}

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
        if let Some((r, label)) = &self.outline {
            let border = canvas::Path::rectangle(
                Point::new(r.x + 1.0, r.y + 1.0),
                Size::new((r.width - 2.0).max(0.0), (r.height - 2.0).max(0.0)),
            );
            frame.stroke(
                &border,
                Stroke::default().with_color(c().accent).with_width(2.0),
            );
            // The label above the outline, or inside it at the top of the screen.
            let width = 7.2 * label.chars().count() as f32 + 16.0;
            let y = if r.y >= 30.0 { r.y - 28.0 } else { r.y + 4.0 };
            let x = r.x.clamp(0.0, (bounds.width - width).max(0.0));
            let tag = canvas::Path::rounded_rectangle(
                Point::new(x, y),
                Size::new(width, 24.0),
                6.0.into(),
            );
            frame.fill(&tag, c().widget);
            frame.fill_text(canvas::Text {
                content: label.clone(),
                position: Point::new(x + 8.0, y + 5.0),
                color: c().text,
                size: 12.0.into(),
                ..canvas::Text::default()
            });
        }
        let centre = (
            (self.at.x * self.scale).floor() as i32,
            (self.at.y * self.scale).floor() as i32,
        );
        let cells = sample(self.pixels, self.width, self.height, centre, RADIUS);
        let n = 2 * RADIUS + 1;
        let side = n as f32 * CELL;
        let label = 24.0;
        // Below-right of the pointer; flipped where it would leave the screen.
        let gap = 24.0;
        let x = if self.at.x + gap + side < bounds.width {
            self.at.x + gap
        } else {
            self.at.x - gap - side
        };
        let y = if self.at.y + gap + side + label < bounds.height {
            self.at.y + gap
        } else {
            self.at.y - gap - side - label
        };
        let body = canvas::Path::rectangle(
            Point::new(x - 2.0, y - 2.0),
            Size::new(side + 4.0, side + label + 4.0),
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
        let [r, g, b, _] = cells[cells.len() / 2];
        frame.fill_text(canvas::Text {
            content: format!("{}, {}   #{r:02X}{g:02X}{b:02X}", centre.0, centre.1),
            position: Point::new(x + 6.0, y + side + 5.0),
            color: c().text,
            size: 12.0.into(),
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}
