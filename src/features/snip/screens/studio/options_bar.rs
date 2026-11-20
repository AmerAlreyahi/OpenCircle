//! The options bar (board 10, STUDIO_V3_SPEC §4): the tool's name and key,
//! then its groups, every control 32 high (`kit::option_bar`). The groups read
//! the editor's current values and send the editor's own messages.
use iced::widget::{Space, container};
use iced::{Center, Color, Element, Fill};

use super::popovers::Pop;
use super::{Msg, Studio};
use crate::features::snip::annotate::{
    Annotator, ERASER_SIZES, Message, TextStyle, Tool, Width, fills_inside, info, style,
};
use crate::shared::app_language::{fmt, tr};
use crate::shared::kit::icon::Icon;
use crate::shared::kit::option_bar::{
    GROUP_GAP, accent_cell, caption, dot, glyph_cell, group, hairline, swatch, text_cell, value,
};
use crate::shared::kit::text::{muted, text};
use crate::shared::kit::tokens::{Role, size, space};
use crate::shared::kit::{dir, keycap, scroll};
use crate::t;

/// Colours on the options bar (and the Edit text bar).
pub(super) const SWATCHES: usize = 8;
const NARROW_SWATCHES: usize = 4;
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

/// What the shared controls can do for the tool / selection now: a control
/// that can't is dimmed (38 %), never hidden (STUDIO v3.1).
#[derive(Clone, Copy)]
struct Can {
    colour: bool,
    stroke: bool,
    opacity: bool,
    outline: bool,
    fill: bool,
}

fn can(e: &Annotator, tool: Tool) -> Can {
    let sel = e.selection();
    let pasted = e.selection_style().is_some_and(|s| s.opacity.is_some());
    if pasted {
        return Can {
            colour: false,
            stroke: false,
            opacity: true,
            outline: false,
            fill: false,
        };
    }
    if sel.len() > 1 {
        return Can {
            colour: true,
            stroke: true,
            opacity: false,
            outline: false,
            fill: false,
        };
    }
    let liney = matches!(
        tool,
        Tool::Rect
            | Tool::Ellipse
            | Tool::Line
            | Tool::Arrow
            | Tool::Pen
            | Tool::Highlighter
            | Tool::Step
    );
    Can {
        colour: liney || matches!(tool, Tool::Text | Tool::Callout | Tool::Redact),
        stroke: liney,
        opacity: false,
        outline: matches!(
            tool,
            Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Callout
        ),
        fill: fills_inside(tool),
    }
}

/// The tool the bar is for: the selected object's, else the active tool's.
fn base_tool(e: &Annotator) -> Tool {
    let sel = e.selection();
    let item = (sel.len() == 1).then(|| e.doc.items.get(sel[0])).flatten();
    match item {
        Some(item) => info(info(super::popovers::tool_of(&item.shape)).slot).tool,
        None => info(info(e.tool).slot).tool,
    }
}

fn colours(e: &Annotator, n: usize, live: bool) -> El<'_> {
    let now = shown(e).colour;
    let cells = style::ANNOTATION.iter().take(n).map(|c| {
        let rgba = c.into_rgba8();
        let shade = Color {
            a: if live { c.a } else { c.a * 0.38 },
            ..*c
        };
        swatch(
            shade,
            live && Some(rgba) == now,
            live.then_some(Message::Colour(rgba)),
        )
    });
    group(cells.collect(), false)
}

/// Three size dots and the value.
fn dots<'a>(
    at: Option<usize>,
    words: String,
    live: bool,
    pick: impl Fn(usize) -> Message,
) -> El<'a> {
    let mut cells: Vec<El<'a>> = vec![caption(t!(sn_size))];
    cells.extend((0..3).map(|i| dot(i, live && at == Some(i), live.then(|| pick(i)))));
    cells.push(value(words));
    group(cells, false)
}

fn toggle<'a>(words: &'a str, on: bool, press: Option<Message>) -> El<'a> {
    group(vec![text_cell(words, on, press)], false)
}

fn stroke(e: &Annotator, live: bool) -> El<'_> {
    let px = shown(e).stroke;
    let at = Width::ALL.iter().position(|w| w.stroke() == px);
    dots(at, format!("{} px", px.round()), live, |i| {
        Message::Width(Width::ALL[i])
    })
}

/// A pasted picture's opacity: three common steps.
fn opacity(e: &Annotator, live: bool, compact: bool) -> El<'_> {
    let now = e.selection_style().and_then(|s| s.opacity).unwrap_or(255);
    let mut cells: Vec<El<'_>> = Vec::new();
    if !compact {
        cells.push(caption(t!(st_opacity)));
    }
    for (words, v) in [("100 %", 255u8), ("70 %", 178), ("40 %", 102)] {
        cells.push(text_cell(
            words,
            live && now.abs_diff(v) < 12,
            live.then_some(Message::Opacity(v)),
        ));
    }
    group(cells, false)
}

fn outline_fill(e: &Annotator, tool: Tool, c: Can) -> El<'_> {
    let now = shown(e);
    let fill = (c.fill && fills_inside(tool)).then_some(Message::Fill(!now.fill));
    group(
        vec![
            text_cell(
                t!(sn_outline),
                c.outline && now.outline,
                c.outline.then_some(Message::Outline(!now.outline)),
            ),
            text_cell(t!(sn_fill), c.fill && now.fill, fill),
        ],
        false,
    )
}

fn font_size(e: &Annotator) -> El<'_> {
    let at = TEXT_SIZES.iter().position(|&p| p == e.text_size);
    dots(at, format!("{} px", e.text_size.round()), true, |i| {
        Message::TextSize(TEXT_SIZES[i])
    })
}

/// The tool's own groups (what the shared ones don't cover).
fn own_groups(e: &Annotator, tool: Tool) -> Vec<El<'_>> {
    match tool {
        Tool::Text => {
            let st = e.text_style;
            let bold = TextStyle {
                bold: !st.bold,
                ..st
            };
            vec![
                font_size(e),
                toggle(t!(sn_bold), st.bold, Some(Message::TextStyle(bold))),
            ]
        }
        Tool::Callout => vec![font_size(e)],
        Tool::Blur => {
            let at = Width::ALL.iter().position(|&w| w == e.blur_cell);
            let px = e.blur_cell.cell();
            vec![
                dots(at, format!("{px} px"), true, |i| {
                    Message::BlurCell(Width::ALL[i])
                }),
                toggle(t!(sn_line_help), e.assist, Some(Message::Assist)),
            ]
        }
        Tool::Eraser => {
            let at = ERASER_SIZES.iter().position(|&s| s == e.eraser);
            vec![
                dots(at, format!("{} px", e.eraser.round()), true, |i| {
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

/// The colour swatches alone (the tests click them).
#[cfg(test)]
pub(super) fn colour_group(e: &Annotator, n: usize) -> Element<'_, Msg> {
    Element::from(colours(e, n, true)).map(Msg::Editor)
}

/// The tool's own groups as one element (the More popover's content).
pub fn tool_group(e: &Annotator) -> Element<'_, Msg> {
    let g = own_groups(e, base_tool(e));
    let body: El<'_> = match g.is_empty() {
        true => muted(Role::ROW_DESC, t!(st_hint_select)).into(),
        false => iced::widget::column(g).spacing(space::GAP).into(),
    };
    Element::from(body).map(Msg::Editor)
}

/// Below `WIDE` the tool's own group folds into "More"; below `NARROW` the
/// colours shrink and the key hides (the bar never wraps and never clips).
const WIDE: f32 = 1360.0;
const NARROW: f32 = 1100.0;

fn opener<'a>(words: &'a str, on: bool, pop: Pop, live: bool) -> Element<'a, Msg> {
    let press = live.then_some(Msg::Pop(Some(pop)));
    group(
        vec![
            text_cell(words, on, press.clone()),
            glyph_cell(Icon::ChevronDown.glyph(), on, press),
        ],
        false,
    )
}

/// The bar: the tool's name and key, the shared controls (always all of them),
/// the tool's own, then Position and Actions.
pub fn view<'a>(s: &'a Studio, e: &'a Annotator) -> Element<'a, Msg> {
    let tool = base_tool(e);
    let c = can(e, tool);
    let wide = s.window.width >= WIDE;
    let swatches = match s.window.width >= NARROW {
        true => SWATCHES,
        false => NARROW_SWATCHES,
    };
    let sel = e.selection();
    let (title, key) = match sel.len() {
        0 => {
            let base = info(info(e.tool).slot);
            (tr(base.name).to_string(), Some(base.key))
        }
        1 => (tr(info(tool).name).to_string(), None),
        count => (
            fmt(crate::shared::app_language::S::st_n_objects, &[&count]),
            None,
        ),
    };
    let mut head: Vec<El<'_>> = vec![text(Role::ROW_TITLE, title).into()];
    if s.window.width >= NARROW {
        head.extend(key.filter(|k| *k != '\0').map(|k| {
            let cap = keycap::keycap(k.to_ascii_uppercase().to_string());
            container(cap).width(size::KEYCAP_H).into()
        }));
    }
    let name: El<'_> = dir::row(head).spacing(space::GAP).align_y(Center).into();
    // the narrowest windows give the name up first (the rail's tool shows it)
    let mut items: Vec<El<'_>> = match s.window.width >= NARROW {
        true => vec![name, hairline()],
        false => Vec::new(),
    };
    items.extend([
        colours(e, swatches, c.colour),
        stroke(e, c.stroke),
        opacity(e, c.opacity, s.window.width < NARROW),
        outline_fill(e, tool, c),
    ]);
    let own = own_groups(e, tool);
    let more = !own.is_empty() && !wide;
    if !own.is_empty() && wide {
        items.push(hairline());
        items.extend(own);
    }
    let mut end: Vec<Element<'a, Msg>> = Vec::new();
    if more {
        end.push(opener(
            t!(st_more),
            s.popover == Some(Pop::More),
            Pop::More,
            true,
        ));
    }
    end.push(opener(
        t!(st_position),
        s.popover == Some(Pop::Position),
        Pop::Position,
        !sel.is_empty(),
    ));
    end.push(opener(
        t!(st_actions_menu),
        s.popover == Some(Pop::Actions),
        Pop::Actions,
        true,
    ));
    let line = dir::row(items).spacing(GROUP_GAP).align_y(Center);
    let line: Element<'a, Msg> = Element::from(line).map(Msg::Editor);
    let ends = Element::from(dir::row(end).spacing(GROUP_GAP).align_y(Center)).map(|m: Msg| m);
    let row = crate::dir_row![line, Space::new().width(Fill), ends]
        .spacing(GROUP_GAP)
        .align_y(Center);
    let bar = container(row)
        .padding(iced::Padding {
            left: 14.0,
            right: 12.0,
            ..iced::Padding::ZERO
        })
        .height(super::OPTIONS)
        .center_y(super::OPTIONS);
    // (a last resort: a bar the estimates missed scrolls rather than clips)
    scroll::across(bar).width(Fill).into()
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
            for group in own_groups(&e, tool) {
                assert_eq!(tall(&group), Length::Fixed(HEIGHT), "{tool:?}");
                built += 1;
            }
        }
        assert!(built >= 5, "the bars were measured: {built}");
    }
}
