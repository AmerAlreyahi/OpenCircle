//! The options bar (board 10, STUDIO_V3_SPEC §4): the tool's name and key,
//! then its groups, every control 32 high (`kit::option_bar`). The groups read
//! the editor's current values and send the editor's own messages.
use iced::widget::container;
use iced::{Center, Element, Fill};

use super::popovers::Pop;
use super::{Msg, Studio};
use crate::features::snip::annotate::{
    Annotator, BlurShape, ERASER_SIZES, Message, TextStyle, Tool, Width, fills_inside, info, style,
};
use crate::shared::app_language::{fmt, tr};
use crate::shared::kit::icon::Icon;
use crate::shared::kit::icon_button;
use crate::shared::kit::option_bar::{
    GROUP_GAP, accent_cell, caption, dot, glyph_cell, group, hairline, swatch, swatch_of,
    text_cell, value,
};
use crate::shared::kit::popover;
use crate::shared::kit::text::{muted, text};
use crate::shared::kit::tokens::{Role, size, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{dir, keycap, scroll};
use crate::t;

/// Colours on the options bar (and the Edit text bar).
pub(super) const SWATCHES: usize = 8;
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
        swatch(
            *c,
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
    captioned: bool,
    pick: impl Fn(usize) -> Message,
) -> El<'a> {
    let mut cells: Vec<El<'a>> = Vec::new();
    if captioned {
        cells.push(caption(t!(sn_size)));
    }
    cells.extend((0..3).map(|i| dot(i, live && at == Some(i), live.then(|| pick(i)))));
    cells.push(value(words));
    group(cells, false)
}

fn toggle<'a>(words: &'a str, on: bool, press: Option<Message>) -> El<'a> {
    group(vec![text_cell(words, on, press)], false)
}

fn stroke(e: &Annotator, live: bool, captioned: bool) -> El<'_> {
    let px = shown(e).stroke;
    let at = Width::ALL.iter().position(|w| w.stroke() == px);
    dots(at, format!("{} px", px.round()), live, captioned, |i| {
        Message::Width(Width::ALL[i])
    })
}

/// A pasted picture's opacity: three common steps.
fn opacity(e: &Annotator, live: bool, captioned: bool) -> El<'_> {
    let now = e.selection_style().and_then(|s| s.opacity).unwrap_or(255);
    let mut cells: Vec<El<'_>> = Vec::new();
    if captioned {
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
    dots(at, format!("{} px", e.text_size.round()), true, true, |i| {
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
            let shapes = [
                (BlurShape::Brush, t!(sn_brush)),
                (BlurShape::Rect, t!(tl_rect)),
                (BlurShape::Ellipse, t!(tl_ellipse)),
            ];
            let styles = crate::features::snip::annotate::blur_styles();
            let mut o = vec![
                group(
                    styles
                        .iter()
                        .map(|&(style, words)| {
                            text_cell(
                                words,
                                e.blur_style == style,
                                Some(Message::BlurStyle(style)),
                            )
                        })
                        .collect(),
                    false,
                ),
                group(
                    shapes
                        .iter()
                        .map(|&(kind, words)| {
                            text_cell(words, e.blur_shape == kind, Some(Message::BlurShape(kind)))
                        })
                        .collect(),
                    false,
                ),
                dots(at, format!("{px} px"), true, true, |i| {
                    Message::BlurCell(Width::ALL[i])
                }),
                toggle(t!(sn_line_help), e.assist, Some(Message::Assist)),
            ];
            if e.blur_style.is_look() {
                o.push(muted(Role::ROW_DESC, t!(sn_looks_note)).into());
            }
            if let Some(both) = e.hideable() {
                o.push(toggle(
                    t!(sn_hide_shape),
                    false,
                    Some(Message::HideShape(false)),
                ));
                if both {
                    o.push(toggle(
                        t!(sn_hide_outline),
                        false,
                        Some(Message::HideShape(true)),
                    ));
                }
            }
            o
        }
        Tool::Eraser => {
            let at = ERASER_SIZES.iter().position(|&s| s == e.eraser);
            vec![
                dots(at, format!("{} px", e.eraser.round()), true, true, |i| {
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

/// The colour swatches alone (the Colour popover's content; the tests click them).
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

/// How the bar is drawn at the room it has: never a control hidden. In order of
/// preference: everything inline; the tool's own group folded into More; the tool
/// name down to its icon; the eight colours down to ONE swatch button that opens
/// a Colour popover; last, the "Size" / "Opacity" captions go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Plan {
    name_full: bool,
    colours_inline: bool,
    own_inline: bool,
    captions: bool,
}

/// Widths at 100 % (measured from the 1280 px golden), logical px.
const W_FIXED: f32 = 177.0 + 219.0 + 101.0 + 98.0 + 97.0; // Size, Opacity, Outline/Fill, Position, Actions
const W_NAME: f32 = 105.0;
const W_ICON: f32 = 34.0;
const W_COLOURS: f32 = 240.0;
const W_COLOUR_BUTTON: f32 = 48.0;
const W_OWN: f32 = 290.0;
const W_MORE: f32 = 90.0;
const W_CAPTIONS: f32 = 85.0;
const W_GAP: f32 = 8.0;
/// The Steps sidebar's toggle at the row's end.
const W_TOGGLE: f32 = 36.0;
/// The bar's side padding.
const PAD_L: f32 = 14.0;
const PAD_R: f32 = 12.0;

impl Plan {
    fn width(self, has_own: bool, scale: f32) -> f32 {
        let mut w = W_FIXED + W_TOGGLE;
        let mut items = 6.0; // the groups, Position, Actions, the name
        w += if self.name_full { W_NAME } else { W_ICON };
        w += if self.colours_inline {
            W_COLOURS
        } else {
            W_COLOUR_BUTTON
        };
        if has_own {
            w += if self.own_inline { W_OWN } else { W_MORE };
            items += 1.0;
        }
        if !self.captions {
            w -= W_CAPTIONS;
        }
        (w + items * W_GAP) * scale + PAD_L + PAD_R
    }

    /// The first plan that fits `avail` px (the last one when none does).
    fn at(avail: f32, scale: f32, has_own: bool) -> Plan {
        let steps = [
            (true, true, true, true),
            (true, true, false, true),
            (false, true, false, true),
            (false, false, false, true),
            (false, false, false, false),
        ];
        let plans = steps.map(|(name_full, colours_inline, own_inline, captions)| Plan {
            name_full,
            colours_inline,
            own_inline,
            captions,
        });
        let fit = plans.iter().find(|p| p.width(has_own, scale) <= avail);
        *fit.unwrap_or(&plans[plans.len() - 1])
    }
}

/// The room the bar has: the window less its rim and the rail (the bar runs over
/// the sidebar to the edge, so the sidebar does not change it).
fn bar_room(s: &Studio) -> f32 {
    s.window.width - 2.0 - (super::RAIL + 1.0)
}

fn opener<'a>(s: &Studio, words: &'a str, pop: Pop, live: bool) -> Element<'a, Msg> {
    let on = s.popover == Some(pop);
    let press = live.then_some(Msg::Pop(Some(pop)));
    let button = group(
        vec![
            text_cell(words, on, press.clone()),
            glyph_cell(Icon::ChevronDown.glyph(), on, press),
        ],
        false,
    );
    // (it remembers where it is drawn: its popover hangs from it)
    popover::track(button, s.anchors.of(pop))
}

/// The narrow bar's colour: ONE swatch button (the current colour) that opens the
/// Colour popover with all of them.
fn colour_button<'a>(s: &Studio, e: &Annotator, live: bool) -> Element<'a, Msg> {
    let on = s.popover == Some(Pop::Colours);
    let press = live.then_some(Msg::Pop(Some(Pop::Colours)));
    let button = group(
        vec![
            swatch_of(shown(e).colour, on, press.clone()),
            glyph_cell(Icon::ChevronDown.glyph(), on, press),
        ],
        false,
    );
    popover::track(button, s.anchors.of(Pop::Colours))
}

/// What a dimmed control's tooltip says (F15); none while it is live.
fn off_note(on: bool) -> Option<String> {
    (!on).then(|| t!(st_not_for_tool).to_string())
}

/// The bar: the tool's name and key, the shared controls (always all of them),
/// the tool's own, then Position and Actions.
pub fn view<'a>(s: &'a Studio, e: &'a Annotator) -> Element<'a, Msg> {
    let tool = base_tool(e);
    let c = can(e, tool);
    let own = own_groups(e, tool);
    let scale = f32::from(s.ui_zoom) / 100.0;
    let plan = Plan::at(bar_room(s), scale, !own.is_empty());
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
    let ed = |g: El<'a>| -> Element<'a, Msg> { Element::from(g).map(Msg::Editor) };
    let mut items: Vec<Element<'a, Msg>> = Vec::new();
    // the name: words and key, or just the tool's icon in the narrow
    match plan.name_full {
        true => {
            let mut head: Vec<El<'_>> = vec![text(Role::ROW_TITLE, title).into()];
            head.extend(key.filter(|k| *k != '\0').map(|k| {
                let cap = keycap::keycap(k.to_ascii_uppercase().to_string());
                container(cap).width(size::KEYCAP_H).into()
            }));
            items.push(ed(dir::row(head)
                .spacing(space::GAP)
                .align_y(Center)
                .into()));
            items.push(ed(hairline()));
        }
        false => {
            let glyph = crate::shared::kit::icon::glyph(info(tool).icon, 16.0);
            items.push(container(glyph).center_x(W_ICON).into());
        }
    }
    // a dimmed control says why in its tooltip (F15)
    let why = |el: Element<'a, Msg>, on: bool| match off_note(on) {
        None => el,
        Some(note) => crate::shared::kit::tooltip::cut(el, note),
    };
    items.push(why(
        match plan.colours_inline {
            true => ed(colours(e, SWATCHES, c.colour)),
            false => colour_button(s, e, c.colour),
        },
        c.colour,
    ));
    items.push(why(ed(stroke(e, c.stroke, plan.captions)), c.stroke));
    items.push(why(ed(opacity(e, c.opacity, plan.captions)), c.opacity));
    items.push(why(ed(outline_fill(e, tool, c)), c.outline || c.fill));
    let more = !own.is_empty() && !plan.own_inline;
    if !own.is_empty() && plan.own_inline {
        items.push(ed(hairline()));
        items.extend(own.into_iter().map(ed));
    }
    let mut end: Vec<Element<'a, Msg>> = Vec::new();
    if more {
        end.push(opener(s, t!(st_more), Pop::More, true));
    }
    end.push(opener(s, t!(st_position), Pop::Position, !sel.is_empty()));
    end.push(opener(s, t!(st_actions_menu), Pop::Actions, true));
    finish(s, items, end)
}

/// The bar of the empty Studio (no picture): the same row, its picture controls dimmed
/// (never hidden), the Steps sidebar's toggle live at the same spot at the row's end.
pub fn empty<'a>(s: &'a Studio) -> Element<'a, Msg> {
    let ed = |g: El<'a>| -> Element<'a, Msg> { Element::from(g).map(Msg::Editor) };
    let swatches = style::ANNOTATION
        .iter()
        .take(SWATCHES)
        .map(|c| swatch(*c, false, None))
        .collect();
    let opacity = [t!(st_opacity)]
        .into_iter()
        .map(caption)
        .chain(["100 %", "70 %", "40 %"].map(|w| text_cell(w, false, None)))
        .collect();
    let items = vec![
        ed(group(swatches, false)),
        ed(dots(None, "5 px".into(), false, true, |i| {
            Message::Width(Width::ALL[i])
        })),
        ed(group(opacity, false)),
        ed(group(
            vec![
                text_cell(t!(sn_outline), false, None),
                text_cell(t!(sn_fill), false, None),
            ],
            false,
        )),
    ];
    let end = vec![
        opener(s, t!(st_position), Pop::Position, false),
        opener(s, t!(st_actions_menu), Pop::Actions, false),
    ];
    finish(s, items, end)
}

/// The row: the groups (scrolling as a last resort), then `end` and the Steps toggle.
fn finish<'a>(
    s: &'a Studio,
    items: Vec<Element<'a, Msg>>,
    end: Vec<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let line = dir::row(items).spacing(GROUP_GAP).align_y(Center);
    let ends = dir::row(end).spacing(GROUP_GAP).align_y(Center);
    // the Steps sidebar's toggle: the end of the row, the same place open or closed
    let shown = s.sidebar_shown();
    let toggle = hint(
        icon_button::square(
            super::sidebar::toggle_glyph(s.sidebar_at_start()),
            icon_button::State {
                active: shown,
                focused: false,
            },
            Some(Msg::Library),
        ),
        match shown {
            true => t!(st_hide_steps),
            false => t!(st_show_steps),
        },
        Why::Keys(super::keys::of(super::keys::Cmd::StepsSidebar)),
        iced::widget::tooltip::Position::Bottom,
    );
    // the groups scroll (a last resort: a bar the estimates missed scrolls rather than
    // clips); Position, Actions and the toggle stay put at the row's end
    let pad = |side: f32| iced::Padding {
        left: side,
        right: side,
        ..iced::Padding::ZERO
    };
    let groups = scroll::across(
        container(line)
            .padding(iced::Padding {
                left: PAD_L,
                ..iced::Padding::ZERO
            })
            .height(super::OPTIONS)
            .center_y(super::OPTIONS),
    )
    .width(Fill);
    let tail = container(
        crate::dir_row![ends, toggle]
            .spacing(GROUP_GAP)
            .align_y(Center),
    )
    .padding(iced::Padding {
        left: pad(GROUP_GAP).left,
        right: PAD_R,
        ..iced::Padding::ZERO
    })
    .height(super::OPTIONS)
    .center_y(super::OPTIONS);
    crate::dir_row![groups, tail].width(Fill).into()
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

    /// F15 (#220): a control the tool cannot use is dimmed AND says so in its tooltip; a live
    /// one has none.
    #[test]
    fn f15_a_dimmed_control_says_why() {
        let mut e = Annotator::new(
            Image {
                width: 64,
                height: 64,
                rgba: vec![255; 64 * 64 * 4],
            },
            1.0,
        );
        e.update(Message::Tool(Tool::Select));
        let none = can(&e, Tool::Select);
        assert!(
            !none.colour && !none.stroke && !none.outline,
            "Select uses none"
        );
        assert_eq!(off_note(none.colour).as_deref(), Some("Not for this tool"));
        let rect = can(&e, Tool::Rect);
        assert!(rect.colour && rect.stroke);
        assert_eq!(off_note(rect.colour), None);
    }

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
