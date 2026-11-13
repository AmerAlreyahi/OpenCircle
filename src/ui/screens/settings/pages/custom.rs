//! The custom look's rows (Appearance, under the look cards while Custom is
//! picked; the design has no Custom card): its colours, glow, dim, corners,
//! bars and each mode's screen edge.
use crate::t;
use iced::Element;
use iced::widget::column;
use iced::widget::tooltip::Position;

use super::super::state::{BARS, BarChoice, Message, PALETTES, PaletteChoice, State};
use crate::labs::{Feature, Labs};
use crate::shared::config::{BracketColour, BracketStyle, ColourSet, EdgeMode, EdgeStyle};
use crate::shared::kit::colour::{picker, swatch};
use crate::shared::kit::dropdown::dropdown;
use crate::shared::kit::segmented::segmented;
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::slider::slider;
use crate::shared::kit::tokens::space;
use crate::shared::kit::tooltip::{Why, hint, tip};
use crate::shared::theme;

/// The bar style row, hidden until glass ships (the Glass version).
const GLASS_BARS: bool = false;

pub fn rows(state: &State) -> Vec<Element<'_, Message>> {
    let c = &state.draft.look.custom;
    let edge = c.edge(state.edge_mode);
    let row_of = |name, desc, control| setting_row::row(title(name), desc, control);
    let mut rows = vec![row_of(
        t!(cu_colours),
        t!(cu_colours_desc),
        tip(
            dropdown(
                &PALETTES,
                Some(PaletteChoice(c.palette)),
                t!(cu_choose),
                Message::Palette,
            ),
            t!(cu_look_colours),
            Position::Top,
        ),
    )];
    if c.palette == ColourSet::TwoColour {
        let rgb = |i: usize| theme::rgb(c.colours[i]);
        let dot = |i: usize, name| {
            let open = state.picker == Some(i);
            hint(
                swatch(rgb(i), open, Some(Message::Picker(i))),
                name,
                Why::Icon,
                Position::Top,
            )
        };
        let mut control = column![
            crate::dir_row![dot(0, t!(cu_first)), dot(1, t!(cu_second))].spacing(space::GAP)
        ]
        .spacing(space::GAP);
        if let Some(i) = state.picker {
            control = control.push(picker(
                &theme::ANNOTATION,
                rgb(i),
                &state.hex,
                Message::PickColour,
                Message::Hex,
            ));
        }
        rows.push(row_of(t!(cu_two), t!(cu_two_desc), control.into()));
    }
    let edge_style = match edge.style {
        // a "thin" edge from an older config shows as the glow it now is
        EdgeStyle::None => EdgeStyle::None,
        _ => EdgeStyle::Glow,
    };
    let modes = edge_modes(&state.draft.labs);
    rows.extend([
        row_of(
            t!(cu_glow),
            t!(cu_glow_desc),
            tip(
                slider(crate::shared::config::GLOW, c.glow, " %", Message::Glow),
                t!(cu_glow),
                Position::Top,
            ),
        ),
        row_of(
            t!(cu_dim),
            t!(cu_dim_desc),
            tip(
                slider(crate::shared::config::DIM, c.dim, " %", Message::Dim),
                t!(cu_dim_tip),
                Position::Top,
            ),
        ),
        row_of(
            t!(cu_corners),
            t!(cu_corners_desc),
            segmented(
                &[
                    (BracketColour::White, t!(cu_white)),
                    (BracketColour::Accent, t!(cu_accent)),
                ],
                c.brackets,
                c.brackets,
                1.0,
                Message::Brackets,
            ),
        ),
        row_of(
            t!(cu_style),
            t!(cu_style_desc),
            segmented(
                &[
                    (BracketStyle::Corners, t!(cu_corners_opt)),
                    (BracketStyle::Frame, t!(cu_frame)),
                ],
                c.bracket_style,
                c.bracket_style,
                1.0,
                Message::BracketStyle,
            ),
        ),
    ]);
    // Glass bars come with the GPU renderer: until then the style
    // changes nothing, so its row stays hidden (the config key is kept).
    if GLASS_BARS {
        rows.push(row_of(
            t!(cu_bars),
            t!(cu_bars_desc),
            tip(
                dropdown(
                    &BARS,
                    Some(BarChoice(c.bar)),
                    t!(cu_style_choose),
                    Message::Bar,
                ),
                t!(cu_bar_style),
                Position::Top,
            ),
        ));
    }
    // (Circle and Snip at least: never a one-choice row)
    rows.push(row_of(
        t!(cu_edge_for),
        t!(cu_edge_for_desc),
        segmented(
            &modes,
            state.edge_mode,
            state.edge_mode,
            1.0,
            Message::EdgeMode,
        ),
    ));
    rows.push(row_of(
        t!(cu_edge),
        t!(cu_edge_desc),
        segmented(
            &[
                (EdgeStyle::Glow, t!(cu_glow_opt)),
                (EdgeStyle::None, t!(cu_none)),
            ],
            edge_style,
            edge_style,
            1.0,
            Message::EdgeStyle,
        ),
    ));
    rows
}

/// The Screen edge segments: Live only while it is on in Labs.
fn edge_modes(labs: &Labs) -> Vec<(EdgeMode, &'static str)> {
    let mut modes = vec![(EdgeMode::Overlay, t!(cu_circle))];
    if labs.has(Feature::Live) {
        modes.push((EdgeMode::Live, t!(cu_live)));
    }
    modes.push((EdgeMode::Snip, t!(cu_snip)));
    modes
}
