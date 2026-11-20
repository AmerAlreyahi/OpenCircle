//! The status bar (board 2): the left panel's toggle, the size chip (drags the
//! picture out), the zoom group (- % +, Fit), Save, Copy (the whole picture,
//! accent) and the right panel's toggle, each on its panel's side.
use iced::widget::tooltip::Position;
use iced::widget::{Space, container};
use iced::{Center, Element, Fill};

use super::popovers::Pop;
use super::view::{MINUS, PLUS};
use super::{Msg, Studio, keys};
use crate::features::snip::annotate::{Annotator, Message};
use crate::features::snip::screens::snip_toolbar::toolbar;
use crate::shared::kit::button::{self, Kind};
use crate::shared::kit::chip;
use crate::shared::kit::option_bar::{glyph_cell, group, text_cell, value};
use crate::shared::kit::tokens::space;
use crate::shared::kit::tooltip::{Why, hint};
use crate::t;

pub fn view<'a>(s: &'a Studio, e: &'a Annotator) -> Element<'a, Msg> {
    let px = e.doc.view().size();
    let size_label = format!("{} × {}", px.width.round(), px.height.round());
    let percent = s.percent().map_or_else(String::new, |p| format!("{p} %"));
    let zoom = group(
        vec![
            glyph_cell(MINUS, false, Some(Msg::ZoomOut)),
            value(percent),
            glyph_cell(PLUS, false, Some(Msg::ZoomIn)),
            text_cell(t!(st_fit), false, Some(Msg::Fit)),
        ],
        false,
    );
    let (save_words, copy_words) = (t!(c_save), t!(c_copy));
    let save = button::labelled(
        toolbar::SAVE,
        save_words,
        button::labelled_width(save_words),
        Kind::Elevated,
        false,
        Some(Msg::Editor(Message::Save)),
    );
    let copy = button::labelled(
        toolbar::COPY,
        copy_words,
        button::labelled_width(copy_words),
        Kind::Accent,
        false,
        Some(Msg::Editor(Message::Copy)),
    );
    let line = crate::dir_row![
        chip::size_handle(size_label, false, Msg::Editor(Message::DragOut)),
        group(
            vec![text_cell(
                t!(st_picture_info),
                s.popover == Some(Pop::Info),
                Some(Msg::Pop(Some(Pop::Info)))
            )],
            false
        ),
        Space::new().width(Fill),
        zoom,
        Space::new().width(Fill),
        hint(save, t!(c_save), Why::Keys(keys::SAVE), Position::Top),
        hint(
            copy,
            t!(st_copy_picture),
            Why::Keys(keys::COPY_PICTURE),
            Position::Top
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
