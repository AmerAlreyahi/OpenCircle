//! The custom look's rows (Appearance, under the look cards while Custom is
//! picked; the design has no Custom card): its colours, glow, dim, corners,
//! bars and each mode's screen edge.
use iced::Element;
use iced::widget::tooltip::Position;
use iced::widget::{column, row};

use super::super::state::{BARS, BarChoice, Message, PALETTES, PaletteChoice, State};
use crate::config::{BracketColour, BracketStyle, ColourSet, EdgeMode, EdgeStyle};
use crate::labs::{Feature, Labs};
use crate::ui::kit::colour::{picker, swatch};
use crate::ui::kit::dropdown::dropdown;
use crate::ui::kit::segmented::segmented;
use crate::ui::kit::setting_row::{self, title};
use crate::ui::kit::slider::slider;
use crate::ui::kit::tokens::space;
use crate::ui::theme;
use crate::ui::widgets::{Why, hint, tip};

pub fn rows(state: &State) -> Vec<Element<'_, Message>> {
    let c = &state.draft.look.custom;
    let edge = c.edge(state.edge_mode);
    let row_of = |name, desc, control| setting_row::row(title(name), desc, control);
    let mut rows = vec![row_of(
        "Colours",
        "The screen edge, selection and buttons",
        tip(
            dropdown(
                &PALETTES,
                Some(PaletteChoice(c.palette)),
                "Choose colours",
                Message::Palette,
            ),
            "Look colours",
            Position::Top,
        ),
    )];
    if c.palette == ColourSet::TwoColour {
        let rgb = |i: usize| theme::rgb(c.colours[i]);
        let dot = |i: usize, name| {
            let open = state.picker == Some(i);
            hint(
                swatch(rgb(i), open, Message::Picker(i)),
                name,
                Why::Icon,
                Position::Top,
            )
        };
        let mut control =
            column![row![dot(0, "First colour"), dot(1, "Second colour")].spacing(space::GAP)]
                .spacing(space::GAP)
                .align_x(iced::Right);
        if let Some(i) = state.picker {
            control = control.push(picker(
                &theme::ANNOTATION,
                rgb(i),
                &state.hex,
                Message::PickColour,
                Message::Hex,
            ));
        }
        rows.push(row_of(
            "Your two colours",
            "From the top-left corner round to the opposite one",
            control.into(),
        ));
    }
    let edge_style = match edge.style {
        // a "thin" edge from an older config shows as the glow it now is
        EdgeStyle::None => EdgeStyle::None,
        _ => EdgeStyle::Glow,
    };
    let modes = edge_modes(&state.draft.labs);
    rows.extend([
        row_of(
            "Glow strength",
            "How bright the edge glow is",
            tip(
                slider(crate::config::GLOW, c.glow, " %", Message::Glow),
                "Glow strength",
                Position::Top,
            ),
        ),
        row_of(
            "Dim",
            "How much the frozen screen darkens",
            tip(
                slider(crate::config::DIM, c.dim, " %", Message::Dim),
                "Dim strength",
                Position::Top,
            ),
        ),
        row_of(
            "Selection corners",
            "The brackets on a snapped box",
            segmented(
                &[
                    (BracketColour::White, "White"),
                    (BracketColour::Accent, "Accent"),
                ],
                c.brackets,
                c.brackets,
                1.0,
                Message::Brackets,
            ),
        ),
        row_of(
            "Corner style",
            "Four corners, or a frame all round",
            segmented(
                &[
                    (BracketStyle::Corners, "Corners"),
                    (BracketStyle::Frame, "Frame"),
                ],
                c.bracket_style,
                c.bracket_style,
                1.0,
                Message::BracketStyle,
            ),
        ),
        row_of(
            "Bars and pills",
            "The action bar, chips and panels",
            tip(
                dropdown(
                    &BARS,
                    Some(BarChoice(c.bar)),
                    "Choose a style",
                    Message::Bar,
                ),
                "Bar style",
                Position::Top,
            ),
        ),
        row_of(
            "Screen edge for",
            "Each mode has its own edge",
            segmented(
                &modes,
                state.edge_mode,
                state.edge_mode,
                1.0,
                Message::EdgeMode,
            ),
        ),
        row_of(
            "Edge style",
            "A soft glow along the screen edge, or none",
            segmented(
                &[(EdgeStyle::Glow, "Glow"), (EdgeStyle::None, "None")],
                edge_style,
                edge_style,
                1.0,
                Message::EdgeStyle,
            ),
        ),
    ]);
    rows
}

/// The Screen edge segments: Live only while live translate is on in Labs.
fn edge_modes(labs: &Labs) -> Vec<(EdgeMode, &'static str)> {
    let mut modes = vec![(EdgeMode::Overlay, "Circle")];
    if labs.has(Feature::Live) {
        modes.push((EdgeMode::Live, "Live"));
    }
    modes.push((EdgeMode::Snip, "Snip"));
    modes
}
