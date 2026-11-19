//! The options bar (board 10, STUDIO_V3_SPEC §4): the tool's name and key,
//! then its groups, every control 32 high (`kit::option_bar`). The groups read
//! the editor's current values and send the editor's own messages.
use iced::widget::container;
use iced::{Center, Element, Fill};

use super::Msg;
use crate::features::snip::annotate::{
    Annotator, ERASER_SIZES, Message, TextStyle, Tool, Width, info, style,
};
use crate::shared::app_language::tr;
use crate::shared::kit::option_bar::{
    GROUP_GAP, accent_cell, caption, dot, group, hairline, swatch, text_cell, value,
};
use crate::shared::kit::text::{muted, text};
use crate::shared::kit::tokens::{Role, size, space};
use crate::shared::kit::{dir, keycap, scroll};
use crate::t;

const SWATCHES: usize = 8;
const TEXT_SIZES: [f32; 3] = [16.0, 26.0, 38.0];

type El<'a> = Element<'a, Message>;

/// The values the bar shows: the selection's (a mixed one reads as none), else
/// the next drawing's (STUDIO_V3 S1: the bar follows the selection).
struct Shown {
    colour: Option<[u8; 4]>,
    stroke: f32,
    outline: bool,
    fill: bool,
}

fn shown(e: &Annotator) -> Shown {
    match e.selection_style() {
        Some(st) => Shown {
            colour: st.colour,
            stroke: st.stroke.unwrap_or(0.0),
            outline: st.outline.unwrap_or(false),
            fill: st.fill.unwrap_or(false),
        },
        None => Shown {
            colour: Some(e.colour),
            stroke: e.stroke,
            outline: e.outline,
            fill: e.fill,
        },
    }
}

fn colours(e: &Annotator) -> El<'_> {
    let now = shown(e).colour;
    let cells = style::ANNOTATION.iter().take(SWATCHES).map(|c| {
        let rgba = c.into_rgba8();
        swatch(*c, Some(rgba) == now, Some(Message::Colour(rgba)))
    });
    group(cells.collect(), false)
}

/// Three size dots and the value.
fn dots<'a>(at: Option<usize>, words: String, pick: impl Fn(usize) -> Message) -> El<'a> {
    let mut cells: Vec<El<'a>> = vec![caption(t!(sn_size))];
    cells.extend((0..3).map(|i| dot(i, at == Some(i), Some(pick(i)))));
    cells.push(value(words));
    group(cells, false)
}

fn toggle<'a>(words: &'a str, on: bool, press: Option<Message>) -> El<'a> {
    group(vec![text_cell(words, on, press)], false)
}

fn stroke(e: &Annotator) -> El<'_> {
    let px = shown(e).stroke;
    let at = Width::ALL.iter().position(|w| w.stroke() == px);
    dots(at, format!("{} px", px.round()), |i| {
        Message::Width(Width::ALL[i])
    })
}

fn font_size(e: &Annotator) -> El<'_> {
    let at = TEXT_SIZES.iter().position(|&p| p == e.text_size);
    dots(at, format!("{} px", e.text_size.round()), |i| {
        Message::TextSize(TEXT_SIZES[i])
    })
}

/// The groups of `tool`, in the order of STUDIO_V3_SPEC §4.
fn groups(e: &Annotator, tool: Tool) -> Vec<El<'_>> {
    let now = shown(e);
    let outline = || {
        toggle(
            t!(sn_outline),
            now.outline,
            Some(Message::Outline(!now.outline)),
        )
    };
    match tool {
        Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow => {
            let fills = crate::features::snip::annotate::fills_inside(tool);
            let fill = fills.then_some(Message::Fill(!now.fill));
            vec![
                colours(e),
                stroke(e),
                outline(),
                toggle(t!(sn_fill), now.fill && fills, fill),
            ]
        }
        Tool::Pen | Tool::Highlighter | Tool::Step => vec![colours(e), stroke(e)],
        Tool::Text => {
            let st = e.text_style;
            let bold = TextStyle {
                bold: !st.bold,
                ..st
            };
            vec![
                colours(e),
                font_size(e),
                toggle(t!(sn_bold), st.bold, Some(Message::TextStyle(bold))),
            ]
        }
        Tool::Callout => vec![colours(e), font_size(e), outline()],
        Tool::Redact => vec![colours(e)],
        Tool::Blur => {
            let at = Width::ALL.iter().position(|&w| w == e.blur_cell);
            let px = e.blur_cell.cell();
            vec![
                dots(at, format!("{px} px"), |i| Message::BlurCell(Width::ALL[i])),
                toggle(t!(sn_line_help), e.assist, Some(Message::Assist)),
            ]
        }
        Tool::Eraser => {
            let at = ERASER_SIZES.iter().position(|&s| s == e.eraser);
            vec![
                dots(at, format!("{} px", e.eraser.round()), |i| {
                    Message::EraserSize(ERASER_SIZES[i])
                }),
                toggle(
                    t!(sn_whole_drawing),
                    e.eraser_whole,
                    Some(Message::EraserWhole(!e.eraser_whole)),
                ),
            ]
        }
        _ => Vec::new(),
    }
}

/// The bar (48 high).
pub fn view(e: &Annotator) -> Element<'_, Msg> {
    let base = info(info(e.tool).slot);
    let name: El<'_> = crate::dir_row![
        text(Role::ROW_TITLE, tr(base.name)),
        container(keycap::keycap(base.key.to_ascii_uppercase().to_string())).width(size::KEYCAP_H),
    ]
    .spacing(space::GAP)
    .align_y(Center)
    .into();
    let mut items: Vec<El<'_>> = vec![name];
    let selected = e.doc.selected.is_some();
    let g = groups(e, base.tool);
    if g.is_empty() || matches!(base.tool, Tool::Select | Tool::Crop) {
        let hint = muted(Role::ROW_DESC, t!(st_hint_select));
        items.push(hairline());
        items.push(hint.into());
    }
    for (i, one) in g.into_iter().enumerate() {
        if i == 0 {
            items.push(hairline());
        }
        items.push(one);
    }
    if selected {
        items.push(crate::shared::kit::chip::tag(t!(st_tool_selected)));
    }
    let line = dir::row(items).spacing(GROUP_GAP).align_y(Center);
    let bar = container(line)
        .padding(iced::Padding {
            left: 14.0,
            right: 12.0,
            ..iced::Padding::ZERO
        })
        .height(super::OPTIONS)
        .center_y(super::OPTIONS);
    Element::from(scroll::across(bar).width(Fill)).map(Msg::Editor)
}

/// The Done group (Edit text): the accent group, text on the fill.
pub fn done<'a>(ready: bool) -> Element<'a, Msg> {
    group(
        vec![accent_cell(
            t!(st_edit_done),
            ready.then_some(Msg::EditDone),
        )],
        true,
    )
}
