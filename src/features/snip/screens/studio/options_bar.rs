//! The options bar (board 10, STUDIO_V3_SPEC §4): the tool's name and key,
//! then its groups, every control 32 high (`kit::option_bar`). The groups read
//! the editor's current values and send the editor's own messages.
use iced::widget::container;
use iced::{Center, Element, Fill};

use super::Msg;
use crate::features::snip::annotate::{
    Annotator, ERASER_SIZES, Message, TextStyle, Tool, Width, info, style,
};
use crate::shared::app_language::{fmt, tr};
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

fn colours(e: &Annotator, n: usize) -> El<'_> {
    let now = shown(e).colour;
    let cells = style::ANNOTATION.iter().take(n).map(|c| {
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
fn groups(e: &Annotator, tool: Tool, n: usize) -> Vec<El<'_>> {
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
                colours(e, n),
                stroke(e),
                outline(),
                toggle(t!(sn_fill), now.fill && fills, fill),
            ]
        }
        Tool::Pen | Tool::Highlighter | Tool::Step => vec![colours(e, n), stroke(e)],
        Tool::Text => {
            let st = e.text_style;
            let bold = TextStyle {
                bold: !st.bold,
                ..st
            };
            vec![
                colours(e, n),
                font_size(e),
                toggle(t!(sn_bold), st.bold, Some(Message::TextStyle(bold))),
            ]
        }
        Tool::Callout => vec![colours(e, n), font_size(e), outline()],
        Tool::Redact => vec![colours(e, n)],
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

/// What the groups are for: the tool's (nothing selected), the selected
/// object's own (by its type), or what several objects share.
fn subject(e: &Annotator, n: usize) -> (String, Option<char>, Vec<El<'_>>) {
    let sel = e.selection();
    let item = (sel.len() == 1).then(|| e.doc.items.get(sel[0])).flatten();
    match (sel.len(), item) {
        (0, _) => {
            let base = info(info(e.tool).slot);
            (
                tr(base.name).to_string(),
                Some(base.key),
                groups(e, base.tool, n),
            )
        }
        (_, Some(item)) => {
            let t = info(info(super::library::tool_of(&item.shape)).slot);
            (tr(t.name).to_string(), None, groups(e, t.tool, n))
        }
        (count, None) => (
            fmt(crate::shared::app_language::S::st_n_objects, &[&count]),
            None,
            vec![colours(e, n), stroke(e)],
        ),
    }
}

/// The selection's own style groups, for Properties (none: nothing selected).
pub fn selection_groups(e: &Annotator, swatches: usize) -> Vec<Element<'_, Msg>> {
    match e.selection().is_empty() {
        true => Vec::new(),
        false => subject(e, swatches)
            .2
            .into_iter()
            .map(|g| g.map(Msg::Editor))
            .collect(),
    }
}

/// The bar (48 high).
pub fn view(e: &Annotator) -> Element<'_, Msg> {
    let (title, key, g) = subject(e, SWATCHES);
    let mut head: Vec<El<'_>> = vec![text(Role::ROW_TITLE, title).into()];
    head.extend(key.map(|k| {
        let cap = keycap::keycap(k.to_ascii_uppercase().to_string());
        container(cap).width(size::KEYCAP_H).into()
    }));
    let name: El<'_> = dir::row(head).spacing(space::GAP).align_y(Center).into();
    let mut items: Vec<El<'_>> = vec![name];
    let selected = !e.selection().is_empty();
    let tool = info(e.tool).slot;
    if !selected && (g.is_empty() || matches!(tool, Tool::Select | Tool::Crop)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::snip::annotate::{Group, Image};
    use crate::shared::kit::option_bar::HEIGHT;
    use iced::Length;

    fn tall(e: &Element<'_, Message>) -> Length {
        iced::advanced::Widget::size(e.as_widget()).height
    }

    /// STUDIO_V3 S4 (board 10): every group the Studio's bar builds, for every
    /// tool, is the one control height (32).
    #[test]
    fn every_group_of_every_tools_bar_is_32_high() {
        let mut e = Annotator::new(
            Image {
                width: 64,
                height: 64,
                rgba: vec![255; 64 * 64 * 4],
            },
            1.0,
        );
        let mut tools: Vec<Tool> = Group::ALL
            .iter()
            .flat_map(|g| g.tools())
            .map(|t| t.tool)
            .collect();
        tools.push(Tool::Crop);
        let mut built = 0;
        for tool in tools {
            e.update(Message::Tool(tool));
            for group in groups(&e, tool, SWATCHES) {
                assert_eq!(tall(&group), Length::Fixed(HEIGHT), "{tool:?}");
                built += 1;
            }
        }
        assert!(built >= 12, "the bars were measured: {built}");
    }
}
