//! The status bar (board 2): the left panel's toggle, the size chip (drags the
//! picture out), the zoom group (- % +, Fit), Save, Copy (the whole picture,
//! accent) and the right panel's toggle, each on its panel's side.
use iced::widget::tooltip::Position;
use iced::widget::{Space, container};
use iced::{Center, Element, Fill};

use super::view::{MINUS, PANEL_LEFT, PANEL_RIGHT, PLUS};
use super::{Msg, Studio};
use crate::features::snip::annotate::{Annotator, Message};
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::shared::kit::button::{self, Kind};
use crate::shared::kit::chip;
use crate::shared::kit::icon_button::{self, State};
use crate::shared::kit::option_bar::{glyph_cell, group, text_cell, value};
use crate::shared::kit::tokens::space;
use crate::shared::kit::tooltip::{Why, hint};
use crate::t;

fn toggle<'a>(
    glyph: char,
    shown: bool,
    press: Msg,
    words: &'a str,
    keys: &'a str,
) -> Element<'a, Msg> {
    let state = State {
        active: shown,
        focused: false,
    };
    hint(
        icon_button::square(glyph, state, Some(press)),
        words,
        Why::Keys(keys),
        Position::Top,
    )
}

pub fn view<'a>(s: &'a Studio, e: &'a Annotator) -> Element<'a, Msg> {
    let px = e.doc.view().size();
    let size_label = format!("{} × {}", px.width.round(), px.height.round());
    let percent = s.percent().map_or_else(String::new, |p| format!("{p} %"));
    // narrow: the left toggle opens the Library over the stage
    let (left_shown, left_press) = match s.narrow() {
        true => (s.overlay, Msg::Overlay),
        false => (s.panels.library, Msg::Library),
    };
    let left_words = match left_shown {
        true => t!(st_hide_library),
        false => t!(st_show_library),
    };
    let right_words = match s.panels.options && !s.narrow() {
        true => t!(st_hide_panel),
        false => t!(st_show_panel),
    };
    let zoom = group(
        vec![
            glyph_cell(MINUS, false, Some(Msg::ZoomOut)),
            value(percent),
            glyph_cell(PLUS, false, Some(Msg::ZoomIn)),
            text_cell(t!(st_fit), false, Some(Msg::Fit)),
        ],
        false,
    );
    let save = button::tool(
        toolbar::SAVE,
        Kind::Elevated,
        false,
        Some(Msg::Editor(Message::Save)),
    );
    let copy = button::tool(
        toolbar::COPY,
        Kind::Accent,
        false,
        Some(Msg::Editor(Message::Copy)),
    );
    let line = crate::dir_row![
        toggle(PANEL_LEFT, left_shown, left_press, left_words, "Ctrl+B"),
        chip::size_handle(size_label, false, Msg::Editor(Message::DragOut)),
        Space::new().width(Fill),
        zoom,
        Space::new().width(Fill),
        hint(save, t!(c_save), Why::Keys("Ctrl+S"), Position::Top),
        hint(
            copy,
            t!(st_copy_picture),
            Why::Keys("Ctrl+Shift+C"),
            Position::Top
        ),
        toggle(
            PANEL_RIGHT,
            s.panels.options && !s.narrow(),
            Msg::Panel,
            right_words,
            "Ctrl+Alt+B"
        ),
    ]
    .spacing(space::GAP)
    .align_y(Center);
    container(line)
        .padding([0.0, 12.0])
        .center_y(super::STATUS)
        .width(Fill)
        .into()
}
