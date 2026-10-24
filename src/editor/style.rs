//! Style presets (one click: colour + weight + the tool they are for) and the
//! copy / paste style between items.
use super::document::{Item, Shape};
use super::{Tool, Width};

pub struct Preset {
    /// On its chip.
    pub short: &'static str,
    /// Its tooltip, with its key.
    pub tip: &'static str,
    /// Index into `theme::ANNOTATION`.
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
        tip: "Red box + arrow (1)",
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
        tip: "Highlight yellow (2)",
        colour: 2,
        width: Width::Medium,
        tool: Tool::Highlighter,
        keeps: &[Tool::Highlighter],
    },
    Preset {
        short: "Callout",
        tip: "Blue callout (3)",
        colour: 5,
        width: Width::Medium,
        tool: Tool::Callout,
        keeps: &[Tool::Callout, Tool::Label, Tool::ArrowLabel],
    },
    Preset {
        short: "Redact",
        tip: "Redact: a solid black box; on a selected box, fills it (4)",
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
