//! Style presets (one click: colour + weight + the tool they are for) and the
//! copy / paste style between items.
use iced::Color;

use super::document::{Item, Shape};
use super::{Tool, Width};

/// The editor's colour picker (annotations keep their colour in both themes:
/// they belong to the image, not the UI). The first is the default.
pub const ANNOTATION: [Color; 12] = [
    Color::from_rgb8(0xff, 0x3b, 0x30), // red
    Color::from_rgb8(0xff, 0x95, 0x00), // orange
    Color::from_rgb8(0xff, 0xd6, 0x0a), // yellow
    Color::from_rgb8(0x34, 0xc7, 0x59), // green
    Color::from_rgb8(0x00, 0xc7, 0xbe), // teal
    Color::from_rgb8(0x0a, 0x84, 0xff), // blue
    Color::from_rgb8(0x5e, 0x5c, 0xe6), // indigo
    Color::from_rgb8(0xaf, 0x52, 0xde), // purple
    Color::from_rgb8(0xff, 0x2d, 0x9a), // pink
    Color::from_rgb8(0xff, 0xff, 0xff), // white
    Color::from_rgb8(0x8e, 0x8e, 0x93), // grey
    Color::from_rgb8(0x1c, 0x1c, 0x1e), // black
];

/// What the canvas draws over the image (the chrome, the same in both themes):
/// the dim outside a crop, the handles and crop edge, a step's note pill.
pub const DIM_OUTSIDE: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.45);
pub const HANDLE: Color = Color::WHITE;
pub const NOTE_FILL: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.94);
pub const NOTE_INK: Color = Color::from_rgb8(28, 28, 36);
pub const NOTE_HINT: Color = Color::from_rgb8(120, 120, 130);
pub const NOTE_FONT: iced::Font = iced::Font {
    family: iced::font::Family::SansSerif,
    weight: iced::font::Weight::Semibold,
    ..iced::Font::DEFAULT
};

pub struct Preset {
    /// On its chip.
    pub short: &'static str,
    /// Its tooltip (with its key, `key`).
    pub name: &'static str,
    pub key: &'static str,
    /// Index into `ANNOTATION`.
    pub colour: usize,
    pub width: Width,
    /// The tool it switches to, unless the active one is in `keeps`.
    pub tool: Tool,
    pub keeps: &'static [Tool],
}

/// Keys 1–9 pick these, in order.
pub const PRESETS: &[Preset] = &[
    Preset {
        short: "Red box",
        name: "Red box + arrow",
        key: "1",
        colour: 0,
        width: Width::Medium,
        tool: Tool::Rect,
        keeps: &[
            Tool::Rect,
            Tool::Ellipse,
            Tool::Arrow,
            Tool::Line,
            Tool::ArrowLabel,
            Tool::Pointer,
        ],
    },
    Preset {
        short: "Highlight",
        name: "Highlight yellow",
        key: "2",
        colour: 2,
        width: Width::Medium,
        tool: Tool::Highlighter,
        keeps: &[Tool::Highlighter],
    },
    Preset {
        short: "Callout",
        name: "Blue callout",
        key: "3",
        colour: 5,
        width: Width::Medium,
        tool: Tool::Callout,
        keeps: &[Tool::Callout, Tool::Label, Tool::ArrowLabel],
    },
    Preset {
        short: "Redact",
        name: "Redact: a solid black box; on a selected box, fills it",
        key: "4",
        colour: 11,
        width: Width::Medium,
        tool: Tool::Redact,
        keeps: &[Tool::Redact],
    },
];

/// The weight an item was drawn with (nearest step), at `zoom`.
pub fn level(item: &Item, zoom: f32) -> Width {
    let (value, of): (f32, fn(Width) -> f32) = match &item.shape {
        Shape::Text { size, .. } => (size * zoom, Width::font),
        Shape::Callout { size, .. } | Shape::Label { size, .. } => {
            (size * zoom / 0.75, Width::font)
        }
        Shape::Step { radius, .. } | Shape::Pointer { radius, .. } => (radius * zoom, Width::badge),
        Shape::Highlighter(_) => (item.width * zoom / 4.0, Width::stroke),
        _ => (item.width * zoom, Width::stroke),
    };
    Width::ALL
        .into_iter()
        .min_by(|a, b| (of(*a) - value).abs().total_cmp(&(of(*b) - value).abs()))
        .unwrap_or(Width::Medium)
}
