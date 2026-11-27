//! The options bar's parts, shared by the Studio's bar and Snip's options row (UX C4): one component.
//! The groups read the editor's current values and send the editor's own messages; Snip's row
//! (`snip_row_focused`) is built from the same parts.
use iced::widget::container;
use iced::{Center, Element};

use crate::features::snip::annotate::{
    Annotator, ERASER_SIZES, Message, STRENGTH_RANGE, TextStyle, Tool, Width, fills_inside, info,
    style,
};
use crate::features::snip::screens::snip_toolbar::toolbar::tip;
use crate::shared::app_language::tr;
use crate::shared::kit::option_bar::{
    GROUP_GAP, caption, dot, glyph_cell, group, hairline, swatch, text_cell, value,
};
use crate::shared::kit::text::text;
use crate::shared::kit::tokens::{Role, size, space};
use crate::shared::kit::tooltip::{Why, hint};
use crate::shared::kit::{dir, keycap, slider};
use crate::t;

/// Colours on the options bar (and the Edit text bar).
pub(crate) const SWATCHES: usize = 8;
pub(crate) const TEXT_SIZES: [f32; 3] = [16.0, 26.0, 38.0];

pub(crate) type El<'a> = Element<'a, Message>;

/// The values the bar shows: the selection's (a mixed one reads as none), else
/// the next drawing's (STUDIO_V3 S1: the bar follows the selection).
pub(crate) struct Shown {
    pub(crate) colour: Option<[u8; 4]>,
    pub(crate) stroke: f32,
    pub(crate) outline: bool,
    pub(crate) fill: bool,
}

pub(crate) fn shown(e: &Annotator) -> Shown {
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
            outline: if base_tool(e) == Tool::Magnifier {
                e.magnifier_outline
            } else {
                e.outline
            },
            fill: e.fill,
        },
    }
}

/// What the shared controls can do for the tool / selection now: a control
/// that can't is dimmed (38 %), never hidden (STUDIO v3.1).
#[derive(Clone, Copy)]
pub(crate) struct Can {
    pub(crate) colour: bool,
    pub(crate) stroke: bool,
    pub(crate) opacity: bool,
    pub(crate) outline: bool,
    pub(crate) fill: bool,
}

pub(crate) fn can(e: &Annotator, tool: Tool) -> Can {
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
            | Tool::Magnifier
    );
    Can {
        colour: liney || matches!(tool, Tool::Text | Tool::Callout | Tool::Redact),
        stroke: liney,
        opacity: false,
        outline: matches!(
            tool,
            Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Callout | Tool::Magnifier
        ),
        fill: fills_inside(tool),
    }
}

/// The tool the bar is for: the active tool's, except with Select, where it is the selected
/// object's (W111: a drawing the last tool left selected never keeps its options on the bar
/// after the user picks another tool; one click on a tool shows that tool's options at once).
pub(crate) fn base_tool(e: &Annotator) -> Tool {
    let sel = e.selection();
    let item = (sel.len() == 1 && e.tool == Tool::Select)
        .then(|| e.doc.items.get(sel[0]))
        .flatten();
    match item {
        Some(item) => info(super::studio::tool_of(&item.shape)).slot,
        None => info(e.tool).slot,
    }
}

pub(crate) fn colours(e: &Annotator, n: usize, live: bool) -> El<'_> {
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
pub(crate) fn dots<'a>(
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

pub(crate) fn toggle<'a>(words: &'a str, on: bool, press: Option<Message>) -> El<'a> {
    group(vec![text_cell(words, on, press)], false)
}

pub(crate) fn stroke(e: &Annotator, live: bool, captioned: bool) -> El<'_> {
    let px = shown(e).stroke;
    let at = Width::ALL.iter().position(|w| w.stroke() == px);
    dots(at, format!("{} px", px.round()), live, captioned, |i| {
        Message::Width(Width::ALL[i])
    })
}

/// A pasted picture's opacity: three common steps.
pub(crate) fn opacity(e: &Annotator, live: bool, captioned: bool) -> El<'_> {
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

pub(crate) fn outline_fill(e: &Annotator, tool: Tool, c: Can) -> El<'_> {
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

pub(crate) fn font_size(e: &Annotator) -> El<'_> {
    let at = TEXT_SIZES.iter().position(|&p| p == e.text_size);
    dots(at, format!("{} px", e.text_size.round()), true, true, |i| {
        Message::TextSize(TEXT_SIZES[i])
    })
}

/// The tool's own groups (what the shared ones don't cover).
pub(crate) fn own_groups(e: &Annotator, tool: Tool) -> Vec<El<'_>> {
    own_groups_with(e, tool, None, true)
}

/// `own_groups`, Hide's style group given (Snip's row draws it as live pictures; built in place, so
/// nothing is built twice and the keyboard's stops are each cell once).
pub(crate) fn own_groups_with<'a>(
    e: &'a Annotator,
    tool: Tool,
    styles_group: Option<El<'a>>,
    custom_slider: bool,
) -> Vec<El<'a>> {
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
            let strengths = crate::features::snip::annotate::blur_strengths();
            let value = e.blur_custom.unwrap_or(e.blur_cell.cell().round() as u8);
            let shapes = crate::features::snip::annotate::blur_shapes();
            let styles = crate::features::snip::annotate::blur_styles();
            let styles_group = styles_group.unwrap_or_else(|| {
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
                )
            });
            let mut o = vec![
                styles_group,
                group(
                    shapes
                        .iter()
                        .map(|&(kind, words)| {
                            // icons, like the Shapes row (W111): the brush is Hide's own glyph
                            let icon = info(match kind {
                                crate::features::snip::annotate::blur::Kind::Brush => Tool::Blur,
                                crate::features::snip::annotate::blur::Kind::Rect => Tool::Rect,
                                crate::features::snip::annotate::blur::Kind::Ellipse => {
                                    Tool::Ellipse
                                }
                            })
                            .icon;
                            let cell = glyph_cell(
                                icon,
                                e.blur_shape == kind,
                                Some(Message::BlurShape(kind)),
                            );
                            hint(
                                cell,
                                words,
                                Why::Icon,
                                crate::shared::kit::option_bar::tips(),
                            )
                        })
                        .collect(),
                    false,
                ),
                group(
                    strengths
                        .iter()
                        .map(|&(w, words)| {
                            text_cell(
                                words,
                                e.blur_custom.is_none() && e.blur_cell == w,
                                Some(Message::BlurCell(w)),
                            )
                        })
                        .collect(),
                    false,
                ),
            ];
            if custom_slider {
                // Studio only (owner 2026-11-24): Custom slider beside Light / Medium / Strong.
                o.push(group(
                    vec![slider::bare(
                        STRENGTH_RANGE,
                        value,
                        96.0,
                        e.blur_custom.is_none(),
                        |v| Message::BlurCustom(Some(v)),
                    )],
                    false,
                ));
            }
            o.push(toggle(t!(sn_line_help), e.assist, Some(Message::Assist)));
            if e.blur_style.is_look() {
                o.push(group(vec![caption(t!(sn_looks_note))], false));
            }
            o
        }
        Tool::Magnifier => {
            let zooms = group(
                crate::features::snip::annotate::magnifier::ZOOMS
                    .iter()
                    .map(|&(z, label)| {
                        text_cell(
                            label,
                            e.magnifier_zoom == z,
                            Some(Message::MagnifierZoom(z)),
                        )
                    })
                    .collect(),
                false,
            );
            let shapes = [
                (
                    crate::features::snip::annotate::magnifier::Kind::Circle,
                    info(Tool::Ellipse).icon,
                    t!(sn_mag_circle),
                ),
                (
                    crate::features::snip::annotate::magnifier::Kind::RoundedSquare,
                    info(Tool::Rect).icon,
                    t!(sn_mag_square),
                ),
            ];
            let shape_group = group(
                shapes
                    .into_iter()
                    .map(|(kind, icon, words)| {
                        let cell = glyph_cell(
                            icon,
                            e.magnifier_shape == kind,
                            Some(Message::MagnifierShape(kind)),
                        );
                        hint(
                            cell,
                            words,
                            Why::Icon,
                            crate::shared::kit::option_bar::tips(),
                        )
                    })
                    .collect(),
                false,
            );
            vec![zooms, shape_group]
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

/// Snip's options row (UX C4): the active tool's options, at once under its
/// toolbar, built from this bar's own groups (and the shared blur lists) so
/// there is one component; none for a tool without options. The colour
/// stays in the toolbar's swatch.
/// `snip_row` with the keyboard's focus ring on its control number `on`.
pub fn snip_row_focused(e: &Annotator, on: Option<usize>) -> Option<El<'_>> {
    crate::shared::kit::option_bar::scope(on, false, || row_of(e)).0
}

/// What Tab walks in the row, in build (reading) order: every live control's message.
pub fn snip_row_stops(e: &Annotator) -> Vec<Message> {
    let (_, stops) = crate::shared::kit::option_bar::scope(None, true, || row_of(e));
    (stops.into_iter())
        .filter_map(|m| m.downcast::<Message>().ok().map(|m| *m))
        .collect()
}

/// R-10: the Pin button's row: Pin | Live pin (Live pin only while Labs "Pins" is on and the
/// desktop has it; dimmed away from the live screen). The choice is remembered.
pub(crate) fn pin_row(e: &Annotator, live_offered: bool) -> El<'_> {
    // (what the button will really do: Live pin only over the live screen, as `pin_route`)
    let live_on = e.pin_live && live_offered && e.scroll;
    let mut cells = vec![text_cell(
        t!(sn_pin_still),
        !live_on,
        Some(Message::PinLive(false)),
    )];
    if live_offered {
        let cell = text_cell(
            t!(sn_live_pin),
            live_on,
            e.scroll.then_some(Message::PinLive(true)),
        );
        cells.push(match e.scroll {
            true => cell,
            // a dimmed cell says why, like Long capture
            false => hint(
                cell,
                t!(sn_live_pin),
                Why::Cut(t!(sn_why_no_long)),
                crate::shared::kit::option_bar::tips(),
            ),
        });
    }
    let head = text(Role::ROW_TITLE, tr(crate::shared::app_language::S::sn_pin));
    dir::row(vec![head.into(), hairline(), group(cells, false)])
        .spacing(GROUP_GAP)
        .align_y(Center)
        .into()
}

pub(crate) fn row_of(e: &Annotator) -> Option<El<'_>> {
    // R-10: the Pin button's row (pointer on it, or the keyboard's focus on it), only where Live
    // pin is offered
    let offered = crate::features::snip::capture::live::offered();
    let focused = e.focus.and_then(|f| {
        crate::features::snip::screens::snip_toolbar::toolbar::bar_stops(e)
            .get(f)
            .cloned()
    }) == Some(Message::PinButton);
    if offered && (e.pin_row || focused) {
        return Some(pin_row(e, true));
    }
    use crate::features::snip::annotate::{TextBack as B, TextFont as F};
    let tool = base_tool(e);
    let c = can(e, tool);
    let st = e.text_style;
    let faces = [
        (F::Sans, t!(sn_sans)),
        (F::Serif, t!(sn_serif)),
        (F::Mono, t!(sn_mono)),
    ];
    let backs = [
        (B::None, t!(sn_none)),
        (B::Outline, t!(sn_back_outline)),
        (B::Box, t!(sn_box)),
    ];
    // a flipped option of the tool (an arrow's label, a step's pointer)
    let variant = |name, plain: Tool, other: Tool| {
        let on = e.tool == other;
        toggle(
            name,
            on,
            Some(Message::Tool(if on { plain } else { other })),
        )
    };
    // the tool's choices lead the row (built first: the Tab ring walks in this order)
    let mut lead: Vec<El<'_>> = Vec::new();
    let choices: Vec<_> = info(tool).group.tools().collect();
    if choices.len() > 1 {
        let cells = choices
            .into_iter()
            .map(|t| {
                let cell = glyph_cell(t.icon, t.tool == tool, Some(Message::Tool(t.tool)));
                hint(
                    cell,
                    tip(t),
                    Why::Icon,
                    crate::shared::kit::option_bar::tips(),
                )
            })
            .collect();
        lead.extend([group(cells, false), hairline()]);
    }
    let mut g: Vec<El<'_>> = match tool {
        Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow => {
            let mut g = vec![stroke(e, true, true), outline_fill(e, tool, c)];
            if tool == Tool::Arrow {
                g.push(variant(t!(sn_label), Tool::Arrow, Tool::ArrowLabel));
            }
            g
        }
        Tool::Pen | Tool::Highlighter => vec![stroke(e, true, true)],
        // (size and Bold are the shared groups; the faces and backdrops are Snip's)
        Tool::Text => {
            let mut g = own_groups(e, Tool::Text);
            let face = group(
                faces
                    .iter()
                    .map(|&(font, words)| {
                        let m = Message::TextStyle(TextStyle { font, ..st });
                        text_cell(words, st.font == font, Some(m))
                    })
                    .collect(),
                false,
            );
            let back = group(
                backs
                    .iter()
                    .map(|&(back, words)| {
                        let m = Message::TextStyle(TextStyle { back, ..st });
                        text_cell(words, st.back == back, Some(m))
                    })
                    .collect(),
                false,
            );
            g.insert(1, face);
            g.push(back);
            g
        }
        Tool::Callout => {
            let mut g = own_groups(e, Tool::Callout);
            g.push(toggle(
                t!(sn_outline),
                e.outline,
                Some(Message::Outline(!e.outline)),
            ));
            g
        }
        Tool::Step => vec![
            stroke(e, true, true),
            variant(t!(sn_pointer), Tool::Step, Tool::Pointer),
            toggle(t!(sn_legend), e.doc.legend, Some(Message::Legend)),
        ],
        Tool::Blur => {
            // Fixed sample swatches per style (W117): cached per light/dark theme, never live
            // thumbnails of the user's screenshot.
            let styles = crate::features::snip::annotate::blur_styles();
            let pictures = match crate::shared::kit::option_bar::lightly() {
                true => {
                    vec![iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]); styles.len()]
                }
                false => crate::features::snip::annotate::style_swatches(
                    crate::shared::kit::themes::current().light,
                ),
            };
            let pictured = group(
                styles
                    .iter()
                    .zip(pictures)
                    .map(|(&(style, words), picture)| {
                        crate::shared::kit::option_bar::picture_cell(
                            picture,
                            words,
                            e.blur_style == style,
                            Some(Message::BlurStyle(style)),
                        )
                    })
                    .collect(),
                false,
            );
            let mut g = own_groups_with(e, Tool::Blur, Some(pictured), false);
            // the brush: Auto (follows the text line) or three sizes
            let auto = e.blur_size.is_none();
            let at = e
                .blur_size
                .and_then(|w| Width::ALL.iter().position(|&x| x == w));
            let mut cells = vec![text_cell(
                t!(sn_auto),
                auto,
                Some(Message::BlurSize(match auto {
                    true => Some(Width::Medium),
                    false => None,
                })),
            )];
            cells.extend((0..3).map(|i| {
                dot(
                    i,
                    at == Some(i),
                    Some(Message::BlurSize(Some(Width::ALL[i]))),
                )
            }));
            g.push(group(cells, false));
            if let Some(both) = e.hideable() {
                let mut chips = vec![text_cell(
                    t!(sn_hide_shape),
                    false,
                    Some(Message::HideShape(false)),
                )];
                if both {
                    chips.push(text_cell(
                        t!(sn_hide_outline),
                        false,
                        Some(Message::HideShape(true)),
                    ));
                }
                g.push(group(chips, false));
            }
            g
        }
        Tool::Redact => vec![group(vec![caption(t!(sn_uses_colour))], false)],
        Tool::Magnifier => {
            let now = shown(e);
            let mut g = vec![
                stroke(e, true, true),
                toggle(
                    t!(sn_outline),
                    now.outline,
                    Some(Message::Outline(!now.outline)),
                ),
            ];
            g.extend(own_groups(e, Tool::Magnifier));
            g
        }
        Tool::Eraser => own_groups(e, Tool::Eraser),
        _ => Vec::new(),
    };
    // (a tool with no options of its own, Spotlight, still shows its group's choices: the way to the others)
    if g.is_empty() && lead.is_empty() {
        return None;
    }
    // the tool's choices first (its group's tools: a Shapes row offers rectangle, ellipse, line,
    // arrow, W107), then its name and key (what the row is for)
    let base = info(info(e.tool).slot);
    let mut head: Vec<El<'_>> = vec![text(Role::ROW_TITLE, tr(info(tool).name)).into()];
    if e.selection().is_empty() {
        let cap = keycap::keycap(base.key.to_ascii_uppercase().to_string());
        head.push(container(cap).width(size::KEYCAP_H).into());
    }
    let mut items = lead;
    items.extend([
        dir::row(head).spacing(space::GAP).align_y(Center).into(),
        hairline(),
    ]);
    items.append(&mut g);
    Some(dir::row(items).spacing(GROUP_GAP).align_y(Center).into())
}
